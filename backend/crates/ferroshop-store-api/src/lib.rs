//! Store-API: Shopware-compatible `/store-api` for headless storefronts such as Shopware Frontends.
//!
//! Every request names its sales channel with `sw-access-key`; `sw-context-token` identifies the
//! visitor's context (language, currency, later customer and cart) and is echoed on every response.

mod catalog;
mod context;
mod guard;
mod listing;
mod pricing;

use axum::Router;
use axum::middleware;
use axum::routing::{get, post};
use ferroshop_core::Db;

pub use context::SalesChannelContext;

#[derive(Clone)]
pub struct StoreState {
    pub db: Db,
}

impl StoreState {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

/// All `/store-api` routes. Shopware accepts GET and POST for read routes; both are wired.
pub fn router(state: StoreState) -> Router {
    let routes = Router::new()
        .route("/context", get(context::show).patch(context::update))
        .route("/product", post(catalog::products).get(catalog::products))
        .route("/product/{id}", post(catalog::product).get(catalog::product))
        .route("/product-listing/{category_id}", post(listing::category_listing).get(listing::category_listing))
        .route("/search", post(listing::search_listing).get(listing::search_listing))
        .route("/search-suggest", post(listing::suggest).get(listing::suggest))
        .route("/category", post(catalog::categories).get(catalog::categories))
        .route("/category/{id}", post(catalog::category).get(catalog::category))
        .route("/navigation/{active_id}/{root_id}", post(catalog::navigation).get(catalog::navigation))
        .route("/currency", post(catalog::currencies).get(catalog::currencies))
        .route("/language", post(catalog::languages).get(catalog::languages))
        .route("/country", post(catalog::countries).get(catalog::countries))
        .route("/payment-method", post(catalog::payment_methods).get(catalog::payment_methods))
        .route("/shipping-method", post(catalog::shipping_methods).get(catalog::shipping_methods))
        .route_layer(middleware::from_fn_with_state(state.clone(), context::resolve));

    Router::new().nest("/store-api", routes).with_state(state)
}
