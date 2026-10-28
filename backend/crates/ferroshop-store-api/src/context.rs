//! Sales channel context: resolved from `sw-access-key`, `sw-context-token` and `sw-language-id`.

use axum::Json;
use axum::extract::{Extension, Request, State};
use axum::http::HeaderValue;
use axum::middleware::Next;
use axum::response::Response;
use ferroshop_admin_api::ApiError;
use ferroshop_core::price::PriceContext;
use ferroshop_core::{Context, Criteria, Db, Error, WriteError, defaults, id, read};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::StoreState;

pub const CONTEXT_TOKEN: &str = "sw-context-token";

/// Everything a Store-API request needs to know about who is shopping where.
#[derive(Debug, Clone)]
pub struct SalesChannelContext {
    pub token: String,
    pub sales_channel_id: Uuid,
    pub navigation_category_id: Option<Uuid>,
    pub language_id: Uuid,
    pub customer_group_id: Uuid,
    pub country_id: Uuid,
    pub payment_method_id: Uuid,
    pub shipping_method_id: Uuid,
    pub price: PriceContext,
    /// The logged-in (or guest) customer.
    pub customer_id: Option<Uuid>,
}

impl SalesChannelContext {
    /// The data-layer context: content language and variant inheritance, as storefronts expect.
    pub fn core(&self) -> Context {
        Context { language_id: self.language_id, inheritance: true, actor_id: None }
    }
}

pub(crate) fn new_token() -> String {
    hex::encode(rand::random::<[u8; 16]>())
}

fn payload_id(payload: &Value, key: &str, fallback: Uuid) -> Uuid {
    payload.get(key).and_then(Value::as_str).and_then(id::parse).unwrap_or(fallback)
}

async fn load(
    db: &Db,
    access_key: &str,
    token: Option<&str>,
    language: Option<&str>,
) -> Result<SalesChannelContext, ApiError> {
    let channel = sqlx::query(
        "SELECT id, navigation_category_id, language_id, currency_id, customer_group_id, country_id, \
         payment_method_id, shipping_method_id FROM sales_channel WHERE access_key = $1 AND active",
    )
    .bind(access_key)
    .fetch_optional(db)
    .await?
    .ok_or(ApiError::Unauthorized("invalid sw-access-key"))?;
    let sales_channel_id: Uuid = channel.try_get("id")?;

    // A token from another sales channel is never reused; unknown tokens start with defaults.
    let mut payload = Value::Null;
    let token = match token.filter(|t| !t.is_empty() && t.len() <= 128) {
        Some(token) => {
            let stored =
                sqlx::query("SELECT sales_channel_id, payload FROM sales_channel_api_context WHERE token = $1")
                    .bind(token)
                    .fetch_optional(db)
                    .await?;
            match stored {
                Some(row) if row.try_get::<Uuid, _>("sales_channel_id")? != sales_channel_id => new_token(),
                Some(row) => {
                    payload = row.try_get("payload")?;
                    token.to_string()
                }
                None => token.to_string(),
            }
        }
        None => new_token(),
    };

    let language_id = match language {
        Some(raw) => id::parse(raw).ok_or_else(|| ApiError::BadRequest("invalid sw-language-id".into()))?,
        None => payload_id(&payload, "languageId", channel.try_get("language_id")?),
    };
    let currency_id = payload_id(&payload, "currencyId", channel.try_get("currency_id")?);

    // A customer's group, default payment method and shipping country replace the channel's
    // defaults; explicit choices in the context still win.
    let customer_id = payload.get("customerId").and_then(Value::as_str).and_then(id::parse);
    let customer = match customer_id {
        Some(customer_id) => {
            sqlx::query(
                "SELECT c.id, c.group_id, \
                 (SELECT p.id FROM payment_method p WHERE p.id = c.default_payment_method_id AND p.active) AS payment, \
                 (SELECT a.country_id FROM customer_address a WHERE a.id = c.default_shipping_address_id) AS country \
                 FROM customer c WHERE c.id = $1 AND c.sales_channel_id = $2 AND c.active",
            )
            .bind(customer_id)
            .bind(sales_channel_id)
            .fetch_optional(db)
            .await?
        }
        None => None,
    };
    let customer_default = |column: &str, fallback: Uuid| -> Result<Uuid, sqlx::Error> {
        Ok(match &customer {
            Some(row) => row.try_get::<Option<Uuid>, _>(column)?.unwrap_or(fallback),
            None => fallback,
        })
    };
    let customer_group_id = customer_default("group_id", channel.try_get("customer_group_id")?)?;
    let country_default = customer_default("country", channel.try_get("country_id")?)?;
    let payment_default = customer_default("payment", channel.try_get("payment_method_id")?)?;
    let details = sqlx::query(
        "SELECT (SELECT factor FROM currency WHERE id = $1) AS factor, \
         (SELECT decimal_precision FROM currency WHERE id = $1) AS precision, \
         (SELECT display_gross FROM customer_group WHERE id = $2) AS gross, \
         EXISTS (SELECT 1 FROM language WHERE id = $3) AS language_exists",
    )
    .bind(currency_id)
    .bind(customer_group_id)
    .bind(language_id)
    .fetch_one(db)
    .await?;
    if !details.try_get::<bool, _>("language_exists")? {
        return Err(ApiError::BadRequest("invalid sw-language-id".into()));
    }
    let factor: Option<Decimal> = details.try_get("factor")?;
    // A currency deleted after it was chosen falls back to the default currency.
    let price = match factor {
        Some(factor) => PriceContext {
            currency_id,
            factor,
            precision: details.try_get::<Option<i32>, _>("precision")?.unwrap_or(2).max(0) as u32,
            gross: details.try_get::<Option<bool>, _>("gross")?.unwrap_or(true),
        },
        None => PriceContext::default(),
    };

    Ok(SalesChannelContext {
        token,
        sales_channel_id,
        navigation_category_id: channel.try_get("navigation_category_id")?,
        language_id,
        customer_group_id,
        country_id: payload_id(&payload, "countryId", country_default),
        payment_method_id: payload_id(&payload, "paymentMethodId", payment_default),
        shipping_method_id: payload_id(&payload, "shippingMethodId", channel.try_get("shipping_method_id")?),
        price,
        customer_id: match &customer {
            Some(row) => Some(row.try_get("id")?),
            None => None,
        },
    })
}

