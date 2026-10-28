//! Demo data, written through the regular entity layer.

use ferroshop_core::order_state::{self, Machine};
use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, Result, defaults, id};
use serde_json::{Value, json};
use uuid::Uuid;

fn price(gross: f64) -> Value {
    let net = (gross / 1.19 * 100.0).round() / 100.0;
    json!([{ "currencyId": id::format(defaults::CURRENCY), "gross": gross, "net": net, "linked": true }])
}

pub async fn run(db: &Db) -> Result<usize> {
    let ctx = Context::default();
    let de = Context { language_id: defaults::LANGUAGE_DE, ..Context::default() };
    let root = id::format(defaults::ROOT_CATEGORY);

    let mut categories = Vec::new();
    for (en, de_name) in [("Clothing", "Bekleidung"), ("Shoes", "Schuhe"), ("Accessories", "Accessoires")] {
        let cid =
            write::write(db, "category", &json!({ "parentId": root, "name": en }), WriteMode::Create, &ctx).await?;
        write::write(db, "category", &json!({ "id": id::format(cid), "name": de_name }), WriteMode::Update, &de)
            .await?;
        categories.push(id::format(cid));
    }

    let mut manufacturers = Vec::new();
    for name in ["Ferris & Co.", "Crab Apparel", "Rusty Goods"] {
        let mid = write::write(db, "product_manufacturer", &json!({ "name": name }), WriteMode::Create, &ctx).await?;
        manufacturers.push(id::format(mid));
    }

    let (sizes, colors, cotton) = properties(db, &ctx).await?;

    let items = [
        ("T-Shirt Ferris", "T-Shirt Ferris", 19.99, 0),
        ("Hoodie Borrow Checker", "Hoodie Borrow Checker", 49.90, 0),
        ("Sneaker Async", "Sneaker Async", 89.00, 1),
        ("Boots Lifetime", "Stiefel Lifetime", 129.00, 1),
        ("Cap Unsafe", "Kappe Unsafe", 14.50, 2),
        ("Mug Zero-Cost", "Tasse Zero-Cost", 9.99, 2),
    ];
    let mut count = 0;
    let mut catalog = Vec::new();
    for (i, (en, de_name, gross, cat)) in items.iter().enumerate() {
        let product = json!({
            "productNumber": format!("FS-{:05}", 10001 + i),
            "name": en,
            "translations": { id::format(defaults::LANGUAGE_DE): { "name": de_name } },
            "description": format!("{en} – made with Rust."),
            "stock": 25 + i as i64 * 7,
            "active": true,
            "taxId": id::format(defaults::TAX_STANDARD),
            "manufacturerId": manufacturers[i % manufacturers.len()],
            "price": price(*gross),
            "categories": [{ "id": categories[*cat] }],
            "properties": if *cat == 0 { json!([{ "id": id::format(cotton) }]) } else { json!([]) },
        });
        let pid = write::write(db, "product", &product, WriteMode::Create, &ctx).await?;
        catalog.push((pid, *en, *gross));
        count += 1;
        if *cat == 0 {
            // Clothing comes in sizes, the T-shirt also in colours.
            let mut options = sizes.clone();
            if i == 0 {
                options.extend(colors.iter().copied());
            }
            let variants = ferroshop_core::variants::generate(db, pid, &options, &ctx).await?;
            for variant in &variants {
                let stock = json!({ "id": id::format(*variant), "stock": 10 });
                write::write(db, "product", &stock, WriteMode::Update, &ctx).await?;
            }
            count += variants.len();
        }
    }
    customers_and_orders(db, &catalog).await?;
    Ok(count)
}

/// Size and colour groups for variants, a material group as a plain (filterable) property.
async fn properties(db: &Db, ctx: &Context) -> Result<(Vec<Uuid>, Vec<Uuid>, Uuid)> {
    let de = id::format(defaults::LANGUAGE_DE);
    let group = |name: &str, de_name: &str, display: &str, sorting: &str, options: Vec<(&str, &str, Option<&str>)>| {
        let options: Vec<Value> = options
            .iter()
            .enumerate()
            .map(|(position, (en, de_option, color))| {
                json!({ "name": en, "translations": { de.clone(): { "name": de_option } },
                        "position": position + 1, "colorHexCode": color })
            })
            .collect();
        json!({ "name": name, "translations": { de.clone(): { "name": de_name } }, "displayType": display,
                "sortingType": sorting, "options": options })
    };
    let size = group("Size", "Größe", "text", "position", vec![("S", "S", None), ("M", "M", None), ("L", "L", None)]);
    let color = group(
        "Colour",
        "Farbe",
        "color",
        "alphanumeric",
        vec![("Rust", "Rost", Some("#b7410e")), ("Ocean", "Ozean", Some("#1f6f8b"))],
    );
    let material = group("Material", "Material", "text", "alphanumeric", vec![("Cotton", "Baumwolle", None)]);
    let mut ids = Vec::new();
    for payload in [size, color, material] {
        let group = write::write(db, "property_group", &payload, WriteMode::Create, ctx).await?;
        let options: Vec<Uuid> =
            sqlx::query_scalar("SELECT id FROM property_group_option WHERE group_id = $1 ORDER BY position")
                .bind(group)
                .fetch_all(db)
                .await?;
        ids.push(options);
    }
    Ok((ids[0].clone(), ids[1].clone(), ids[2][0]))
}

