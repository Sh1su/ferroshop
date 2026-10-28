//! Store-API: Shopware-compatible `/store-api` for headless storefronts such as Shopware Frontends.
//!
//! Every request names its sales channel with `sw-access-key`; `sw-context-token` identifies the
//! visitor's context (language, currency, later customer and cart) and is echoed on every response.

mod account;
mod cart;
mod catalog;
mod context;
mod guard;
mod listing;
mod order;
mod pricing;

use axum::Router;
use axum::middleware;
use axum::routing::{get, patch, post};
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
        .route("/checkout/cart", get(cart::show).delete(cart::clear))
        .route("/checkout/cart/line-item", post(cart::add).patch(cart::update).delete(cart::remove))
        .route("/checkout/cart/line-item/delete", post(cart::remove_post))
        .route("/checkout/order", post(order::place))
        .route("/order", post(order::list).get(order::list))
        .route("/account/register", post(account::register))
        .route("/account/login", post(account::login))
        .route("/account/logout", post(account::logout))
        .route("/account/customer", post(account::customer).get(account::customer))
        .route("/account/change-profile", post(account::change_profile))
        .route("/account/change-email", post(account::change_email))
        .route("/account/change-password", post(account::change_password))
        .route("/account/list-address", post(account::list_addresses).get(account::list_addresses))
        .route("/account/address", post(account::create_address))
        .route("/account/address/{id}", patch(account::update_address).delete(account::delete_address))
        .route("/account/address/default-billing/{id}", patch(account::default_billing))
        .route("/account/address/default-shipping/{id}", patch(account::default_shipping))
        .route("/seo-url", post(catalog::seo_urls).get(catalog::seo_urls))
        .route("/currency", post(catalog::currencies).get(catalog::currencies))
        .route("/language", post(catalog::languages).get(catalog::languages))
        .route("/country", post(catalog::countries).get(catalog::countries))
        .route("/payment-method", post(catalog::payment_methods).get(catalog::payment_methods))
        .route("/shipping-method", post(catalog::shipping_methods).get(catalog::shipping_methods))
        .route_layer(middleware::from_fn_with_state(state.clone(), context::resolve));

    Router::new().nest("/store-api", routes).with_state(state)
}
