//! Order state machines: order, payment and delivery state with named transitions,
//! following Shopware's `/_action/order/{id}/state/{transition}` style.

// `column` is one of three static column names, never user input.
use sqlx::{AssertSqlSafe, Row};
use uuid::Uuid;

use crate::Db;
use crate::context::Context;
use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    Order,
    Payment,
    Delivery,
}

impl Machine {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "state" => Some(Self::Order),
            "payment-state" => Some(Self::Payment),
            "delivery-state" => Some(Self::Delivery),
            _ => None,
        }
    }

    fn column(self) -> &'static str {
        match self {
            Self::Order => "state",
            Self::Payment => "payment_state",
            Self::Delivery => "delivery_state",
        }
    }

    pub fn api_name(self) -> &'static str {
        match self {
            Self::Order => "state",
            Self::Payment => "paymentState",
            Self::Delivery => "deliveryState",
        }
    }

    /// `(transition, from states, to state)`.
    fn transitions(self) -> &'static [(&'static str, &'static [&'static str], &'static str)] {
        match self {
            Self::Order => &[
                ("process", &["open"], "in_progress"),
                ("complete", &["in_progress"], "completed"),
                ("cancel", &["open", "in_progress"], "cancelled"),
                ("reopen", &["cancelled", "completed"], "open"),
            ],
            Self::Payment => &[
                ("pay", &["open"], "paid"),
                ("refund", &["paid"], "refunded"),
                ("cancel", &["open"], "cancelled"),
                ("reopen", &["cancelled", "refunded"], "open"),
            ],
            Self::Delivery => &[
                ("ship", &["open"], "shipped"),
                ("deliver", &["shipped"], "delivered"),
                ("retour", &["shipped", "delivered"], "returned"),
                ("cancel", &["open"], "cancelled"),
                ("reopen", &["cancelled", "returned"], "open"),
            ],
        }
    }

    /// Transitions available from `state`.
    pub fn available(self, state: &str) -> Vec<&'static str> {
        self.transitions().iter().filter(|(_, from, _)| from.contains(&state)).map(|(name, _, _)| *name).collect()
    }
}

/// Applies a transition and returns the new state.
pub async fn transition(db: &Db, order_id: Uuid, machine: Machine, name: &str, ctx: &Context) -> Result<String> {
    let column = machine.column();
    let mut tx = db.begin().await?;
    let current: String =
        sqlx::query(AssertSqlSafe(format!("SELECT {column} AS s FROM sales_order WHERE id = $1 FOR UPDATE")))
            .bind(order_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| Error::NotFound { entity: "order".into(), id: crate::id::format(order_id) })?
            .try_get("s")?;
    let Some((_, _, to)) =
        machine.transitions().iter().find(|(t, from, _)| *t == name && from.contains(&current.as_str()))
    else {
        return Err(Error::Conflict(format!(
            "transition `{name}` is not possible from {} `{current}`; available: {}",
            machine.api_name(),
            machine.available(&current).join(", ")
        )));
    };
    sqlx::query(AssertSqlSafe(format!("UPDATE sales_order SET {column} = $1, updated_at = now() WHERE id = $2")))
        .bind(*to)
        .bind(order_id)
        .execute(&mut *tx)
        .await?;
    if machine == Machine::Order {
        restock(&mut tx, order_id, &current, to).await?;
    }
    sqlx::query(
        "INSERT INTO audit_log (id, entity, entity_id, action, payload, actor_id) VALUES ($1, 'order', $2, 'state', $3, $4)",
    )
    .bind(crate::id::new_id())
    .bind(order_id)
    .bind(serde_json::json!({ "machine": machine.api_name(), "transition": name, "from": current, "to": to }))
    .bind(ctx.actor_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(to.to_string())
}

/// Cancelling an order that booked stock gives it back; reopening it books it again.
async fn restock(tx: &mut sqlx::PgConnection, order_id: Uuid, from: &str, to: &str) -> Result<()> {
    let (sign, expected, next) = match (from, to) {
        (_, "cancelled") => ("+", "booked", "released"),
        ("cancelled", _) => ("-", "released", "booked"),
        _ => return Ok(()),
    };
    let changed = sqlx::query("UPDATE sales_order SET stock_state = $2 WHERE id = $1 AND stock_state = $3")
        .bind(order_id)
        .bind(next)
        .bind(expected)
        .execute(&mut *tx)
        .await?;
    if changed.rows_affected() == 0 {
        return Ok(());
    }
    sqlx::query(AssertSqlSafe(format!(
        "UPDATE product p SET stock = p.stock {sign} li.quantity FROM \
         (SELECT product_id, sum(quantity)::int AS quantity FROM order_line_item \
          WHERE order_id = $1 AND product_id IS NOT NULL GROUP BY product_id) li WHERE p.id = li.product_id"
    )))
    .bind(order_id)
    .execute(&mut *tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn available_transitions() {
        assert_eq!(Machine::Order.available("open"), ["process", "cancel"]);
        assert_eq!(Machine::Payment.available("paid"), ["refund"]);
        assert!(Machine::Delivery.available("delivered").contains(&"retour"));
        assert_eq!(Machine::parse("payment-state"), Some(Machine::Payment));
    }
}
