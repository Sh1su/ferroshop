//! Catalogue and master data routes: products, categories, navigation, currencies, languages,
//! countries, payment and shipping methods.

use std::collections::{HashMap, HashSet};

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, RawQuery, State};
use ferroshop_admin_api::ApiError;
use ferroshop_core::criteria::Filter;
use ferroshop_core::read::{self, SearchResult};
use ferroshop_core::{Criteria, Error, id};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{SalesChannelContext, StoreState, cms, guard, listing, pricing, properties};

/// Default page size for routes that do not define their own.
const DEFAULT_LIMIT: u64 = 100;

/// Searches a public entity: checks and restricts the criteria, prices products, applies includes.
pub(crate) async fn search(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    criteria: Criteria,
    default_limit: u64,
) -> Result<SearchResult, ApiError> {
    search_in(state, sc, entity, criteria, default_limit, false).await
}

/// [`search`] over a customer's own records (orders, addresses). The caller adds the filter
/// that limits the root entity to the logged-in customer.
pub(crate) async fn search_own(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    criteria: Criteria,
    default_limit: u64,
) -> Result<SearchResult, ApiError> {
    search_in(state, sc, entity, criteria, default_limit, true).await
}

async fn search_in(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    mut criteria: Criteria,
    default_limit: u64,
    own: bool,
) -> Result<SearchResult, ApiError> {
    let def = guard::definition(entity);
    guard::check_in(def, &criteria, own)?;
    // `sw-include-seo-urls: 1` asks for every product and category to carry its URL.
    if sc.include_seo_urls && matches!(def.name, "product" | "category") {
        criteria.associations.entry("seoUrls".into()).or_default();
    }
    guard::restrict(def, &mut criteria, sc);
    guard::limit(&mut criteria, default_limit);
    let mut result = read::search_def(&state.db, def, &criteria, &sc.core()).await?;
    pricing::enrich(&state.db, sc, &mut result.data).await?;
    if let Some(includes) = &criteria.includes {
        result.data.iter_mut().for_each(|row| read::apply_includes(row, includes));
    }
    Ok(result)
}

/// Shopware's `dal_entity_search_result`.
pub(crate) fn search_result(entity: &str, criteria_page: Option<u64>, limit: u64, result: SearchResult) -> Value {
    json!({
        "entity": entity,
        "total": result.total,
        "aggregations": result.aggregations,
        "page": criteria_page.unwrap_or(1).max(1),
        "limit": limit,
        "elements": result.data,
        "apiAlias": "dal_entity_search_result",
    })
}

async fn search_route(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    body: &[u8],
) -> Result<Json<Value>, ApiError> {
    let criteria = guard::criteria(body)?;
    let page = criteria.page;
    let limit = criteria.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, guard::MAX_LIMIT);
    let result = search(state, sc, entity, criteria, DEFAULT_LIMIT).await?;
    Ok(Json(search_result(entity, page, limit, result)))
}

fn not_found(entity: &str, raw: &str) -> ApiError {
    Error::NotFound { entity: entity.into(), id: raw.into() }.into()
}

/// Reads one public entity by id with the request's criteria.
async fn single(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    raw_id: &str,
    mut criteria: Criteria,
) -> Result<Value, ApiError> {
    let parsed = id::parse(raw_id).ok_or_else(|| not_found(entity, raw_id))?;
    criteria.ids = Some(vec![id::format(parsed)]);
    criteria.page = None;
    search(state, sc, entity, criteria, 1).await?.data.pop().ok_or_else(|| not_found(entity, raw_id))
}

pub async fn products(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "product", &body).await
}

/// `/store-api/product/{id}`: Shopware's `product_detail` (no property configurator yet).
pub async fn product(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(product_id): Path<String>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let parsed = id::parse(&product_id).ok_or_else(|| not_found("product", &product_id))?;
    let mut criteria = guard::criteria(&body)?;
    // Variant options name the variant on the product page.
    criteria.associations.entry("options".into()).or_default().associations.entry("group".into()).or_default();
    let wants_properties = criteria
        .includes
        .as_ref()
        .and_then(|i| i.get("product"))
        .is_none_or(|fields| fields.iter().any(|f| f == "sortedProperties"));
    let mut product = single(&state, &sc, "product", &product_id, criteria).await?;
    // Configurator and properties need ids that `includes` may have stripped.
    let raw = single(&state, &sc, "product", &id::format(parsed), Criteria::default()).await?;
    let configurator = properties::configurator(&state, &sc, &raw).await?;
    if wants_properties {
        product["sortedProperties"] = properties::sorted_properties(&state, &sc, &raw).await?;
    }
    Ok(Json(json!({ "product": product, "configurator": configurator, "apiAlias": "product_detail" })))
}

pub async fn categories(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "category", &body).await
}

/// Resolves `main-navigation` (and Shopware's other aliases) to a category id.
fn category_id(sc: &SalesChannelContext, raw: &str) -> Result<Uuid, ApiError> {
    match raw {
        // Footer and service navigation are not configurable yet and fall back to the main one.
        "main-navigation" | "footer-navigation" | "service-navigation" => {
            sc.navigation_category_id.ok_or_else(|| not_found("category", raw))
        }
        other => id::parse(other).ok_or_else(|| not_found("category", other)),
    }
}

