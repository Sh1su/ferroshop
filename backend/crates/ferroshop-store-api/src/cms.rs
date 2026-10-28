//! Shopping experiences: CMS pages with their sections, blocks and slots, each slot carrying the
//! `data` its element needs (Shopware's CMS resolvers): text, images, image sliders, product
//! boxes, product sliders and the category's product listing.

use std::collections::HashMap;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, RawQuery, State};
use ferroshop_admin_api::ApiError;
use ferroshop_core::{Criteria, Error, defaults, id};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{SalesChannelContext, StoreState, catalog, guard, listing};

fn page_criteria(page_id: Uuid) -> Criteria {
    let mut criteria: Criteria = serde_json::from_value(json!({ "associations": {
        "sections": {
            "sort": [{ "field": "position" }],
            "associations": {
                "backgroundMedia": {},
                "blocks": { "sort": [{ "field": "position" }], "associations": { "slots": {} } },
            },
        },
    }}))
    .expect("static criteria");
    criteria.ids = Some(vec![id::format(page_id)]);
    criteria
}

/// `{"media": {"source": "static", "value": "<id>"}}` → the static value of a config field.
fn value<'a>(config: &'a Value, field: &str) -> &'a Value {
    &config[field]["value"]
}

fn ids_of(value: &Value) -> Vec<String> {
    match value {
        Value::String(s) => id::parse(s).map(id::format).into_iter().collect(),
        Value::Array(items) => items.iter().flat_map(ids_of).collect(),
        _ => Vec::new(),
    }
}

fn slider_items(config: &Value) -> &[Value] {
    value(config, "sliderItems").as_array().map(Vec::as_slice).unwrap_or_default()
}

async fn by_id(
    state: &StoreState,
    sc: &SalesChannelContext,
    entity: &str,
    ids: Vec<String>,
    associations: &[&str],
) -> Result<HashMap<String, Value>, ApiError> {
    let mut out = HashMap::new();
    let mut unique = ids;
    unique.sort();
    unique.dedup();
    for chunk in unique.chunks(guard::MAX_LIMIT as usize) {
        let mut criteria = Criteria::with_ids(chunk.to_vec());
        for association in associations {
            criteria.associations.insert((*association).into(), Criteria::default());
        }
        for row in catalog::search(state, sc, entity, criteria, guard::MAX_LIMIT).await?.data {
            out.insert(row["id"].as_str().unwrap_or_default().to_string(), row);
        }
    }
    Ok(out)
}

