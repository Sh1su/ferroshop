//! Cart: `/checkout/cart` and its line items, stored per context token.
//!
//! Only references and quantities are stored. Every read recalculates the cart against the
//! current catalogue: prices, purchase rules, stock of closeout products and shipping costs.
//! Adjustments are reported once in `errors` and written back.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, RawQuery, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ferroshop_admin_api::ApiError;
use ferroshop_core::price::{CalculatedPrice, CartPrice};
use ferroshop_core::{Criteria, Error, WriteError, id};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{PgConnection, Row};

use crate::{SalesChannelContext, StoreState, catalog, guard, pricing};

/// Most distinct line items a cart can hold.
pub const MAX_LINE_ITEMS: usize = 100;
/// Upper purchase limit when a product defines none (Shopware's default).
const DEFAULT_MAX_PURCHASE: u32 = 100;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredItem {
    pub id: String,
    pub referenced_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub quantity: u32,
}

/// A recalculated cart: the JSON answer plus what is needed to place an order from it.
pub struct Calculated {
    pub json: Value,
    pub items: Vec<StoredItem>,
    /// Adjustments made during this calculation (empty when the cart was unchanged).
    pub errors: BTreeMap<String, Value>,
}

pub(crate) async fn load(
    conn: &mut PgConnection,
    sc: &SalesChannelContext,
    lock: bool,
) -> Result<Vec<StoredItem>, ApiError> {
    let query = format!(
        "SELECT line_items FROM cart WHERE token = $1 AND sales_channel_id = $2{}",
        if lock { " FOR UPDATE" } else { "" }
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(query))
        .bind(&sc.token)
        .bind(sc.sales_channel_id)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(match row {
        Some(row) => serde_json::from_value(row.try_get("line_items")?).unwrap_or_default(),
        None => Vec::new(),
    })
}

pub(crate) async fn save(
    conn: &mut PgConnection,
    sc: &SalesChannelContext,
    items: &[StoredItem],
) -> Result<(), ApiError> {
    if items.is_empty() {
        sqlx::query("DELETE FROM cart WHERE token = $1 AND sales_channel_id = $2")
            .bind(&sc.token)
            .bind(sc.sales_channel_id)
            .execute(&mut *conn)
            .await?;
        return Ok(());
    }
    sqlx::query(
        "INSERT INTO cart (token, sales_channel_id, line_items) VALUES ($1, $2, $3) \
         ON CONFLICT (token) DO UPDATE SET line_items = EXCLUDED.line_items, updated_at = now() \
         WHERE cart.sales_channel_id = EXCLUDED.sales_channel_id",
    )
    .bind(&sc.token)
    .bind(sc.sales_channel_id)
    .bind(serde_json::to_value(items).expect("items serialize"))
    .execute(&mut *conn)
    .await?;
    Ok(())
}

fn cart_error(errors: &mut BTreeMap<String, Value>, message_key: &str, level: u8, product: &Value, message: String) {
    let product_id = product["id"].as_str().unwrap_or_default();
    let key = format!("{message_key}{product_id}");
    errors.insert(
        key.clone(),
        json!({
            "id": product_id,
            "name": product["translated"]["name"],
            "key": key,
            "messageKey": message_key,
            "level": level,
            "message": message,
            "apiAlias": "cart_error",
        }),
    );
}

const LEVEL_WARNING: u8 = 10;
const LEVEL_ERROR: u8 = 20;

fn int(product: &Value, key: &str) -> Option<u32> {
    product[key].as_i64().map(|v| v.clamp(0, u32::MAX as i64) as u32)
}

/// Applies minimum, maximum, purchase steps and (for closeout products) stock.
/// Returns `None` when the product cannot be bought at all.
fn allowed_quantity(product: &Value, requested: u32, errors: &mut BTreeMap<String, Value>) -> Option<u32> {
    let min = int(product, "minPurchase").unwrap_or(1).max(1);
    let steps = int(product, "purchaseSteps").unwrap_or(1).max(1);
    let mut max = int(product, "maxPurchase").filter(|m| *m > 0).unwrap_or(DEFAULT_MAX_PURCHASE);
    let closeout = product["isCloseout"].as_bool().unwrap_or(false);
    let stock = int(product, "stock").unwrap_or(0);
    if closeout {
        max = max.min(stock);
    }
    if max < min {
        let message = format!("{} is out of stock", product["translated"]["name"].as_str().unwrap_or_default());
        cart_error(errors, "product-out-of-stock", LEVEL_ERROR, product, message);
        return None;
    }
    let clamped = requested.clamp(min, max);
    let quantity = min + (clamped - min) / steps * steps;
    if closeout && requested > stock {
        let message = format!("Only {quantity} available");
        cart_error(errors, "product-stock-reached", LEVEL_WARNING, product, message);
    }
    Some(quantity)
}

