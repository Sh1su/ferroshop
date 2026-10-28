//! What storefront visitors may read: a fixed set of catalogue entities, and of those only what is
//! active. Criteria are checked before they reach the data layer, including filters, sorting and
//! aggregations on associations (`manufacturer.products.customer...` must not reach customers).

use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::Filter;
use ferroshop_core::{Criteria, EntityDefinition, FieldKind, entities, id};
use serde_json::Value;

use crate::SalesChannelContext;

/// Hard upper bound for one Store-API page.
pub const MAX_LIMIT: u64 = 100;

const PUBLIC: &[&str] = &[
    "product",
    "product_manufacturer",
    "product_media",
    "media",
    "category",
    "tax",
    "currency",
    "language",
    "country",
    "payment_method",
    "shipping_method",
    "seo_url",
    "property_group",
    "property_group_option",
    "cms_page",
    "cms_section",
    "cms_block",
    "cms_slot",
];

/// What a logged-in customer may additionally read, always filtered to their own records.
pub const CUSTOMER_OWNED: &[&str] = &["order", "order_line_item", "order_address", "customer_address"];

fn allowed(name: &str, own: bool) -> bool {
    PUBLIC.contains(&name) || (own && CUSTOMER_OWNED.contains(&name))
}

pub fn definition(name: &str) -> &'static EntityDefinition {
    entities::get(name).expect("store-api entity is registered")
}

fn association_target(def: &EntityDefinition, name: &str) -> Option<&'static EntityDefinition> {
    def.field(name).and_then(|f| match f.kind {
        FieldKind::ManyToOne { entity, .. }
        | FieldKind::OneToMany { entity, .. }
        | FieldKind::ManyToMany { entity, .. } => entities::get(entity),
        _ => None,
    })
}

fn forbidden(def: &EntityDefinition, name: &str) -> ApiError {
    ApiError::Forbidden(format!("association `{name}` of `{}` is not available in the Store-API", def.name))
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

/// Every association a field path passes must lead to a public entity.
fn check_path(def: &'static EntityDefinition, path: &str, own: bool) -> Result<(), ApiError> {
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
            Some(target) if allowed(target.name, own) => current = target,
            Some(_) => return Err(forbidden(current, segment)),
            // Scalars and unknown fields are left to the SQL compiler.
            None => break,
        }
    }
    Ok(())
}

/// Validates a criteria (recursively through associations) against the public entities, plus
/// [`CUSTOMER_OWNED`] ones when `own` is set. The caller must
/// filter the root entity to the customer's records.
pub fn check_in(def: &'static EntityDefinition, criteria: &Criteria, own: bool) -> Result<(), ApiError> {
    let mut paths: Vec<&str> = Vec::new();
    for filter in criteria.filter.iter().chain(&criteria.post_filter) {
        filter_paths(filter, &mut paths);
    }
    paths.extend(criteria.sort.iter().map(|s| s.field.as_str()));
    paths.extend(criteria.aggregations.iter().map(|a| a.field.as_str()));
    for path in paths {
        check_path(def, path, own)?;
    }
    for (name, sub) in criteria.nested_associations() {
        match association_target(def, &name) {
            Some(target) if allowed(target.name, own) => check_in(target, &sub, own)?,
            Some(_) => return Err(forbidden(def, &name)),
            None => {
                return Err(ApiError::BadRequest(format!("association `{name}` does not exist on `{}`", def.name)));
            }
        }
    }
    Ok(())
}

fn equals(field: &str, value: Value) -> Filter {
    Filter::Equals { field: field.into(), value }
}

fn visibility(def: &EntityDefinition, sc: &SalesChannelContext, nested: bool) -> Vec<Filter> {
    match def.name {
        "product" | "category" | "country" | "payment_method" | "shipping_method" => {
            vec![equals("active", Value::Bool(true))]
        }
        // URLs of this sales channel and language; entities carry only their canonical URL,
        // while `/seo-url` also finds old ones (for redirects).
        "seo_url" => {
            let mut filters = vec![
                equals("salesChannelId", Value::String(id::format(sc.sales_channel_id))),
                equals("languageId", Value::String(id::format(sc.language_id))),
                equals("isDeleted", Value::Bool(false)),
            ];
            if nested {
                filters.push(equals("isCanonical", Value::Bool(true)));
            }
            filters
        }
        _ => Vec::new(),
    }
}

/// Adds visibility filters to the criteria and every loaded association. Call after [`check_in`].
pub fn restrict(def: &'static EntityDefinition, criteria: &mut Criteria, sc: &SalesChannelContext) {
    restrict_at(def, criteria, sc, false);
}

fn restrict_at(def: &'static EntityDefinition, criteria: &mut Criteria, sc: &SalesChannelContext, nested: bool) {
    criteria.filter.extend(visibility(def, sc, nested));
    let mut associations = criteria.nested_associations();
    for (name, sub) in associations.iter_mut() {
        if let Some(target) = association_target(def, name) {
            restrict_at(target, sub, sc, true);
        }
    }
    criteria.associations = associations;
}

/// Applies the Store-API page size: `default` when none is given, never more than [`MAX_LIMIT`].
pub fn limit(criteria: &mut Criteria, default: u64) {
    criteria.limit = Some(criteria.limit.unwrap_or(default).clamp(1, MAX_LIMIT));
}

/// Parses a criteria body; empty bodies (GET requests) mean "no criteria".
pub fn criteria(body: &[u8]) -> Result<Criteria, ApiError> {
    if body.iter().all(u8::is_ascii_whitespace) {
        return Ok(Criteria::default());
    }
    serde_json::from_slice(body).map_err(|e| ApiError::BadRequest(format!("invalid criteria: {e}")))
}

/// Parses a JSON request body into `T`, answering with Shopware's error format on failure.
pub fn body<T: serde::de::DeserializeOwned>(body: &[u8]) -> Result<T, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::BadRequest(format!("invalid request body: {e}")))
}