const GERMANY: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7001";
const CUSTOMER_GROUP: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a6e01";
const SALES_CHANNEL: &str = "98432def39fc4624b33213a56b8c944d";
const PREPAYMENT: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7101";
const INVOICE: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7102";
const STANDARD_SHIPPING: &str = "0191a6f0f4a07c7e9b1c2d3e4f5a7201";

async fn customers_and_orders(db: &Db, catalog: &[(uuid::Uuid, &str, f64)]) -> Result<()> {
    let ctx = Context::default();
    let people = [
        ("Ada", "Lovelace", "ada@example.com", "Hauptstraße 1", "10115", "Berlin"),
        ("Grace", "Hopper", "grace@example.com", "Ringstraße 7", "80331", "München"),
        ("Linus", "Torvalds", "linus@example.com", "Hafenweg 3", "20095", "Hamburg"),
        ("Margaret", "Hamilton", "margaret@example.com", "Domplatz 12", "50667", "Köln"),
    ];
    let round = |v: f64| (v * 100.0).round() / 100.0;
    for (i, (first, last, email, street, zip, city)) in people.iter().enumerate() {
        let address_id = id::format(id::new_id());
        let address = json!({ "id": address_id, "firstName": first, "lastName": last, "street": street,
            "zipcode": zip, "city": city, "countryId": GERMANY });
        let customer = json!({
            "groupId": CUSTOMER_GROUP, "salesChannelId": SALES_CHANNEL,
            "languageId": id::format(if i % 2 == 0 { defaults::LANGUAGE_DE } else { defaults::LANGUAGE_SYSTEM }),
            "firstName": first, "lastName": last, "email": email, "password": "demo-password",
            "defaultPaymentMethodId": PREPAYMENT, "addresses": [address],
        });
        let cid = write::write(db, "customer", &customer, WriteMode::Create, &ctx).await?;
        let defaults = json!({ "id": id::format(cid), "defaultBillingAddressId": address_id, "defaultShippingAddressId": address_id });
        write::write(db, "customer", &defaults, WriteMode::Update, &ctx).await?;

        for o in 0..(i % 3 + 1) {
            let picks: Vec<&(uuid::Uuid, &str, f64)> = catalog.iter().skip((i + o) % catalog.len()).take(2).collect();
            let mut positions = 0.0;
            let line_items: Vec<_> = picks
                .iter()
                .enumerate()
                .map(|(p, (pid, label, gross))| {
                    let quantity = (p + o) as i64 % 3 + 1;
                    let total = round(gross * quantity as f64);
                    positions += total;
                    json!({ "productId": id::format(*pid), "label": label, "quantity": quantity,
                        "unitPrice": gross, "totalPrice": total, "taxRate": 19, "position": p + 1 })
                })
                .collect();
            let total = round(positions + 4.99);
            let order_address = id::format(id::new_id());
            let order = json!({
                "salesChannelId": SALES_CHANNEL, "currencyId": id::format(defaults::CURRENCY),
                "languageId": id::format(defaults::LANGUAGE_SYSTEM), "customerId": id::format(cid),
                "customerEmail": email, "customerFirstName": first, "customerLastName": last,
                "paymentMethodId": if o % 2 == 0 { PREPAYMENT } else { INVOICE },
                "shippingMethodId": STANDARD_SHIPPING,
                "positionPrice": round(positions), "shippingTotal": 4.99,
                "amountTotal": total, "amountNet": round(total / 1.19),
                "lineItems": line_items,
                "addresses": [{ "id": order_address, "firstName": first, "lastName": last, "street": street,
                    "zipcode": zip, "city": city, "countryId": GERMANY }],
            });
            let oid = write::write(db, "order", &order, WriteMode::Create, &ctx).await?;
            let link =
                json!({ "id": id::format(oid), "billingAddressId": order_address, "shippingAddressId": order_address });
            write::write(db, "order", &link, WriteMode::Update, &ctx).await?;
            let steps: &[(Machine, &str)] = match (i + o) % 4 {
                0 => &[],
                1 => &[(Machine::Payment, "pay"), (Machine::Order, "process")],
                2 => &[(Machine::Payment, "pay"), (Machine::Order, "process"), (Machine::Delivery, "ship")],
                _ => &[(Machine::Order, "cancel"), (Machine::Payment, "cancel")],
            };
            for (machine, transition) in steps {
                order_state::transition(db, oid, *machine, transition, &ctx).await?;
            }
        }
    }
    Ok(())
}
