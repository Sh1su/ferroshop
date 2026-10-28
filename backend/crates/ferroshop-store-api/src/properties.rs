//! Properties and variants: the configurator on product pages, finding a variant for chosen
//! options, `sortedProperties`, and property filters and aggregations in listings.

use std::collections::{BTreeSet, HashMap};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::Filter;
use ferroshop_core::{Criteria, Error, id, read};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::{SalesChannelContext, StoreState, catalog, guard};

/// Most products whose options feed a listing's property aggregation.
const AGGREGATION_SCOPE: u64 = 10_000;

/// Loads options with their group, in the context language (in pages of the Store-API limit).
async fn options(state: &StoreState, sc: &SalesChannelContext, ids: Vec<String>) -> Result<Vec<Value>, ApiError> {
    let mut out = Vec::new();
    for chunk in ids.chunks(guard::MAX_LIMIT as usize) {
        let mut criteria = Criteria::with_ids(chunk.to_vec());
        criteria.associations.insert("group".into(), Criteria::default());
        criteria.associations.insert("media".into(), Criteria::default());
        out.extend(catalog::search(state, sc, "property_group_option", criteria, guard::MAX_LIMIT).await?.data);
    }
    Ok(out)
}

fn name(value: &Value) -> String {
    value["translated"]["name"].as_str().unwrap_or_default().to_lowercase()
}

/// Groups options under their group: groups by position and name, options as the group sorts.
fn grouped(options: Vec<Value>) -> Vec<Value> {
    let mut groups: Vec<Value> = Vec::new();
    for mut option in options {
        let group = option.as_object_mut().and_then(|o| o.remove("group")).unwrap_or(Value::Null);
        let Some(group_id) = group["id"].as_str().map(str::to_string) else { continue };
        match groups.iter_mut().find(|g| g["id"] == group_id.as_str()) {
            Some(existing) => existing["options"].as_array_mut().expect("options array").push(option),
            None => {
                let mut group = group;
                group["options"] = json!([option]);
                groups.push(group);
            }
        }
    }
    for group in &mut groups {
        let by_position = group["sortingType"] == "position";
        let options = group["options"].as_array_mut().expect("options array");
        options.sort_by(|a, b| {
            let position = |v: &Value| v["position"].as_i64().unwrap_or(0);
            if by_position {
                position(a).cmp(&position(b)).then_with(|| name(a).cmp(&name(b)))
            } else {
                name(a).cmp(&name(b))
            }
        });
    }
    groups.sort_by(|a, b| {
        let position = |v: &Value| v["position"].as_i64().unwrap_or(0);
        position(a).cmp(&position(b)).then_with(|| name(a).cmp(&name(b)))
    });
    groups
}

/// Options of every active variant of `parent`, by variant (in product number order).
async fn variant_options(state: &StoreState, parent: Uuid) -> Result<Vec<(String, BTreeSet<String>)>, ApiError> {
    let rows = sqlx::query(
        "SELECT p.id, p.product_number, po.option_id FROM product p \
         JOIN product_option po ON po.product_id = p.id \
         WHERE p.parent_id = $1 AND COALESCE(p.active, (SELECT pp.active FROM product pp WHERE pp.id = $1)) \
         ORDER BY p.product_number",
    )
    .bind(parent)
    .fetch_all(&state.db)
    .await?;
    let mut variants: Vec<(String, BTreeSet<String>)> = Vec::new();
    for row in rows {
        let variant = id::format(row.try_get::<Uuid, _>("id")?);
        let option = id::format(row.try_get::<Uuid, _>("option_id")?);
        match variants.last_mut() {
            Some((last, set)) if *last == variant => {
                set.insert(option);
            }
            _ => variants.push((variant, BTreeSet::from([option]))),
        }
    }
    Ok(variants)
}

/// The parent whose variants make up the configurator: the product itself or its parent.
fn variant_parent(product: &Value) -> Option<Uuid> {
    match product["parentId"].as_str().and_then(id::parse) {
        Some(parent) => Some(parent),
        None if product["childCount"].as_i64().unwrap_or(0) > 0 => product["id"].as_str().and_then(id::parse),
        None => None,
    }
}

