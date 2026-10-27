//! OAuth2 token endpoint (password, refresh_token, client_credentials) and bearer-token guard.

use axum::Json;
use axum::body::Bytes;
use axum::extract::{Request, State};
use axum::http::{HeaderMap, header};
use axum::middleware::Next;
use axum::response::Response;
use chrono::{Duration, Utc};
use ferroshop_core::{id, password};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{ApiError, AppState};

pub const ACCESS_TOKEN_TTL: i64 = 600;
pub const REFRESH_TOKEN_TTL_DAYS: i64 = 7;

#[derive(Clone)]
pub struct TokenKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl TokenKeys {
    pub fn new(secret: &str) -> Self {
        Self {
            encoding: EncodingKey::from_secret(secret.as_bytes()),
            decoding: DecodingKey::from_secret(secret.as_bytes()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrincipalKind {
    User,
    Integration,
}

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    kind: PrincipalKind,
    iat: i64,
    exp: i64,
}

/// The authenticated caller, available as a request extension on protected routes.
#[derive(Debug, Clone, Copy)]
pub struct Principal {
    pub kind: PrincipalKind,
    pub id: Uuid,
}

#[derive(Debug, Default, Deserialize)]
struct TokenRequest {
    grant_type: String,
    username: Option<String>,
    password: Option<String>,
    refresh_token: Option<String>,
    client_id: Option<String>,
    client_secret: Option<String>,
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn issue_access_token(keys: &TokenKeys, principal: Principal) -> Result<String, ApiError> {
    let now = Utc::now().timestamp();
    let claims = Claims { sub: id::format(principal.id), kind: principal.kind, iat: now, exp: now + ACCESS_TOKEN_TTL };
    jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &keys.encoding)
        .map_err(|_| ApiError::Unauthorized("could not issue token"))
}

async fn issue_refresh_token(state: &AppState, user_id: Uuid) -> Result<String, ApiError> {
    let token = hex::encode(rand::random::<[u8; 32]>());
    sqlx::query("INSERT INTO refresh_token (token_hash, user_id, expires_at) VALUES ($1, $2, $3)")
        .bind(hash_token(&token))
        .bind(user_id)
        .bind(Utc::now() + Duration::days(REFRESH_TOKEN_TTL_DAYS))
        .execute(&state.db)
        .await?;
    Ok(token)
}

async fn user_response(state: &AppState, user_id: Uuid) -> Result<Json<Value>, ApiError> {
    let access = issue_access_token(&state.keys, Principal { kind: PrincipalKind::User, id: user_id })?;
    let refresh = issue_refresh_token(state, user_id).await?;
    Ok(Json(json!({
        "token_type": "Bearer",
        "expires_in": ACCESS_TOKEN_TTL,
        "access_token": access,
        "refresh_token": refresh,
    })))
}

const INVALID_GRANT: ApiError = ApiError::Unauthorized("The user credentials were incorrect.");

/// `POST /api/oauth/token`, accepting JSON or form-encoded bodies like Shopware.
pub async fn token(State(state): State<AppState>, headers: HeaderMap, body: Bytes) -> Result<Json<Value>, ApiError> {
    let is_form = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/x-www-form-urlencoded"));
    let request: TokenRequest = if is_form {
        serde_urlencoded::from_bytes(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?
    } else {
        serde_json::from_slice(&body).map_err(|e| ApiError::BadRequest(e.to_string()))?
    };

    match request.grant_type.as_str() {
        "password" => {
            let (Some(username), Some(password)) = (request.username, request.password) else {
                return Err(ApiError::BadRequest("username and password are required".into()));
            };
            let row = sqlx::query("SELECT id, password FROM admin_user WHERE username = $1 AND active")
                .bind(&username)
                .fetch_optional(&state.db)
                .await?;
            let Some(row) = row else {
                // Hash anyway so unknown users take as long as wrong passwords.
                password::verify(&password, "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQ$aGFzaA");
                return Err(INVALID_GRANT);
            };
            let hash: String = row.try_get("password")?;
            if !password::verify(&password, &hash) {
                return Err(INVALID_GRANT);
            }
            user_response(&state, row.try_get("id")?).await
        }
        "refresh_token" => {
            let token = request.refresh_token.ok_or(ApiError::BadRequest("refresh_token is required".into()))?;
            // Rotation: a refresh token can be used exactly once.
            let row = sqlx::query(
                "DELETE FROM refresh_token r USING admin_user u
                 WHERE r.token_hash = $1 AND r.user_id = u.id AND u.active AND r.expires_at > now()
                 RETURNING r.user_id",
            )
            .bind(hash_token(&token))
            .fetch_optional(&state.db)
            .await?;
            let row = row.ok_or(ApiError::Unauthorized("The refresh token is invalid."))?;
            user_response(&state, row.try_get("user_id")?).await
        }
        "client_credentials" => {
            let (Some(key), Some(secret)) = (request.client_id, request.client_secret) else {
                return Err(ApiError::BadRequest("client_id and client_secret are required".into()));
            };
            let row = sqlx::query("SELECT id, secret_access_key FROM integration WHERE access_key = $1")
                .bind(&key)
                .fetch_optional(&state.db)
                .await?
                .ok_or(ApiError::Unauthorized("Client authentication failed."))?;
            let hash: String = row.try_get("secret_access_key")?;
            if !password::verify(&secret, &hash) {
                return Err(ApiError::Unauthorized("Client authentication failed."));
            }
            let principal = Principal { kind: PrincipalKind::Integration, id: row.try_get("id")? };
            Ok(Json(json!({
                "token_type": "Bearer",
                "expires_in": ACCESS_TOKEN_TTL,
                "access_token": issue_access_token(&state.keys, principal)?,
            })))
        }
        other => Err(ApiError::BadRequest(format!("unsupported grant_type `{other}`"))),
    }
}

/// Middleware: validates the bearer token and stores the [`Principal`] in the request.
pub async fn require_token(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized("Header \"Authorization\" is required."))?;
    let data = jsonwebtoken::decode::<Claims>(token, &state.keys.decoding, &Validation::new(Algorithm::HS256))
        .map_err(|_| ApiError::Unauthorized("The access token is invalid or expired."))?;
    let id = id::parse(&data.claims.sub).ok_or(ApiError::Unauthorized("The access token is invalid or expired."))?;
    request.extensions_mut().insert(Principal { kind: data.claims.kind, id });
    Ok(next.run(request).await)
}
