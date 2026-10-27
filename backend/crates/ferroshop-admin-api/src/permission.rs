//! Permission checks for the generic entity endpoints.

use ferroshop_core::acl::{Action, Permissions};
use ferroshop_core::definition::{EntityDefinition, FieldKind};
use ferroshop_core::{Criteria, entities};
use serde_json::Value;

use crate::ApiError;

pub fn definition(entity: &str) -> Result<&'static EntityDefinition, ApiError> {
    entities::get(entity).ok_or_else(|| ferroshop_core::Error::UnknownEntity(entity.to_string()).into())
}

pub fn require(perms: &Permissions, entity: &str, action: Action) -> Result<(), ApiError> {
    if perms.allows(entity, action) {
        Ok(())
    } else {
        Err(ApiError::Forbidden(format!("missing privilege `{entity}:{}`", action.as_str())))
    }
}

/// Reading through associations needs read access on every associated entity.
pub fn require_criteria(perms: &Permissions, def: &EntityDefinition, criteria: &Criteria) -> Result<(), ApiError> {
    for (name, sub) in &criteria.associations {
        let mut current = def;
        for segment in name.split('.') {
            let target = association_target(current, segment)?;
            require(perms, target.name, Action::Read)?;
            current = target;
        }
        require_criteria(perms, current, sub)?;
    }
    Ok(())
}

fn association_target(def: &EntityDefinition, name: &str) -> Result<&'static EntityDefinition, ApiError> {
    let target = def.field(name).and_then(|f| match f.kind {
        FieldKind::ManyToOne { entity, .. }
        | FieldKind::OneToMany { entity, .. }
        | FieldKind::ManyToMany { entity, .. } => Some(entity),
        _ => None,
    });
    target
        .and_then(entities::get)
        .ok_or_else(|| ApiError::BadRequest(format!("association `{name}` does not exist on `{}`", def.name)))
}

/// Nested writes need create+update on the nested entity. Granting admin rights or roles
/// is reserved for administrators, otherwise any `user:update` would allow escalation.
pub fn require_write(perms: &Permissions, def: &EntityDefinition, payload: &Value) -> Result<(), ApiError> {
    let Some(obj) = payload.as_object() else { return Ok(()) };
    let sensitive = match def.name {
        "acl_role" => true,
        "user" | "integration" => obj.contains_key("admin") || obj.contains_key("aclRoles"),
        _ => false,
    };
    if sensitive && !perms.admin {
        return Err(ApiError::Forbidden("only administrators can change admin rights and roles".into()));
    }
    for (key, value) in obj {
        let Some(field) = def.field(key) else { continue };
        let nested: Vec<&Value> = match (field.kind, value) {
            (FieldKind::ManyToOne { .. }, Value::Object(_)) => vec![value],
            (FieldKind::OneToMany { .. }, Value::Array(items)) => items.iter().collect(),
            // Linking existing entities by id only needs update on the owning side.
            (FieldKind::ManyToMany { .. }, Value::Array(items)) => {
                items.iter().filter(|i| i.as_object().is_some_and(|o| o.len() > 1)).collect()
            }
            _ => continue,
        };
        if nested.is_empty() {
            continue;
        }
        let target = association_target(def, key)?;
        require(perms, target.name, Action::Create)?;
        require(perms, target.name, Action::Update)?;
        for item in nested {
            require_write(perms, target, item)?;
        }
    }
    Ok(())
}
