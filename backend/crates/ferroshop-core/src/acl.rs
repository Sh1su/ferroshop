//! Permissions: admins may do everything, everyone else gets the union of their roles'
//! privileges. A privilege is `<entity>:<action>` (e.g. `product:update`) or `*`.

use std::collections::BTreeSet;

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::Db;
use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Read,
    Create,
    Update,
    Delete,
}

impl Action {
    pub const ALL: [Action; 4] = [Action::Read, Action::Create, Action::Update, Action::Delete];

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Read => "read",
            Action::Create => "create",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

/// Reference data every logged-in user may read; changing it still needs privileges.
pub const PUBLIC_READ: &[&str] = &[
    "language",
    "currency",
    "tax",
    "country",
    "customer_group",
    "payment_method",
    "shipping_method",
    "sales_channel",
    "media",
];

/// Entities that only exist as part of another one share its privileges
/// (`order:update` covers line items and order addresses).
pub fn owner(entity: &str) -> &str {
    match entity {
        "order_line_item" | "order_address" => "order",
        "customer_address" => "customer",
        "product_media" => "product",
        other => other,
    }
}

#[derive(Debug, Clone, Default)]
pub struct Permissions {
    pub admin: bool,
    pub privileges: BTreeSet<String>,
}

impl Permissions {
    pub fn full() -> Self {
        Self { admin: true, privileges: BTreeSet::new() }
    }

    pub fn allows(&self, entity: &str, action: Action) -> bool {
        let entity = owner(entity);
        self.admin
            || self.privileges.contains("*")
            || (action == Action::Read && PUBLIC_READ.contains(&entity))
            || self.privileges.contains(&format!("{entity}:{}", action.as_str()))
    }

    fn from_rows(admin: bool, privileges: impl IntoIterator<Item = Value>) -> Self {
        let privileges = privileges
            .into_iter()
            .filter_map(|v| v.as_array().cloned())
            .flatten()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
        Self { admin, privileges }
    }

    /// Loads an active user's permissions; `None` if the user is missing or inactive.
    pub async fn for_user(db: &Db, user_id: Uuid) -> Result<Option<Self>> {
        let Some(row) = sqlx::query("SELECT admin FROM admin_user WHERE id = $1 AND active")
            .bind(user_id)
            .fetch_optional(db)
            .await?
        else {
            return Ok(None);
        };
        let privileges: Vec<Value> = sqlx::query_scalar(
            "SELECT r.privileges FROM acl_role r JOIN acl_user_role ur ON ur.acl_role_id = r.id WHERE ur.user_id = $1",
        )
        .bind(user_id)
        .fetch_all(db)
        .await?;
        Ok(Some(Self::from_rows(row.try_get("admin")?, privileges)))
    }

    pub async fn for_integration(db: &Db, integration_id: Uuid) -> Result<Option<Self>> {
        let Some(row) =
            sqlx::query("SELECT admin FROM integration WHERE id = $1").bind(integration_id).fetch_optional(db).await?
        else {
            return Ok(None);
        };
        let privileges: Vec<Value> = sqlx::query_scalar(
            "SELECT r.privileges FROM acl_role r JOIN integration_role ir ON ir.acl_role_id = r.id \
             WHERE ir.integration_id = $1",
        )
        .bind(integration_id)
        .fetch_all(db)
        .await?;
        Ok(Some(Self::from_rows(row.try_get("admin")?, privileges)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn privileges() {
        let p = Permissions::from_rows(false, [json!(["product:read", "product:update"]), json!(["order:read"])]);
        assert!(p.allows("product", Action::Read));
        assert!(p.allows("order", Action::Read));
        assert!(!p.allows("product", Action::Delete));
        assert!(!p.allows("customer", Action::Read));
        assert!(Permissions::from_rows(false, [json!(["*"])]).allows("customer", Action::Delete));
        assert!(Permissions::full().allows("anything", Action::Create));
        assert!(p.allows("order_line_item", Action::Read), "owned entities follow their owner");
        assert!(p.allows("country", Action::Read), "reference data is readable");
        assert!(!p.allows("country", Action::Update));
    }
}
