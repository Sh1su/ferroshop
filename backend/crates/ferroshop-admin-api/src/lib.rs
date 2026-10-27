//! Admin-API: OAuth2, generic entity CRUD/search and schema endpoints under `/api`.

mod auth;
mod entity;
mod error;
mod info;

use axum::Router;
use axum::middleware;
use axum::routing::{delete, get, post};
use ferroshop_core::Db;

pub use auth::TokenKeys;
pub use error::ApiError;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub keys: TokenKeys,
}

impl AppState {
    pub fn new(db: Db, secret: &str) -> Self {
        Self { db, keys: TokenKeys::new(secret) }
    }
}

/// All `/api` routes. Everything except the token endpoint requires a bearer token.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/_info/me", get(info::me))
        .route("/_info/entity-schema.json", get(info::entity_schema))
        .route("/search/{entity}", post(entity::search))
        .route("/{entity}", get(entity::list).post(entity::create))
        .route("/{entity}/{id}", get(entity::detail).patch(entity::update).delete(entity::remove))
        .route("/{entity}/{id}/{association}/{reference_id}", delete(entity::remove_mapping))
        .route_layer(middleware::from_fn_with_state(state.clone(), auth::require_token));

    let public = Router::new()
        .route("/oauth/token", post(auth::token))
        .route("/_info/openapi3.json", get(info::openapi))
        .route("/_info/version", get(info::version));

    Router::new().nest("/api", public.merge(protected)).with_state(state)
}
