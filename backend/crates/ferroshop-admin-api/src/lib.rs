//! Admin-API: OAuth2, generic entity CRUD/search and schema endpoints under `/api`.

mod actions;
mod auth;
mod entity;
mod error;
mod info;
mod permission;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::middleware;
use axum::routing::{delete, get, post};
use ferroshop_core::Db;
use ferroshop_core::media::MediaStore;

pub use auth::TokenKeys;
pub use error::ApiError;

/// Maximum upload size for media files.
pub const MAX_UPLOAD_BYTES: usize = 50 * 1024 * 1024;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub keys: TokenKeys,
    pub media: MediaStore,
}

impl AppState {
    pub fn new(db: Db, secret: &str, media: MediaStore) -> Self {
        Self { db, keys: TokenKeys::new(secret), media }
    }
}

/// Public `/media/...` file delivery.
pub fn media_router(state: AppState) -> Router {
    Router::new().route("/media/{*path}", get(actions::serve_media)).with_state(state)
}

/// All `/api` routes. Everything except the token endpoint requires a bearer token.
pub fn router(state: AppState) -> Router {
    let protected = Router::new()
        .route("/_info/me", get(info::me).patch(info::update_me))
        .route("/_action/media/{id}/upload", post(actions::upload_media).layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES)))
        .route("/_action/order/{id}/transitions", get(actions::order_transitions))
        .route("/_action/order/{id}/{machine}/{transition}", post(actions::order_transition))
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
