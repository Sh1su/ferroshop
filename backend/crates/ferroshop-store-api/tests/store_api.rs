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
