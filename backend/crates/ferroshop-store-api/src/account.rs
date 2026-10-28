//! Customer accounts: registration, login, profile, password, e-mail and addresses.
//!
//! The context token is the session. Login, logout and registration always issue a new token,
//! so a token handed out before login never becomes a logged-in session.

use std::sync::LazyLock;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::Filter;
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Criteria, Db, Error, WriteError, id, password, read};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::context::{CONTEXT_TOKEN, new_token};
use crate::{SalesChannelContext, StoreState, catalog, guard};

pub(crate) fn not_logged_in() -> ApiError {
    ApiError::Code(StatusCode::FORBIDDEN, "CHECKOUT__CUSTOMER_NOT_LOGGED_IN", "customer is not logged in".into())
}

pub(crate) fn require_customer(sc: &SalesChannelContext) -> Result<Uuid, ApiError> {
    sc.customer_id.ok_or_else(not_logged_in)
}

/// Registered (non-guest) customers must log in; guests only exist for one checkout.
async fn require_registered(state: &StoreState, sc: &SalesChannelContext) -> Result<Uuid, ApiError> {
    let customer_id = require_customer(sc)?;
    let guest: bool =
        sqlx::query_scalar("SELECT guest FROM customer WHERE id = $1").bind(customer_id).fetch_one(&state.db).await?;
    if guest {
        return Err(not_logged_in());
    }
    Ok(customer_id)
}

fn internal(sc: &SalesChannelContext, customer_id: Uuid) -> Context {
    Context { actor_id: Some(customer_id), ..sc.core() }
}

/// The customer as Shopware returns it, with default and active addresses resolved.
pub(crate) async fn customer_json(db: &Db, sc: &SalesChannelContext, customer_id: Uuid) -> Result<Value, ApiError> {
    let criteria: Criteria = serde_json::from_value(json!({ "associations": {
        "defaultBillingAddress": { "associations": { "country": {} } },
        "defaultShippingAddress": { "associations": { "country": {} } },
        "group": {},
        "salutation": {},
    }}))
    .expect("static criteria");
    let mut customer = read::read(db, "customer", &id::format(customer_id), &criteria, &sc.core()).await?;
    customer["activeBillingAddress"] = customer["defaultBillingAddress"].clone();
    customer["activeShippingAddress"] = customer["defaultShippingAddress"].clone();
    Ok(customer)
}

/// Starts a new session: a fresh token with the old context's choices, `customerId` set or
/// cleared. The old context row is removed so the previous token loses any login.
async fn rotate(
    db: &Db,
    sc: &SalesChannelContext,
    customer_id: Option<Uuid>,
    keep_cart: bool,
) -> Result<String, ApiError> {
    let token = new_token();
    let mut tx = db.begin().await?;
    let old: Option<Value> = sqlx::query_scalar(
        "DELETE FROM sales_channel_api_context WHERE token = $1 AND sales_channel_id = $2 RETURNING payload",
    )
    .bind(&sc.token)
    .bind(sc.sales_channel_id)
    .fetch_optional(&mut *tx)
    .await?;
    let mut payload = match old {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    match customer_id {
        Some(customer_id) => payload.insert("customerId".into(), json!(id::format(customer_id))),
        None => payload.remove("customerId"),
    };
    sqlx::query("INSERT INTO sales_channel_api_context (token, sales_channel_id, payload) VALUES ($1, $2, $3)")
        .bind(&token)
        .bind(sc.sales_channel_id)
        .bind(Value::Object(payload))
        .execute(&mut *tx)
        .await?;
    if keep_cart {
        sqlx::query("UPDATE cart SET token = $1, updated_at = now() WHERE token = $2 AND sales_channel_id = $3")
            .bind(&token)
            .bind(&sc.token)
            .bind(sc.sales_channel_id)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query("DELETE FROM cart WHERE token = $1 AND sales_channel_id = $2")
            .bind(&sc.token)
            .bind(sc.sales_channel_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(token)
}

fn with_token(token: &str, body: Value) -> Response {
    let mut response = Json(body).into_response();
    response.headers_mut().insert(CONTEXT_TOKEN, HeaderValue::from_str(token).expect("hex token"));
    response
}

fn violation(pointer: &str, code: &'static str, detail: &str) -> WriteError {
    WriteError::new(pointer, code, detail)
}

fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else { return false };
    !local.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !email.contains(' ')
}

async fn email_taken(db: &Db, email: &str, except: Option<Uuid>) -> Result<bool, ApiError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM customer WHERE lower(email) = lower($1) AND NOT guest \
         AND id IS DISTINCT FROM $2)",
    )
    .bind(email)
    .bind(except)
    .fetch_one(db)
    .await?)
}

