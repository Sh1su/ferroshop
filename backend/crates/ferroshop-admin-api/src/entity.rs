//! Generic entity endpoints, identical for every entity definition.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Criteria, entities, id, read, schema};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use ferroshop_core::acl::{Action, Permissions};

use crate::auth::Principal;
use crate::permission;
use crate::{ApiError, AppState};

/// Builds the request context from `sw-language-id` / `sw-inheritance` headers.
pub(crate) fn context(headers: &HeaderMap, principal: &Principal) -> Result<Context, ApiError> {
    let mut ctx = Context { actor_id: Some(principal.id), ..Context::default() };
    if let Some(lang) = headers.get("sw-language-id").and_then(|v| v.to_str().ok()) {
        ctx.language_id = id::parse(lang).ok_or_else(|| ApiError::BadRequest("invalid sw-language-id".into()))?;
    }
    ctx.inheritance = headers
        .get("sw-inheritance")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));
    Ok(ctx)
}

fn capped(mut criteria: Criteria) -> Criteria {
    criteria.limit = Some(criteria.limit.unwrap_or(read::MAX_LIMIT).min(read::MAX_LIMIT));
    criteria
}

fn search_response(result: read::SearchResult) -> Json<Value> {
    Json(json!({ "total": result.total, "data": result.data, "aggregations": result.aggregations }))
}

/// `POST /api/search/{entity}` with a criteria body.
pub async fn search(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path(entity): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let ctx = context(&headers, &principal)?;
    let criteria: Criteria = if body.is_empty() {
        Criteria::default()
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(format!("invalid criteria: {e}")))?
    };
    let def = permission::definition(&entity)?;
    permission::require(&perms, def.name, Action::Read)?;
    permission::require_criteria(&perms, def, &criteria)?;
    Ok(search_response(read::search(&state.db, &entity, &capped(criteria), &ctx).await?))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    limit: Option<u64>,
    page: Option<u64>,
    term: Option<String>,
}

/// `GET /api/{entity}?limit=&page=&term=`.
pub async fn list(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path(entity): Path<String>,
    Query(query): Query<ListQuery>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let ctx = context(&headers, &principal)?;
    permission::require(&perms, permission::definition(&entity)?.name, Action::Read)?;
    let criteria = Criteria { limit: query.limit, page: query.page, term: query.term, ..Default::default() };
    Ok(search_response(read::search(&state.db, &entity, &capped(criteria), &ctx).await?))
}

