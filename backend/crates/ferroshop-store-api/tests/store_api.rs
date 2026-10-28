//! Store-API over HTTP: sales channel context, catalogue visibility, prices, listings, navigation.

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, defaults, id};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ACCESS_KEY: &str = "SWSCFERROSHOPDEFAULT00000";

struct Reply {
    status: StatusCode,
    token: Option<String>,
    body: Value,
}

struct Store {
    app: Router,
}

impl Store {
    fn new(db: Db) -> Self {
        Self { app: ferroshop_store_api::router(ferroshop_store_api::StoreState::new(db)) }
    }

    async fn send(&self, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> Reply {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("sw-access-key", ACCESS_KEY);
        if let Some(token) = token {
            request = request.header("sw-context-token", token);
        }
        let body = body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty);
        let response = self.app.clone().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let token = response.headers().get("sw-context-token").map(|v| v.to_str().unwrap().to_string());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        Reply { status, token, body: serde_json::from_slice(&bytes).unwrap_or(Value::Null) }
    }

    async fn post(&self, uri: &str, body: Value) -> Reply {
        self.send(Method::POST, uri, None, Some(body)).await
    }
}

async fn create(db: &Db, entity: &str, payload: Value) -> String {
    id::format(write::write(db, entity, &payload, WriteMode::Create, &Context::default()).await.unwrap())
}

fn price(gross: f64) -> Value {
    let net = (gross / 1.19 * 100.0).round() / 100.0;
    json!([{ "currencyId": id::format(defaults::CURRENCY), "gross": gross, "net": net, "linked": true }])
}

struct Catalog {
    shoes: String,
    sneakers: String,
    hidden_category: String,
    acme: String,
    boot: String,
    sneaker: String,
    variant: String,
}

/// root → Shoes → Sneakers; an inactive category; products in both, a variant and an inactive one.
async fn catalog(db: &Db) -> Catalog {
    let root = id::format(defaults::ROOT_CATEGORY);
    let tax = id::format(defaults::TAX_STANDARD);
    let shoes = create(db, "category", json!({ "parentId": root, "name": "Shoes", "position": 1 })).await;
    let sneakers = create(db, "category", json!({ "parentId": shoes, "name": "Sneakers" })).await;
    let hidden_category = create(db, "category", json!({ "parentId": root, "name": "Hidden", "active": false })).await;
    create(db, "category", json!({ "parentId": root, "name": "Accessories", "position": 2 })).await;
    let acme = create(db, "product_manufacturer", json!({ "name": "Acme" })).await;
    let zenith = create(db, "product_manufacturer", json!({ "name": "Zenith" })).await;
    let product = |number: &str, name: &str, gross: f64, manufacturer: &str, category: &str| {
        json!({
            "productNumber": number, "name": name, "stock": 10, "taxId": tax, "price": price(gross),
            "manufacturerId": manufacturer, "active": true, "categories": [{ "id": category }],
        })
    };
    let boot = create(db, "product", product("B-1", "Boot", 119.0, &acme, &shoes)).await;
    let sneaker = create(db, "product", product("S-1", "Sneaker", 59.5, &zenith, &sneakers)).await;
    let variant = create(
        db,
        "product",
        json!({ "parentId": sneaker, "productNumber": "S-1.1", "stock": 3, "name": "Sneaker red" }),
    )
    .await;
    let mut inactive = product("X-1", "Prototype", 10.0, &acme, &shoes);
    inactive["active"] = json!(false);
    create(db, "product", inactive).await;
    Catalog { shoes, sneakers, hidden_category, acme, boot, sneaker, variant }
}