/// Loads a page and resolves its slots. `navigation` is the category whose products a
/// product-listing slot shows; `params` are the shopper's listing parameters.
pub(crate) async fn page(
    state: &StoreState,
    sc: &SalesChannelContext,
    page_id: Uuid,
    navigation: Option<Uuid>,
    params: &Map<String, Value>,
) -> Result<Option<Value>, ApiError> {
    let Some(mut page) = catalog::search(state, sc, "cms_page", page_criteria(page_id), 1).await?.data.pop() else {
        return Ok(None);
    };

    // Slots read their translated config; collect what they reference to load it in batches.
    let mut slots: Vec<&mut Value> = Vec::new();
    for section in page["sections"].as_array_mut().into_iter().flatten() {
        for block in section["blocks"].as_array_mut().into_iter().flatten() {
            slots.extend(block["slots"].as_array_mut().into_iter().flatten());
        }
    }
    let mut media_ids = Vec::new();
    let mut product_ids = Vec::new();
    for slot in slots.iter_mut() {
        let config = slot["translated"]["config"].clone();
        let config = if config.is_object() { config } else { json!({}) };
        match slot["type"].as_str().unwrap_or_default() {
            "image" => media_ids.extend(ids_of(value(&config, "media"))),
            "image-slider" | "image-gallery" => {
                media_ids.extend(slider_items(&config).iter().flat_map(|i| ids_of(&i["mediaId"])));
            }
            "product-box" => product_ids.extend(ids_of(value(&config, "product"))),
            "product-slider" => product_ids.extend(ids_of(value(&config, "products"))),
            _ => {}
        }
        let field_config: Vec<Value> = config
            .as_object()
            .into_iter()
            .flatten()
            .map(|(name, field)| {
                json!({ "name": name, "source": field["source"], "value": field["value"],
                        "apiAlias": "cms_data_resolver_field_config" })
            })
            .collect();
        slot["fieldConfig"] = Value::Array(field_config);
        slot["config"] = config;
    }
    let media = by_id(state, sc, "media", media_ids, &[]).await?;
    let products = by_id(state, sc, "product", product_ids, &["cover.media"]).await?;
    let has_listing = slots.iter().any(|s| s["type"] == "product-listing");
    let listing = match (has_listing, navigation) {
        (true, Some(category)) => listing::for_category(state, sc, category, params.clone()).await?,
        _ => None,
    };

    for slot in slots {
        let config = slot["config"].clone();
        let media_of = |v: &Value| ids_of(v).first().and_then(|m| media.get(m)).cloned().unwrap_or(Value::Null);
        let data = match slot["type"].as_str().unwrap_or_default() {
            "text" => json!({ "content": value(&config, "content"), "apiAlias": "cms_text" }),
            "image" => json!({
                "media": media_of(value(&config, "media")),
                "url": value(&config, "url"),
                "newTab": value(&config, "newTab").as_bool().unwrap_or(false),
                "apiAlias": "cms_image",
            }),
            "image-slider" | "image-gallery" => {
                let items: Vec<Value> = slider_items(&config)
                    .iter()
                    .map(|item| {
                        json!({ "media": media_of(&item["mediaId"]), "url": item["url"],
                                "newTab": item["newTab"].as_bool().unwrap_or(false),
                                "apiAlias": "cms_image_slider_item" })
                    })
                    .filter(|item| !item["media"].is_null())
                    .collect();
                json!({ "sliderItems": items, "apiAlias": "cms_image_slider" })
            }
            "product-box" => {
                let product = ids_of(value(&config, "product")).first().and_then(|p| products.get(p)).cloned();
                json!({ "product": product, "apiAlias": "cms_product_box" })
            }
            "product-slider" => {
                // Keep the configured order; hidden products drop out.
                let list: Vec<Value> =
                    ids_of(value(&config, "products")).iter().filter_map(|p| products.get(p)).cloned().collect();
                json!({ "products": list, "apiAlias": "cms_product_slider" })
            }
            "product-listing" => json!({ "listing": listing, "apiAlias": "cms_product_listing" }),
            _ => Value::Null,
        };
        slot["data"] = data;
    }
    Ok(Some(page))
}

/// The page a category shows: its own, or the default listing page for ordinary categories.
pub(crate) fn category_page(category: &Value) -> Option<Uuid> {
    match category["cmsPageId"].as_str().and_then(id::parse) {
        Some(page) => Some(page),
        None if category["type"].as_str().is_none_or(|t| t == "page") => Some(defaults::CMS_DEFAULT_LISTING_PAGE),
        None => None,
    }
}

/// `POST /store-api/cms/{id}`: a page on its own; `navigationId` names the category of a
/// product-listing slot (the sales channel's navigation root otherwise).
pub async fn show(
    State(state): State<StoreState>,
    Extension(sc): Extension<SalesChannelContext>,
    Path(raw_id): Path<String>,
    RawQuery(query): RawQuery,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let not_found = || ApiError::from(Error::NotFound { entity: "cms_page".into(), id: raw_id.clone() });
    let page_id = id::parse(&raw_id).ok_or_else(not_found)?;
    let params = listing::params(query.as_deref(), &body)?;
    let navigation =
        params.get("navigationId").and_then(Value::as_str).and_then(id::parse).or(sc.navigation_category_id);
    let page = page(&state, &sc, page_id, navigation, &params).await?.ok_or_else(not_found)?;
    Ok(Json(page))
}
