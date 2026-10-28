//! Adds Shopware's calculated prices to every product in a Store-API response.

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

async fn tax_rates(db: &Db, data: &[Value]) -> Result<HashMap<String, Decimal>, ApiError> {
    let mut ids = Vec::new();
    data.iter().for_each(|row| collect_tax_ids(row, &mut ids));
    ids.sort();
    ids.dedup();
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows =
        sqlx::query("SELECT id, tax_rate FROM tax WHERE id = ANY($1::text[]::uuid[])").bind(&ids).fetch_all(db).await?;
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
            map.insert("calculatedPrices".into(), Value::Array(Vec::new()));
            map.insert("calculatedCheapestPrice".into(), calculated.clone());
            map.insert("calculatedPrice".into(), calculated);
        }
        Value::Array(items) => items.iter_mut().for_each(|v| apply(v, rates, sc)),
        _ => {}
    }
}

/// Sets `calculatedPrice`, `calculatedPrices` and `calculatedCheapestPrice` on all products,
/// nested ones included. Must run before `includes` strip `price` and `taxId`.
pub async fn enrich(db: &Db, sc: &SalesChannelContext, data: &mut [Value]) -> Result<(), ApiError> {
    let rates = tax_rates(db, data).await?;
    data.iter_mut().for_each(|row| apply(row, &rates, sc));
    Ok(())
}
