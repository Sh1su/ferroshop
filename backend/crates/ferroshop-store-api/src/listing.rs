//! Product listings: category pages, search and search suggestions (Shopware's `product_listing`).
//!
//! Besides a criteria, listings accept Shopware's listing parameters in the query string or body:
//! `order`, `p`, `limit`, `manufacturer` and `properties` (ids separated by `|`), `min-price`, `max-price`,
//! `search` and `no-aggregations`.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, RawQuery, State};
use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::{Aggregation, Filter, RangeParameters, Sort, TotalCountMode};
use ferroshop_core::{Criteria, defaults, id};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::catalog::search;
use crate::{SalesChannelContext, StoreState, guard, properties};

const LISTING_LIMIT: u64 = 24;
const SUGGEST_LIMIT: u64 = 10;

/// Listing parameters; everything else in the request is read as criteria.
const LISTING_PARAMS: &[&str] = &[
    "order",
    "p",
    "page",
    "limit",
    "manufacturer",
    "properties",
    "min-price",
    "max-price",
    "search",
    "no-aggregations",
];

struct Sorting {
    key: &'static str,
    priority: u32,
    label_en: &'static str,
    label_de: &'static str,
    /// `(field, descending)` pairs; the product id is always the final tie-breaker.
    fields: &'static [(&'static str, bool)],
}

const SORTINGS: &[Sorting] = &[
    Sorting {
        key: "score",
        priority: 0,
        label_en: "Top results",
        label_de: "Beste Ergebnisse",
        fields: &[("markAsTopseller", true), ("name", false)],
    },
    Sorting { key: "name-asc", priority: 4, label_en: "Name A-Z", label_de: "Name A-Z", fields: &[("name", false)] },
    Sorting { key: "name-desc", priority: 3, label_en: "Name Z-A", label_de: "Name Z-A", fields: &[("name", true)] },
    Sorting {
        key: "price-asc",
        priority: 2,
        label_en: "Price ascending",
        label_de: "Preis aufsteigend",
        fields: &[("cheapestGross", false), ("name", false)],
    },
    Sorting {
        key: "price-desc",
        priority: 1,
        label_en: "Price descending",
        label_de: "Preis absteigend",
        fields: &[("cheapestGross", true), ("name", false)],
    },
    Sorting {
        key: "topseller",
        priority: 5,
        label_en: "Topseller",
        label_de: "Topseller",
        fields: &[("markAsTopseller", true), ("name", false)],
    },
];

/// Listing parameters from the query string, overridden by the JSON body.
fn params(query: Option<&str>, body: &[u8]) -> Result<Map<String, Value>, ApiError> {
    let mut out = Map::new();
    if let Some(query) = query {
        let pairs: Vec<(String, String)> = serde_urlencoded::from_str(query)
            .map_err(|e| ApiError::BadRequest(format!("invalid query string: {e}")))?;
        for (key, value) in pairs {
            out.insert(key, Value::String(value));
        }
    }
    if !body.iter().all(u8::is_ascii_whitespace) {
        match serde_json::from_slice(body) {
            Ok(Value::Object(map)) => out.extend(map),
            Ok(_) => return Err(ApiError::BadRequest("expected a JSON object".into())),
            Err(e) => return Err(ApiError::BadRequest(format!("invalid criteria: {e}"))),
        }
    }
    Ok(out)
}

fn number(params: &Map<String, Value>, key: &str) -> Option<Decimal> {
    match params.get(key)? {
        Value::Number(n) => n.to_string().parse().ok(),
        Value::String(s) if !s.is_empty() => s.parse().ok(),
        _ => None,
    }
}

fn unsigned(params: &Map<String, Value>, key: &str) -> Option<u64> {
    number(params, key).and_then(|d| d.to_u64())
}

fn flag(params: &Map<String, Value>, key: &str) -> bool {
    match params.get(key) {
        Some(Value::Bool(b)) => *b,
        Some(Value::String(s)) => s == "1" || s == "true",
        Some(Value::Number(n)) => n.as_u64() == Some(1),
        _ => false,
    }
}

fn id_list(value: Option<&Value>) -> Vec<String> {
    let raw: Vec<String> = match value {
        Some(Value::String(s)) => s.split('|').map(str::to_string).collect(),
        Some(Value::Array(items)) => items.iter().filter_map(Value::as_str).map(str::to_string).collect(),
        _ => Vec::new(),
    };
    raw.iter().filter_map(|r| id::parse(r)).map(id::format).collect()
}

