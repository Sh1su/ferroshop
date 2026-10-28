//! SEO URLs: readable paths for categories (`Shoes/Sneakers/`) and products (`Boot/B-1`) in
//! every sales channel and language, following Shopware's default templates.
//!
//! Generation only touches what changed: a new path becomes canonical, the old one stays as a
//! non-canonical row so existing links can still be resolved. Hand-edited URLs (`isModified`)
//! are never overwritten.

use std::collections::{HashMap, HashSet};

use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::context::Context;
use crate::criteria::{Criteria, Filter};
use crate::error::Result;
use crate::{Db, id, read};

pub const ROUTE_NAVIGATION: &str = "frontend.navigation.page";
pub const ROUTE_DETAIL: &str = "frontend.detail.page";

/// One URL segment: transliterated, everything but letters and digits collapsed to `-`, case
/// kept (like Shopware's slugifier).
pub fn slug(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        let mapped = match c {
            'ä' => "ae",
            'ö' => "oe",
            'ü' => "ue",
            'Ä' => "Ae",
            'Ö' => "Oe",
            'Ü' => "Ue",
            'ß' => "ss",
            'à' | 'á' | 'â' | 'ã' | 'å' => "a",
            'è' | 'é' | 'ê' | 'ë' => "e",
            'ì' | 'í' | 'î' | 'ï' => "i",
            'ò' | 'ó' | 'ô' | 'õ' | 'ø' => "o",
            'ù' | 'ú' | 'û' => "u",
            'ç' => "c",
            'ñ' => "n",
            c if c.is_ascii_alphanumeric() => {
                out.push(c);
                continue;
            }
            _ => "-",
        };
        out.push_str(mapped);
    }
    out.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-")
}

/// A URL generation wants to exist.
#[derive(Clone)]
struct Desired {
    foreign_key: Uuid,
    path: String,
}

/// What to rebuild: everything, or these entities (plus their variants / sub-categories).
#[derive(Debug, Clone)]
pub enum Scope {
    All,
    Products(Vec<Uuid>),
    Categories(Vec<Uuid>),
}

/// Ids of the entities and of everything below them (variants, sub-categories).
/// Call before deleting, so their URLs can be removed afterwards.
pub async fn with_descendants(db: &Db, table: &str, ids: &[Uuid]) -> Result<Vec<Uuid>> {
    let query = match table {
        "product" => "SELECT id FROM product WHERE id = ANY($1) OR parent_id = ANY($1)",
        _ => {
            "WITH RECURSIVE tree AS (SELECT id, 0 AS n FROM category WHERE id = ANY($1) \
             UNION ALL SELECT c.id, tree.n + 1 FROM category c JOIN tree ON c.parent_id = tree.id WHERE tree.n < 50) \
             SELECT DISTINCT id FROM tree"
        }
    };
    let mut found: Vec<Uuid> = sqlx::query_scalar(query).bind(ids).fetch_all(db).await?;
    // Deleted ids are kept so their URLs are removed.
    for id in ids {
        if !found.contains(id) {
            found.push(*id);
        }
    }
    Ok(found)
}

async fn products(db: &Db, ctx: &Context, ids: Option<&[Uuid]>) -> Result<Vec<Desired>> {
    let mut criteria = Criteria::default();
    if let Some(ids) = ids {
        criteria.filter.push(Filter::EqualsAny {
            field: "id".into(),
            value: Value::Array(ids.iter().map(|i| Value::String(id::format(*i))).collect()),
        });
    }
    let rows = read::search(db, "product", &criteria, ctx).await?.data;
    Ok(rows
        .into_iter()
        .filter_map(|p| {
            let foreign_key = p["id"].as_str().and_then(id::parse)?;
            let name = slug(p["translated"]["name"].as_str().unwrap_or_default());
            let number = slug(p["productNumber"].as_str().unwrap_or_default());
            let path = [name, number].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/");
            (!path.is_empty()).then_some(Desired { foreign_key, path })
        })
        .collect())
}