pub async fn detail(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path((entity, entity_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let ctx = context(&headers, &principal)?;
    permission::require(&perms, permission::definition(&entity)?.name, Action::Read)?;
    let data = read::read(&state.db, &entity, &entity_id, &Criteria::default(), &ctx).await?;
    Ok(Json(json!({ "data": data })))
}

#[derive(Debug, Deserialize)]
pub struct WriteQuery {
    #[serde(rename = "_response")]
    response: Option<String>,
}

fn parse_body(body: &Bytes) -> Result<Value, ApiError> {
    serde_json::from_slice(body).map_err(|e| ApiError::BadRequest(format!("invalid JSON: {e}")))
}

/// Shopware answers writes with `204` + `Location`, or the entity when `?_response` is set.
async fn write_response(
    state: &AppState,
    entity: &str,
    entity_id: uuid::Uuid,
    ctx: &Context,
    query: &WriteQuery,
) -> Result<Response, ApiError> {
    let hex = id::format(entity_id);
    let def = entities::get(entity).expect("entity validated by write");
    let location = format!("/api/{}/{hex}", def.api_name());
    if query.response.as_deref().is_some_and(|r| !r.is_empty() && r != "false") {
        let data = read::read(&state.db, entity, &hex, &Criteria::default(), ctx).await?;
        return Ok((StatusCode::OK, [(header::LOCATION, location)], Json(json!({ "data": data }))).into_response());
    }
    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    Ok(response)
}

pub async fn create(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path(entity): Path<String>,
    Query(query): Query<WriteQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let ctx = context(&headers, &principal)?;
    let payload = parse_body(&body)?;
    let def = permission::definition(&entity)?;
    permission::require(&perms, def.name, Action::Create)?;
    permission::require_write(&perms, def, &payload)?;
    let entity_id = write::write(&state.db, &entity, &payload, WriteMode::Create, &ctx).await?;
    write_response(&state, &entity, entity_id, &ctx, &query).await
}

pub async fn update(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path((entity, entity_id)): Path<(String, String)>,
    Query(query): Query<WriteQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, ApiError> {
    let ctx = context(&headers, &principal)?;
    let mut payload = parse_body(&body)?;
    let def = permission::definition(&entity)?;
    permission::require(&perms, def.name, Action::Update)?;
    permission::require_write(&perms, def, &payload)?;
    match payload.as_object_mut() {
        Some(map) => {
            map.insert("id".into(), Value::String(entity_id));
        }
        None => return Err(ApiError::BadRequest("expected a JSON object".into())),
    }
    let entity_id = write::write(&state.db, &entity, &payload, WriteMode::Update, &ctx).await?;
    write_response(&state, &entity, entity_id, &ctx, &query).await
}

pub async fn remove(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path((entity, entity_id)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let ctx = context(&headers, &principal)?;
    let def = permission::definition(&entity)?;
    permission::require(&perms, def.name, Action::Delete)?;
    if def.name == "user" && id::parse(&entity_id) == Some(principal.id) {
        return Err(ApiError::BadRequest("you cannot delete your own account".into()));
    }
    write::delete(&state.db, &entity, &[entity_id], &ctx).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn remove_mapping(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path((entity, entity_id, association, reference_id)): Path<(String, String, String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    let ctx = context(&headers, &principal)?;
    let def = permission::definition(&entity)?;
    permission::require(&perms, def.name, Action::Update)?;
    if def.name == "user" && association == "aclRoles" && !perms.admin {
        return Err(ApiError::Forbidden("only administrators can change roles".into()));
    }
    write::delete_mapping(&state.db, &entity, &entity_id, &association, &reference_id, &ctx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The OpenAPI 3 document for the Admin-API, generated from the entity definitions.
pub fn openapi_document() -> Value {
    let mut paths = Map::new();
    let error = json!({ "description": "Error", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Errors" } } } });
    let id_param = json!({ "name": "id", "in": "path", "required": true, "schema": { "type": "string", "pattern": "^[0-9a-f]{32}$" } });
    let write_query = json!({ "name": "_response", "in": "query", "required": false, "schema": { "type": "string" },
        "description": "Return the written entity instead of 204." });
    for def in entities::ALL {
        let api = def.api_name();
        let schema_ref = json!({ "$ref": format!("#/components/schemas/{}", schema::schema_name(def.name)) });
        let list = json!({ "type": "object", "properties": {
            "total": { "type": "integer" }, "data": { "type": "array", "items": schema_ref }, "aggregations": { "type": "object" } } });
        let single = json!({ "type": "object", "properties": { "data": schema_ref } });
        let tag = json!([schema::schema_name(def.name)]);
        let ok =
            |schema: &Value| json!({ "description": "OK", "content": { "application/json": { "schema": schema } } });
        let body = json!({ "required": true, "content": { "application/json": { "schema": schema_ref } } });

        paths.insert(format!("/search/{api}"), json!({ "post": {
            "tags": tag, "operationId": format!("search{}", schema::schema_name(def.name)),
            "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/Criteria" } } } },
            "responses": { "200": ok(&list), "400": error } } }));
        paths.insert(
            format!("/{api}"),
            json!({
            "get": { "tags": tag, "operationId": format!("list{}", schema::schema_name(def.name)),
                "parameters": [
                    { "name": "limit", "in": "query", "schema": { "type": "integer" } },
                    { "name": "page", "in": "query", "schema": { "type": "integer" } },
                    { "name": "term", "in": "query", "schema": { "type": "string" } } ],
                "responses": { "200": ok(&list) } },
            "post": { "tags": tag, "operationId": format!("create{}", schema::schema_name(def.name)),
                "parameters": [write_query], "requestBody": body,
                "responses": { "204": { "description": "Created" }, "200": ok(&single), "400": error } } }),
        );
        paths.insert(
            format!("/{api}/{{id}}"),
            json!({
            "get": { "tags": tag, "operationId": format!("get{}", schema::schema_name(def.name)),
                "parameters": [id_param], "responses": { "200": ok(&single), "404": error } },
            "patch": { "tags": tag, "operationId": format!("update{}", schema::schema_name(def.name)),
                "parameters": [id_param, write_query], "requestBody": body,
                "responses": { "204": { "description": "Updated" }, "200": ok(&single), "400": error, "404": error } },
            "delete": { "tags": tag, "operationId": format!("delete{}", schema::schema_name(def.name)),
                "parameters": [id_param], "responses": { "204": { "description": "Deleted" }, "404": error } } }),
        );
    }
    paths.insert("/oauth/token".into(), json!({ "post": {
        "tags": ["Auth"], "operationId": "token", "security": [],
        "requestBody": { "content": { "application/json": { "schema": { "$ref": "#/components/schemas/TokenRequest" } } } },
        "responses": { "200": ok_token(), "401": error } } }));
    paths.insert(
        "/_info/me".into(),
        json!({ "get": {
        "tags": ["Info"], "operationId": "me",
        "responses": { "200": { "description": "OK", "content": { "application/json": { "schema": {
            "type": "object", "properties": { "data": { "$ref": "#/components/schemas/User" } } } } } } } } }),
    );

    let mut schemas = schema::openapi_components();
    schemas.insert("Errors".into(), json!({ "type": "object", "properties": { "errors": { "type": "array", "items": {
        "type": "object", "properties": {
            "status": { "type": "string" }, "code": { "type": "string" }, "title": { "type": "string" },
            "detail": { "type": "string" }, "source": { "type": "object", "properties": { "pointer": { "type": "string" } } } } } } } }));
    schemas.insert(
        "Criteria".into(),
        json!({ "type": "object", "properties": {
        "page": { "type": "integer" }, "limit": { "type": "integer" }, "term": { "type": "string" },
        "ids": { "type": "array", "items": { "type": "string" } },
        "filter": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
        "post-filter": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
        "sort": { "type": "array", "items": { "type": "object", "properties": {
            "field": { "type": "string" }, "order": { "type": "string", "enum": ["ASC", "DESC"] } } } },
        "associations": { "type": "object", "additionalProperties": true },
        "aggregations": { "type": "array", "items": { "type": "object", "additionalProperties": true } },
        "includes": { "type": "object", "additionalProperties": { "type": "array", "items": { "type": "string" } } },
        "total-count-mode": { "type": "integer" } } }),
    );
    schemas.insert(
        "TokenRequest".into(),
        json!({ "type": "object", "required": ["grant_type"], "properties": {
        "grant_type": { "type": "string", "enum": ["password", "refresh_token", "client_credentials"] },
        "client_id": { "type": "string" }, "client_secret": { "type": "string" },
        "username": { "type": "string" }, "password": { "type": "string" }, "refresh_token": { "type": "string" },
        "scopes": { "type": "string" } } }),
    );
    schemas.insert(
        "TokenResponse".into(),
        json!({ "type": "object", "properties": {
        "token_type": { "type": "string" }, "expires_in": { "type": "integer" },
        "access_token": { "type": "string" }, "refresh_token": { "type": "string" } } }),
    );

    json!({
        "openapi": "3.0.3",
        "info": { "title": "ferroshop Admin API", "version": env!("CARGO_PKG_VERSION") },
        "servers": [{ "url": "/api" }],
        "security": [{ "oAuth": [] }],
        "paths": paths,
        "components": {
            "schemas": schemas,
            "securitySchemes": { "oAuth": { "type": "http", "scheme": "bearer", "bearerFormat": "JWT" } }
        }
    })
}

fn ok_token() -> Value {
    json!({ "description": "OK", "content": { "application/json": { "schema": { "$ref": "#/components/schemas/TokenResponse" } } } })
}