fn line_item(item: &StoredItem, product: &Value, price: &CalculatedPrice) -> Value {
    let price_json = price.to_json();
    json!({
        "id": item.id,
        "referencedId": item.referenced_id,
        "label": product["translated"]["name"],
        "quantity": price.quantity,
        "type": "product",
        "good": true,
        "description": null,
        "cover": product["cover"]["media"],
        "deliveryInformation": {
            "stock": product["stock"],
            "weight": product["weight"],
            "freeDelivery": product["shippingFree"].as_bool().unwrap_or(false),
            "restockTime": null,
            "deliveryTime": null,
            "height": product["height"],
            "width": product["width"],
            "length": product["length"],
            "apiAlias": "cart_delivery_information",
        },
        "price": price_json,
        "priceDefinition": {
            "price": price.unit_price,
            "quantity": price.quantity,
            "taxRules": price_json["taxRules"],
            "isCalculated": true,
            "listPrice": price.list_price,
            "referencePriceDefinition": null,
            "regulationPrice": null,
            "type": "quantity",
            "apiAlias": "cart_price_quantity",
        },
        "payload": {
            "productNumber": product["productNumber"],
            "manufacturerId": product["manufacturerId"],
            "parentId": product["parentId"],
            "taxId": product["taxId"],
            "isCloseout": product["isCloseout"],
            "shippingFree": product["shippingFree"],
            "stock": product["stock"],
            "options": [],
        },
        "quantityInformation": {
            "minPurchase": int(product, "minPurchase").unwrap_or(1).max(1),
            "maxPurchase": int(product, "maxPurchase").filter(|m| *m > 0).unwrap_or(DEFAULT_MAX_PURCHASE),
            "purchaseSteps": int(product, "purchaseSteps").unwrap_or(1).max(1),
            "apiAlias": "cart_line_item_quantity_information",
        },
        "removable": true,
        "stackable": true,
        "modified": false,
        "children": [],
        "states": ["is-physical"],
        "apiAlias": "line_item",
    })
}