pub async fn category(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let category_uuid = category_id(&sc, &raw_id)?;
    let category_id = id::format(category_uuid);
    let mut category = single(&state, &sc, "category", &category_id, guard::criteria(&body)?).await?;
    // The page needs `cmsPageId` and `type`, which `includes` may have stripped.
    let raw = single(&state, &sc, "category", &category_id, Criteria::default()).await?;
    let page = match cms::category_page(&raw) {
        Some(page_id) => {
            // Only listing parameters reach the product listing; the rest is the category's criteria.
            let mut params = listing::params(query.as_deref(), &body)?;
            params.retain(|key, _| listing::LISTING_PARAMS.contains(&key.as_str()));
            cms::page(&state, &sc, page_id, Some(category_uuid), &params).await?
        }
        None => None,
    };
    if category.is_object() {
        category["cmsPage"] = page.unwrap_or(Value::Null);
    }
    Ok(Json(category))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct NavigationParams {
    depth: u32,
    build_tree: bool,
}

impl Default for NavigationParams {
    fn default() -> Self {
        Self { depth: 2, build_tree: true }
    }
}

/// Upper bound for `depth`, so a single request cannot walk an arbitrarily deep tree.
const MAX_DEPTH: u32 = 10;

/// Ids from `category_id` up to the top of the tree, closest first.
async fn ancestors(state: &StoreState, category_id: Uuid) -> Result<Vec<String>, ApiError> {
    let ids: Vec<Uuid> = sqlx::query_scalar(
        "WITH RECURSIVE up AS (SELECT id, parent_id, 0 AS n FROM category WHERE id = $1 \
         UNION ALL SELECT c.id, c.parent_id, up.n + 1 FROM category c JOIN up ON c.id = up.parent_id WHERE up.n < 50) \
         SELECT id FROM up ORDER BY n",
    )
    .bind(category_id)
    .fetch_all(&state.db)
    .await?;
    Ok(ids.into_iter().map(id::format).collect())
}

fn attach_children(node: &mut Value, by_parent: &mut HashMap<String, Vec<Value>>) {
    let id = node["id"].as_str().unwrap_or_default().to_string();
    let mut children = by_parent.remove(&id).unwrap_or_default();
    children.iter_mut().for_each(|c| attach_children(c, by_parent));
    node["children"] = Value::Array(children);
}

/// `/store-api/navigation/{activeId}/{rootId}`: visible, active categories below the root,
/// `depth` levels deep plus the path down to the active category.
pub async fn navigation(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path((active_raw, root_raw)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let root = id::format(category_id(&sc, &root_raw)?);
    let active = category_id(&sc, &active_raw)?;
    let params: NavigationParams = if body.iter().all(u8::is_ascii_whitespace) {
        NavigationParams::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(format!("invalid navigation request: {e}")))?
    };
    let base = guard::criteria(&body)?;

    // Expand the active category's ancestors too, as long as they lie below the root.
    let path = ancestors(&state, active).await?;
    let mut active_path: Vec<String> = match path.iter().position(|p| *p == root) {
        Some(index) => path[..index].to_vec(),
        None => Vec::new(),
    };

    let mut loaded: Vec<Value> = Vec::new();
    let mut expanded: HashSet<String> = HashSet::new();
    let mut frontier = vec![root.clone()];
    let mut level = 0;
    while !frontier.is_empty() {
        let in_depth = level < params.depth.min(MAX_DEPTH);
        let parents: Vec<String> = frontier
            .drain(..)
            .filter(|p| (in_depth || active_path.contains(p)) && expanded.insert(p.clone()))
            .collect();
        // Path categories deeper than `depth` are expanded once their parent was loaded.
        if parents.is_empty() {
            break;
        }
        let mut criteria = base.clone();
        criteria.limit = None;
        criteria.page = None;
        criteria.filter.push(Filter::EqualsAny { field: "parentId".into(), value: json!(parents) });
        criteria.filter.push(Filter::Equals { field: "visible".into(), value: Value::Bool(true) });
        if criteria.sort.is_empty() {
            criteria.sort =
                serde_json::from_value(json!([{ "field": "position" }, { "field": "name" }])).expect("static sort");
        }
        let children = search(&state, &sc, "category", criteria, guard::MAX_LIMIT).await?.data;
        frontier = children.iter().filter_map(|c| c["id"].as_str().map(str::to_string)).collect();
        loaded.extend(children);
        level += 1;
        active_path.retain(|p| !expanded.contains(p));
    }

    if !params.build_tree {
        return Ok(Json(Value::Array(loaded)));
    }
    let mut by_parent: HashMap<String, Vec<Value>> = HashMap::new();
    for category in loaded {
        let parent = category["parentId"].as_str().unwrap_or_default().to_string();
        by_parent.entry(parent).or_default().push(category);
    }
    let mut top = by_parent.remove(&root).unwrap_or_default();
    top.iter_mut().for_each(|c| attach_children(c, &mut by_parent));
    Ok(Json(Value::Array(top)))
}

/// `/store-api/currency` answers with a plain list, unlike the other master data routes.
pub async fn currencies(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let mut criteria = guard::criteria(&body)?;
    if criteria.sort.is_empty() {
        criteria.sort = serde_json::from_value(json!([{ "field": "position" }])).expect("static sort");
    }
    Ok(Json(Value::Array(search(&state, &sc, "currency", criteria, DEFAULT_LIMIT).await?.data)))
}

pub async fn languages(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "language", &body).await
}

pub async fn countries(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "country", &body).await
}

pub async fn payment_methods(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "payment_method", &body).await
}

/// `/store-api/seo-url`: resolves storefront paths (`Shoes/Sneakers/`) to categories and products.
pub async fn seo_urls(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "seo_url", &body).await
}

pub async fn shipping_methods(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    search_route(&state, &sc, "shipping_method", &body).await
}