/// Middleware: resolves the context for every route and echoes the context token.
pub async fn resolve(State(state): State<StoreState>, mut request: Request, next: Next) -> Result<Response, ApiError> {
    let headers = request.headers();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (access_key, token, language) = (header("sw-access-key"), header(CONTEXT_TOKEN), header("sw-language-id"));
    let access_key = access_key.ok_or(ApiError::Unauthorized("missing sw-access-key"))?;
    let ctx = load(&state.db, &access_key, token.as_deref(), language.as_deref()).await?;
    let token = HeaderValue::from_str(&ctx.token).expect("hex token");
    request.extensions_mut().insert(ctx);
    let mut response = next.run(request).await;
    // Login, logout and registration answer with a new token of their own.
    if !response.headers().contains_key(CONTEXT_TOKEN) {
        response.headers_mut().insert(CONTEXT_TOKEN, token);
    }
    Ok(response)
}

async fn entity(db: &Db, name: &str, entity_id: Uuid, ctx: &Context) -> Result<Value, ApiError> {
    match read::read(db, name, &id::format(entity_id), &Criteria::default(), ctx).await {
        Ok(value) => Ok(value),
        Err(Error::NotFound { .. }) => Ok(Value::Null),
        Err(err) => Err(err.into()),
    }
}

/// `GET /store-api/context`: Shopware's `sales_channel_context`.
pub async fn show(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
) -> Result<Json<Value>, ApiError> {
    let ctx = sc.core();
    let db = &state.db;
    let group = entity(db, "customer_group", sc.customer_group_id, &ctx).await?;
    let language = entity(db, "language", sc.language_id, &ctx).await?;
    let rounding = json!({
        "decimals": sc.price.precision,
        "interval": Decimal::new(1, sc.price.precision),
        "roundForNet": true,
        "apiAlias": "shopware_core_framework_data_abstraction_layer_pricing_cash_rounding_config",
    });
    let tax_state = if sc.price.gross { "gross" } else { "net" };
    let mut chain = vec![id::format(sc.language_id)];
    if sc.language_id != defaults::LANGUAGE_SYSTEM {
        chain.push(id::format(defaults::LANGUAGE_SYSTEM));
    }
    Ok(Json(json!({
        "token": sc.token,
        "currentCustomerGroup": group,
        "fallbackCustomerGroup": group,
        "currency": entity(db, "currency", sc.price.currency_id, &ctx).await?,
        "salesChannel": entity(db, "sales_channel", sc.sales_channel_id, &ctx).await?,
        "customer": match sc.customer_id {
            Some(customer_id) => crate::account::customer_json(db, &sc, customer_id).await?,
            None => Value::Null,
        },
        "paymentMethod": entity(db, "payment_method", sc.payment_method_id, &ctx).await?,
        "shippingMethod": entity(db, "shipping_method", sc.shipping_method_id, &ctx).await?,
        "shippingLocation": {
            "country": entity(db, "country", sc.country_id, &ctx).await?,
            "state": null,
            "address": null,
            "apiAlias": "cart_delivery_shipping_location",
        },
        "taxState": tax_state,
        "itemRounding": rounding,
        "totalRounding": rounding,
        "context": {
            "languageIdChain": chain,
            "versionId": id::format(defaults::LIVE_VERSION),
            "currencyId": id::format(sc.price.currency_id),
            "currencyFactor": sc.price.factor,
            "currencyPrecision": sc.price.precision,
            "taxState": tax_state,
            "scope": "user",
            "apiAlias": "context",
        },
        "languageInfo": {
            "name": language["name"],
            "localeCode": language["localeCode"],
            "apiAlias": "sales_channel_context_language_info",
        },
        "apiAlias": "sales_channel_context",
    })))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContextUpdate {
    language_id: Option<String>,
    currency_id: Option<String>,
    country_id: Option<String>,
    payment_method_id: Option<String>,
    shipping_method_id: Option<String>,
}

/// `PATCH /store-api/context`: switches language, currency, country, payment or shipping method.
pub async fn update(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Option<Json<ContextUpdate>>,
) -> Result<Json<Value>, ApiError> {
    let body = body.map(|Json(b)| b).unwrap_or_default();
    let checks = [
        ("languageId", &body.language_id, "SELECT EXISTS (SELECT 1 FROM language WHERE id = $1)"),
        ("currencyId", &body.currency_id, "SELECT EXISTS (SELECT 1 FROM currency WHERE id = $1)"),
        ("countryId", &body.country_id, "SELECT EXISTS (SELECT 1 FROM country WHERE id = $1 AND active)"),
        (
            "paymentMethodId",
            &body.payment_method_id,
            "SELECT EXISTS (SELECT 1 FROM payment_method WHERE id = $1 AND active)",
        ),
        (
            "shippingMethodId",
            &body.shipping_method_id,
            "SELECT EXISTS (SELECT 1 FROM shipping_method WHERE id = $1 AND active)",
        ),
    ];
    let mut changes = Map::new();
    let mut errors = Vec::new();
    for (key, value, query) in checks {
        let Some(raw) = value else { continue };
        let exists = match id::parse(raw) {
            Some(parsed) => sqlx::query_scalar::<_, bool>(query).bind(parsed).fetch_one(&state.db).await?,
            None => false,
        };
        match (exists, id::parse(raw)) {
            (true, Some(parsed)) => {
                changes.insert(key.into(), Value::String(id::format(parsed)));
            }
            _ => errors.push(WriteError::new(
                format!("/{key}"),
                "VIOLATION::ENTITY_DOES_NOT_EXIST",
                format!("`{raw}` is not a valid {key}"),
            )),
        }
    }
    if !errors.is_empty() {
        return Err(Error::Validation(errors).into());
    }
    if !changes.is_empty() {
        sqlx::query(
            "INSERT INTO sales_channel_api_context (token, sales_channel_id, payload) VALUES ($1, $2, $3) \
             ON CONFLICT (token) DO UPDATE SET payload = sales_channel_api_context.payload || EXCLUDED.payload, \
             updated_at = now() WHERE sales_channel_api_context.sales_channel_id = EXCLUDED.sales_channel_id",
        )
        .bind(&sc.token)
        .bind(sc.sales_channel_id)
        .bind(Value::Object(changes))
        .execute(&state.db)
        .await?;
    }
    Ok(Json(json!({ "contextToken": sc.token, "redirectUrl": null, "apiAlias": "array_struct" })))
}
