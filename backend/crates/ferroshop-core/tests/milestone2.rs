//! Customers, orders, media and permissions against a real PostgreSQL.

use bytes::Bytes;
use ferroshop_core::acl::{Action, Permissions};
use ferroshop_core::criteria::Criteria;
use ferroshop_core::media::{self, MediaStore};
use ferroshop_core::order_state::{self, Machine};
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, Error, id, read};
use serde_json::{Value, json};

const GERMANY: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7001";
const CUSTOMER_GROUP: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a6e01";
const SALES_CHANNEL: &str = "98432def39fc4624b33213a56b8c944d";

fn criteria(value: Value) -> Criteria {
    serde_json::from_value(value).unwrap()
}

fn address(id: &str) -> Value {
    json!({ "id": id, "firstName": "Ada", "lastName": "Lovelace", "street": "Main St 1", "city": "Berlin", "countryId": GERMANY })
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn customer_with_default_address_in_one_payload(db: Db) {
    let ctx = Context::default();
    let address_id = id::format(id::new_id());
    let customer = json!({
        "groupId": CUSTOMER_GROUP, "salesChannelId": SALES_CHANNEL, "languageId": "2fbb5fe2e29a4d70aa5854ce7ce3e20b",
        "firstName": "Ada", "lastName": "Lovelace", "email": "ada@example.com",
        "addresses": [address(&address_id)],
        "defaultBillingAddressId": address_id,
    });
    let cid = write::write(&db, "customer", &customer, WriteMode::Create, &ctx).await.unwrap();
    let loaded = read::read(
        &db,
        "customer",
        &id::format(cid),
        &criteria(json!({ "associations": { "defaultBillingAddress": { "associations": { "country": {} } } } })),
        &ctx,
    )
    .await
    .unwrap();
    assert_eq!(loaded["customerNumber"], "10001", "numbers come from the sequence");
    assert_eq!(loaded["defaultBillingAddress"]["country"]["translated"]["name"], "Germany");

    // E-mail is unique (case-insensitive) for registered customers.
    let mut duplicate = customer.clone();
    duplicate["email"] = json!("ADA@example.com");
    duplicate["addresses"] = json!([]);
    duplicate.as_object_mut().unwrap().remove("defaultBillingAddressId");
    let err = write::write(&db, "customer", &duplicate, WriteMode::Create, &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Validation(_)), "{err:?}");
}

async fn create_order(db: &Db) -> uuid::Uuid {
    let order = json!({
        "salesChannelId": SALES_CHANNEL, "currencyId": "b7d2554b0ce847cd82f3ac9bd1c0dfca",
        "languageId": "2fbb5fe2e29a4d70aa5854ce7ce3e20b",
        "customerEmail": "ada@example.com", "customerFirstName": "Ada", "customerLastName": "Lovelace",
        "amountTotal": 23.8, "amountNet": 20,
        "lineItems": [{ "label": "Shirt", "quantity": 2, "unitPrice": 11.9, "totalPrice": 23.8, "taxRate": 19 }],
    });
    write::write(db, "order", &order, WriteMode::Create, &Context::default()).await.unwrap()
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn order_state_transitions(db: Db) {
    let ctx = Context::default();
    let oid = create_order(&db).await;

    assert_eq!(order_state::transition(&db, oid, Machine::Payment, "pay", &ctx).await.unwrap(), "paid");
    let err = order_state::transition(&db, oid, Machine::Payment, "pay", &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Conflict(_)), "{err:?}");
    order_state::transition(&db, oid, Machine::Order, "process", &ctx).await.unwrap();
    order_state::transition(&db, oid, Machine::Order, "complete", &ctx).await.unwrap();

    let order =
        read::read(&db, "order", &id::format(oid), &criteria(json!({ "associations": { "lineItems": {} } })), &ctx)
            .await
            .unwrap();
    assert_eq!(order["state"], "completed");
    assert_eq!(order["paymentState"], "paid");
    assert_eq!(order["lineItems"][0]["quantity"], 2);

    // States are read-only for the generic write path.
    write::write(&db, "order", &json!({ "id": id::format(oid), "state": "open" }), WriteMode::Update, &ctx)
        .await
        .unwrap();
    let order = read::read(&db, "order", &id::format(oid), &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(order["state"], "completed");
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn media_upload_creates_thumbnails_and_replaces_files(db: Db) {
    let ctx = Context::default();
    let store = MediaStore::memory();
    let mid = write::write(&db, "media", &json!({ "title": "Shirt" }), WriteMode::Create, &ctx).await.unwrap();

    let img = image::DynamicImage::new_rgb8(1000, 1000);
    let mut png = std::io::Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).unwrap();
    media::upload(&db, &store, mid, "Shirt Front", "PNG", Bytes::from(png.into_inner()), &ctx).await.unwrap();

    let first = read::read(&db, "media", &id::format(mid), &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(first["mimeType"], "image/png");
    assert_eq!(first["thumbnails"].as_array().unwrap().len(), 2);
    let first_path = first["path"].as_str().unwrap().to_string();
    assert!(first_path.ends_with("/shirt-front.png"), "{first_path}");
    assert!(store.get(&first_path).await.is_some());

    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    media::upload(&db, &store, mid, "doc", "pdf", Bytes::from_static(b"%PDF-1.4"), &ctx).await.unwrap();
    let second = read::read(&db, "media", &id::format(mid), &Criteria::default(), &ctx).await.unwrap();
    assert_eq!(second["thumbnails"], json!([]));
    assert!(store.get(&first_path).await.is_none(), "replaced files are deleted");

    let err = media::upload(&db, &store, mid, "x", "svg", Bytes::from_static(b"<svg/>"), &ctx).await.unwrap_err();
    assert!(matches!(err, Error::Validation(_)));
}

#[sqlx::test(migrator = "ferroshop_core::MIGRATOR")]
async fn permissions_follow_roles(db: Db) {
    let ctx = Context::default();
    let role = write::write(
        &db,
        "acl_role",
        &json!({ "name": "Orders", "privileges": ["order:read", "order:update"] }),
        WriteMode::Create,
        &ctx,
    )
    .await
    .unwrap();
    let user = write::write(
        &db,
        "user",
        &json!({ "username": "clerk", "email": "clerk@example.com", "password": "clerk-password",
                 "aclRoles": [{ "id": id::format(role) }] }),
        WriteMode::Create,
        &ctx,
    )
    .await
    .unwrap();
    let perms = Permissions::for_user(&db, user).await.unwrap().unwrap();
    assert!(perms.allows("order", Action::Update));
    assert!(!perms.allows("order", Action::Delete));
    assert!(!perms.allows("product", Action::Read));

    write::write(&db, "user", &json!({ "id": id::format(user), "active": false }), WriteMode::Update, &ctx)
        .await
        .unwrap();
    assert!(Permissions::for_user(&db, user).await.unwrap().is_none(), "inactive users have no permissions");
}