/// Recalculates stored items against the catalogue.
pub async fn calculate(
    state: &StoreState,
    sc: &SalesChannelContext,
    stored: Vec<StoredItem>,
) -> Result<Calculated, ApiError> {
    let mut errors = BTreeMap::new();
    let product_ids: Vec<String> = stored.iter().map(|i| i.referenced_id.clone()).collect();
    let products: HashMap<String, Value> = if product_ids.is_empty() {
        HashMap::new()
    } else {
        let mut criteria = Criteria::with_ids(product_ids);
        criteria.associations.insert("cover.media".into(), Criteria::default());
        let found = catalog::search(state, sc, "product", criteria, guard::MAX_LIMIT).await?.data;
        found.into_iter().map(|p| (p["id"].as_str().unwrap_or_default().to_string(), p)).collect()
    };
    let rates = pricing::tax_rates(&state.db, &products.values().cloned().collect::<Vec<_>>()).await?;

    let mut items = Vec::new();
    let mut lines = Vec::new();
    let mut prices = Vec::new();
    let mut all_shipping_free = true;
    for item in stored {
        let Some(product) = products.get(&item.referenced_id) else {
            let placeholder = json!({ "id": item.referenced_id, "translated": { "name": null } });
            cart_error(&mut errors, "product-not-found", LEVEL_ERROR, &placeholder, "Product not found".into());
            continue;
        };
        // Products with variants are only buyable as one of their variants.
        if product["childCount"].as_i64().unwrap_or(0) > 0 {
            cart_error(&mut errors, "product-not-found", LEVEL_ERROR, product, "Choose a variant".into());
            continue;
        }
        let Some(quantity) = allowed_quantity(product, item.quantity, &mut errors) else { continue };
        let rate = product["taxId"].as_str().and_then(|t| rates.get(t)).copied().unwrap_or_default();
        let Some(price) = ferroshop_core::price::compute(&product["price"], rate, quantity, &sc.price) else {
            cart_error(&mut errors, "product-not-found", LEVEL_ERROR, product, "Product has no price".into());
            continue;
        };
        all_shipping_free &= product["shippingFree"].as_bool().unwrap_or(false);
        let item = StoredItem { quantity, ..item };
        lines.push(line_item(&item, product, &price));
        prices.push(price);
        items.push(item);
    }

    let mut deliveries = Vec::new();
    let mut shipping_costs = Vec::new();
    if !items.is_empty() {
        let mut criteria = Criteria::with_ids(vec![id::format(sc.shipping_method_id)]);
        criteria.limit = Some(1);
        let method = catalog::search(state, sc, "shipping_method", criteria, 1).await?.data.pop();
        match method {
            Some(method) => {
                // Shipping is taxed with the highest rate in the cart.
                let rate = prices.iter().map(|p| p.tax_rate).max().unwrap_or_default();
                let cost = if all_shipping_free {
                    None
                } else {
                    ferroshop_core::price::compute(&method["price"], rate, 1, &sc.price)
                }
                .unwrap_or_else(|| CalculatedPrice::from_unit(Decimal::ZERO, 1, rate, &sc.price));
                let positions: Vec<Value> = items
                    .iter()
                    .zip(&lines)
                    .map(|(item, line)| {
                        json!({
                            "identifier": item.id, "quantity": item.quantity, "price": line["price"],
                            "deliveryDate": { "earliest": null, "latest": null, "apiAlias": "cart_delivery_date" },
                            "apiAlias": "cart_delivery_position",
                        })
                    })
                    .collect();
                deliveries.push(json!({
                    "deliveryDate": { "earliest": null, "latest": null, "apiAlias": "cart_delivery_date" },
                    "location": { "country": { "id": id::format(sc.country_id) }, "state": null, "address": null,
                                  "apiAlias": "cart_delivery_shipping_location" },
                    "positions": positions,
                    "shippingMethod": method,
                    "shippingCosts": cost.to_json(),
                    "apiAlias": "cart_delivery",
                }));
                shipping_costs.push(cost);
            }
            None => {
                let method = json!({ "id": id::format(sc.shipping_method_id), "translated": { "name": null } });
                cart_error(
                    &mut errors,
                    "shipping-method-blocked",
                    LEVEL_ERROR,
                    &method,
                    "Shipping method blocked".into(),
                );
            }
        }
    }

    let cart_price =
        CartPrice::new(&prices.iter().collect::<Vec<_>>(), &shipping_costs.iter().collect::<Vec<_>>(), &sc.price);
    let price_json = cart_price.to_json();
    let transactions = if items.is_empty() {
        Vec::new()
    } else {
        vec![json!({
            "paymentMethodId": id::format(sc.payment_method_id),
            "amount": {
                "unitPrice": cart_price.total_price,
                "quantity": 1,
                "totalPrice": cart_price.total_price,
                "calculatedTaxes": price_json["calculatedTaxes"],
                "taxRules": price_json["taxRules"],
                "referencePrice": null,
                "listPrice": null,
                "regulationPrice": null,
                "apiAlias": "calculated_price",
            },
            "apiAlias": "cart_transaction",
        })]
    };
    // Shopware renders an empty error collection as a list, a filled one as an object.
    let errors_json = if errors.is_empty() { json!([]) } else { json!(&errors) };
    let json = json!({
        "name": "sales-channel",
        "token": sc.token,
        "price": price_json,
        "lineItems": lines,
        "errors": errors_json,
        "deliveries": deliveries,
        "transactions": transactions,
        "modified": false,
        "customerComment": null,
        "affiliateCode": null,
        "campaignCode": null,
        "apiAlias": "cart",
    });
    Ok(Calculated { json, items, errors })
}

/// Loads, changes and recalculates the cart in one transaction, then stores the result.
async fn change(
    state: &StoreState,
    sc: &SalesChannelContext,
    apply: impl FnOnce(&mut Vec<StoredItem>) -> Result<(), ApiError>,
) -> Result<Json<Value>, ApiError> {
    let mut tx = state.db.begin().await?;
    let mut items = load(&mut tx, sc, true).await?;
    let before = items.clone();
    apply(&mut items)?;
    let calculated = calculate(state, sc, items).await?;
    if calculated.items != before {
        save(&mut tx, sc, &calculated.items).await?;
    }
    tx.commit().await?;
    Ok(Json(calculated.json))
}

/// `GET /store-api/checkout/cart`.
pub async fn show(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
) -> Result<Json<Value>, ApiError> {
    change(&state, &sc, |_| Ok(())).await
}

