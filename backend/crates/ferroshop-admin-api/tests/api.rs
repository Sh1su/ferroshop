//! Admin-API over HTTP: authentication, permission guards, order actions and media.

use std::io::Cursor;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use ferroshop_admin_api::AppState;
use ferroshop_core::media::MediaStore;
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, id};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const PASSWORD: &str = "shopware1";

struct Api {
    app: Router,
}

struct Reply {
    status: StatusCode,
    headers: header::HeaderMap,
    body: Vec<u8>,
}

impl Reply {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

impl Api {
    fn new(db: Db) -> Self {
        let state = AppState::new(db, "test-secret-0123456789abcdef0123456789", MediaStore::memory());
        let app = ferroshop_admin_api::router(state.clone()).merge(ferroshop_admin_api::media_router(state));
        Self { app }
    }

    async fn send(&self, method: Method, uri: &str, token: Option<&str>, body: Body, content_type: &str) -> Reply {
        let mut request = Request::builder().method(method).uri(uri).header(header::CONTENT_TYPE, content_type);
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        let response = self.app.clone().oneshot(request.body(body).unwrap()).await.unwrap();
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
        Reply { status, headers, body }
    }

    async fn call(&self, method: Method, uri: &str, token: Option<&str>, body: Option<Value>) -> Reply {
        let body = body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty);
        self.send(method, uri, token, body, "application/json").await
    }

    async fn login(&self, username: &str, password: &str) -> Reply {
        let body = json!({ "grant_type": "password", "client_id": "administration", "username": username, "password": password });
        self.call(Method::POST, "/api/oauth/token", None, Some(body)).await
    }

    async fn token(&self, username: &str) -> String {
        self.token_with(username, PASSWORD).await
    }

    async fn token_with(&self, username: &str, password: &str) -> String {
        let reply = self.login(username, password).await;
        assert_eq!(reply.status, StatusCode::OK, "login as {username}");
        reply.json()["access_token"].as_str().unwrap().to_string()
    }
}

async fn create(db: &Db, entity: &str, payload: Value) -> String {
    id::format(write::write(db, entity, &payload, WriteMode::Create, &Context::default()).await.unwrap())
}

async fn user(db: &Db, username: &str, admin: bool) -> String {
    create(
        db,
        "user",
        json!({ "username": username, "email": format!("{username}@example.com"), "password": PASSWORD, "admin": admin }),
    )
    .await
}