/// Shopware's configurator: the variant groups with their options. An option is `combinable`
/// when some variant has it together with the current variant's options of the other groups.
pub async fn configurator(state: &StoreState, sc: &SalesChannelContext, product: &Value) -> Result<Value, ApiError> {
    let Some(parent) = variant_parent(product) else { return Ok(json!([])) };
    let variants = variant_options(state, parent).await?;
    let all: BTreeSet<String> = variants.iter().flat_map(|(_, set)| set.iter().cloned()).collect();
    let loaded = options(state, sc, all.into_iter().collect()).await?;
    let group_of: HashMap<String, String> = loaded
        .iter()
        .filter_map(|o| Some((o["id"].as_str()?.to_string(), o["groupId"].as_str()?.to_string())))
        .collect();
    let current = product["id"].as_str().unwrap_or_default();
    let selected: BTreeSet<String> =
        variants.iter().find(|(v, _)| v == current).map(|(_, set)| set.clone()).unwrap_or_default();

    let mut with_flags = Vec::new();
    for mut option in loaded {
        let option_id = option["id"].as_str().unwrap_or_default().to_string();
        let group = group_of.get(&option_id);
        let others: Vec<&String> = selected.iter().filter(|s| group_of.get(*s) != group).collect();
        let combinable =
            variants.iter().any(|(_, set)| set.contains(&option_id) && others.iter().all(|o| set.contains(*o)));
        option["combinable"] = json!(combinable);
        with_flags.push(option);
    }
    Ok(Value::Array(grouped(with_flags)))
}

/// Visible properties of a product (falling back to its parent's), grouped (`sortedProperties`).
pub async fn sorted_properties(
    state: &StoreState,
    sc: &SalesChannelContext,
    product: &Value,
) -> Result<Value, ApiError> {
    let own = product["id"].as_str().and_then(id::parse);
    let parent = product["parentId"].as_str().and_then(id::parse);
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT option_id FROM product_property WHERE product_id = $1 \
         UNION SELECT option_id FROM product_property WHERE product_id = $2 \
         AND NOT EXISTS (SELECT 1 FROM product_property WHERE product_id = $1)",
    )
    .bind(own)
    .bind(parent)
    .fetch_all(&state.db)
    .await?;
    let loaded = options(state, sc, ids.into_iter().map(id::format).collect()).await?;
    let visible = loaded.into_iter().filter(|o| o["group"]["visibleOnProductDetailPage"].as_bool().unwrap_or(true));
    Ok(Value::Array(grouped(visible.collect())))
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FindVariant {
    options: Vec<String>,
    switched_group: Option<String>,
}