fn ids(elements: &Value) -> Vec<String> {
    elements.as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap().to_string()).collect()
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn access_key_and_context_token(db: Db) {
    let store = Store::new(db.clone());
    let request = Request::post("/store-api/context").body(Body::empty()).unwrap();
    let response = store.app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let request = Request::get("/store-api/context").header("sw-access-key", "nope").body(Body::empty()).unwrap();
    assert_eq!(store.app.clone().oneshot(request).await.unwrap().status(), StatusCode::UNAUTHORIZED);

    let first = store.send(Method::GET, "/store-api/context", None, None).await;
    assert_eq!(first.status, StatusCode::OK);
    let token = first.token.clone().unwrap();
    assert_eq!(first.body["token"], token);
    assert_eq!(first.body["apiAlias"], "sales_channel_context");
    assert_eq!(first.body["currency"]["isoCode"], "EUR");
    assert_eq!(first.body["taxState"], "gross");
    assert_eq!(first.body["shippingLocation"]["country"]["iso"], "DE");

    // Switching language persists under the token.
    let de = id::format(defaults::LANGUAGE_DE);
    let patched =
        store.send(Method::PATCH, "/store-api/context", Some(&token), Some(json!({ "languageId": de }))).await;
    assert_eq!(patched.status, StatusCode::OK, "{}", patched.body);
    assert_eq!(patched.body["contextToken"], token);
    let again = store.send(Method::GET, "/store-api/context", Some(&token), None).await;
    assert_eq!(again.token.as_deref(), Some(token.as_str()));
    assert_eq!(again.body["context"]["languageIdChain"][0], de);
    assert_eq!(again.body["languageInfo"]["localeCode"], "de-DE");

    let invalid = store
        .send(Method::PATCH, "/store-api/context", Some(&token), Some(json!({ "currencyId": "0".repeat(32) })))
        .await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    assert_eq!(invalid.body["errors"][0]["source"]["pointer"], "/currencyId");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn products_are_active_only_and_priced(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;

    let reply = store
        .post("/store-api/product", json!({ "filter": [{ "type": "equals", "field": "parentId", "value": null }] }))
        .await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    assert_eq!(reply.body["apiAlias"], "dal_entity_search_result");
    let found = ids(&reply.body["elements"]);
    assert_eq!(found.len(), 2, "the inactive product is hidden");
    assert!(found.contains(&catalog.boot) && found.contains(&catalog.sneaker));

    let boot = store
        .post(&format!("/store-api/product/{}", catalog.boot), json!({ "associations": { "manufacturer": {} } }))
        .await;
    assert_eq!(boot.status, StatusCode::OK);
    assert_eq!(boot.body["apiAlias"], "product_detail");
    let product = &boot.body["product"];
    assert_eq!(product["manufacturer"]["name"], "Acme");
    assert_eq!(product["calculatedPrice"]["unitPrice"], 119.0);
    assert_eq!(product["calculatedPrice"]["calculatedTaxes"][0]["tax"], 19.0);
    assert_eq!(product["calculatedPrice"]["calculatedTaxes"][0]["taxRate"], 19.0);

    // Variants inherit price and tax from their parent; includes still allow calculated prices.
    let variant = store
        .post(
            &format!("/store-api/product/{}", catalog.variant),
            json!({ "includes": { "product": ["id", "name", "calculatedPrice"] } }),
        )
        .await;
    let product = &variant.body["product"];
    assert_eq!(product["calculatedPrice"]["unitPrice"], 59.5);
    assert!(product.get("price").is_none(), "includes are applied after pricing");

    // Prices follow the context currency, converted from the default currency by its factor.
    let chf = create(
        &db,
        "currency",
        json!({ "isoCode": "CHF", "factor": 0.95, "symbol": "Fr.", "shortName": "CHF", "name": "Swiss franc" }),
    )
    .await;
    let token = store.send(Method::GET, "/store-api/context", None, None).await.token.unwrap();
    store.send(Method::PATCH, "/store-api/context", Some(&token), Some(json!({ "currencyId": chf }))).await;
    let converted = store.send(Method::POST, &format!("/store-api/product/{}", catalog.boot), Some(&token), None).await;
    assert_eq!(converted.body["product"]["calculatedPrice"]["unitPrice"], 113.05);

    let unknown = store.post("/store-api/product", json!({ "associations": { "nope": {} } })).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    let missing = store.post(&format!("/store-api/product/{}", "0".repeat(32)), json!({})).await;
    assert_eq!(missing.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn category_listing_filters_sorts_and_aggregates(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;
    let uri = format!("/store-api/product-listing/{}", catalog.shoes);

    let listing = store.post(&uri, json!({})).await;
    assert_eq!(listing.status, StatusCode::OK, "{}", listing.body);
    assert_eq!(listing.body["apiAlias"], "product_listing");
    assert_eq!(listing.body["sorting"], "name-asc");
    // Products of sub-categories are listed, variants and inactive products are not.
    assert_eq!(ids(&listing.body["elements"]), vec![catalog.boot.clone(), catalog.sneaker.clone()]);
    assert_eq!(listing.body["total"], 2);
    let manufacturers = &listing.body["aggregations"]["manufacturer"];
    assert_eq!(manufacturers["apiAlias"], "manufacturer_aggregation");
    assert_eq!(manufacturers["entities"].as_array().unwrap().len(), 2);
    assert_eq!(listing.body["aggregations"]["price"]["min"], 59.5);
    assert_eq!(listing.body["aggregations"]["price"]["max"], 119.0);

    let sorted = store.post(&uri, json!({ "order": "price-desc", "limit": 1, "p": 2 })).await;
    assert_eq!(ids(&sorted.body["elements"]), vec![catalog.sneaker.clone()]);
    assert_eq!(sorted.body["total"], 2);
    assert_eq!(sorted.body["page"], 2);

    // Shopper filters are post filters: the manufacturer aggregation keeps both options.
    let filtered = store.post(&format!("{uri}?manufacturer={}&max-price=200", catalog.acme), json!({})).await;
    assert_eq!(ids(&filtered.body["elements"]), vec![catalog.boot.clone()]);
    assert_eq!(filtered.body["aggregations"]["manufacturer"]["entities"].as_array().unwrap().len(), 2);
    assert_eq!(filtered.body["currentFilters"]["manufacturer"][0], catalog.acme);

    let cheap = store.post(&uri, json!({ "max-price": 60 })).await;
    assert_eq!(ids(&cheap.body["elements"]), vec![catalog.sneaker.clone()]);

    let leaf = store.post(&format!("/store-api/product-listing/{}", catalog.sneakers), json!({})).await;
    assert_eq!(ids(&leaf.body["elements"]), vec![catalog.sneaker.clone()]);
    let hidden = store.post(&format!("/store-api/product-listing/{}", catalog.hidden_category), json!({})).await;
    assert_eq!(hidden.status, StatusCode::NOT_FOUND);
    let bad_sort = store.post(&uri, json!({ "order": "random" })).await;
    assert_eq!(bad_sort.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn search_and_suggest(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;

    let missing = store.post("/store-api/search", json!({})).await;
    assert_eq!(missing.status, StatusCode::BAD_REQUEST);

    let found = store.post("/store-api/search?search=sneak", json!({})).await;
    assert_eq!(found.status, StatusCode::OK, "{}", found.body);
    assert_eq!(ids(&found.body["elements"]), vec![catalog.sneaker.clone()]);
    assert_eq!(found.body["sorting"], "score");
    assert_eq!(found.body["currentFilters"]["search"], "sneak");

    let suggest = store.post("/store-api/search-suggest", json!({ "search": "o" })).await;
    assert_eq!(suggest.body["limit"], 10);
    assert!(suggest.body["aggregations"].as_object().unwrap().is_empty());
    assert!(!ids(&suggest.body["elements"]).contains(&catalog.variant));
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn navigation_tree_follows_depth_and_active_path(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;

    let tree = store.post("/store-api/navigation/main-navigation/main-navigation", json!({ "depth": 1 })).await;
    assert_eq!(tree.status, StatusCode::OK, "{}", tree.body);
    let top = tree.body.as_array().unwrap();
    let names: Vec<&str> = top.iter().map(|c| c["name"].as_str().unwrap()).collect();
    assert_eq!(names, ["Shoes", "Accessories"], "inactive categories are hidden, sorted by position");
    assert_eq!(top[0]["children"], json!([]));

    // The active category's path is expanded beyond `depth`.
    let active =
        store.post(&format!("/store-api/navigation/{}/main-navigation", catalog.sneakers), json!({ "depth": 1 })).await;
    assert_eq!(active.body[0]["children"][0]["id"], catalog.sneakers);

    let flat = store.post("/store-api/navigation/main-navigation/main-navigation", json!({ "buildTree": false })).await;
    assert_eq!(flat.body.as_array().unwrap().len(), 3);

    let category = store.post("/store-api/category/main-navigation", json!({})).await;
    assert_eq!(category.body["id"], id::format(defaults::ROOT_CATEGORY));
    let currencies = store.post("/store-api/currency", json!({})).await;
    assert_eq!(currencies.body[0]["isoCode"], "EUR");
    let payment = store.post("/store-api/payment-method", json!({})).await;
    assert_eq!(payment.body["total"], 3);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn cart_line_items_prices_and_shipping(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;

    let empty = store.send(Method::GET, "/store-api/checkout/cart", None, None).await;
    assert_eq!(empty.status, StatusCode::OK, "{}", empty.body);
    assert_eq!(empty.body["apiAlias"], "cart");
    assert_eq!(empty.body["lineItems"], json!([]));
    assert_eq!(empty.body["deliveries"], json!([]));
    assert_eq!(empty.body["price"]["totalPrice"], 0.0);
    let token = empty.token.unwrap();
    let cart = |method: Method, uri: &str, body: Option<Value>| {
        let (store, token, uri) = (&store, token.clone(), uri.to_string());
        async move { store.send(method, &uri, Some(&token), body).await }
    };

    let item =
        |id: &str, quantity: u32| json!({ "items": [{ "type": "product", "referencedId": id, "quantity": quantity }] });
    let added = cart(Method::POST, "/store-api/checkout/cart/line-item", Some(item(&catalog.boot, 2))).await;
    assert_eq!(added.status, StatusCode::OK, "{}", added.body);
    assert_eq!(added.body["token"], token);
    let line = &added.body["lineItems"][0];
    assert_eq!(line["id"], catalog.boot);
    assert_eq!(line["label"], "Boot");
    assert_eq!(line["price"]["totalPrice"], 238.0);
    let price = &added.body["price"];
    assert_eq!(price["positionPrice"], 238.0);
    assert_eq!(added.body["deliveries"][0]["shippingCosts"]["totalPrice"], 4.99);
    assert_eq!(added.body["deliveries"][0]["shippingMethod"]["technicalName"], "standard");
    assert_eq!(price["totalPrice"], 242.99);
    assert_eq!(price["calculatedTaxes"][0]["tax"], 38.8);
    assert_eq!(price["netPrice"], 204.19);
    assert_eq!(added.body["transactions"][0]["amount"]["totalPrice"], 242.99);

    // Adding the same product again stacks; quantities can be changed and items removed.
    let stacked = cart(Method::POST, "/store-api/checkout/cart/line-item", Some(item(&catalog.boot, 1))).await;
    assert_eq!(stacked.body["lineItems"][0]["quantity"], 3);
    let patched = cart(
        Method::PATCH,
        "/store-api/checkout/cart/line-item",
        Some(json!({ "items": [{ "id": catalog.boot, "quantity": 1 }] })),
    )
    .await;
    assert_eq!(patched.body["lineItems"][0]["quantity"], 1);
    let unknown = cart(
        Method::PATCH,
        "/store-api/checkout/cart/line-item",
        Some(json!({ "items": [{ "id": "nope", "quantity": 1 }] })),
    )
    .await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
    assert_eq!(unknown.body["errors"][0]["code"], "CHECKOUT__CART_LINE_ITEM_NOT_FOUND");

    // Variants are buyable with their parent's price; parents with variants are not.
    let variant = cart(Method::POST, "/store-api/checkout/cart/line-item", Some(item(&catalog.variant, 1))).await;
    assert_eq!(variant.body["lineItems"][1]["price"]["unitPrice"], 59.5);
    let parent = cart(Method::POST, "/store-api/checkout/cart/line-item", Some(item(&catalog.sneaker, 1))).await;
    assert_eq!(parent.body["lineItems"].as_array().unwrap().len(), 2);
    assert_eq!(parent.body["errors"][format!("product-not-found{}", catalog.sneaker)]["level"], 20);
    // Errors are reported once; the invalid item was dropped from the stored cart.
    let again = cart(Method::GET, "/store-api/checkout/cart", None).await;
    assert_eq!(again.body["errors"], json!([]));

    let other = store.send(Method::GET, "/store-api/checkout/cart", None, None).await;
    assert_eq!(other.body["lineItems"], json!([]), "carts belong to their context token");

    let removed = cart(Method::DELETE, "/store-api/checkout/cart/line-item?ids[]=", None).await;
    assert_eq!(removed.status, StatusCode::BAD_REQUEST);
    let uri = format!("/store-api/checkout/cart/line-item?ids[]={}", catalog.variant);
    let removed = cart(Method::DELETE, &uri, None).await;
    assert_eq!(removed.body["lineItems"].as_array().unwrap().len(), 1);
    let removed =
        cart(Method::POST, "/store-api/checkout/cart/line-item/delete", Some(json!({ "ids": [catalog.boot] }))).await;
    assert_eq!(removed.body["lineItems"], json!([]));
    assert_eq!(removed.body["price"]["totalPrice"], 0.0);

    cart(Method::POST, "/store-api/checkout/cart/line-item", Some(item(&catalog.boot, 1))).await;
    let cleared = cart(Method::DELETE, "/store-api/checkout/cart", None).await;
    assert_eq!(cleared.status, StatusCode::NO_CONTENT);
    assert_eq!(cart(Method::GET, "/store-api/checkout/cart", None).await.body["lineItems"], json!([]));

    let bad = cart(
        Method::POST,
        "/store-api/checkout/cart/line-item",
        Some(json!({ "items": [{ "type": "promotion", "referencedId": "SALE" }, { "referencedId": catalog.boot, "quantity": 0 }] })),
    )
    .await;
    assert_eq!(bad.status, StatusCode::BAD_REQUEST);
    assert_eq!(bad.body["errors"].as_array().unwrap().len(), 2);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn cart_applies_purchase_rules_and_stock(db: Db) {
    let store = Store::new(db.clone());
    let tax = id::format(defaults::TAX_STANDARD);
    let closeout = create(
        &db,
        "product",
        json!({ "productNumber": "C-1", "name": "Last pieces", "stock": 2, "taxId": tax, "price": price(10.0),
                "active": true, "isCloseout": true, "shippingFree": true }),
    )
    .await;
    let packs = create(
        &db,
        "product",
        json!({ "productNumber": "P-1", "name": "Socks", "stock": 100, "taxId": tax, "price": price(5.0),
                "active": true, "minPurchase": 2, "purchaseSteps": 2, "maxPurchase": 6, "shippingFree": true }),
    )
    .await;
    let token = store.send(Method::GET, "/store-api/checkout/cart", None, None).await.token.unwrap();
    let add = |id: &str, quantity: u32| {
        let body = json!({ "items": [{ "type": "product", "referencedId": id, "quantity": quantity }] });
        let token = token.clone();
        let store = &store;
        async move { store.send(Method::POST, "/store-api/checkout/cart/line-item", Some(&token), Some(body)).await }
    };

    let limited = add(&closeout, 5).await;
    assert_eq!(limited.body["lineItems"][0]["quantity"], 2);
    assert_eq!(limited.body["errors"][format!("product-stock-reached{closeout}")]["level"], 10);
    // Only shipping-free products: no shipping costs.
    assert_eq!(limited.body["deliveries"][0]["shippingCosts"]["totalPrice"], 0.0);

    let stepped = add(&packs, 5).await;
    assert_eq!(stepped.body["lineItems"][1]["quantity"], 4, "rounded down to a purchase step");
    let capped = add(&packs, 9).await;
    assert_eq!(capped.body["lineItems"][1]["quantity"], 6, "capped at maxPurchase");

    // Products that sell out or are deactivated leave the cart on the next read.
    sqlx::query("UPDATE product SET stock = 0 WHERE product_number = 'C-1'").execute(&db).await.unwrap();
    sqlx::query("UPDATE product SET active = false WHERE product_number = 'P-1'").execute(&db).await.unwrap();
    let cart = store.send(Method::GET, "/store-api/checkout/cart", Some(&token), None).await;
    assert_eq!(cart.body["lineItems"], json!([]));
    assert_eq!(cart.body["errors"][format!("product-out-of-stock{closeout}")]["level"], 20);
    assert_eq!(cart.body["errors"][format!("product-not-found{packs}")]["level"], 20);
}

const GERMANY: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7001";
const AUSTRIA: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7002";

fn registration(email: &str) -> Value {
    json!({
        "firstName": "Ada", "lastName": "Lovelace", "email": email, "password": "analytical-engine",
        "billingAddress": { "street": "Main St 1", "zipcode": "10115", "city": "Berlin", "countryId": GERMANY },
    })
}

impl Store {
    async fn with(&self, method: Method, uri: &str, token: &str, body: Value) -> Reply {
        self.send(method, uri, Some(token), Some(body)).await
    }

    async fn register(&self, email: &str) -> String {
        let reply = self.post("/store-api/account/register", registration(email)).await;
        assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
        reply.token.unwrap()
    }

    async fn login(&self, token: Option<&str>, email: &str, password: &str) -> Reply {
        let body = json!({ "username": email, "password": password });
        self.send(Method::POST, "/store-api/account/login", token, Some(body)).await
    }
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn registration_login_and_sessions(db: Db) {
    let store = Store::new(db.clone());

    let invalid = store.post("/store-api/account/register", json!({ "firstName": "Ada", "email": "nope" })).await;
    assert_eq!(invalid.status, StatusCode::BAD_REQUEST);
    let pointers: Vec<&str> =
        invalid.body["errors"].as_array().unwrap().iter().map(|e| e["source"]["pointer"].as_str().unwrap()).collect();
    assert_eq!(pointers, ["/lastName", "/email", "/password", "/billingAddress"]);

    let anonymous = store.send(Method::GET, "/store-api/context", None, None).await.token.unwrap();
    let registered = store
        .send(Method::POST, "/store-api/account/register", Some(&anonymous), Some(registration("ada@example.com")))
        .await;
    assert_eq!(registered.status, StatusCode::OK, "{}", registered.body);
    assert_eq!(registered.body["email"], "ada@example.com");
    assert_eq!(registered.body["activeBillingAddress"]["country"]["iso"], "DE");
    assert!(registered.body.get("password").is_none());
    let session = registered.token.unwrap();
    assert_ne!(session, anonymous, "registration starts a new session");
    let context = store.send(Method::GET, "/store-api/context", Some(&session), None).await;
    assert_eq!(context.body["customer"]["firstName"], "Ada");
    let old = store.send(Method::GET, "/store-api/context", Some(&anonymous), None).await;
    assert_eq!(old.body["customer"], Value::Null);

    let duplicate = store.post("/store-api/account/register", registration("ADA@example.com")).await;
    assert_eq!(duplicate.body["errors"][0]["code"], "VIOLATION::CUSTOMER_EMAIL_NOT_UNIQUE");

    let logout = store.send(Method::POST, "/store-api/account/logout", Some(&session), None).await;
    assert_eq!(logout.status, StatusCode::OK);
    let after = logout.token.unwrap();
    assert_ne!(after, session);
    for token in [&session, &after] {
        let reply = store.send(Method::GET, "/store-api/account/customer", Some(token), None).await;
        assert_eq!(reply.status, StatusCode::FORBIDDEN);
        assert_eq!(reply.body["errors"][0]["code"], "CHECKOUT__CUSTOMER_NOT_LOGGED_IN");
    }

    let wrong = store.login(None, "ada@example.com", "wrong-password").await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong.body["errors"][0]["code"], "CHECKOUT__CUSTOMER_AUTH_BAD_CREDENTIALS");
    let unknown = store.login(None, "nobody@example.com", "analytical-engine").await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);

    // The anonymous cart moves into the session on login.
    let catalog = catalog(&db).await;
    let guest_token = store.send(Method::GET, "/store-api/context", None, None).await.token.unwrap();
    let item = json!({ "items": [{ "type": "product", "referencedId": catalog.boot }] });
    store.with(Method::POST, "/store-api/checkout/cart/line-item", &guest_token, item).await;
    let login = store.login(Some(&guest_token), "Ada@Example.com", "analytical-engine").await;
    assert_eq!(login.status, StatusCode::OK, "{}", login.body);
    let token = login.body["contextToken"].as_str().unwrap().to_string();
    assert_eq!(login.token.as_deref(), Some(token.as_str()));
    let cart = store.send(Method::GET, "/store-api/checkout/cart", Some(&token), None).await;
    assert_eq!(cart.body["lineItems"][0]["id"], catalog.boot);
    let customer = store.send(Method::GET, "/store-api/account/customer", Some(&token), None).await;
    assert_eq!(customer.body["lastName"], "Lovelace");
    assert!(customer.body["lastLogin"].is_string());
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn profile_password_and_addresses(db: Db) {
    let store = Store::new(db.clone());
    store.register("ada@example.com").await;
    let first = store.login(None, "ada@example.com", "analytical-engine").await.token.unwrap();
    let second = store.login(None, "ada@example.com", "analytical-engine").await.token.unwrap();

    let profile = json!({ "firstName": "Augusta", "lastName": "King", "company": "Analytical" });
    let changed = store.with(Method::POST, "/store-api/account/change-profile", &first, profile).await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    let customer = store.send(Method::GET, "/store-api/account/customer", Some(&first), None).await;
    assert_eq!(customer.body["firstName"], "Augusta");

    let wrong =
        json!({ "password": "nope", "newPassword": "difference-engine", "newPasswordConfirm": "difference-engine" });
    let reply = store.with(Method::POST, "/store-api/account/change-password", &first, wrong).await;
    assert_eq!(reply.body["errors"][0]["code"], "VIOLATION::CUSTOMER_PASSWORD_NOT_CORRECT");
    let short = json!({ "password": "analytical-engine", "newPassword": "short", "newPasswordConfirm": "short" });
    let reply = store.with(Method::POST, "/store-api/account/change-password", &first, short).await;
    assert_eq!(reply.body["errors"][0]["source"]["pointer"], "/newPassword");
    let good = json!({ "password": "analytical-engine", "newPassword": "difference-engine",
                       "newPasswordConfirm": "difference-engine" });
    let reply = store.with(Method::POST, "/store-api/account/change-password", &first, good).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    let other = store.send(Method::GET, "/store-api/account/customer", Some(&second), None).await;
    assert_eq!(other.status, StatusCode::FORBIDDEN, "other sessions end with a password change");
    assert_eq!(store.login(None, "ada@example.com", "difference-engine").await.status, StatusCode::OK);

    let email = json!({ "email": "augusta@example.com", "emailConfirmation": "augusta@example.com",
                        "password": "difference-engine" });
    let reply = store.with(Method::POST, "/store-api/account/change-email", &first, email).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);

    let address = json!({ "street": "Ring 2", "zipcode": "1010", "city": "Wien", "countryId": AUSTRIA });
    let created = store.with(Method::POST, "/store-api/account/address", &first, address).await;
    assert_eq!(created.status, StatusCode::OK, "{}", created.body);
    assert_eq!(created.body["firstName"], "Augusta", "names default to the customer's");
    let vienna = created.body["id"].as_str().unwrap().to_string();
    let listed = store.with(Method::POST, "/store-api/account/list-address", &first, json!({})).await;
    assert_eq!(listed.body["total"], 2);

    let uri = format!("/store-api/account/address/default-shipping/{vienna}");
    store.with(Method::PATCH, &uri, &first, json!({})).await;
    let context = store.send(Method::GET, "/store-api/context", Some(&first), None).await;
    assert_eq!(context.body["shippingLocation"]["country"]["iso"], "AT", "shipping country follows the address");
    let removed = store.send(Method::DELETE, &format!("/store-api/account/address/{vienna}"), Some(&first), None).await;
    assert_eq!(removed.body["errors"][0]["code"], "CHECKOUT__CUSTOMER_ADDRESS_IS_DEFAULT");

    let patched = store
        .with(
            Method::PATCH,
            &format!("/store-api/account/address/{vienna}"),
            &first,
            json!({ "street": "Graben 3", "city": "Wien", "countryId": AUSTRIA }),
        )
        .await;
    assert_eq!(patched.body["street"], "Graben 3");

    // Another customer cannot see or change these addresses.
    let intruder = store.register("eve@example.com").await;
    let foreign =
        store.with(Method::PATCH, &format!("/store-api/account/address/{vienna}"), &intruder, json!({})).await;
    assert_eq!(foreign.status, StatusCode::NOT_FOUND);
    let theirs = store.with(Method::POST, "/store-api/account/list-address", &intruder, json!({})).await;
    assert_eq!(theirs.body["total"], 1);
    let sneaky = store
        .with(Method::POST, "/store-api/account/list-address", &intruder, json!({ "associations": { "customer": {} } }))
        .await;
    assert_eq!(sneaky.status, StatusCode::FORBIDDEN);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn checkout_places_orders_and_books_stock(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;
    let anonymous = store.send(Method::POST, "/store-api/checkout/order", None, None).await;
    assert_eq!(anonymous.status, StatusCode::FORBIDDEN);

    let token = store.register("ada@example.com").await;
    let empty = store.send(Method::POST, "/store-api/checkout/order", Some(&token), None).await;
    assert_eq!(empty.body["errors"][0]["code"], "CHECKOUT__CART_EMPTY");

    let items = json!({ "items": [
        { "type": "product", "referencedId": catalog.boot, "quantity": 2 },
        { "type": "product", "referencedId": catalog.variant, "quantity": 1 },
    ] });
    let cart = store.with(Method::POST, "/store-api/checkout/cart/line-item", &token, items).await;
    let total = cart.body["price"]["totalPrice"].clone();
    let order =
        store.with(Method::POST, "/store-api/checkout/order", &token, json!({ "customerComment": "Ring twice" })).await;
    assert_eq!(order.status, StatusCode::OK, "{}", order.body);
    assert_eq!(order.body["amountTotal"], total);
    assert_eq!(order.body["shippingTotal"], 4.99);
    assert_eq!(order.body["state"], "open");
    assert_eq!(order.body["customerEmail"], "ada@example.com");
    assert_eq!(order.body["customerComment"], "Ring twice");
    assert_eq!(order.body["billingAddress"]["city"], "Berlin");
    assert_eq!(order.body["billingAddressId"], order.body["shippingAddressId"]);
    let lines = order.body["lineItems"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert!(lines.iter().any(|l| l["label"] == "Boot" && l["quantity"] == 2 && l["totalPrice"] == 238.0));

    let stock: Vec<i32> = sqlx::query_scalar(
        "SELECT stock FROM product WHERE product_number IN ('B-1', 'S-1.1') ORDER BY product_number",
    )
    .fetch_all(&db)
    .await
    .unwrap();
    assert_eq!(stock, [8, 2]);
    let cart = store.send(Method::GET, "/store-api/checkout/cart", Some(&token), None).await;
    assert_eq!(cart.body["lineItems"], json!([]), "the cart is emptied");

    let orders =
        store.with(Method::POST, "/store-api/order", &token, json!({ "associations": { "lineItems": {} } })).await;
    assert_eq!(orders.status, StatusCode::OK, "{}", orders.body);
    assert_eq!(orders.body["orders"]["total"], 1);
    assert_eq!(orders.body["orders"]["elements"][0]["lineItems"].as_array().unwrap().len(), 2);
    let other = store.register("eve@example.com").await;
    let none = store.with(Method::POST, "/store-api/order", &other, json!({})).await;
    assert_eq!(none.body["orders"]["total"], 0);

    // A cart that changed since it was shown is not ordered; the adjusted cart is kept.
    let tax = id::format(defaults::TAX_STANDARD);
    let last = create(
        &db,
        "product",
        json!({ "productNumber": "L-1", "name": "Last one", "stock": 2, "taxId": tax, "price": price(20.0),
                "active": true, "isCloseout": true }),
    )
    .await;
    let item = json!({ "items": [{ "type": "product", "referencedId": last, "quantity": 2 }] });
    store.with(Method::POST, "/store-api/checkout/cart/line-item", &other, item).await;
    sqlx::query("UPDATE product SET stock = 1 WHERE product_number = 'L-1'").execute(&db).await.unwrap();
    let refused = store.send(Method::POST, "/store-api/checkout/order", Some(&other), None).await;
    assert_eq!(refused.body["errors"][0]["code"], "CHECKOUT__CART_INVALID");
    let cart = store.send(Method::GET, "/store-api/checkout/cart", Some(&other), None).await;
    assert_eq!(cart.body["lineItems"][0]["quantity"], 1);
    let placed = store.send(Method::POST, "/store-api/checkout/order", Some(&other), None).await;
    assert_eq!(placed.status, StatusCode::OK, "{}", placed.body);
    let left: i32 =
        sqlx::query_scalar("SELECT stock FROM product WHERE product_number = 'L-1'").fetch_one(&db).await.unwrap();
    assert_eq!(left, 0);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn guests_can_order_without_an_account(db: Db) {
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;
    let mut guest = registration("guest@example.com");
    guest["guest"] = json!(true);
    guest.as_object_mut().unwrap().remove("password");
    guest["shippingAddress"] = json!({ "firstName": "Bob", "street": "Ring 2", "city": "Wien", "countryId": AUSTRIA });
    let registered = store.post("/store-api/account/register", guest.clone()).await;
    assert_eq!(registered.status, StatusCode::OK, "{}", registered.body);
    let token = registered.token.unwrap();
    // Guests may use the same address again and cannot log in.
    assert_eq!(store.post("/store-api/account/register", guest).await.status, StatusCode::OK);
    assert_eq!(store.login(None, "guest@example.com", "").await.status, StatusCode::UNAUTHORIZED);
    let profile = json!({ "firstName": "X", "lastName": "Y" });
    assert_eq!(
        store.with(Method::POST, "/store-api/account/change-profile", &token, profile).await.status,
        StatusCode::FORBIDDEN
    );

    let item = json!({ "items": [{ "type": "product", "referencedId": catalog.boot }] });
    store.with(Method::POST, "/store-api/checkout/cart/line-item", &token, item).await;
    let order = store.send(Method::POST, "/store-api/checkout/order", Some(&token), None).await;
    assert_eq!(order.status, StatusCode::OK, "{}", order.body);
    assert_eq!(order.body["shippingAddress"]["firstName"], "Bob");
    assert_eq!(order.body["shippingAddress"]["country"]["iso"], "AT");
    assert_ne!(order.body["billingAddressId"], order.body["shippingAddressId"]);
}

async fn seo_url(store: &Store, path: &str) -> Value {
    let criteria = json!({ "filter": [{ "type": "equals", "field": "seoPathInfo", "value": path }] });
    let reply = store.post("/store-api/seo-url", criteria).await;
    assert_eq!(reply.status, StatusCode::OK, "{}", reply.body);
    reply.body["elements"].get(0).cloned().unwrap_or(Value::Null)
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn seo_urls_are_generated_kept_and_resolved(db: Db) {
    use ferroshop_core::seo::{self, Scope};
    let store = Store::new(db.clone());
    let catalog = catalog(&db).await;
    seo::update(&db, Scope::All).await.unwrap();

    let sneakers = seo_url(&store, "Shoes/Sneakers/").await;
    assert_eq!(sneakers["foreignKey"], catalog.sneakers);
    assert_eq!(sneakers["routeName"], "frontend.navigation.page");
    assert_eq!(sneakers["pathInfo"], format!("/navigation/{}", catalog.sneakers));
    assert_eq!(sneakers["isCanonical"], true);
    assert_eq!(seo_url(&store, "Boot/B-1").await["foreignKey"], catalog.boot);
    assert_eq!(seo_url(&store, "Sneaker-red/S-1-1").await["foreignKey"], catalog.variant, "variants have their own");
    assert_eq!(seo_url(&store, "Hidden/").await["foreignKey"], catalog.hidden_category);

    // Entities carry their canonical URL when asked to.
    let request = |uri: &str, body: Value| {
        Request::post(uri)
            .header("sw-access-key", ACCESS_KEY)
            .header("sw-include-seo-urls", "true")
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    };
    let read = |request: Request<Body>| async {
        let response = store.app.clone().oneshot(request).await.unwrap();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        serde_json::from_slice::<Value>(&bytes).unwrap()
    };
    let listing = read(request(&format!("/store-api/product-listing/{}", catalog.shoes), json!({}))).await;
    assert_eq!(listing["elements"][0]["seoUrls"][0]["seoPathInfo"], "Boot/B-1");
    let navigation = read(request("/store-api/navigation/main-navigation/main-navigation", json!({}))).await;
    assert_eq!(navigation[0]["children"][0]["seoUrls"][0]["seoPathInfo"], "Shoes/Sneakers/");
    let plain = store.post(&format!("/store-api/product/{}", catalog.boot), json!({})).await;
    assert!(plain.body["product"].get("seoUrls").is_none());

    // Renaming moves the canonical URL (sub-categories too); old URLs still resolve.
    let ctx = Context::default();
    write::write(&db, "category", &json!({ "id": catalog.shoes, "name": "Footwear" }), WriteMode::Update, &ctx)
        .await
        .unwrap();
    seo::update(&db, Scope::Categories(vec![id::parse(&catalog.shoes).unwrap()])).await.unwrap();
    assert_eq!(seo_url(&store, "Footwear/Sneakers/").await["isCanonical"], true);
    let old = seo_url(&store, "Shoes/Sneakers/").await;
    assert_eq!((old["foreignKey"].clone(), old["isCanonical"].clone()), (json!(catalog.sneakers), json!(false)));

    // Same name under the same parent: the second one is numbered.
    let root = id::format(defaults::ROOT_CATEGORY);
    let twin = create(&db, "category", json!({ "parentId": root, "name": "Footwear" })).await;
    seo::update(&db, Scope::Categories(vec![id::parse(&twin).unwrap()])).await.unwrap();
    assert_eq!(seo_url(&store, "Footwear-2/").await["foreignKey"], twin);

    // Hand-edited URLs are left alone.
    sqlx::query("UPDATE seo_url SET seo_path_info = 'boots', is_modified = true WHERE seo_path_info = 'Boot/B-1'")
        .execute(&db)
        .await
        .unwrap();
    write::write(&db, "product", &json!({ "id": catalog.boot, "name": "Winter boot" }), WriteMode::Update, &ctx)
        .await
        .unwrap();
    seo::update(&db, Scope::Products(vec![id::parse(&catalog.boot).unwrap()])).await.unwrap();
    assert_eq!(seo_url(&store, "boots").await["isCanonical"], true);
    assert_eq!(seo_url(&store, "Winter-boot/B-1").await, Value::Null);

    // Deleting a product removes its URLs and those of its variants.
    let sneaker = id::parse(&catalog.sneaker).unwrap();
    let affected = seo::with_descendants(&db, "product", &[sneaker]).await.unwrap();
    write::delete(&db, "product", std::slice::from_ref(&catalog.sneaker), &ctx).await.unwrap();
    seo::update(&db, Scope::Products(affected)).await.unwrap();
    assert_eq!(seo_url(&store, "Sneaker/S-1").await, Value::Null);
    assert_eq!(seo_url(&store, "Sneaker-red/S-1-1").await, Value::Null);

    // URLs belong to their sales channel and language.
    let de = store.send(Method::GET, "/store-api/context", None, None).await.token.unwrap();
    store
        .with(Method::PATCH, "/store-api/context", &de, json!({ "languageId": id::format(defaults::LANGUAGE_DE) }))
        .await;
    let criteria = json!({ "filter": [{ "type": "equals", "field": "languageId", "value": id::format(defaults::LANGUAGE_SYSTEM) }] });
    let foreign = store.with(Method::POST, "/store-api/seo-url", &de, criteria).await;
    assert_eq!(foreign.body["total"], 0);
}