/// What kind of listing is built; the category tree is resolved up front.
enum Scope {
    Category { navigation_id: String, category_ids: Vec<String> },
    Search { term: String, suggest: bool },
}

/// The category and all categories below it: products in sub-categories are listed too.
async fn category_tree(state: &StoreState, category_id: Uuid) -> Result<Vec<String>, ApiError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE tree AS (SELECT id, 0 AS n FROM category WHERE id = $1 AND active \
         UNION ALL SELECT c.id, tree.n + 1 FROM category c JOIN tree ON c.parent_id = tree.id \
         WHERE c.active AND tree.n < 50) SELECT id FROM tree",
    )
    .bind(category_id)
    .fetch_all(&state.db)
    .await?;
    Ok(ids.into_iter().map(id::format).collect())
}

async fn listing(
    state: &StoreState,
    sc: &SalesChannelContext,
    scope: Scope,
    params: Map<String, Value>,
) -> Result<Json<Value>, ApiError> {
    let mut raw = params.clone();
    for key in LISTING_PARAMS {
        raw.remove(*key);
    }
    let mut criteria: Criteria = serde_json::from_value(Value::Object(raw))
        .map_err(|e| ApiError::BadRequest(format!("invalid criteria: {e}")))?;
    let (default_sort, default_limit) = match &scope {
        Scope::Category { .. } => ("name-asc", LISTING_LIMIT),
        Scope::Search { suggest: true, .. } => ("score", SUGGEST_LIMIT),
        Scope::Search { .. } => ("score", LISTING_LIMIT),
    };
    let limit = unsigned(&params, "limit").unwrap_or(default_limit).clamp(1, guard::MAX_LIMIT);
    let page = unsigned(&params, "p").or(criteria.page).unwrap_or(1).max(1);
    criteria.limit = Some(limit);
    criteria.page = Some(page);
    if criteria.total_count_mode == TotalCountMode::None {
        criteria.total_count_mode = TotalCountMode::Exact;
    }

    // Main products only; variants are reachable through `children`.
    criteria.filter.push(Filter::Equals { field: "parentId".into(), value: Value::Null });
    let mut current = Map::new();
    match &scope {
        Scope::Category { navigation_id, category_ids } => {
            criteria.filter.push(Filter::EqualsAny { field: "categories.id".into(), value: json!(category_ids) });
            current.insert("navigationId".into(), json!(navigation_id));
        }
        Scope::Search { term, .. } => {
            criteria.term = Some(term.clone());
            current.insert("search".into(), json!(term));
        }
    }

    let order = params.get("order").and_then(Value::as_str).unwrap_or(default_sort);
    let sorting = SORTINGS
        .iter()
        .find(|s| s.key == order)
        .ok_or_else(|| ApiError::BadRequest(format!("unknown sorting `{order}`")))?;
    if criteria.sort.is_empty() {
        criteria.sort = sorting
            .fields
            .iter()
            .map(|(field, desc)| Sort {
                field: (*field).into(),
                order: if *desc { "DESC" } else { "ASC" }.into(),
                natural_sorting: false,
            })
            .collect();
    }

    // Filters the shopper picked are post filters, so the aggregations still show every option.
    let manufacturers = id_list(params.get("manufacturer"));
    if !manufacturers.is_empty() {
        criteria.post_filter.push(Filter::EqualsAny { field: "manufacturerId".into(), value: json!(manufacturers) });
    }
    // `cheapestGross` is in the default currency; the shopper filters in the context currency.
    let factor = if sc.price.currency_id == defaults::CURRENCY { Decimal::ONE } else { sc.price.factor };
    let to_default = |v: Decimal| if factor.is_zero() { v } else { v / factor };
    let min_price = number(&params, "min-price");
    let max_price = number(&params, "max-price");
    if min_price.is_some() || max_price.is_some() {
        criteria.post_filter.push(Filter::Range {
            field: "cheapestGross".into(),
            parameters: RangeParameters {
                gte: min_price.map(|v| json!(to_default(v))),
                lte: max_price.map(|v| json!(to_default(v))),
                ..Default::default()
            },
        });
    }
    current.insert("manufacturer".into(), json!(manufacturers));
    current.insert("price".into(), json!({ "min": min_price, "max": max_price }));
    let property_ids = id_list(params.get("properties"));
    criteria.post_filter.extend(properties::filters(state, &property_ids).await?);
    current.insert("properties".into(), json!(property_ids));
    current.insert("shipping-free".into(), json!(false));
    current.insert("rating".into(), Value::Null);

    let with_aggregations = !flag(&params, "no-aggregations") && !matches!(scope, Scope::Search { suggest: true, .. });
    if with_aggregations {
        criteria.aggregations.push(Aggregation {
            name: "manufacturer".into(),
            kind: "terms".into(),
            field: "manufacturerId".into(),
            limit: None,
        });
        criteria.aggregations.push(Aggregation {
            name: "price".into(),
            kind: "stats".into(),
            field: "cheapestGross".into(),
            limit: None,
        });
    }

    let property_aggregation =
        if with_aggregations { Some(properties::aggregation(state, sc, &criteria).await?) } else { None };
    let mut result = search(state, sc, "product", criteria, default_limit).await?;
    if let Some(aggregation) = property_aggregation {
        result.aggregations.insert("properties".into(), aggregation);
    }
    if with_aggregations {
        listing_aggregations(state, sc, &mut result.aggregations, factor).await?;
    }

    let german = sc.language_id == defaults::LANGUAGE_DE;
    let available: Vec<Value> = SORTINGS
        .iter()
        .filter(|s| s.key != "score" || matches!(scope, Scope::Search { .. }))
        .map(|s| {
            let label = if german { s.label_de } else { s.label_en };
            json!({ "key": s.key, "priority": s.priority, "label": label, "translated": { "label": label },
                    "apiAlias": "product_sorting" })
        })
        .collect();

    Ok(Json(json!({
        "entity": "product",
        "total": result.total,
        "aggregations": result.aggregations,
        "page": page,
        "limit": limit,
        "elements": result.data,
        "sorting": sorting.key,
        "currentFilters": current,
        "availableSortings": available,
        "apiAlias": "product_listing",
    })))
}