/// Category paths below the sales channel's navigation root; the root itself is the home page.
async fn categories(db: &Db, ctx: &Context, root: Uuid, ids: Option<&HashSet<Uuid>>) -> Result<Vec<Desired>> {
    let rows = read::search(db, "category", &Criteria::default(), ctx).await?.data;
    let mut parent: HashMap<Uuid, Option<Uuid>> = HashMap::new();
    let mut names: HashMap<Uuid, (String, bool)> = HashMap::new();
    for row in &rows {
        let Some(category) = row["id"].as_str().and_then(id::parse) else { continue };
        parent.insert(category, row["parentId"].as_str().and_then(id::parse));
        let link = row["type"].as_str() == Some("link");
        names.insert(category, (slug(row["translated"]["name"].as_str().unwrap_or_default()), link));
    }
    let mut out = Vec::new();
    for (&category, (_, link)) in &names {
        if *link || category == root || ids.is_some_and(|ids| !ids.contains(&category)) {
            continue;
        }
        let mut segments = Vec::new();
        let mut current = Some(category);
        let mut below_root = false;
        while let Some(node) = current {
            if node == root {
                below_root = true;
                break;
            }
            if segments.len() > 50 {
                break;
            }
            segments.push(names.get(&node).map(|n| n.0.clone()).unwrap_or_default());
            current = parent.get(&node).copied().flatten();
        }
        if !below_root || segments.iter().any(String::is_empty) {
            continue;
        }
        segments.reverse();
        out.push(Desired { foreign_key: category, path: format!("{}/", segments.join("/")) });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn path_info(route: &str, foreign_key: Uuid) -> String {
    match route {
        ROUTE_DETAIL => format!("/detail/{}", id::format(foreign_key)),
        _ => format!("/navigation/{}", id::format(foreign_key)),
    }
}

/// `Boot/B-1` → `Boot/B-1-2`, `Shoes/` → `Shoes-2/`.
fn numbered(path: &str, n: u32) -> String {
    match path.strip_suffix('/') {
        Some(base) => format!("{base}-{n}/"),
        None => format!("{path}-{n}"),
    }
}

/// Writes the desired URLs of one sales channel and language and removes URLs of entities in
/// `scope` that no longer get one.
async fn apply(
    db: &Db,
    channel: Uuid,
    language: Uuid,
    route: &'static str,
    desired: Vec<Desired>,
    scope: Option<&HashSet<Uuid>>,
) -> Result<()> {
    let rows = sqlx::query(
        "SELECT foreign_key, route_name, seo_path_info, is_canonical, is_modified FROM seo_url \
         WHERE sales_channel_id = $1 AND language_id = $2",
    )
    .bind(channel)
    .bind(language)
    .fetch_all(db)
    .await?;
    let mut owners: HashMap<String, Uuid> = HashMap::new();
    let mut canonical: HashMap<Uuid, (String, bool)> = HashMap::new();
    let mut existing: HashSet<Uuid> = HashSet::new();
    for row in rows {
        let foreign_key: Uuid = row.try_get("foreign_key")?;
        let path: String = row.try_get("seo_path_info")?;
        owners.insert(path.clone(), foreign_key);
        if row.try_get::<String, _>("route_name")? != route {
            continue;
        }
        existing.insert(foreign_key);
        if row.try_get("is_canonical")? {
            canonical.insert(foreign_key, (path, row.try_get("is_modified")?));
        }
    }

    let wanted: HashSet<Uuid> = desired.iter().map(|d| d.foreign_key).collect();
    let gone: Vec<Uuid> =
        existing.iter().filter(|fk| !wanted.contains(fk) && scope.is_none_or(|s| s.contains(fk))).copied().collect();
    let mut tx = db.begin().await?;
    if !gone.is_empty() {
        sqlx::query(
            "DELETE FROM seo_url WHERE sales_channel_id = $1 AND language_id = $2 AND route_name = $3 \
             AND foreign_key = ANY($4)",
        )
        .bind(channel)
        .bind(language)
        .bind(route)
        .bind(&gone)
        .execute(&mut *tx)
        .await?;
    }
    for want in desired {
        match canonical.get(&want.foreign_key) {
            Some((_, true)) => continue,
            Some((path, false)) if *path == want.path => continue,
            _ => {}
        }
        let mut candidate = want.path.clone();
        let mut n = 1;
        while owners.get(&candidate).is_some_and(|owner| *owner != want.foreign_key) {
            n += 1;
            candidate = numbered(&want.path, n);
        }
        if canonical.get(&want.foreign_key).is_some_and(|(path, _)| *path == candidate) {
            continue;
        }
        sqlx::query(
            "UPDATE seo_url SET is_canonical = false, updated_at = now() WHERE sales_channel_id = $1 \
             AND language_id = $2 AND foreign_key = $3 AND route_name = $4 AND is_canonical",
        )
        .bind(channel)
        .bind(language)
        .bind(want.foreign_key)
        .bind(route)
        .execute(&mut *tx)
        .await?;
        // A path this entity had before becomes canonical again.
        sqlx::query(
            "INSERT INTO seo_url (id, sales_channel_id, language_id, foreign_key, route_name, path_info, \
             seo_path_info, is_canonical) VALUES ($1, $2, $3, $4, $5, $6, $7, true) \
             ON CONFLICT (sales_channel_id, language_id, seo_path_info) DO UPDATE \
             SET is_canonical = true, is_deleted = false, updated_at = now() \
             WHERE seo_url.foreign_key = EXCLUDED.foreign_key",
        )
        .bind(id::new_id())
        .bind(channel)
        .bind(language)
        .bind(want.foreign_key)
        .bind(route)
        .bind(path_info(route, want.foreign_key))
        .bind(&candidate)
        .execute(&mut *tx)
        .await?;
        owners.insert(candidate.clone(), want.foreign_key);
        canonical.insert(want.foreign_key, (candidate, false));
    }
    tx.commit().await?;
    Ok(())
}

/// Brings SEO URLs up to date for `scope` in every sales channel and language.
pub async fn update(db: &Db, scope: Scope) -> Result<()> {
    let channels: Vec<(Uuid, Option<Uuid>)> = sqlx::query("SELECT id, navigation_category_id FROM sales_channel")
        .fetch_all(db)
        .await?
        .into_iter()
        .map(|r| Ok((r.try_get("id")?, r.try_get("navigation_category_id")?)))
        .collect::<std::result::Result<_, sqlx::Error>>()?;
    let languages: Vec<Uuid> = sqlx::query_scalar("SELECT id FROM language").fetch_all(db).await?;
    let (product_ids, category_ids) = match &scope {
        Scope::All => (None, None),
        Scope::Products(ids) => (Some(with_descendants(db, "product", ids).await?), None),
        Scope::Categories(ids) => (None, Some(with_descendants(db, "category", ids).await?)),
    };
    let do_products = !matches!(scope, Scope::Categories(_));
    let do_categories = !matches!(scope, Scope::Products(_));
    let product_scope: Option<HashSet<Uuid>> = product_ids.as_ref().map(|ids| ids.iter().copied().collect());
    let category_scope: Option<HashSet<Uuid>> = category_ids.as_ref().map(|ids| ids.iter().copied().collect());

    for language in languages {
        let ctx = Context { language_id: language, inheritance: true, actor_id: None };
        let product_urls = if do_products { Some(products(db, &ctx, product_ids.as_deref()).await?) } else { None };
        for (channel, root) in &channels {
            if let Some(urls) = &product_urls {
                apply(db, *channel, language, ROUTE_DETAIL, urls.clone(), product_scope.as_ref()).await?;
            }
            if do_categories {
                let urls = match root {
                    Some(root) => categories(db, &ctx, *root, category_scope.as_ref()).await?,
                    None => Vec::new(),
                };
                apply(db, *channel, language, ROUTE_NAVIGATION, urls, category_scope.as_ref()).await?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_keep_case_and_transliterate() {
        assert_eq!(slug("Herren Schuhe & Stiefel"), "Herren-Schuhe-Stiefel");
        assert_eq!(slug("Größe Ü"), "Groesse-Ue");
        assert_eq!(slug("SW-10001.2"), "SW-10001-2");
        assert_eq!(slug("../x"), "x");
        assert_eq!(numbered("Shoes/", 2), "Shoes-2/");
        assert_eq!(numbered("Boot/B-1", 3), "Boot/B-1-3");
    }
}
