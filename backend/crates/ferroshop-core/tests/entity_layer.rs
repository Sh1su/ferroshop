//! Integration tests for the generic entity layer against a real PostgreSQL
//! (`sqlx::test` creates a fresh database per test and applies the migrations).

use ferroshop_core::criteria::Criteria;
use ferroshop_core::read;
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, Error, defaults, id};
use serde_json::{Value, json};

fn price(gross: f64) -> Value {
    json!([{ "currencyId": id::format(defaults::CURRENCY), "gross": gross, "net": gross / 1.19, "linked": true }])
}

fn product(number: &str, name: &str, stock: i64) -> Value {
    json!({
        "productNumber": number,
        "name": name,
        "stock": stock,
        "taxId": id::format(defaults::TAX_STANDARD),
        "price": price(10.0),
    })
}

fn criteria(value: Value) -> Criteria {
    serde_json::from_value(value).expect("valid criteria")
}

async fn names(db: &Db, criteria_json: Value, ctx: &Context) -> Vec<String> {
    read::search(db, "product", &criteria(criteria_json), ctx)
        .await
        .expect("search")
        .data
        .iter()
        .map(|p| p["translated"]["name"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn create_and_read_with_translations(db: Db) {
    let ctx = Context::default();
    let de = Context { language_id: defaults::LANGUAGE_DE, ..Context::default() };
    let mut payload = product("P-1", "Shirt", 3);
    payload["translations"] = json!({ id::format(defaults::LANGUAGE_DE): { "name": "Hemd" } });
    let pid = write::write(&db, "product", &payload, WriteMode::Create, &ctx).await.unwrap();
    let hex = id::format(pid);

    let en = read::read(&db, "product", &hex, &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(en["id"], hex);
    assert_eq!(en["name"], "Shirt");
    assert_eq!(en["price"][0]["gross"], 10.0);

    let german = read::read(&db, "product", &hex, &Criteria::default(), &de).await.unwrap();
    assert_eq!(german["name"], "Hemd");

    // Description only exists in English: raw value is null, `translated` falls back.
    write::write(&db, "product", &json!({ "id": hex, "description": "Cotton" }), WriteMode::Update, &ctx)
        .await
        .unwrap();
    let german = read::read(&db, "product", &hex, &Criteria::default(), &de).await.unwrap();
    assert_eq!(german["description"], Value::Null);
    assert_eq!(german["translated"]["description"], "Cotton");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn creating_in_other_language_seeds_system_translation(db: Db) {
    let de = Context { language_id: defaults::LANGUAGE_DE, ..Context::default() };
    let pid = write::write(&db, "product", &product("P-1", "Hose", 1), WriteMode::Create, &de).await.unwrap();
    let en = read::read(&db, "product", &id::format(pid), &Criteria::default(), &Context::default()).await.unwrap();
    assert_eq!(en["name"], "Hose");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn validation_errors_point_at_fields(db: Db) {
    let ctx = Context::default();
    let err = write::write(
        &db,
        "product",
        &json!({ "productNumber": "", "stock": "many", "unknown": 1, "price": [] }),
        WriteMode::Create,
        &ctx,
    )
    .await
    .unwrap_err();
    let Error::Validation(errors) = err else { panic!("expected validation error, got {err:?}") };
    let pointers: Vec<&str> = errors.iter().map(|e| e.pointer.as_str()).collect();
    for expected in ["/productNumber", "/stock", "/unknown", "/price", "/taxId", "/name"] {
        assert!(pointers.contains(&expected), "missing {expected} in {pointers:?}");
    }
    assert_eq!(pointers.iter().filter(|p| **p == "/price").count(), 1);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn duplicate_product_number_is_rejected(db: Db) {
    let ctx = Context::default();
    write::write(&db, "product", &product("P-1", "A", 1), WriteMode::Create, &ctx).await.unwrap();
    let err = write::write(&db, "product", &product("P-1", "B", 1), WriteMode::Create, &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Validation(_)), "{err:?}");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn criteria_filters_sorting_and_paging(db: Db) {
    let ctx = Context::default();
    for (n, name, stock) in [("A-1", "Alpha", 5), ("B-1", "Beta", 0), ("C-1", "Gamma", 12), ("D-1", "Delta", 7)] {
        write::write(&db, "product", &product(n, name, stock), WriteMode::Create, &ctx).await.unwrap();
    }
    let sorted = names(&db, json!({ "sort": [{ "field": "name", "order": "DESC" }] }), &ctx).await;
    assert_eq!(sorted, ["Gamma", "Delta", "Beta", "Alpha"]);

    let filtered = names(
        &db,
        json!({
            "filter": [{ "type": "multi", "operator": "or", "queries": [
                { "type": "range", "field": "stock", "parameters": { "gte": 7 } },
                { "type": "equals", "field": "productNumber", "value": "A-1" }
            ]}],
            "sort": [{ "field": "productNumber" }]
        }),
        &ctx,
    )
    .await;
    assert_eq!(filtered, ["Alpha", "Gamma", "Delta"]);

    let page = read::search(
        &db,
        "product",
        &criteria(json!({ "limit": 2, "page": 2, "sort": [{ "field": "productNumber" }], "total-count-mode": 1 })),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(page.total, 4);
    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0]["productNumber"], "C-1");

    let negated = names(
        &db,
        json!({ "filter": [{ "type": "not", "queries": [
        { "type": "contains", "field": "name", "value": "ta" } ] }], "sort": [{ "field": "name" }] }),
        &ctx,
    )
    .await;
    assert_eq!(negated, ["Alpha", "Gamma"]);

    let term = names(&db, json!({ "term": "gam" }), &ctx).await;
    assert_eq!(term, ["Gamma"]);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn filters_and_loads_associations(db: Db) {
    let ctx = Context::default();
    let shoes = write::write(&db, "category", &json!({ "name": "Shoes" }), WriteMode::Create, &ctx).await.unwrap();
    let mut boot = product("B-1", "Boot", 1);
    boot["categories"] = json!([{ "id": id::format(shoes) }]);
    boot["manufacturer"] = json!({ "name": "Ferris" });
    write::write(&db, "product", &boot, WriteMode::Create, &ctx).await.unwrap();
    write::write(&db, "product", &product("S-1", "Shirt", 1), WriteMode::Create, &ctx).await.unwrap();

    assert_eq!(
        names(&db, json!({ "filter": [{ "type": "equals", "field": "categories.name", "value": "Shoes" }] }), &ctx)
            .await,
        ["Boot"]
    );
    assert_eq!(
        names(
            &db,
            json!({ "filter": [{ "type": "equals", "field": "categories.id", "value": id::format(shoes) }] }),
            &ctx
        )
        .await,
        ["Boot"]
    );
    assert_eq!(
        names(&db, json!({ "filter": [{ "type": "equals", "field": "manufacturer.name", "value": null }] }), &ctx)
            .await,
        ["Shirt"]
    );

    let result = read::search(
        &db,
        "product",
        &criteria(json!({
            "filter": [{ "type": "equals", "field": "productNumber", "value": "B-1" }],
            "associations": { "categories": {}, "manufacturer": {} },
            "aggregations": [{ "name": "makers", "type": "terms", "field": "manufacturer.name" }],
            "includes": { "product": ["name", "categories", "manufacturer"], "category": ["name"] }
        })),
        &ctx,
    )
    .await
    .unwrap();
    let boot = &result.data[0];
    assert_eq!(boot["categories"][0]["name"], "Shoes");
    assert_eq!(boot["manufacturer"]["name"], "Ferris");
    assert!(boot.get("productNumber").is_none(), "includes must strip fields");
    assert_eq!(result.aggregations["makers"]["buckets"][0]["key"], "Ferris");

    let category =
        read::read(&db, "category", &id::format(shoes), &criteria(json!({ "associations": { "products": {} } })), &ctx)
            .await
            .unwrap();
    assert_eq!(category["products"][0]["name"], "Boot");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn variants_inherit_from_parent(db: Db) {
    let ctx = Context::default();
    let parent = write::write(&db, "product", &product("P-1", "Shirt", 3), WriteMode::Create, &ctx).await.unwrap();
    let variant = json!({ "parentId": id::format(parent), "productNumber": "P-1.1", "stock": 2 });
    let vid = write::write(&db, "product", &variant, WriteMode::Create, &ctx).await.unwrap();

    let raw = read::read(&db, "product", &id::format(vid), &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(raw["name"], Value::Null);
    assert_eq!(raw["price"], Value::Null);

    let inherited_ctx = Context { inheritance: true, ..Context::default() };
    let inherited = read::read(&db, "product", &id::format(vid), &Criteria::default(), &inherited_ctx).await.unwrap();
    assert_eq!(inherited["name"], "Shirt");
    assert_eq!(inherited["price"][0]["gross"], 10.0);
    assert_eq!(inherited["stock"], 2);

    let parent_row = read::read(&db, "product", &id::format(parent), &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(parent_row["childCount"], 1);
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn delete_and_mapping_removal(db: Db) {
    let ctx = Context::default();
    let cat = write::write(&db, "category", &json!({ "name": "Sale" }), WriteMode::Create, &ctx).await.unwrap();
    let mut p = product("P-1", "Shirt", 1);
    p["categories"] = json!([{ "id": id::format(cat) }]);
    let pid = write::write(&db, "product", &p, WriteMode::Create, &ctx).await.unwrap();

    write::delete_mapping(&db, "product", &id::format(pid), "categories", &id::format(cat), &ctx).await.unwrap();
    let loaded =
        read::read(&db, "product", &id::format(pid), &criteria(json!({ "associations": { "categories": {} } })), &ctx)
            .await
            .unwrap();
    assert_eq!(loaded["categories"], json!([]));

    // A tax that is still used by a product cannot be deleted.
    let err = write::delete(&db, "tax", &[id::format(defaults::TAX_STANDARD)], &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err:?}");

    write::delete(&db, "product", &[id::format(pid)], &ctx).await.unwrap();
    let err = read::read(&db, "product", &id::format(pid), &Criteria::default(), &ctx).await.unwrap_err();
    assert!(matches!(err, Error::NotFound { .. }));

    let audit: i64 = sqlx::query_scalar("SELECT count(*) FROM audit_log WHERE entity_id = $1")
        .bind(pid)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(audit, 2, "insert + delete are audited");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn passwords_are_hashed_and_never_returned(db: Db) {
    let ctx = Context::default();
    let uid = write::write(
        &db,
        "user",
        &json!({ "username": "jane", "email": "jane@example.com", "password": "secret-password" }),
        WriteMode::Create,
        &ctx,
    )
    .await
    .unwrap();
    let user = read::read(&db, "user", &id::format(uid), &Criteria::default(), &ctx).await.unwrap();
    assert!(user.get("password").is_none());
    let hash: String =
        sqlx::query_scalar("SELECT password FROM admin_user WHERE id = $1").bind(uid).fetch_one(&db).await.unwrap();
    assert!(ferroshop_core::password::verify("secret-password", &hash));
    let logged: Value = sqlx::query_scalar("SELECT payload FROM audit_log WHERE entity_id = $1")
        .bind(uid)
        .fetch_one(&db)
        .await
        .unwrap();
    assert_eq!(logged["password"], "***");
}
