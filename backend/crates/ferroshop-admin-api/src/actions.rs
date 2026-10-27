//! Non-CRUD endpoints: media upload/delivery and order state transitions.

use axum::Json;
use axum::body::{Body, Bytes};
use axum::extract::{Extension, Path, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ferroshop_core::acl::{Action, Permissions};
use ferroshop_core::order_state::{self, Machine};
use ferroshop_core::{id, media};
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::Row;

use crate::auth::Principal;
use crate::entity::context;
use crate::permission::require;
use crate::{ApiError, AppState};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadQuery {
    extension: String,
    file_name: Option<String>,
}

/// `POST /api/_action/media/{id}/upload?extension=png&fileName=shirt` with the raw file as body.
pub async fn upload_media(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path(media_id): Path<String>,
    Query(query): Query<UploadQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    require(&perms, "media", Action::Update)?;
    let ctx = context(&headers, &principal)?;
    let media_id = id::parse(&media_id)
        .ok_or_else(|| ferroshop_core::Error::NotFound { entity: "media".into(), id: media_id.clone() })?;
    let file_name = query.file_name.unwrap_or_else(|| id::format(media_id));
    media::upload(&state.db, &state.media, media_id, &file_name, &query.extension, body, &ctx).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /media/{path}`: public, immutable (every upload gets a new path).
pub async fn serve_media(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let extension = path.rsplit_once('.').map(|(_, ext)| ext).unwrap_or_default();
    let (Some(mime), Some(bytes)) = (media::mime_for(extension), state.media.get(&path).await) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    (
        [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Body::from(bytes),
    )
        .into_response()
}

async fn order_states(state: &AppState, order_id: uuid::Uuid) -> Result<Value, ApiError> {
    let row = sqlx::query("SELECT state, payment_state, delivery_state FROM sales_order WHERE id = $1")
        .bind(order_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| ferroshop_core::Error::NotFound { entity: "order".into(), id: id::format(order_id) })?;
    let mut out = serde_json::Map::new();
    for (machine, column) in
        [(Machine::Order, "state"), (Machine::Payment, "payment_state"), (Machine::Delivery, "delivery_state")]
    {
        let current: String = row.try_get(column)?;
        out.insert(
            machine.api_name().into(),
            json!({ "current": current, "transitions": machine.available(&current) }),
        );
    }
    Ok(Value::Object(out))
}

fn parse_order_id(raw: &str) -> Result<uuid::Uuid, ApiError> {
    id::parse(raw).ok_or_else(|| ferroshop_core::Error::NotFound { entity: "order".into(), id: raw.into() }.into())
}

/// `GET /api/_action/order/{id}/transitions`: current states and possible transitions.
pub async fn order_transitions(
    State(state): State<AppState>,
    Extension(perms): Extension<Permissions>,
    Path(order_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    require(&perms, "order", Action::Read)?;
    Ok(Json(json!({ "data": order_states(&state, parse_order_id(&order_id)?).await? })))
}

/// `POST /api/_action/order/{id}/{state|payment-state|delivery-state}/{transition}`.
pub async fn order_transition(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
    Path((order_id, machine, transition)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    require(&perms, "order", Action::Update)?;
    let ctx = context(&headers, &principal)?;
    let machine =
        Machine::parse(&machine).ok_or_else(|| ApiError::BadRequest(format!("unknown state machine `{machine}`")))?;
    let order_id = parse_order_id(&order_id)?;
    order_state::transition(&state.db, order_id, machine, &transition, &ctx).await?;
    Ok(Json(json!({ "data": order_states(&state, order_id).await? })))
}
