//! Shopware price collections. Amounts are validated as decimals, never as floats.

use rust_decimal::{Decimal, RoundingStrategy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::defaults;

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

/// How prices are shown in a sales channel context.
#[derive(Debug, Clone)]
pub struct PriceContext {
    pub currency_id: Uuid,
    /// Exchange rate from the default currency, used when no price exists for `currency_id`.
    pub factor: Decimal,
    pub precision: u32,
    /// Gross (B2C) or net (B2B) display, from the customer group.
    pub gross: bool,
}

impl Default for PriceContext {
    fn default() -> Self {
        Self { currency_id: defaults::CURRENCY, factor: Decimal::ONE, precision: 2, gross: true }
    }
}

fn round(value: Decimal, precision: u32) -> Decimal {
    value.round_dp_with_strategy(precision, RoundingStrategy::MidpointAwayFromZero)
}

/// Shopware's `calculated_price` for a stored price collection, or `None` without a usable price.
pub fn calculate(prices: &Value, tax_rate: Decimal, quantity: u32, ctx: &PriceContext) -> Option<Value> {
    let prices: Vec<Price> = serde_json::from_value(prices.clone()).ok()?;
    let wanted = crate::id::format(ctx.currency_id);
    let (price, factor) = match prices.iter().find(|p| p.currency_id == wanted) {
        Some(p) => (p, Decimal::ONE),
        None => (prices.iter().find(|p| p.currency_id == crate::id::format(defaults::CURRENCY))?, ctx.factor),
    };
    let pick = |gross: Decimal, net: Decimal| round(if ctx.gross { gross } else { net } * factor, ctx.precision);
    let unit = pick(price.gross, price.net);
    let total = unit * Decimal::from(quantity);
    let rate = tax_rate / Decimal::ONE_HUNDRED;
    let tax = round(if ctx.gross { total - total / (Decimal::ONE + rate) } else { total * rate }, ctx.precision);
    let list_price = price.list_price.as_ref().map(|l| pick(l.gross, l.net)).filter(|list| *list > unit).map(|list| {
        json!({
            "price": list,
            "discount": unit - list,
            "percentage": round((list - unit) / list * Decimal::ONE_HUNDRED, 2),
            "apiAlias": "cart_list_price",
        })
    });
    Some(json!({
        "unitPrice": unit,
        "quantity": quantity,
        "totalPrice": total,
        "calculatedTaxes": [{ "tax": tax, "taxRate": tax_rate, "price": total, "apiAlias": "cart_tax" }],
        "taxRules": [{ "taxRate": tax_rate, "percentage": 100, "apiAlias": "cart_tax_rule" }],
        "referencePrice": null,
        "listPrice": list_price,
        "regulationPrice": null,
        "apiAlias": "calculated_price",
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_gross_net_and_foreign_currency() {
        let prices = json!([{
            "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca", "gross": 11.9, "net": 10, "linked": true,
            "listPrice": {"gross": 14.9, "net": 12.52, "linked": true}
        }]);
        let rate = Decimal::from(19);
        let gross = calculate(&prices, rate, 2, &PriceContext::default()).unwrap();
        assert_eq!(gross["unitPrice"], 11.9);
        assert_eq!(gross["totalPrice"], 23.8);
        assert_eq!(gross["calculatedTaxes"][0]["tax"], 3.8);
        assert_eq!(gross["listPrice"]["percentage"], 20.13);

        let net = calculate(&prices, rate, 1, &PriceContext { gross: false, ..Default::default() }).unwrap();
        assert_eq!(net["unitPrice"], 10.0);
        assert_eq!(net["calculatedTaxes"][0]["tax"], 1.9);

        let chf = PriceContext { currency_id: Uuid::nil(), factor: Decimal::new(105, 2), ..Default::default() };
        assert_eq!(calculate(&prices, rate, 1, &chf).unwrap()["unitPrice"], 12.5);
        assert!(calculate(&json!("nope"), rate, 1, &chf).is_none());
    }

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
