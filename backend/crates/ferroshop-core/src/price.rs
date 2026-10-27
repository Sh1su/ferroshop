//! Shopware price collections. Amounts are validated as decimals, never as floats.

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub currency_id: String,
    pub gross: Decimal,
    pub net: Decimal,
    pub linked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list_price: Option<ListPrice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListPrice {
    pub gross: Decimal,
    pub net: Decimal,
    #[serde(default)]
    pub linked: bool,
}

/// Validates and normalises a price collection; returns a human readable error on failure.
pub fn normalize(value: &Value) -> Result<Value, String> {
    let prices: Vec<Price> = serde_json::from_value(value.clone()).map_err(|e| format!("invalid price: {e}"))?;
    if prices.is_empty() {
        return Err("at least one price is required".into());
    }
    let mut seen = std::collections::HashSet::new();
    for price in &prices {
        let id = crate::id::parse(&price.currency_id)
            .ok_or_else(|| format!("invalid currencyId `{}`", price.currency_id))?;
        if !seen.insert(id) {
            return Err(format!("duplicate price for currency `{}`", price.currency_id));
        }
        if price.gross.is_sign_negative() || price.net.is_sign_negative() {
            return Err("prices must not be negative".into());
        }
    }
    let normalized: Vec<Price> = prices
        .into_iter()
        .map(|mut p| {
            p.currency_id = crate::id::format(crate::id::parse(&p.currency_id).expect("checked"));
            p
        })
        .collect();
    Ok(serde_json::to_value(normalized).expect("prices serialize"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn accepts_shopware_prices() {
        let v = normalize(&json!([{
            "currencyId": "b7d2554b-0ce8-47cd-82f3-ac9bd1c0dfca",
            "gross": 11.9, "net": 10, "linked": true,
            "listPrice": {"gross": 15, "net": 12.61, "linked": true}
        }]))
        .unwrap();
        assert_eq!(v[0]["currencyId"], "b7d2554b0ce847cd82f3ac9bd1c0dfca");
        assert_eq!(v[0]["gross"], 11.9);
    }

    #[test]
    fn rejects_invalid_prices() {
        assert!(normalize(&json!([])).is_err());
        assert!(normalize(&json!([{"currencyId": "x", "gross": 1, "net": 1, "linked": true}])).is_err());
        assert!(
            normalize(
                &json!([{"currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca", "gross": -1, "net": 1, "linked": true}])
            )
            .is_err()
        );
    }
}
