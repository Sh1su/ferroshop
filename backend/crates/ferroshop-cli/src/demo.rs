//! Demo data, written through the regular entity layer.

use ferroshop_core::write::{self, WriteMode};
use ferroshop_core::{Context, Db, Result, defaults, id};
use serde_json::{Value, json};

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

    let items = [
        ("T-Shirt Ferris", "T-Shirt Ferris", 19.99, 0),
        ("Hoodie Borrow Checker", "Hoodie Borrow Checker", 49.90, 0),
        ("Sneaker Async", "Sneaker Async", 89.00, 1),
        ("Boots Lifetime", "Stiefel Lifetime", 129.00, 1),
        ("Cap Unsafe", "Kappe Unsafe", 14.50, 2),
        ("Mug Zero-Cost", "Tasse Zero-Cost", 9.99, 2),
    ];
    let mut count = 0;
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
        });
        let pid = write::write(db, "product", &product, WriteMode::Create, &ctx).await?;
        count += 1;
        if *cat == 0 {
            for size in ["S", "M", "L"] {
                let variant = json!({
                    "parentId": id::format(pid),
                    "productNumber": format!("FS-{:05}.{size}", 10001 + i),
                    "stock": 10,
                });
                write::write(db, "product", &variant, WriteMode::Create, &ctx).await?;
                count += 1;
            }
        }
    }
    Ok(count)
}