fn not_specified() -> Value {
    json!(id::format(ferroshop_core::defaults::SALUTATION_NOT_SPECIFIED))
}

/// A valid salutation id, `fallback` when none is given; `null` (with an error) if invalid.
async fn salutation(
    db: &Db,
    raw: Option<&str>,
    fallback: Value,
    pointer: &str,
    errors: &mut Vec<WriteError>,
) -> Result<Value, ApiError> {
    let Some(raw) = raw.filter(|r| !r.is_empty()) else { return Ok(fallback) };
    let exists = match id::parse(raw) {
        Some(parsed) => sqlx::query_scalar::<_, bool>("SELECT EXISTS (SELECT 1 FROM salutation WHERE id = $1)")
            .bind(parsed)
            .fetch_one(db)
            .await?
            .then_some(parsed),
        None => None,
    };
    match exists {
        Some(parsed) => Ok(json!(id::format(parsed))),
        None => {
            errors.push(violation(pointer, "VIOLATION::ENTITY_DOES_NOT_EXIST", "unknown salutation"));
            Ok(Value::Null)
        }
    }
}

/// Address fields a shopper may set; everything else (customer, ids) is decided by the server.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AddressInput {
    first_name: Option<String>,
    last_name: Option<String>,
    company: Option<String>,
    street: Option<String>,
    additional_address_line1: Option<String>,
    zipcode: Option<String>,
    city: Option<String>,
    country_id: Option<String>,
    phone_number: Option<String>,
    salutation_id: Option<String>,
}

impl AddressInput {
    /// Write payload for the address; names and salutation default to the customer's.
    async fn payload(
        &self,
        db: &Db,
        pointer: &str,
        names: (&str, &str),
        default_salutation: &Value,
        errors: &mut Vec<WriteError>,
    ) -> Result<Map<String, Value>, ApiError> {
        let mut out = Map::new();
        let pointer_salutation = format!("{pointer}/salutationId");
        let salutation =
            salutation(db, self.salutation_id.as_deref(), default_salutation.clone(), &pointer_salutation, errors)
                .await?;
        out.insert("salutationId".into(), salutation);
        let country = self.country_id.as_deref().and_then(id::parse);
        let active: bool = match country {
            Some(country) => {
                sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM country WHERE id = $1 AND active)")
                    .bind(country)
                    .fetch_one(db)
                    .await?
            }
            None => false,
        };
        match country {
            Some(country) if active => {
                out.insert("countryId".into(), json!(id::format(country)));
            }
            _ => errors.push(violation(
                &format!("{pointer}/countryId"),
                "VIOLATION::ENTITY_DOES_NOT_EXIST",
                "a valid country is required",
            )),
        }
        let text = |v: &Option<String>| v.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
        for (key, value, required) in [
            ("firstName", text(&self.first_name).or_else(|| Some(names.0.to_string()).filter(|s| !s.is_empty())), true),
            ("lastName", text(&self.last_name).or_else(|| Some(names.1.to_string()).filter(|s| !s.is_empty())), true),
            ("street", text(&self.street), true),
            ("city", text(&self.city), true),
            ("zipcode", text(&self.zipcode), false),
            ("company", text(&self.company), false),
            ("additionalAddressLine1", text(&self.additional_address_line1), false),
            ("phoneNumber", text(&self.phone_number), false),
        ] {
            match value {
                Some(value) => {
                    out.insert(key.into(), json!(value));
                }
                None if required => errors.push(violation(
                    &format!("{pointer}/{key}"),
                    "VIOLATION::IS_BLANK_ERROR",
                    "this value should not be blank",
                )),
                None => {
                    out.insert(key.into(), Value::Null);
                }
            }
        }
        Ok(out)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Registration {
    salutation_id: Option<String>,
    title: Option<String>,
    first_name: String,
    last_name: String,
    email: String,
    password: Option<String>,
    guest: bool,
    company: Option<String>,
    billing_address: Option<AddressInput>,
    shipping_address: Option<AddressInput>,
}

/// `POST /store-api/account/register`: creates the customer and logs them in.
pub async fn register(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let input: Registration = guard::body(&body)?;
    let db = &state.db;
    let mut errors = Vec::new();
    let (first, last, email) = (input.first_name.trim(), input.last_name.trim(), input.email.trim());
    for (key, value) in [("firstName", first), ("lastName", last)] {
        if value.is_empty() {
            errors.push(violation(&format!("/{key}"), "VIOLATION::IS_BLANK_ERROR", "this value should not be blank"));
        }
    }
    if !valid_email(email) {
        errors.push(violation("/email", "VIOLATION::INVALID_EMAIL_FORMAT_ERROR", "not a valid e-mail address"));
    } else if !input.guest && email_taken(db, email, None).await? {
        errors.push(violation(
            "/email",
            "VIOLATION::CUSTOMER_EMAIL_NOT_UNIQUE",
            "e-mail address is already registered",
        ));
    }
    if !input.guest && input.password.as_deref().unwrap_or_default().chars().count() < password::MIN_LENGTH {
        let detail = format!("password needs at least {} characters", password::MIN_LENGTH);
        errors.push(violation("/password", "VIOLATION::TOO_SHORT_ERROR", &detail));
    }
    let salutation_id =
        salutation(db, input.salutation_id.as_deref(), not_specified(), "/salutationId", &mut errors).await?;
    let billing = match &input.billing_address {
        Some(address) => {
            Some(address.payload(db, "/billingAddress", (first, last), &salutation_id, &mut errors).await?)
        }
        None => {
            errors.push(violation("/billingAddress", "VIOLATION::IS_BLANK_ERROR", "a billing address is required"));
            None
        }
    };
    let shipping = match &input.shipping_address {
        Some(address) => {
            Some(address.payload(db, "/shippingAddress", (first, last), &salutation_id, &mut errors).await?)
        }
        None => None,
    };
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }

    let billing_id = id::format(id::new_id());
    let mut addresses = vec![];
    if let Some(mut billing) = billing {
        billing.insert("id".into(), json!(billing_id));
        addresses.push(Value::Object(billing));
    }
    let shipping_id = match shipping {
        Some(mut shipping) => {
            let shipping_id = id::format(id::new_id());
            shipping.insert("id".into(), json!(shipping_id));
            addresses.push(Value::Object(shipping));
            shipping_id
        }
        None => billing_id.clone(),
    };
    let mut payload = json!({
        "groupId": id::format(sc.customer_group_id),
        "salesChannelId": id::format(sc.sales_channel_id),
        "languageId": id::format(sc.language_id),
        "defaultPaymentMethodId": id::format(sc.payment_method_id),
        "salutationId": salutation_id,
        "title": input.title,
        "firstName": first,
        "lastName": last,
        "company": input.company,
        "email": email,
        "guest": input.guest,
        "active": true,
        "addresses": addresses,
        "defaultBillingAddressId": billing_id,
        "defaultShippingAddressId": shipping_id,
    });
    if !input.guest {
        payload["password"] = json!(input.password);
    }
    let customer_id = write::write(db, "customer", &payload, WriteMode::Create, &sc.core()).await?;
    let token = rotate(db, &sc, Some(customer_id), true).await?;
    let customer = customer_json(db, &sc, customer_id).await?;
    Ok(with_token(&token, customer))
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Login {
    username: String,
    password: String,
}

/// A valid hash to verify against for unknown users, so response times do not reveal accounts.
static DUMMY_HASH: LazyLock<String> = LazyLock::new(|| password::hash("ferroshop-dummy-password"));

/// `POST /store-api/account/login`: answers with a new context token for the customer.
pub async fn login(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let input: Login = guard::body(&body)?;
    let row = sqlx::query(
        "SELECT id, password, active FROM customer WHERE lower(email) = lower($1) AND NOT guest \
         AND sales_channel_id = $2",
    )
    .bind(input.username.trim())
    .bind(sc.sales_channel_id)
    .fetch_optional(&state.db)
    .await?;
    let stored: Option<String> = match &row {
        Some(row) => row.try_get("password")?,
        None => None,
    };
    let hash = stored.unwrap_or_else(|| DUMMY_HASH.clone());
    let (supplied, hash) = (input.password.clone(), hash);
    let matches = tokio::task::spawn_blocking(move || password::verify(&supplied, &hash))
        .await
        .map_err(|_| ApiError::BadRequest("login failed".into()))?;
    let customer = match row {
        Some(row) if matches && row.try_get::<bool, _>("active")? => row.try_get::<Uuid, _>("id")?,
        _ => {
            return Err(ApiError::Code(
                StatusCode::UNAUTHORIZED,
                "CHECKOUT__CUSTOMER_AUTH_BAD_CREDENTIALS",
                "invalid username and/or password".into(),
            ));
        }
    };
    sqlx::query("UPDATE customer SET last_login = now() WHERE id = $1").bind(customer).execute(&state.db).await?;
    let token = rotate(&state.db, &sc, Some(customer), true).await?;
    Ok(with_token(&token, json!({ "contextToken": token, "redirectUrl": null, "apiAlias": "array_struct" })))
}

/// `POST /store-api/account/logout`: ends the session and discards its cart.
pub async fn logout(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
) -> Result<Response, ApiError> {
    require_customer(&sc)?;
    let token = rotate(&state.db, &sc, None, false).await?;
    Ok(with_token(&token, json!({ "contextToken": token, "redirectUrl": null, "apiAlias": "array_struct" })))
}

/// `/store-api/account/customer`.
pub async fn customer(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    Ok(Json(customer_json(&state.db, &sc, customer_id).await?))
}

fn success() -> Json<Value> {
    Json(json!({ "success": true, "apiAlias": "array_struct" }))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Profile {
    salutation_id: Option<String>,
    title: Option<String>,
    first_name: String,
    last_name: String,
    company: Option<String>,
    birthday: Option<String>,
}

/// `POST /store-api/account/change-profile`.
pub async fn change_profile(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_registered(&state, &sc).await?;
    let input: Profile = guard::body(&body)?;
    let (first, last) = (input.first_name.trim(), input.last_name.trim());
    let mut errors = Vec::new();
    for (key, value) in [("firstName", first), ("lastName", last)] {
        if value.is_empty() {
            errors.push(violation(&format!("/{key}"), "VIOLATION::IS_BLANK_ERROR", "this value should not be blank"));
        }
    }
    let salutation_id =
        salutation(&state.db, input.salutation_id.as_deref(), not_specified(), "/salutationId", &mut errors).await?;
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    let payload = json!({
        "id": id::format(customer_id), "salutationId": salutation_id, "title": input.title, "firstName": first, "lastName": last,
        "company": input.company, "birthday": input.birthday.filter(|b| !b.is_empty()),
    });
    write::write(&state.db, "customer", &payload, WriteMode::Update, &internal(&sc, customer_id)).await?;
    Ok(success())
}

async fn check_password(state: &StoreState, customer_id: Uuid, supplied: &str) -> Result<(), ApiError> {
    let hash: Option<String> = sqlx::query_scalar("SELECT password FROM customer WHERE id = $1")
        .bind(customer_id)
        .fetch_one(&state.db)
        .await?;
    let (supplied, hash) = (supplied.to_string(), hash.unwrap_or_default());
    let ok = tokio::task::spawn_blocking(move || password::verify(&supplied, &hash)).await.unwrap_or(false);
    if ok {
        Ok(())
    } else {
        Err(Error::Validation(vec![violation(
            "/password",
            "VIOLATION::CUSTOMER_PASSWORD_NOT_CORRECT",
            "password is wrong",
        )])
        .into())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EmailChange {
    email: String,
    email_confirmation: String,
    password: String,
}

/// `POST /store-api/account/change-email`.
pub async fn change_email(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_registered(&state, &sc).await?;
    let input: EmailChange = guard::body(&body)?;
    check_password(&state, customer_id, &input.password).await?;
    let email = input.email.trim();
    let error = if !valid_email(email) {
        Some(violation("/email", "VIOLATION::INVALID_EMAIL_FORMAT_ERROR", "not a valid e-mail address"))
    } else if email != input.email_confirmation.trim() {
        Some(violation("/emailConfirmation", "VIOLATION::EQUAL_TO_ERROR", "e-mail addresses do not match"))
    } else if email_taken(&state.db, email, Some(customer_id)).await? {
        Some(violation("/email", "VIOLATION::CUSTOMER_EMAIL_NOT_UNIQUE", "e-mail address is already registered"))
    } else {
        None
    };
    if let Some(error) = error {
        return Err(Error::Validation(vec![error]).into());
    }
    let payload = json!({ "id": id::format(customer_id), "email": email });
    write::write(&state.db, "customer", &payload, WriteMode::Update, &internal(&sc, customer_id)).await?;
    Ok(success())
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PasswordChange {
    password: String,
    new_password: String,
    new_password_confirm: String,
}

/// `POST /store-api/account/change-password`: other sessions of the customer are ended.
pub async fn change_password(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_registered(&state, &sc).await?;
    let input: PasswordChange = guard::body(&body)?;
    check_password(&state, customer_id, &input.password).await?;
    if input.new_password != input.new_password_confirm {
        let error = violation("/newPasswordConfirm", "VIOLATION::EQUAL_TO_ERROR", "passwords do not match");
        return Err(Error::Validation(vec![error]).into());
    }
    let payload = json!({ "id": id::format(customer_id), "password": input.new_password });
    write::write(&state.db, "customer", &payload, WriteMode::Update, &internal(&sc, customer_id)).await.map_err(
        |err| match err {
            // The write layer reports the minimum length on `/password`.
            Error::Validation(mut list) => {
                list.iter_mut().for_each(|e| e.pointer = "/newPassword".into());
                Error::Validation(list)
            }
            other => other,
        },
    )?;
    sqlx::query("DELETE FROM sales_channel_api_context WHERE payload->>'customerId' = $1 AND token <> $2")
        .bind(id::format(customer_id))
        .bind(&sc.token)
        .execute(&state.db)
        .await?;
    Ok(success())
}

/// `POST /store-api/account/list-address`: the customer's addresses, with criteria.
pub async fn list_addresses(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let mut criteria = guard::criteria(&body)?;
    criteria.filter.push(Filter::Equals { field: "customerId".into(), value: json!(id::format(customer_id)) });
    let page = criteria.page;
    let limit = criteria.limit.unwrap_or(guard::MAX_LIMIT).clamp(1, guard::MAX_LIMIT);
    let result = catalog::search_own(&state, &sc, "customer_address", criteria, guard::MAX_LIMIT).await?;
    Ok(Json(catalog::search_result("customer_address", page, limit, result)))
}

/// Ensures the address belongs to the customer; answers 404 otherwise.
async fn own_address(state: &StoreState, customer_id: Uuid, raw: &str) -> Result<Uuid, ApiError> {
    let not_found = || ApiError::from(Error::NotFound { entity: "customer_address".into(), id: raw.into() });
    let address = id::parse(raw).ok_or_else(not_found)?;
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM customer_address WHERE id = $1 AND customer_id = $2)")
            .bind(address)
            .bind(customer_id)
            .fetch_one(&state.db)
            .await?;
    if owned { Ok(address) } else { Err(not_found()) }
}

/// Name and salutation new addresses start with.
async fn customer_names(state: &StoreState, customer_id: Uuid) -> Result<(String, String, Value), ApiError> {
    let row = sqlx::query("SELECT first_name, last_name, salutation_id FROM customer WHERE id = $1")
        .bind(customer_id)
        .fetch_one(&state.db)
        .await?;
    let salutation = row.try_get::<Option<Uuid>, _>("salutation_id")?.map(|s| json!(id::format(s)));
    Ok((row.try_get("first_name")?, row.try_get("last_name")?, salutation.unwrap_or_else(not_specified)))
}

async fn address_json(state: &StoreState, sc: &SalesChannelContext, address: Uuid) -> Result<Value, ApiError> {
    let criteria: Criteria =
        serde_json::from_value(json!({ "associations": { "country": {} } })).expect("static criteria");
    Ok(read::read(&state.db, "customer_address", &id::format(address), &criteria, &sc.core()).await?)
}

/// `POST /store-api/account/address`: adds an address.
pub async fn create_address(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let input: AddressInput = guard::body(&body)?;
    let names = customer_names(&state, customer_id).await?;
    let mut errors = Vec::new();
    let mut payload = input.payload(&state.db, "", (&names.0, &names.1), &names.2, &mut errors).await?;
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    payload.insert("customerId".into(), json!(id::format(customer_id)));
    let ctx = internal(&sc, customer_id);
    let address = write::write(&state.db, "customer_address", &Value::Object(payload), WriteMode::Create, &ctx).await?;
    Ok(Json(address_json(&state, &sc, address).await?))
}

/// `PATCH /store-api/account/address/{id}`: replaces an address's fields.
pub async fn update_address(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(&sc)?;
    let address = own_address(&state, customer_id, &raw).await?;
    let input: AddressInput = guard::body(&body)?;
    let names = customer_names(&state, customer_id).await?;
    let mut errors = Vec::new();
    let mut payload = input.payload(&state.db, "", (&names.0, &names.1), &names.2, &mut errors).await?;
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    payload.insert("id".into(), json!(id::format(address)));
    let ctx = internal(&sc, customer_id);
    write::write(&state.db, "customer_address", &Value::Object(payload), WriteMode::Update, &ctx).await?;
    Ok(Json(address_json(&state, &sc, address).await?))
}

/// `DELETE /store-api/account/address/{id}`: default addresses cannot be deleted.
pub async fn delete_address(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw): Path<String>,
) -> Result<Response, ApiError> {
    let customer_id = require_customer(&sc)?;
    let address = own_address(&state, customer_id, &raw).await?;
    let is_default: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM customer WHERE id = $1 \
         AND $2 IN (default_billing_address_id, default_shipping_address_id))",
    )
    .bind(customer_id)
    .bind(address)
    .fetch_one(&state.db)
    .await?;
    if is_default {
        return Err(ApiError::Code(
            StatusCode::BAD_REQUEST,
            "CHECKOUT__CUSTOMER_ADDRESS_IS_DEFAULT",
            "default addresses cannot be deleted".into(),
        ));
    }
    write::delete(&state.db, "customer_address", &[id::format(address)], &internal(&sc, customer_id)).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn set_default(
    state: &StoreState,
    sc: &SalesChannelContext,
    raw: &str,
    field: &str,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_customer(sc)?;
    let address = own_address(state, customer_id, raw).await?;
    let mut payload = Map::new();
    payload.insert("id".into(), json!(id::format(customer_id)));
    payload.insert(field.into(), json!(id::format(address)));
    let payload = Value::Object(payload);
    write::write(&state.db, "customer", &payload, WriteMode::Update, &internal(sc, customer_id)).await?;
    Ok(Json(json!(id::format(address))))
}

/// `PATCH /store-api/account/address/default-billing/{id}`.
pub async fn default_billing(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw): Path<String>,
) -> Result<Json<Value>, ApiError> {
    set_default(&state, &sc, &raw, "defaultBillingAddressId").await
}

/// `PATCH /store-api/account/address/default-shipping/{id}`.
pub async fn default_shipping(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw): Path<String>,
) -> Result<Json<Value>, ApiError> {
    set_default(&state, &sc, &raw, "defaultShippingAddressId").await
}

/// `POST /store-api/account/change-payment-method/{paymentMethodId}`: the customer's default.
pub async fn change_payment_method(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let customer_id = require_registered(&state, &sc).await?;
    let method = id::parse(&raw);
    let active: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM payment_method WHERE id = $1 AND active)")
        .bind(method)
        .fetch_one(&state.db)
        .await?;
    let Some(method) = method.filter(|_| active) else {
        return Err(ApiError::Code(
            StatusCode::BAD_REQUEST,
            "CHECKOUT__UNKNOWN_PAYMENT_METHOD",
            format!("unknown payment method `{raw}`"),
        ));
    };
    let payload = json!({ "id": id::format(customer_id), "defaultPaymentMethodId": id::format(method) });
    write::write(&state.db, "customer", &payload, WriteMode::Update, &internal(&sc, customer_id)).await?;
    Ok(success())
}
