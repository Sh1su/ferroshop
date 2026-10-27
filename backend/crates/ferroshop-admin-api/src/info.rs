use axum::Json;
use axum::extract::{Extension, State};
use ferroshop_core::{Context, Criteria, read, schema};
use serde_json::{Value, json};

use crate::auth::{Principal, PrincipalKind};
use crate::{ApiError, AppState};

pub async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

pub async fn entity_schema() -> Json<Value> {
    Json(schema::entity_schema())
}

/// The logged-in admin user (`GET /api/_info/me`).
pub async fn me(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Value>, ApiError> {
    if principal.kind != PrincipalKind::User {
        return Err(ApiError::BadRequest("integrations have no user profile".into()));
    }
    let criteria = Criteria::default();
    let user = read::read(&state.db, "user", &ferroshop_core::id::format(principal.id), &criteria, &Context::default())
        .await?;
    Ok(Json(json!({ "data": user })))
}

pub async fn openapi() -> Json<Value> {
    Json(crate::entity::openapi_document())
}
