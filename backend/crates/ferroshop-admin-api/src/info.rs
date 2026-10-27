use axum::Json;
use axum::body::Bytes;
use axum::extract::{Extension, State};
use axum::http::HeaderMap;
use ferroshop_core::acl::Permissions;
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Criteria, entities, password, read, schema};
use serde_json::{Map, Value, json};
use sqlx::Row;

use crate::auth::{Principal, PrincipalKind};
use crate::{ApiError, AppState};

pub async fn version() -> Json<Value> {
    Json(json!({ "version": env!("CARGO_PKG_VERSION") }))
}

pub async fn entity_schema() -> Json<Value> {
    Json(schema::entity_schema())
}

/// The logged-in admin user plus its effective privileges (`GET /api/_info/me`).
pub async fn me(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    Extension(perms): Extension<Permissions>,
) -> Result<Json<Value>, ApiError> {
    if principal.kind != PrincipalKind::User {
        return Err(ApiError::BadRequest("integrations have no user profile".into()));
    }
    let criteria = Criteria::default();
    let mut user =
        read::read(&state.db, "user", &ferroshop_core::id::format(principal.id), &criteria, &Context::default())
            .await?;
    user["aclPrivileges"] = json!(perms.privileges);
    // Lets the UI decide which modules to show without knowing the privilege rules.
    let entities: Map<String, Value> = entities::ALL
        .iter()
        .map(|def| {
            let allowed: Vec<&str> = ferroshop_core::acl::Action::ALL
                .iter()
                .filter(|a| perms.allows(def.name, **a))
                .map(|a| a.as_str())
                .collect();
            (def.name.to_string(), json!(allowed))
        })
        .collect();
    user["permissions"] = Value::Object(entities);
    Ok(Json(json!({ "data": user })))
}

const SELF_EDITABLE: &[&str] = &["firstName", "lastName", "email", "localeCode", "password"];

/// `PATCH /api/_info/me`: own profile; changing the password requires `currentPassword`.
pub async fn update_me(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<axum::http::StatusCode, ApiError> {
    if principal.kind != PrincipalKind::User {
        return Err(ApiError::BadRequest("integrations have no user profile".into()));
    }
    let ctx = crate::entity::context(&headers, &principal)?;
    let Value::Object(mut payload) =
        serde_json::from_slice::<Value>(&body).map_err(|e| ApiError::BadRequest(format!("invalid JSON: {e}")))?
    else {
        return Err(ApiError::BadRequest("expected a JSON object".into()));
    };
    let current = payload.remove("currentPassword");
    if let Some(key) = payload.keys().find(|k| !SELF_EDITABLE.contains(&k.as_str())) {
        return Err(ApiError::Forbidden(format!("`{key}` cannot be changed here")));
    }
    let changes_password = payload.contains_key("password");
    if changes_password {
        let hash: String = sqlx::query("SELECT password FROM admin_user WHERE id = $1")
            .bind(principal.id)
            .fetch_one(&state.db)
            .await?
            .try_get("password")?;
        if !current.as_ref().and_then(Value::as_str).is_some_and(|p| password::verify(p, &hash)) {
            return Err(ferroshop_core::Error::Validation(vec![ferroshop_core::WriteError::new(
                "/currentPassword",
                "FRAMEWORK__INVALID_CURRENT_PASSWORD",
                "The current password is wrong.",
            )])
            .into());
        }
    }
    payload.insert("id".into(), json!(ferroshop_core::id::format(principal.id)));
    write::write(&state.db, "user", &Value::Object(payload), WriteMode::Update, &ctx).await?;
    if changes_password {
        // Other sessions must log in again after a password change.
        sqlx::query("DELETE FROM refresh_token WHERE user_id = $1").bind(principal.id).execute(&state.db).await?;
    }
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn openapi() -> Json<Value> {
    Json(crate::entity::openapi_document())
}
