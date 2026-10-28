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

/// A price for a quantity in a sales channel context (Shopware's `calculated_price`).
#[derive(Debug, Clone, PartialEq)]
pub struct CalculatedPrice {
    pub unit_price: Decimal,
    pub quantity: u32,
    pub total_price: Decimal,
    pub tax_rate: Decimal,
    /// Tax contained in (gross) or added to (net) `total_price`.
    pub tax: Decimal,
    pub list_price: Option<Decimal>,
}

impl CalculatedPrice {
    /// A price from an already converted unit price.
    pub fn from_unit(unit_price: Decimal, quantity: u32, tax_rate: Decimal, ctx: &PriceContext) -> Self {
        let total_price = unit_price * Decimal::from(quantity);
        let rate = tax_rate / Decimal::ONE_HUNDRED;
        let tax = if ctx.gross { total_price - total_price / (Decimal::ONE + rate) } else { total_price * rate };
        Self { unit_price, quantity, total_price, tax_rate, tax: round(tax, ctx.precision), list_price: None }
    }

    pub fn to_json(&self) -> Value {
        let list_price = self.list_price.filter(|list| *list > self.unit_price).map(|list| {
            json!({
                "price": list,
                "discount": self.unit_price - list,
                "percentage": round((list - self.unit_price) / list * Decimal::ONE_HUNDRED, 2),
                "apiAlias": "cart_list_price",
            })
        });
        json!({
            "unitPrice": self.unit_price,
            "quantity": self.quantity,
            "totalPrice": self.total_price,
            "calculatedTaxes": [{ "tax": self.tax, "taxRate": self.tax_rate, "price": self.total_price, "apiAlias": "cart_tax" }],
            "taxRules": [{ "taxRate": self.tax_rate, "percentage": 100, "apiAlias": "cart_tax_rule" }],
            "referencePrice": null,
            "listPrice": list_price,
            "regulationPrice": null,
            "apiAlias": "calculated_price",
        })
    }
}

/// Calculates a stored price collection for a quantity, or `None` without a usable price.
pub fn compute(prices: &Value, tax_rate: Decimal, quantity: u32, ctx: &PriceContext) -> Option<CalculatedPrice> {
    let prices: Vec<Price> = serde_json::from_value(prices.clone()).ok()?;
    let wanted = crate::id::format(ctx.currency_id);
    let (price, factor) = match prices.iter().find(|p| p.currency_id == wanted) {
        Some(p) => (p, Decimal::ONE),
        None => (prices.iter().find(|p| p.currency_id == crate::id::format(defaults::CURRENCY))?, ctx.factor),
    };
    let pick = |gross: Decimal, net: Decimal| round(if ctx.gross { gross } else { net } * factor, ctx.precision);
    let mut calculated = CalculatedPrice::from_unit(pick(price.gross, price.net), quantity, tax_rate, ctx);
    calculated.list_price = price.list_price.as_ref().map(|l| pick(l.gross, l.net));
    Some(calculated)
}

/// Shopware's `calculated_price` JSON for a stored price collection.
pub fn calculate(prices: &Value, tax_rate: Decimal, quantity: u32, ctx: &PriceContext) -> Option<Value> {
    compute(prices, tax_rate, quantity, ctx).map(|p| p.to_json())
}

/// Totals of a cart or order (Shopware's `cart_price`).
#[derive(Debug, Clone, PartialEq)]
pub struct CartPrice {
    pub net_price: Decimal,
    pub total_price: Decimal,
    pub position_price: Decimal,
    /// `(tax rate, tax, price the tax applies to)`, ascending by rate.
    pub taxes: Vec<(Decimal, Decimal, Decimal)>,
    pub gross: bool,
}

impl CartPrice {
    pub fn new(positions: &[&CalculatedPrice], shipping: &[&CalculatedPrice], ctx: &PriceContext) -> Self {
        let position_price: Decimal = positions.iter().map(|p| p.total_price).sum();
        let shipping_price: Decimal = shipping.iter().map(|p| p.total_price).sum();
        let mut by_rate: std::collections::BTreeMap<Decimal, (Decimal, Decimal)> = Default::default();
        for price in positions.iter().chain(shipping) {
            let entry = by_rate.entry(price.tax_rate.normalize()).or_default();
            entry.0 += price.tax;
            entry.1 += price.total_price;
        }
        let taxes: Vec<_> = by_rate.into_iter().map(|(rate, (tax, price))| (rate, tax, price)).collect();
        let tax_sum: Decimal = taxes.iter().map(|t| t.1).sum();
        let (net_price, total_price) = if ctx.gross {
            (position_price + shipping_price - tax_sum, position_price + shipping_price)
        } else {
            (position_price + shipping_price, position_price + shipping_price + tax_sum)
        };
        Self { net_price, total_price, position_price, taxes, gross: ctx.gross }
    }

    pub fn to_json(&self) -> Value {
        let base: Decimal = self.taxes.iter().map(|t| t.2).sum();
        let rules: Vec<Value> = self
            .taxes
            .iter()
            .map(|(rate, _, price)| {
                let share =
                    if base.is_zero() { Decimal::ONE_HUNDRED } else { round(*price / base * Decimal::ONE_HUNDRED, 2) };
                json!({ "taxRate": rate, "percentage": share, "apiAlias": "cart_tax_rule" })
            })
            .collect();
        let taxes: Vec<Value> = self
            .taxes
            .iter()
            .map(|(rate, tax, price)| json!({ "tax": tax, "taxRate": rate, "price": price, "apiAlias": "cart_tax" }))
            .collect();
        json!({
            "netPrice": self.net_price,
            "totalPrice": self.total_price,
            "calculatedTaxes": taxes,
            "taxRules": rules,
            "positionPrice": self.position_price,
            "rawTotal": self.total_price,
            "taxStatus": if self.gross { "gross" } else { "net" },
            "apiAlias": "cart_price",
        })
    }
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
    fn cart_totals_group_taxes_by_rate() {
        let ctx = PriceContext::default();
        let a = CalculatedPrice::from_unit(Decimal::new(119, 1), 2, Decimal::from(19), &ctx);
        let b = CalculatedPrice::from_unit(Decimal::new(107, 1), 1, Decimal::from(7), &ctx);
        let shipping = CalculatedPrice::from_unit(Decimal::new(499, 2), 1, Decimal::from(19), &ctx);
        let cart = CartPrice::new(&[&a, &b], &[&shipping], &ctx);
        assert_eq!(cart.position_price, Decimal::new(345, 1));
        assert_eq!(cart.total_price, Decimal::new(3949, 2));
        assert_eq!(
            cart.taxes,
            vec![
                (Decimal::from(7), Decimal::new(70, 2), Decimal::new(107, 1)),
                (Decimal::from(19), Decimal::new(460, 2), Decimal::new(2879, 2)),
            ]
        );
        assert_eq!(cart.net_price, Decimal::new(3419, 2));

        let net = PriceContext { gross: false, ..Default::default() };
        let line = CalculatedPrice::from_unit(Decimal::from(10), 3, Decimal::from(19), &net);
        let cart = CartPrice::new(&[&line], &[], &net);
        assert_eq!((cart.net_price, cart.total_price), (Decimal::from(30), Decimal::new(357, 1)));
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
