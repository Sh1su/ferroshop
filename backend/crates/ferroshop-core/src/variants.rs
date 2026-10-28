//! Variant generation: one variant per combination of the chosen options (colour × size),
//! like the Administration's variant generator in Shopware.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde_json::{Value, json};
use sqlx::Row;
use uuid::Uuid;

use crate::context::Context;
use crate::error::{Error, Result, WriteError};
use crate::write::{self, WriteMode};
use crate::{Db, id};

/// Most variants one generation may create.
pub const MAX_COMBINATIONS: usize = 500;

fn invalid(pointer: &str, detail: impl Into<String>) -> Error {
    Error::Validation(vec![WriteError::new(pointer, "CONTENT__PRODUCT_VARIANT_GENERATION", detail)])
}

/// Every combination taking one option per group.
fn combinations(groups: &[Vec<Uuid>]) -> Vec<Vec<Uuid>> {
    groups.iter().fold(vec![Vec::new()], |acc, options| {
        acc.iter()
            .flat_map(|combo| {
                options.iter().map(move |option| {
                    let mut next = combo.clone();
                    next.push(*option);
                    next
                })
            })
            .collect()
    })
}

/// Creates the missing variants of `parent` for all combinations of `option_ids` (grouped by
/// property group). Existing combinations are kept; new variants get `<number>.<n>` and no stock.
/// Returns the ids of the created variants.
pub async fn generate(db: &Db, parent: Uuid, option_ids: &[Uuid], ctx: &Context) -> Result<Vec<Uuid>> {
    let product = sqlx::query("SELECT product_number, parent_id FROM product WHERE id = $1")
        .bind(parent)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| Error::NotFound { entity: "product".into(), id: id::format(parent) })?;
    if product.try_get::<Option<Uuid>, _>("parent_id")?.is_some() {
        return Err(invalid("/", "variants can only be generated for a main product"));
    }
    let number: String = product.try_get("product_number")?;

    let rows = sqlx::query(
        "SELECT o.id, o.group_id FROM property_group_option o WHERE o.id = ANY($1) \
         ORDER BY o.group_id, o.position, o.id",
    )
    .bind(option_ids)
    .fetch_all(db)
    .await?;
    let mut groups: BTreeMap<Uuid, Vec<Uuid>> = BTreeMap::new();
    for row in &rows {
        groups.entry(row.try_get("group_id")?).or_default().push(row.try_get("id")?);
    }
    let known: HashSet<Uuid> = groups.values().flatten().copied().collect();
    if let Some(unknown) = option_ids.iter().find(|o| !known.contains(o)) {
        return Err(invalid("/options", format!("option `{}` does not exist", id::format(*unknown))));
    }
    if groups.is_empty() {
        return Err(invalid("/options", "choose at least one option"));
    }
    let groups: Vec<Vec<Uuid>> = groups.into_values().collect();
    let count: usize = groups.iter().map(Vec::len).product();
    if count > MAX_COMBINATIONS {
        return Err(invalid("/options", format!("{count} combinations, at most {MAX_COMBINATIONS} are allowed")));
    }

    let existing_rows = sqlx::query(
        "SELECT p.id, p.product_number, po.option_id FROM product p \
         LEFT JOIN product_option po ON po.product_id = p.id WHERE p.parent_id = $1",
    )
    .bind(parent)
    .fetch_all(db)
    .await?;
    let mut existing: BTreeMap<Uuid, BTreeSet<Uuid>> = BTreeMap::new();
    let mut numbers: HashSet<String> = HashSet::new();
    for row in existing_rows {
        let entry = existing.entry(row.try_get("id")?).or_default();
        if let Some(option) = row.try_get::<Option<Uuid>, _>("option_id")? {
            entry.insert(option);
        }
        numbers.insert(row.try_get("product_number")?);
    }
    let existing: HashSet<BTreeSet<Uuid>> = existing.into_values().collect();

    let mut n = 0;
    let mut payloads = Vec::new();
    for combo in combinations(&groups) {
        if existing.contains(&combo.iter().copied().collect::<BTreeSet<_>>()) {
            continue;
        }
        let variant_number = loop {
            n += 1;
            let candidate = format!("{number}.{n}");
            if !numbers.contains(&candidate) {
                break candidate;
            }
        };
        let options: Vec<Value> = combo.iter().map(|o| json!({ "id": id::format(*o) })).collect();
        payloads.push(json!({
            "parentId": id::format(parent),
            "productNumber": variant_number,
            "stock": 0,
            "options": options,
        }));
    }
    if payloads.is_empty() {
        return Ok(Vec::new());
    }
    write::write_many(db, "product", &payloads, WriteMode::Create, ctx).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_one_option_per_group() {
        let (a, b, c) = (Uuid::from_u128(1), Uuid::from_u128(2), Uuid::from_u128(3));
        assert_eq!(combinations(&[vec![a, b], vec![c]]), vec![vec![a, c], vec![b, c]]);
        assert_eq!(combinations(&[vec![a, b], vec![b, c]]).len(), 4);
    }
}
