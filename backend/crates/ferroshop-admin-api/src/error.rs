use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use ferroshop_core::Error;
use serde_json::{Value, json};

/// Errors rendered in Shopware's format: `{"errors": [{status, code, title, detail, source}]}`.
#[derive(Debug)]
pub enum ApiError {
    Core(Error),
    Unauthorized(&'static str),
    Forbidden(String),
    BadRequest(String),
    /// Any status with a specific Shopware error code, e.g. `CHECKOUT__CART_EMPTY`.
    Code(StatusCode, &'static str, String),
}

impl From<Error> for ApiError {
    fn from(err: Error) -> Self {
        Self::Core(err)
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        Self::Core(Error::Database(err))
    }
}

fn entry(status: StatusCode, code: &str, detail: impl Into<String>) -> Value {
    json!({
        "status": status.as_u16().to_string(),
        "code": code,
        "title": status.canonical_reason().unwrap_or_default(),
        "detail": detail.into(),
    })
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, errors) = match self {
            Self::Unauthorized(detail) => {
                (StatusCode::UNAUTHORIZED, vec![entry(StatusCode::UNAUTHORIZED, "FRAMEWORK__UNAUTHORIZED", detail)])
            }
            Self::Forbidden(detail) => {
                (StatusCode::FORBIDDEN, vec![entry(StatusCode::FORBIDDEN, "FRAMEWORK__PERMISSION_DENIED", detail)])
            }
            Self::Code(status, code, detail) => (status, vec![entry(status, code, detail)]),
            Self::BadRequest(detail) => {
                (StatusCode::BAD_REQUEST, vec![entry(StatusCode::BAD_REQUEST, "FRAMEWORK__INVALID_REQUEST", detail)])
            }
            Self::Core(err) => match err {
                Error::UnknownEntity(_) => (
                    StatusCode::NOT_FOUND,
                    vec![entry(StatusCode::NOT_FOUND, "FRAMEWORK__DEFINITION_NOT_FOUND", err.to_string())],
                ),
                Error::NotFound { .. } => (
                    StatusCode::NOT_FOUND,
                    vec![entry(StatusCode::NOT_FOUND, "FRAMEWORK__RESOURCE_NOT_FOUND", err.to_string())],
                ),
                Error::InvalidCriteria(_) => (
                    StatusCode::BAD_REQUEST,
                    vec![entry(StatusCode::BAD_REQUEST, "FRAMEWORK__INVALID_CRITERIA", err.to_string())],
                ),
                Error::Conflict(detail) => (
                    StatusCode::CONFLICT,
                    vec![entry(StatusCode::CONFLICT, "FRAMEWORK__RESTRICT_DELETE_VIOLATION", detail)],
                ),
                Error::Validation(list) => {
                    let errors = list
                        .into_iter()
                        .map(|e| {
                            let mut v = entry(StatusCode::BAD_REQUEST, e.code, e.detail);
                            v["source"] = json!({ "pointer": e.pointer });
                            v
                        })
                        .collect();
                    (StatusCode::BAD_REQUEST, errors)
                }
                Error::Storage(detail) => {
                    tracing::error!(error = %detail, "storage error");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        vec![entry(StatusCode::INTERNAL_SERVER_ERROR, "CONTENT__MEDIA_STORAGE", "storage error")],
                    )
                }
                Error::Database(db) => {
                    tracing::error!(error = %db, "database error");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        vec![entry(StatusCode::INTERNAL_SERVER_ERROR, "FRAMEWORK__INTERNAL", "internal error")],
                    )
                }
            },
        };
        (status, Json(json!({ "errors": errors }))).into_response()
    }
}