/// `DELETE /store-api/checkout/cart`.
pub async fn clear(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
) -> Result<Response, ApiError> {
    let mut conn = state.db.acquire().await?;
    save(&mut conn, &sc, &[]).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewItem {
    id: Option<String>,
    referenced_id: Option<String>,
    #[serde(rename = "type", default = "product_type")]
    kind: String,
    quantity: Option<i64>,
}

fn product_type() -> String {
    "product".into()
}

#[derive(Debug, Deserialize)]
pub struct ItemsBody<T> {
    items: Vec<T>,
}

fn invalid(index: usize, field: &str, code: &'static str, detail: String) -> WriteError {
    WriteError::new(format!("/items/{index}/{field}"), code, detail)
}

fn quantity(index: usize, raw: Option<i64>, errors: &mut Vec<WriteError>) -> u32 {
    match raw {
        None => 1,
        Some(q) if (1..=i64::from(u32::MAX)).contains(&q) => q as u32,
        Some(q) => {
            errors.push(invalid(
                index,
                "quantity",
                "CHECKOUT__CART_INVALID_LINE_ITEM_QUANTITY",
                format!("invalid quantity {q}"),
            ));
            1
        }
    }
}

/// `POST /store-api/checkout/cart/line-item`: adds products; adding one again raises its quantity.
pub async fn add(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let body: ItemsBody<NewItem> = guard::body(&body)?;
    let mut errors = Vec::new();
    let mut additions = Vec::new();
    for (index, item) in body.items.into_iter().enumerate() {
        let quantity = quantity(index, item.quantity, &mut errors);
        if item.kind != "product" {
            errors.push(invalid(
                index,
                "type",
                "CHECKOUT__CART_LINE_ITEM_TYPE_NOT_SUPPORTED",
                format!("line item type `{}` is not supported", item.kind),
            ));
            continue;
        }
        let Some(product_id) = item.referenced_id.as_deref().or(item.id.as_deref()).and_then(id::parse) else {
            errors.push(invalid(
                index,
                "referencedId",
                "CHECKOUT__CART_INVALID_LINE_ITEM",
                "a product id is required".into(),
            ));
            continue;
        };
        let line_id = item.id.unwrap_or_else(|| id::format(product_id));
        additions.push(StoredItem { id: line_id, referenced_id: id::format(product_id), kind: item.kind, quantity });
    }
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    change(&state, &sc, |items| {
        for addition in additions {
            match items.iter_mut().find(|i| i.id == addition.id) {
                Some(existing) => existing.quantity = existing.quantity.saturating_add(addition.quantity),
                None => items.push(addition),
            }
        }
        if items.len() > MAX_LINE_ITEMS {
            return Err(ApiError::BadRequest(format!("a cart holds at most {MAX_LINE_ITEMS} line items")));
        }
        Ok(())
    })
    .await
}

#[derive(Debug, Deserialize)]
pub struct ChangedItem {
    id: String,
    quantity: Option<i64>,
}

fn not_found(index: usize, line_id: &str) -> WriteError {
    invalid(index, "id", "CHECKOUT__CART_LINE_ITEM_NOT_FOUND", format!("line item `{line_id}` not found"))
}

/// `PATCH /store-api/checkout/cart/line-item`: changes quantities.
pub async fn update(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let body: ItemsBody<ChangedItem> = guard::body(&body)?;
    change(&state, &sc, |items| {
        let mut errors = Vec::new();
        for (index, changed) in body.items.iter().enumerate() {
            let quantity = quantity(index, changed.quantity, &mut errors);
            match items.iter_mut().find(|i| i.id == changed.id) {
                Some(item) => item.quantity = quantity,
                None => errors.push(not_found(index, &changed.id)),
            }
        }
        if errors.is_empty() { Ok(()) } else { Err(Error::Validation(errors).into()) }
    })
    .await
}

fn remove_ids(items: &mut Vec<StoredItem>, ids: &[String]) -> Result<(), ApiError> {
    let errors: Vec<WriteError> = ids
        .iter()
        .enumerate()
        .filter(|(_, line_id)| !items.iter().any(|i| &i.id == *line_id))
        .map(|(index, line_id)| {
            WriteError::new(
                format!("/ids/{index}"),
                "CHECKOUT__CART_LINE_ITEM_NOT_FOUND",
                format!("line item `{line_id}` not found"),
            )
        })
        .collect();
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    items.retain(|i| !ids.contains(&i.id));
    Ok(())
}

/// `DELETE /store-api/checkout/cart/line-item?ids[]=…` (Shopware 6.5 style).
pub async fn remove(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    RawQuery(query): RawQuery,
) -> Result<Json<Value>, ApiError> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query.as_deref().unwrap_or_default())
        .map_err(|e| ApiError::BadRequest(format!("invalid query string: {e}")))?;
    let ids: Vec<String> = pairs
        .into_iter()
        .filter(|(key, _)| key == "ids" || key == "ids[]" || key.starts_with("ids["))
        .flat_map(|(_, value)| value.split('|').map(str::to_string).collect::<Vec<_>>())
        .collect();
    change(&state, &sc, |items| remove_ids(items, &ids)).await
}

#[derive(Debug, Deserialize)]
pub struct DeleteBody {
    ids: Vec<String>,
}

/// `POST /store-api/checkout/cart/line-item/delete` (Shopware 6.6 style).
pub async fn remove_post(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let body: DeleteBody = guard::body(&body)?;
    change(&state, &sc, |items| remove_ids(items, &body.ids)).await
}