/// A non-admin user holding the given privileges through one role.
async fn user_with(db: &Db, username: &str, privileges: &[&str]) -> String {
    let user_id = user(db, username, false).await;
    create(
        db,
        "acl_role",
        json!({ "name": format!("{username} role"), "privileges": privileges, "users": [{ "id": user_id }] }),
    )
    .await;
    user_id
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn tokens_and_refresh_rotation(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;

    assert_eq!(api.call(Method::GET, "/api/_info/me", None, None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(api.call(Method::GET, "/api/_info/me", Some("garbage"), None).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(api.login("admin", "wrong-password").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(api.login("nobody", PASSWORD).await.status, StatusCode::UNAUTHORIZED);

    let login = api.login("admin", PASSWORD).await.json();
    let me = api.call(Method::GET, "/api/_info/me", login["access_token"].as_str(), None).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.json()["data"]["username"], "admin");
    assert_eq!(me.json()["data"]["permissions"]["product"], json!(["read", "create", "update", "delete"]));

    // Form-encoded like Shopware's administration; a refresh token works exactly once.
    let form = format!(
        "grant_type=refresh_token&client_id=administration&refresh_token={}",
        login["refresh_token"].as_str().unwrap()
    );
    let refresh = || {
        api.send(Method::POST, "/api/oauth/token", None, Body::from(form.clone()), "application/x-www-form-urlencoded")
    };
    assert_eq!(refresh().await.status, StatusCode::OK);
    assert_eq!(refresh().await.status, StatusCode::UNAUTHORIZED);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn privileges_are_enforced_per_entity_and_association(db: Db) {
    let api = Api::new(db.clone());
    user_with(&db, "editor", &["product:read", "product:update"]).await;
    let token = api.token("editor").await;

    let search = |body: Value| api.call(Method::POST, "/api/search/product", Some(&token), Some(body));
    assert_eq!(search(json!({})).await.status, StatusCode::OK);
    // Loading an association needs read access on its entity too.
    let denied = search(json!({ "associations": { "manufacturer": {} } })).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    assert_eq!(denied.json()["errors"][0]["code"], "FRAMEWORK__PERMISSION_DENIED");
    // The same holds for filters, sorting and aggregations that walk into another entity.
    for criteria in [
        json!({ "filter": [{ "type": "prefix", "field": "manufacturer.name", "value": "A" }] }),
        json!({ "filter": [{ "type": "multi", "queries": [{ "type": "equals", "field": "product.categories.name", "value": "x" }] }] }),
        json!({ "sort": [{ "field": "manufacturer.translated.name" }] }),
        json!({ "aggregations": [{ "name": "m", "type": "terms", "field": "manufacturer.name" }] }),
    ] {
        assert_eq!(search(criteria.clone()).await.status, StatusCode::FORBIDDEN, "{criteria}");
    }
    let own_fields = json!({ "filter": [{ "type": "range", "field": "stock", "parameters": { "gte": 0 } }], "sort": [{ "field": "name" }] });
    assert_eq!(search(own_fields).await.status, StatusCode::OK);
    // Reference data is readable by everyone, other entities are not.
    assert_eq!(api.call(Method::GET, "/api/currency", Some(&token), None).await.status, StatusCode::OK);
    assert_eq!(api.call(Method::GET, "/api/customer", Some(&token), None).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        api.call(Method::POST, "/api/product", Some(&token), Some(json!({}))).await.status,
        StatusCode::FORBIDDEN
    );
    assert_eq!(api.call(Method::GET, "/api/nope", Some(&token), None).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn only_admins_can_change_admins_and_roles(db: Db) {
    let api = Api::new(db.clone());
    let admin_id = user(&db, "admin", true).await;
    let colleague_id = user(&db, "colleague", false).await;
    let integration_id = create(
        &db,
        "integration",
        json!({ "label": "ERP", "accessKey": "SWIAERP", "secretAccessKey": "erp-secret-123", "admin": true }),
    )
    .await;
    user_with(
        &db,
        "manager",
        &[
            "user:read",
            "user:create",
            "user:update",
            "user:delete",
            "integration:update",
            "integration:delete",
            "acl_role:update",
            "acl_role:delete",
        ],
    )
    .await;
    let token = api.token("manager").await;
    let role_id: uuid::Uuid =
        sqlx::query_scalar("SELECT id FROM acl_role WHERE name = 'manager role'").fetch_one(&db).await.unwrap();
    let role_id = id::format(role_id);

    let patch = |uri: String, body: Value| {
        let token = token.clone();
        let api = &api;
        async move { api.call(Method::PATCH, &uri, Some(&token), Some(body)).await.status }
    };
    let delete = |uri: String| {
        let token = token.clone();
        let api = &api;
        async move { api.call(Method::DELETE, &uri, Some(&token), None).await.status }
    };

    // Taking over an administrator by resetting their password must not work.
    assert_eq!(
        patch(format!("/api/user/{admin_id}"), json!({ "password": "taken-over-1" })).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(api.login("admin", "taken-over-1").await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(delete(format!("/api/user/{admin_id}")).await, StatusCode::FORBIDDEN);
    assert_eq!(
        patch(format!("/api/integration/{integration_id}"), json!({ "secretAccessKey": "taken-over-1" })).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(delete(format!("/api/integration/{integration_id}")).await, StatusCode::FORBIDDEN);

    // Roles are reserved for administrators, including deleting and unlinking them.
    assert_eq!(patch(format!("/api/acl-role/{role_id}"), json!({ "privileges": ["*"] })).await, StatusCode::FORBIDDEN);
    assert_eq!(delete(format!("/api/acl-role/{role_id}")).await, StatusCode::FORBIDDEN);
    assert_eq!(patch(format!("/api/user/{colleague_id}"), json!({ "admin": true })).await, StatusCode::FORBIDDEN);

    // Ordinary colleagues stay editable with `user:update`.
    assert_eq!(
        patch(format!("/api/user/{colleague_id}"), json!({ "firstName": "Grace" })).await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(delete(format!("/api/user/{colleague_id}")).await, StatusCode::NO_CONTENT);

    // Forms send `admin: false` and `aclRoles: []` even when nothing is granted.
    let create_user = |extra: Value| {
        let token = token.clone();
        let api = &api;
        async move {
            let mut body = json!({ "username": "new", "email": "new@example.com", "password": PASSWORD });
            body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            api.call(Method::POST, "/api/user", Some(&token), Some(body)).await.status
        }
    };
    assert_eq!(create_user(json!({ "admin": true })).await, StatusCode::FORBIDDEN);
    assert_eq!(create_user(json!({ "aclRoles": [{ "id": role_id }] })).await, StatusCode::FORBIDDEN);
    assert_eq!(create_user(json!({ "admin": false, "aclRoles": [] })).await, StatusCode::NO_CONTENT);

    let admin_token = api.token("admin").await;
    let status = api
        .call(
            Method::PATCH,
            &format!("/api/acl-role/{role_id}"),
            Some(&admin_token),
            Some(json!({ "description": "x" })),
        )
        .await
        .status;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn password_changes_end_other_sessions(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    let colleague_id = user(&db, "colleague", false).await;
    let admin_token = api.token("admin").await;
    let session = api.login("colleague", PASSWORD).await.json();
    let refresh = json!({ "grant_type": "refresh_token", "refresh_token": session["refresh_token"] });

    let reply = api
        .call(
            Method::PATCH,
            &format!("/api/user/{colleague_id}"),
            Some(&admin_token),
            Some(json!({ "password": "new-password-1" })),
        )
        .await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
    assert!(reply.headers.get(header::LOCATION).is_some());
    assert_eq!(api.call(Method::POST, "/api/oauth/token", None, Some(refresh)).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(api.login("colleague", "new-password-1").await.status, StatusCode::OK);

    // Own profile: the password needs the current one, privileged fields are off limits.
    let token = api.token_with("colleague", "new-password-1").await;
    let me = |body: Value| api.call(Method::PATCH, "/api/_info/me", Some(&token), Some(body));
    assert_eq!(me(json!({ "password": "another-one-1" })).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(me(json!({ "admin": true })).await.status, StatusCode::FORBIDDEN);
    assert_eq!(
        me(json!({ "password": "another-one-1", "currentPassword": "new-password-1" })).await.status,
        StatusCode::NO_CONTENT
    );
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn order_transitions_over_http(db: Db) {
    let api = Api::new(db.clone());
    user_with(&db, "support", &["order:read"]).await;
    user(&db, "admin", true).await;
    let order_id = create(
        &db,
        "order",
        json!({
            "salesChannelId": "98432def39fc4624b33213a56b8c944d", "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca",
            "languageId": "2fbb5fe2e29a4d70aa5854ce7ce3e20b",
            "customerEmail": "ada@example.com", "customerFirstName": "Ada", "customerLastName": "Lovelace",
        }),
    )
    .await;
    let support = api.token("support").await;
    let admin = api.token("admin").await;

    let states =
        api.call(Method::GET, &format!("/api/_action/order/{order_id}/transitions"), Some(&support), None).await;
    assert_eq!(states.status, StatusCode::OK);
    assert_eq!(states.json()["data"]["paymentState"]["current"], "open");
    let pay = format!("/api/_action/order/{order_id}/payment-state/pay");
    assert_eq!(api.call(Method::POST, &pay, Some(&support), None).await.status, StatusCode::FORBIDDEN);

    let paid = api.call(Method::POST, &pay, Some(&admin), None).await;
    assert_eq!(paid.status, StatusCode::OK);
    assert_eq!(paid.json()["data"]["paymentState"]["current"], "paid");
    assert_eq!(api.call(Method::POST, &pay, Some(&admin), None).await.status, StatusCode::CONFLICT);
    let unknown = format!("/api/_action/order/{order_id}/nonsense/pay");
    assert_eq!(api.call(Method::POST, &unknown, Some(&admin), None).await.status, StatusCode::BAD_REQUEST);
    let missing = format!("/api/_action/order/{}/transitions", id::format(id::new_id()));
    assert_eq!(api.call(Method::GET, &missing, Some(&admin), None).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn media_upload_and_delivery(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    let token = api.token("admin").await;
    let media_id = create(&db, "media", json!({ "title": "Shirt" })).await;

    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::new_rgb8(600, 300).write_to(&mut png, image::ImageFormat::Png).unwrap();
    let upload = |extension: &str, body: Vec<u8>| {
        let uri = format!("/api/_action/media/{media_id}/upload?extension={extension}&fileName=Red%20Shirt");
        let token = token.clone();
        let api = &api;
        async move { api.send(Method::POST, &uri, Some(&token), Body::from(body), "application/octet-stream").await.status }
    };
    assert_eq!(upload("svg", b"<svg onload=alert(1)>".to_vec()).await, StatusCode::BAD_REQUEST);
    assert_eq!(upload("png", b"not a png".to_vec()).await, StatusCode::BAD_REQUEST);
    assert_eq!(upload("png", png.into_inner()).await, StatusCode::NO_CONTENT);

    let media =
        api.call(Method::GET, &format!("/api/media/{media_id}"), Some(&token), None).await.json()["data"].clone();
    assert_eq!(media["fileName"], "red-shirt");
    assert_eq!(media["width"], 600);
    let file = api.call(Method::GET, media["url"].as_str().unwrap(), None, None).await;
    assert_eq!(file.status, StatusCode::OK);
    assert_eq!(file.headers[header::CONTENT_TYPE], "image/png");
    assert_eq!(file.headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    let thumb = media["thumbnails"][0]["url"].as_str().unwrap();
    assert_eq!(api.call(Method::GET, thumb, None, None).await.status, StatusCode::OK);
    assert_eq!(api.call(Method::GET, "/media/../etc/passwd.png", None, None).await.status, StatusCode::NOT_FOUND);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn catalogue_writes_keep_seo_urls_current(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    let token = api.token("admin").await;
    let paths = |db: Db| async move {
        sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT seo_path_info FROM seo_url WHERE is_canonical ORDER BY seo_path_info",
        )
        .fetch_all(&db)
        .await
        .unwrap()
    };

    let body = json!({ "parentId": id::format(ferroshop_core::defaults::ROOT_CATEGORY), "name": "Garden Tools" });
    let created = api.call(Method::POST, "/api/category?_response=1", Some(&token), Some(body)).await;
    assert_eq!(created.status, StatusCode::OK);
    let category = created.json()["data"]["id"].as_str().unwrap().to_string();
    assert_eq!(paths(db.clone()).await, ["Garden-Tools/"]);

    let rename = json!({ "name": "Garden" });
    api.call(Method::PATCH, &format!("/api/category/{category}"), Some(&token), Some(rename)).await;
    assert_eq!(paths(db.clone()).await, ["Garden/"]);

    api.call(Method::DELETE, &format!("/api/category/{category}"), Some(&token), None).await;
    assert!(paths(db.clone()).await.is_empty());
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM seo_url").fetch_one(&db).await.unwrap();
    assert_eq!(left, 0, "old URLs of deleted entities are removed too");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn variants_are_generated_from_options(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    user_with(&db, "reader", &["product:read", "property_group_option:read"]).await;
    let token = api.token("admin").await;
    let group = create(
        &db,
        "property_group",
        json!({ "name": "Size", "options": [{ "name": "S" }, { "name": "M" }, { "name": "L" }] }),
    )
    .await;
    let options: Vec<String> =
        sqlx::query_scalar::<_, uuid::Uuid>("SELECT id FROM property_group_option WHERE group_id = $1")
            .bind(id::parse(&group).unwrap())
            .fetch_all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(id::format)
            .collect();
    let product = create(
        &db,
        "product",
        json!({ "productNumber": "P-1", "name": "Shirt", "stock": 0, "taxId": id::format(ferroshop_core::defaults::TAX_STANDARD),
                "price": [{ "currencyId": id::format(ferroshop_core::defaults::CURRENCY), "gross": 10, "net": 8.4, "linked": true }] }),
    )
    .await;
    let uri = format!("/api/_action/product/{product}/generate-variants");

    let denied =
        api.call(Method::POST, &uri, Some(&api.token("reader").await), Some(json!({ "options": options }))).await;
    assert_eq!(denied.status, StatusCode::FORBIDDEN);
    let generated = api.call(Method::POST, &uri, Some(&token), Some(json!({ "options": options }))).await;
    assert_eq!(generated.status, StatusCode::OK, "{:?}", generated.json());
    assert_eq!(generated.json()["data"].as_array().unwrap().len(), 3);
    let numbers: Vec<String> =
        sqlx::query_scalar("SELECT product_number FROM product WHERE parent_id IS NOT NULL ORDER BY product_number")
            .fetch_all(&db)
            .await
            .unwrap();
    assert_eq!(numbers, ["P-1.1", "P-1.2", "P-1.3"]);
    let again = api.call(Method::POST, &uri, Some(&token), Some(json!({ "options": options }))).await;
    assert_eq!(again.json()["data"], json!([]));
    let unknown = api.call(Method::POST, &uri, Some(&token), Some(json!({ "options": ["0".repeat(32)] }))).await;
    assert_eq!(unknown.status, StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn the_default_listing_page_cannot_be_deleted(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    let token = api.token("admin").await;
    let page = id::format(ferroshop_core::defaults::CMS_DEFAULT_LISTING_PAGE);
    let reply = api.call(Method::DELETE, &format!("/api/cms-page/{page}"), Some(&token), None).await;
    assert_eq!(reply.status, StatusCode::CONFLICT);
    assert_eq!(reply.json()["errors"][0]["code"], "CONTENT__CMS_PAGE_LOCKED");
    let own = create(&db, "cms_page", json!({ "name": "Landing" })).await;
    let reply = api.call(Method::DELETE, &format!("/api/cms-page/{own}"), Some(&token), None).await;
    assert_eq!(reply.status, StatusCode::NO_CONTENT);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn sales_channel_domains_are_admin_managed_and_searchable(db: Db) {
    let api = Api::new(db.clone());
    user(&db, "admin", true).await;
    let token = api.token("admin").await;
    let type_id: uuid::Uuid = sqlx::query_scalar("SELECT type_id FROM sales_channel ORDER BY id LIMIT 1")
        .fetch_one(&db)
        .await
        .unwrap();
    let created_channel = api
        .call(
            Method::POST,
            "/api/sales-channel?_response=true",
            Some(&token),
            Some(json!({
                "typeId": id::format(type_id),
                "accessKey": "SWSC-DOMAIN-TEST",
                "name": "Domain test",
                "languageId": "0191a6f0f4a07c7e9b1c2d3e4f5a6b7c",
                "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca"
            })),
        )
        .await;
    assert_eq!(created_channel.status, StatusCode::OK, "{}", String::from_utf8_lossy(&created_channel.body));
    assert_eq!(created_channel.json()["data"]["active"], false, "API creates new sales channels as drafts");
    let channel_id = created_channel.json()["data"]["id"].as_str().unwrap();

    let blocked_activation = api
        .call(
            Method::PATCH,
            &format!("/api/sales-channel/{channel_id}"),
            Some(&token),
            Some(json!({ "active": true })),
        )
        .await;
    assert_eq!(blocked_activation.status, StatusCode::BAD_REQUEST);

    let created = api
        .call(
            Method::POST,
            "/api/sales-channel-domain?_response=true",
            Some(&token),
            Some(json!({
                "salesChannelId": channel_id,
                "url": "https://shop.example.test/de",
                "languageId": "0191a6f0f4a07c7e9b1c2d3e4f5a6b7c",
                "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca",
                "browserLanguage": false
            })),
        )
        .await;
    assert_eq!(created.status, StatusCode::OK, "{}", String::from_utf8_lossy(&created.body));
    let domain_id = created.json()["data"]["id"].as_str().unwrap();
    assert_eq!(created.json()["data"]["url"], "https://shop.example.test/de");

    let search = api
        .call(
            Method::POST,
            "/api/search/sales-channel-domain",
            Some(&token),
            Some(json!({
                "filter": [{ "type": "equals", "field": "salesChannelId", "value": channel_id }],
                "associations": { "language": {}, "currency": {} }
            })),
        )
        .await;
    assert_eq!(search.status, StatusCode::OK);
    assert_eq!(search.json()["data"][0]["id"], domain_id);
    assert_eq!(search.json()["data"][0]["language"]["localeCode"], "de-DE");
    assert_eq!(search.json()["data"][0]["currency"]["isoCode"], "EUR");

    let activated = api
        .call(
            Method::PATCH,
            &format!("/api/sales-channel/{channel_id}"),
            Some(&token),
            Some(json!({ "active": true })),
        )
        .await;
    assert_eq!(activated.status, StatusCode::NO_CONTENT);

    let duplicate = api
        .call(
            Method::POST,
            "/api/sales-channel-domain",
            Some(&token),
            Some(json!({
                "salesChannelId": channel_id,
                "url": "https://shop.example.test/de",
                "languageId": "0191a6f0f4a07c7e9b1c2d3e4f5a6b7c",
                "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca"
            })),
        )
        .await;
    assert_eq!(duplicate.status, StatusCode::BAD_REQUEST);
    assert_eq!(duplicate.json()["errors"][0]["code"], "FRAMEWORK__DUPLICATE_ENTRY");

    let invalid_url = api
        .call(
            Method::POST,
            "/api/sales-channel-domain",
            Some(&token),
            Some(json!({
                "salesChannelId": channel_id,
                "url": "shop.example.test/de",
                "languageId": "0191a6f0f4a07c7e9b1c2d3e4f5a6b7c",
                "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca"
            })),
        )
        .await;
    assert_eq!(invalid_url.status, StatusCode::BAD_REQUEST);
}
