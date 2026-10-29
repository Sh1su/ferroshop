//! Adds what storefronts read from every product in a Store-API response: Shopware's calculated
//! prices and the availability fields (`available`, `availableStock`).

use std::collections::HashMap;

use ferroshop_admin_api::ApiError;
use ferroshop_core::price;
use ferroshop_core::{Db, id};
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::Row;

use crate::SalesChannelContext;

fn is_product(value: &Value) -> bool {
    value.get("apiAlias").and_then(Value::as_str) == Some("product")
}

fn collect_tax_ids(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            if is_product(value)
                && let Some(tax_id) = map.get("taxId").and_then(Value::as_str)
            {
                out.push(tax_id.to_string());
            }
            map.values().for_each(|v| collect_tax_ids(v, out));
        }
        Value::Array(items) => items.iter().for_each(|v| collect_tax_ids(v, out)),
        _ => {}
    }
}

pub(crate) async fn tax_rates(db: &Db, data: &[Value], country_id: uuid::Uuid) -> Result<HashMap<String, Decimal>, ApiError> {
    let mut ids = Vec::new();
    data.iter().for_each(|row| collect_tax_ids(row, &mut ids));
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows =
        sqlx::query("SELECT t.id, COALESCE(r.tax_rate, t.tax_rate) AS tax_rate FROM tax t LEFT JOIN LATERAL (SELECT tax_rate FROM tax_rule WHERE tax_id = t.id AND country_id = $2 ORDER BY priority DESC, id ASC LIMIT 1) r ON true WHERE t.id = ANY($1::text[]::uuid[])").bind(&ids).bind(country_id).fetch_all(db).await?;
    rows.into_iter()
        .map(|r| Ok((id::format(r.try_get("id")?), r.try_get("tax_rate")?)))
        .collect::<Result<_, sqlx::Error>>()
        .map_err(Into::into)
}

fn apply(value: &mut Value, rates: &HashMap<String, Decimal>, sc: &SalesChannelContext) {
    match value {
        Value::Object(map) => {
            map.values_mut().for_each(|v| apply(v, rates, sc));
            if map.get("apiAlias").and_then(Value::as_str) != Some("product") {
                return;
            }
            let rate = map.get("taxId").and_then(Value::as_str).and_then(|t| rates.get(t)).copied().unwrap_or_default();
            let calculated =
                map.get("price").and_then(|p| price::calculate(p, rate, 1, &sc.price)).unwrap_or(Value::Null);
            // Stock is booked when an order is placed, so all of it is available. Closeout
            // products can only be bought while the minimum purchase is in stock.
            let stock = map.get("stock").and_then(Value::as_i64).unwrap_or(0);
            let min_purchase = map.get("minPurchase").and_then(Value::as_i64).unwrap_or(1).max(1);
            let closeout = map.get("isCloseout").and_then(Value::as_bool).unwrap_or(false);
            map.insert("availableStock".into(), Value::from(stock));
            map.insert("available".into(), Value::Bool(!closeout || stock >= min_purchase));
            map.insert("calculatedPrices".into(), Value::Array(Vec::new()));
            map.insert("calculatedCheapestPrice".into(), calculated.clone());
            map.insert("calculatedPrice".into(), calculated);
        }
        Value::Array(items) => items.iter_mut().for_each(|v| apply(v, rates, sc)),
        _ => {}
    }
}

/// Sets `calculatedPrice`, `calculatedPrices`, `calculatedCheapestPrice`, `available` and
/// `availableStock` on all products, nested ones included. Must run before `includes` strip the
/// fields they are computed from.
pub async fn enrich(db: &Db, sc: &SalesChannelContext, data: &mut [Value]) -> Result<(), ApiError> {
    let rates = tax_rates(db, data, sc.country_id).await?;
    data.iter_mut().for_each(|row| apply(row, &rates, sc));
    Ok(())
}