/// `POST /store-api/product/{productId}/find-variant`: the variant for chosen options. When the
/// exact combination does not exist, the variant that keeps the switched group's option and
/// most of the others wins.
pub async fn find_variant(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(product_id): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let input: FindVariant = guard::body(&body)?;
    let not_found = || {
        ApiError::Code(
            StatusCode::NOT_FOUND,
            "CONTENT__PRODUCT_VARIANT_NOT_FOUND",
            "no variant for these options".into(),
        )
    };
    let parsed = id::parse(&product_id)
        .ok_or_else(|| ApiError::from(Error::NotFound { entity: "product".into(), id: product_id.clone() }))?;
    let mut criteria = Criteria::with_ids(vec![id::format(parsed)]);
    criteria.limit = Some(1);
    let product = catalog::search(&state, &sc, "product", criteria, 1)
        .await?
        .data
        .pop()
        .ok_or_else(|| ApiError::from(Error::NotFound { entity: "product".into(), id: product_id.clone() }))?;
    let parent = variant_parent(&product).ok_or_else(not_found)?;
    let wanted: BTreeSet<String> = input.options.iter().filter_map(|o| id::parse(o)).map(id::format).collect();
    let variants = variant_options(&state, parent).await?;

    let exact = variants.iter().find(|(_, set)| wanted.is_subset(set));
    let found = match (exact, &input.switched_group) {
        (Some(found), _) => Some(found),
        (None, Some(group)) => {
            let group = id::parse(group).map(id::format).unwrap_or_default();
            let groups: HashMap<String, String> =
                sqlx::query("SELECT id, group_id FROM property_group_option WHERE id = ANY($1::text[]::uuid[])")
                    .bind(wanted.iter().cloned().collect::<Vec<_>>())
                    .fetch_all(&state.db)
                    .await?
                    .into_iter()
                    .map(|r| Ok((id::format(r.try_get("id")?), id::format(r.try_get("group_id")?))))
                    .collect::<Result<_, sqlx::Error>>()?;
            let switched: Vec<&String> = wanted.iter().filter(|o| groups.get(*o) == Some(&group)).collect();
            // Most matching options wins; on a tie the first variant by product number.
            let mut best: Option<(usize, &(String, BTreeSet<String>))> = None;
            for variant in variants.iter().filter(|(_, set)| switched.iter().all(|o| set.contains(*o))) {
                let matches = wanted.intersection(&variant.1).count();
                if best.is_none_or(|(most, _)| matches > most) {
                    best = Some((matches, variant));
                }
            }
            best.map(|(_, variant)| variant)
        }
        (None, None) => None,
    };
    let (variant, options) = found.ok_or_else(not_found)?;
    Ok(Json(json!({
        "foundCombination": { "variantId": variant, "options": options },
        "apiAlias": "found_combination",
    })))
}

/// Post filters for `properties=<id>|<id>`: options of one group are alternatives, groups must
/// all match. A main product matches through its own properties or any variant's.
pub async fn filters(state: &StoreState, ids: &[String]) -> Result<Vec<Filter>, ApiError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows = sqlx::query("SELECT id, group_id FROM property_group_option WHERE id = ANY($1::text[]::uuid[])")
        .bind(ids)
        .fetch_all(&state.db)
        .await?;
    let mut by_group: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in rows {
        by_group.entry(row.try_get("group_id")?).or_default().push(id::format(row.try_get("id")?));
    }
    let mut groups: Vec<_> = by_group.into_values().collect();
    groups.sort();
    Ok(groups
        .into_iter()
        .map(|options| Filter::Multi {
            operator: "or".into(),
            queries: ["properties.id", "options.id", "children.properties.id", "children.options.id"]
                .into_iter()
                .map(|field| Filter::EqualsAny { field: field.into(), value: json!(options) })
                .collect(),
        })
        .collect())
}

/// The `properties` listing aggregation: filterable groups with the options that occur in the
/// listing (ignoring the shopper's filters), as Shopware's `properties_aggregation`.
pub async fn aggregation(state: &StoreState, sc: &SalesChannelContext, criteria: &Criteria) -> Result<Value, ApiError> {
    let mut base = criteria.clone();
    guard::restrict(guard::definition("product"), &mut base, sc);
    let products = read::matching_ids(&state.db, "product", &base, &sc.core(), AGGREGATION_SCOPE).await?;
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT option_id FROM product_property WHERE product_id = ANY($1::text[]::uuid[]) \
         UNION SELECT x.option_id FROM product c JOIN product_option x ON x.product_id = c.id \
         WHERE c.parent_id = ANY($1::text[]::uuid[]) \
         UNION SELECT x.option_id FROM product c JOIN product_property x ON x.product_id = c.id \
         WHERE c.parent_id = ANY($1::text[]::uuid[])",
    )
    .bind(&products)
    .fetch_all(&state.db)
    .await?;
    let loaded = options(state, sc, ids.into_iter().map(id::format).collect()).await?;
    let filterable = loaded.into_iter().filter(|o| o["group"]["filterable"].as_bool().unwrap_or(true));
    Ok(json!({ "entities": grouped(filterable.collect()), "apiAlias": "properties_aggregation" }))
}
