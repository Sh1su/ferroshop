//! Permission checks for the generic entity endpoints.

use ferroshop_core::acl::{Action, Permissions};
use ferroshop_core::criteria::Filter;
use ferroshop_core::definition::{EntityDefinition, FieldKind};
use ferroshop_core::{Criteria, Db, entities, id};
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

/// Reading through associations needs read access on every associated entity, whether they are
/// loaded or only used in filters, sorting or aggregations (`customer.email` on orders would
/// otherwise leak customer data to anyone with `order:read`).
pub fn require_criteria(perms: &Permissions, def: &EntityDefinition, criteria: &Criteria) -> Result<(), ApiError> {
    let mut paths: Vec<&str> = Vec::new();
    for filter in criteria.filter.iter().chain(&criteria.post_filter) {
        filter_paths(filter, &mut paths);
    }
    paths.extend(criteria.sort.iter().map(|s| s.field.as_str()));
    paths.extend(criteria.aggregations.iter().map(|a| a.field.as_str()));
    for path in paths {
        require_path(perms, def, path)?;
    }
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

fn filter_paths<'a>(filter: &'a Filter, out: &mut Vec<&'a str>) {
    match filter {
        Filter::Equals { field, .. }
        | Filter::EqualsAny { field, .. }
        | Filter::Contains { field, .. }
        | Filter::Prefix { field, .. }
        | Filter::Suffix { field, .. }
        | Filter::Range { field, .. } => out.push(field),
        Filter::Not { queries, .. } | Filter::Multi { queries, .. } => {
            queries.iter().for_each(|q| filter_paths(q, out));
        }
    }
}

/// Walks `manufacturer.translated.name` and requires read on every association it passes.
/// Unknown fields are left to the SQL compiler, which reports them as invalid criteria.
fn require_path(perms: &Permissions, def: &EntityDefinition, path: &str) -> Result<(), ApiError> {
    let mut current = def;
    let mut segments = path.split('.').peekable();
    if segments.peek() == Some(&def.name) {
        segments.next();
    }
    for segment in segments {
        if segment == "translated" {
            continue;
        }
        match association_target(current, segment) {
            Ok(target) => {
                require(perms, target.name, Action::Read)?;
                current = target;
            }
            Err(_) => break,
        }
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

/// Guards changes to an existing record: roles, and users or integrations with admin rights,
/// can only be updated, deleted or unlinked by administrators. Otherwise `user:update` would
/// allow setting an administrator's password and logging in as them.
pub async fn require_target(
    db: &Db,
    perms: &Permissions,
    def: &EntityDefinition,
    raw_id: &str,
) -> Result<(), ApiError> {
    if perms.admin {
        return Ok(());
    }
    let query = match def.name {
        "acl_role" => return Err(ApiError::Forbidden("only administrators can change roles".into())),
        "user" => "SELECT admin FROM admin_user WHERE id = $1",
        "integration" => "SELECT admin FROM integration WHERE id = $1",
        _ => return Ok(()),
    };
    // Unknown ids fall through to the regular not-found handling.
    let Some(target) = id::parse(raw_id) else { return Ok(()) };
    let admin: Option<bool> = sqlx::query_scalar(query).bind(target).fetch_optional(db).await?;
    if admin == Some(true) {
        return Err(ApiError::Forbidden("only administrators can change administrators".into()));
    }
    Ok(())
}

/// Nested writes need create+update on the nested entity. Granting admin rights or roles
/// is reserved for administrators, otherwise any `user:update` would allow escalation.
pub fn require_write(perms: &Permissions, def: &EntityDefinition, payload: &Value) -> Result<(), ApiError> {
    let Some(obj) = payload.as_object() else { return Ok(()) };
    // `admin: false` and `aclRoles: []` grant nothing (administrators themselves are guarded by
    // `require_target`), so forms that always send them keep working for non-admins.
    let sensitive = match def.name {
        "acl_role" => true,
        "user" | "integration" => {
            obj.get("admin").is_some_and(|v| *v != Value::Bool(false))
                || obj.get("aclRoles").is_some_and(|v| v.as_array().is_none_or(|roles| !roles.is_empty()))
        }
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
