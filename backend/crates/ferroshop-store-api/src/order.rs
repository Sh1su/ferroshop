//! Checkout: `/checkout/order` turns the cart into an order; `/order` lists the customer's orders.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::{Filter, Sort};
use ferroshop_core::order_state::{self, Machine};
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Criteria, id, read};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use sqlx::postgres::PgRow;
use uuid::Uuid;

use crate::account::require_customer;
use crate::{SalesChannelContext, StoreState, cart, catalog, guard};

fn invalid(code: &'static str, detail: &str) -> ApiError {
    ApiError::Code(StatusCode::BAD_REQUEST, code, detail.into())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderInput {
    customer_comment: Option<String>,
}

/// An order address copied from a customer address, so later changes do not alter the order.
fn order_address(row: &PgRow, address_id: &str) -> Result<Value, sqlx::Error> {
    Ok(json!({
        "id": address_id,
        "countryId": id::format(row.try_get::<Uuid, _>("country_id")?),
        "salutationId": row.try_get::<Option<Uuid>, _>("salutation_id")?.map(id::format),
        "firstName": row.try_get::<String, _>("first_name")?,
        "lastName": row.try_get::<String, _>("last_name")?,
        "company": row.try_get::<Option<String>, _>("company")?,
        "street": row.try_get::<String, _>("street")?,
        "zipcode": row.try_get::<Option<String>, _>("zipcode")?,
        "city": row.try_get::<String, _>("city")?,
        "phoneNumber": row.try_get::<Option<String>, _>("phone_number")?,
    }))
}

const ADDRESS_QUERY: &str = "SELECT country_id, salutation_id, first_name, last_name, company, street, zipcode, city, phone_number \
     FROM customer_address WHERE id = $1 AND customer_id = $2";

fn order_criteria() -> Criteria {
    serde_json::from_value(json!({ "associations": {
        "lineItems": {},
        "billingAddress": { "associations": { "country": {} } },
        "shippingAddress": { "associations": { "country": {} } },
        "paymentMethod": {},
        "shippingMethod": {},
        "currency": {},
    }}))
    .expect("static criteria")
}

/// `POST /store-api/checkout/order`: places the order for the logged-in (or guest) customer.
///
/// The cart is recalculated first. If that changes anything, the adjusted cart is stored and the
/// order is refused, so the shopper confirms what they actually buy. Stock is booked in the same
/// transaction; closeout products cannot be oversold by concurrent checkouts.
pub async fn place(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let input: OrderInput =
        if body.iter().all(u8::is_ascii_whitespace) { OrderInput::default() } else { guard::body(&body)? };

    let mut tx = state.db.begin().await?;
    let stored = cart::load(&mut tx, &sc, true).await?;
    if stored.is_empty() {
        return Err(invalid("CHECKOUT__CART_EMPTY", "the cart is empty"));
    }
    let calculated = cart::calculate(&state, &sc, stored).await?;
    if !calculated.errors.is_empty() {
        cart::save(&mut tx, &sc, &calculated.items).await?;
        tx.commit().await?;
        return Err(invalid("CHECKOUT__CART_INVALID", "the cart changed, please review it before ordering"));
    }
    let cart = &calculated.json;
    let delivery =
        cart["deliveries"].get(0).ok_or_else(|| invalid("CHECKOUT__CART_INVALID", "no delivery possible"))?;

    let payment_ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM payment_method WHERE id = $1 AND active)")
        .bind(sc.payment_method_id)
        .fetch_one(&mut *tx)
        .await?;
    if !payment_ok {
        return Err(invalid("CHECKOUT__UNKNOWN_PAYMENT_METHOD", "the payment method is not available"));
    }

    let customer = sqlx::query(
        "SELECT customer_number, email, first_name, last_name, company, default_billing_address_id, \
         default_shipping_address_id FROM customer WHERE id = $1",
    )
    .bind(customer_id)
    .fetch_one(&mut *tx)
    .await?;
    let address = |column: &str| -> Result<Option<Uuid>, sqlx::Error> { customer.try_get(column) };
    let missing = || invalid("CHECKOUT__CUSTOMER_ADDRESS_NOT_FOUND", "the customer has no billing address");
    let billing_source = address("default_billing_address_id")?.ok_or_else(missing)?;
    let shipping_source = address("default_shipping_address_id")?.unwrap_or(billing_source);
    let billing_row = sqlx::query(ADDRESS_QUERY)
        .bind(billing_source)
        .bind(customer_id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(missing)?;
    let billing_id = id::format(id::new_id());
    let mut addresses = vec![order_address(&billing_row, &billing_id)?];
    let shipping_id = if shipping_source == billing_source {
        billing_id.clone()
    } else {
        let row = sqlx::query(ADDRESS_QUERY)
            .bind(shipping_source)
            .bind(customer_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(missing)?;
        let shipping_id = id::format(id::new_id());
        addresses.push(order_address(&row, &shipping_id)?);
        shipping_id
    };

    // Book stock; closeout products only while enough is left.
    for line in cart["lineItems"].as_array().into_iter().flatten() {
        let product = line["referencedId"].as_str().and_then(id::parse);
        let quantity = line["quantity"].as_i64().unwrap_or(0);
        let closeout = line["payload"]["isCloseout"].as_bool().unwrap_or(false);
        let booked = sqlx::query("UPDATE product SET stock = stock - $2 WHERE id = $1 AND (NOT $3 OR stock >= $2)")
            .bind(product)
            .bind(quantity as i32)
            .bind(closeout)
            .execute(&mut *tx)
            .await?;
        if booked.rows_affected() == 0 {
            // Dropping the transaction rolls back stock already booked for other lines.
            return Err(invalid("CHECKOUT__CART_INVALID", "a product sold out in the meantime"));
        }
    }

    let line_items: Vec<Value> = cart["lineItems"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(position, line)| {
            json!({
                "productId": line["referencedId"],
                "type": "product",
                "label": line["label"],
                "quantity": line["quantity"],
                "unitPrice": line["price"]["unitPrice"],
                "totalPrice": line["price"]["totalPrice"],
                "taxRate": line["price"]["taxRules"][0]["taxRate"],
                "position": position + 1,
                "payload": line["payload"],
            })
        })
        .collect();
    let price = &cart["price"];
    let order = json!({
        "salesChannelId": id::format(sc.sales_channel_id),
        "currencyId": id::format(sc.price.currency_id),
        "currencyFactor": sc.price.factor,
        "languageId": id::format(sc.language_id),
        "customerId": id::format(customer_id),
        "customerNumber": customer.try_get::<String, _>("customer_number")?,
        "customerEmail": customer.try_get::<String, _>("email")?,
        "customerFirstName": customer.try_get::<String, _>("first_name")?,
        "customerLastName": customer.try_get::<String, _>("last_name")?,
        "customerCompany": customer.try_get::<Option<String>, _>("company")?,
        "addresses": addresses,
        "billingAddressId": billing_id,
        "shippingAddressId": shipping_id,
        "paymentMethodId": id::format(sc.payment_method_id),
        "shippingMethodId": delivery["shippingMethod"]["id"],
        "amountTotal": price["totalPrice"],
        "amountNet": price["netPrice"],
        "positionPrice": price["positionPrice"],
        "shippingTotal": delivery["shippingCosts"]["totalPrice"],
        "taxStatus": price["taxStatus"],
        "customerComment": input.customer_comment.filter(|c| !c.trim().is_empty()),
        "lineItems": line_items,
    });
    let ctx = Context { actor_id: Some(customer_id), ..sc.core() };
    let order_id = write::write_in(&mut tx, "order", &order, WriteMode::Create, &ctx).await?;
    // Cancelling gives the booked stock back (see `order_state`).
    sqlx::query("UPDATE sales_order SET stock_state = 'booked' WHERE id = $1").bind(order_id).execute(&mut *tx).await?;
    cart::save(&mut tx, &sc, &[]).await?;
    tx.commit().await?;

    Ok(Json(read::read(&state.db, "order", &id::format(order_id), &order_criteria(), &sc.core()).await?))
}

/// `POST /store-api/order`: the customer's orders, newest first unless sorted otherwise.
pub async fn list(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let mut criteria = guard::criteria(&body)?;
    criteria.filter.push(Filter::Equals { field: "customerId".into(), value: json!(id::format(customer_id)) });
    if criteria.sort.is_empty() {
        criteria.sort = vec![Sort { field: "orderDateTime".into(), order: "DESC".into(), natural_sorting: false }];
    }
    let page = criteria.page;
    let limit = criteria.limit.unwrap_or(10).clamp(1, guard::MAX_LIMIT);
    let result = catalog::search_own(&state, &sc, "order", criteria, 10).await?;
    Ok(Json(json!({
        "orders": catalog::search_result("order", page, limit, result),
        "paymentChangeable": [],
        "apiAlias": "order_route_response",
    })))
}

async fn own_order(state: &StoreState, customer_id: Uuid, raw: &str) -> Result<(Uuid, String), ApiError> {
    let not_found = || ApiError::from(ferroshop_core::Error::NotFound { entity: "order".into(), id: raw.into() });
    let order = id::parse(raw).ok_or_else(not_found)?;
    let state_name: Option<String> =
        sqlx::query_scalar("SELECT state FROM sales_order WHERE id = $1 AND customer_id = $2")
            .bind(order)
            .bind(customer_id)
            .fetch_optional(&state.db)
            .await?;
    Ok((order, state_name.ok_or_else(not_found)?))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OrderReference {
    order_id: String,
}

/// `POST /store-api/handle-payment`: all of ferroshop's payment methods are paid outside the
/// shop (prepayment, invoice, cash on delivery), so there is never a payment provider to
/// redirect to and the storefront continues to its finish page.
pub async fn handle_payment(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let input: OrderReference = guard::body(&body)?;
    own_order(&state, customer_id, &input.order_id).await?;
    Ok(Json(json!({ "redirectUrl": null, "apiAlias": "handle_payment_method_route_response" })))
}

/// `POST /store-api/order/state/cancel`: customers can cancel orders that are still open;
/// booked stock is given back.
pub async fn cancel(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let input: OrderReference = guard::body(&body)?;
    let (order, current) = own_order(&state, customer_id, &input.order_id).await?;
    if current != "open" {
        return Err(invalid("CHECKOUT__ORDER_NOT_CANCELLABLE", "only open orders can be cancelled"));
    }
    let ctx = Context { actor_id: Some(customer_id), ..sc.core() };
    let to = order_state::transition(&state.db, order, Machine::Order, "cancel", &ctx).await?;
    let name = if sc.language_id == ferroshop_core::defaults::LANGUAGE_DE { "Storniert" } else { "Cancelled" };
    Ok(Json(
        json!({ "technicalName": to, "name": name, "translated": { "name": name }, "apiAlias": "state_machine_state" }),
    ))
}