/// Turns the raw aggregations into Shopware's listing format: manufacturer entities and a
/// price range in the context currency.
async fn listing_aggregations(
    state: &StoreState,
    sc: &SalesChannelContext,
    aggregations: &mut Map<String, Value>,
    factor: Decimal,
) -> Result<(), ApiError> {
    if let Some(terms) = aggregations.get("manufacturer") {
        let ids: Vec<String> = terms["buckets"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|b| b["key"].as_str())
            .map(str::to_string)
            .collect();
        let entities = if ids.is_empty() {
            Vec::new()
        } else {
            let mut criteria = Criteria::with_ids(ids);
            criteria.sort = vec![Sort { field: "name".into(), order: "ASC".into(), natural_sorting: false }];
            search(state, sc, "product_manufacturer", criteria, guard::MAX_LIMIT).await?.data
        };
        aggregations
            .insert("manufacturer".into(), json!({ "entities": entities, "apiAlias": "manufacturer_aggregation" }));
    }
    if let Some(stats) = aggregations.get_mut("price") {
        let precision = sc.price.precision;
        for key in ["min", "max", "avg", "sum"] {
            let converted = stats
                .get(key)
                .and_then(|v| v.to_string().parse::<Decimal>().ok())
                .map(|v| (v * factor).round_dp(precision));
            stats[key] = json!(converted);
        }
    }
    Ok(())
}

pub async fn category_listing(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let category_id = id::parse(&raw_id).ok_or_else(|| {
        ApiError::from(ferroshop_core::Error::NotFound { entity: "category".into(), id: raw_id.clone() })
    })?;
    let category_ids = category_tree(&state, category_id).await?;
    if category_ids.is_empty() {
        return Err(ferroshop_core::Error::NotFound { entity: "category".into(), id: raw_id }.into());
    }
    let scope = Scope::Category { navigation_id: id::format(category_id), category_ids };
    listing(&state, &sc, scope, params(query.as_deref(), &body)?).await
}

fn search_scope(params: &mut Map<String, Value>, suggest: bool) -> Result<Scope, ApiError> {
    let term = params.get("search").and_then(Value::as_str).map(str::trim).unwrap_or_default().to_string();
    if term.is_empty() {
        return Err(ApiError::BadRequest("parameter `search` is required".into()));
    }
    // `term` in the criteria would otherwise be overwritten silently.
    params.remove("term");
    Ok(Scope::Search { term, suggest })
}

async fn run_search(
    state: StoreState,
    sc: SalesChannelContext,
    query: Option<String>,
    body: Bytes,
    suggest: bool,
) -> Result<Json<Value>, ApiError> {
    let mut params = params(query.as_deref(), &body)?;
    let scope = search_scope(&mut params, suggest)?;
    listing(&state, &sc, scope, params).await
}

pub async fn search_listing(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    run_search(state, sc, query, body, false).await
}

pub async fn suggest(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    run_search(state, sc, query, body, true).await
}
