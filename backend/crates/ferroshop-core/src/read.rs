//! Generic search / read for every entity.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value, json};
use sqlx::Row;

use crate::Db;
use crate::context::Context;
use crate::criteria::{Criteria, Filter, TotalCountMode};
use crate::definition::{EntityDefinition, FieldKind};
use crate::entities;
use crate::error::{Error, Result};
use crate::sql::{self, Binds, Param};

/// Hard upper bound for one page.
pub const MAX_LIMIT: u64 = 500;

#[derive(Debug, Default)]
pub struct SearchResult {
    pub total: i64,
    pub data: Vec<Value>,
    pub aggregations: Map<String, Value>,
}

pub async fn search(db: &Db, entity: &str, criteria: &Criteria, ctx: &Context) -> Result<SearchResult> {
    let mut result = search_def(db, entities::require(entity)?, criteria, ctx).await?;
    if let Some(includes) = &criteria.includes {
        result.data.iter_mut().for_each(|row| apply_includes(row, includes));
    }
    Ok(result)
}

/// Strips fields not listed in `includes`, keyed by each object's `apiAlias`.
/// Runs last so association loading can still rely on ids and foreign keys.
fn apply_includes(value: &mut Value, includes: &HashMap<String, Vec<String>>) {
    match value {
        Value::Object(map) => {
            let alias = map.get("apiAlias").and_then(Value::as_str).map(str::to_string);
            if let Some(allowed) = alias.and_then(|a| includes.get(&a)) {
                map.retain(|key, _| key == "apiAlias" || allowed.iter().any(|i| i == key));
            }
            map.values_mut().for_each(|v| apply_includes(v, includes));
        }
        Value::Array(items) => items.iter_mut().for_each(|v| apply_includes(v, includes)),
        _ => {}
    }
}

/// Reads a single entity by id.
pub async fn read(db: &Db, entity: &str, id: &str, criteria: &Criteria, ctx: &Context) -> Result<Value> {
    let def = entities::require(entity)?;
    if crate::id::parse(id).is_none() {
        return Err(Error::NotFound { entity: def.name.into(), id: id.into() });
    }
    let mut criteria = criteria.clone();
    criteria.ids = Some(vec![id.to_string()]);
    let mut result = search_def(db, def, &criteria, ctx).await?;
    if let Some(includes) = &criteria.includes {
        result.data.iter_mut().for_each(|row| apply_includes(row, includes));
    }
    result.data.pop().ok_or_else(|| Error::NotFound { entity: def.name.into(), id: id.into() })
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub fn search_def<'a>(
    db: &'a Db,
    def: &'static EntityDefinition,
    criteria: &'a Criteria,
    ctx: &'a Context,
) -> BoxFuture<'a, Result<SearchResult>> {
    Box::pin(async move {
        let alias = "t0";
        // No limit means "all": the API layer caps public requests, associations load fully.
        let limit = criteria.limit.map(|l| format!("LIMIT {}", l.min(MAX_LIMIT))).unwrap_or_default();

        let mut binds = Binds::new(ctx);
        let select = sql::select_json(def, alias, ctx);
        let where_sql = sql::where_clause(&mut binds, def, alias, criteria, ctx, true)?;
        let order = sql::order_by(&mut binds, def, alias, &criteria.sort, ctx)?;
        let query = format!(
            "SELECT {select} AS data FROM {table} {alias} WHERE {where_sql} ORDER BY {order} {limit} OFFSET {offset}",
            table = def.table,
            offset = criteria.offset(),
        );
        let rows = binds.query(&query).fetch_all(db).await?;
        let mut data: Vec<Value> =
            rows.into_iter().map(|row| row.try_get::<Value, _>("data")).collect::<std::result::Result<_, _>>()?;

        let total = match criteria.total_count_mode {
            TotalCountMode::None => data.len() as i64,
            TotalCountMode::Exact | TotalCountMode::NextPages => {
                let mut binds = Binds::new(ctx);
                let where_sql = sql::where_clause(&mut binds, def, alias, criteria, ctx, true)?;
                let query = format!("SELECT count(*) AS n FROM {} {alias} WHERE {where_sql}", def.table);
                binds.query(&query).fetch_one(db).await?.try_get::<i64, _>("n")?
            }
        };

        let aggregations = aggregate(db, def, criteria, ctx).await?;
        load_associations(db, def, &mut data, criteria, ctx).await?;
        Ok(SearchResult { total, data, aggregations })
    })
}

async fn aggregate(
    db: &Db,
    def: &'static EntityDefinition,
    criteria: &Criteria,
    ctx: &Context,
) -> Result<Map<String, Value>> {
    let mut out = Map::new();
    for agg in &criteria.aggregations {
        let alias = "t0";
        let mut binds = Binds::new(ctx);
        let where_sql = sql::where_clause(&mut binds, def, alias, criteria, ctx, false)?;
        let (expr, _) = sql::aggregation_expr(&mut binds, def, alias, &agg.field, ctx)?;
        let from = format!("FROM {} {alias} WHERE {where_sql}", def.table);
        let query = match agg.kind.as_str() {
            "count" => format!("SELECT jsonb_build_object('count', count(DISTINCT {expr})) AS r {from}"),
            "sum" | "avg" | "min" | "max" => {
                format!("SELECT jsonb_build_object('{0}', {0}({expr})) AS r {from}", agg.kind)
            }
            "stats" => format!(
                "SELECT jsonb_build_object('min', min({expr}), 'max', max({expr}), 'avg', avg({expr}), \
                 'sum', sum({expr})) AS r {from}"
            ),
            "terms" => {
                let limit = agg.limit.map(|l| format!("LIMIT {l}")).unwrap_or_default();
                format!(
                    "SELECT jsonb_build_object('buckets', COALESCE(jsonb_agg(jsonb_build_object(\
                     'key', k, 'count', c, 'apiAlias', 'aggregation_bucket')), '[]')) AS r FROM (\
                     SELECT {expr} AS k, count(*) AS c {from} AND {expr} IS NOT NULL \
                     GROUP BY 1 ORDER BY 2 DESC, 1 {limit}) buckets"
                )
            }
            other => return Err(Error::criteria(format!("unsupported aggregation type `{other}`"))),
        };
        let mut result: Value = binds.query(&query).fetch_one(db).await?.try_get("r")?;
        result["apiAlias"] = json!(format!("{}_aggregation", agg.name));
        out.insert(agg.name.clone(), result);
    }
    Ok(out)
}

/// Turns `{"manufacturer.media": {}}` into nested criteria.
fn normalized_associations(criteria: &Criteria) -> BTreeMap<String, Criteria> {
    let mut out: BTreeMap<String, Criteria> = BTreeMap::new();
    for (name, sub) in &criteria.associations {
        let mut parts = name.splitn(2, '.');
        let head = parts.next().unwrap_or_default().to_string();
        let entry = out.entry(head).or_default();
        match parts.next() {
            Some(rest) => {
                entry.associations.insert(rest.to_string(), sub.clone());
            }
            None => {
                let nested = std::mem::take(&mut entry.associations);
                *entry = sub.clone();
                entry.associations.extend(nested);
            }
        }
    }
    out
}

fn id_list(rows: &[Value], key: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    rows.iter()
        .filter_map(|r| r.get(key).and_then(Value::as_str))
        .filter(|id| seen.insert(id.to_string()))
        .map(str::to_string)
        .collect()
}

async fn load_associations(
    db: &Db,
    def: &'static EntityDefinition,
    rows: &mut [Value],
    criteria: &Criteria,
    ctx: &Context,
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    for (name, mut sub) in normalized_associations(criteria) {
        let field = def
            .field(&name)
            .filter(|f| f.kind.is_association())
            .ok_or_else(|| Error::criteria(format!("association `{name}` does not exist on `{}`", def.name)))?;
        sub.page = None;
        sub.limit = None;
        match field.kind {
            FieldKind::ManyToOne { entity, fk } => {
                let target = entities::require(entity)?;
                let ids = id_list(rows, fk);
                let found = fetch_by_ids(db, target, &sub, ids, ctx).await?;
                let by_id: HashMap<String, Value> =
                    found.into_iter().map(|v| (v["id"].as_str().unwrap_or_default().to_string(), v)).collect();
                for row in rows.iter_mut() {
                    let value = row.get(fk).and_then(Value::as_str).and_then(|id| by_id.get(id)).cloned();
                    row[name.as_str()] = value.unwrap_or(Value::Null);
                }
            }
            FieldKind::OneToMany { entity, reference } => {
                let target = entities::require(entity)?;
                let ids = id_list(rows, "id");
                sub.filter.push(Filter::EqualsAny { field: reference.into(), value: json!(ids) });
                let children = search_def(db, target, &sub, ctx).await?.data;
                for row in rows.iter_mut() {
                    let id = row["id"].clone();
                    let mine: Vec<Value> = children.iter().filter(|c| c[reference] == id).cloned().collect();
                    row[name.as_str()] = Value::Array(mine);
                }
            }
            FieldKind::ManyToMany { entity, mapping, local, reference } => {
                let target = entities::require(entity)?;
                let ids = id_list(rows, "id");
                let query = format!(
                    "SELECT replace({local}::text, '-', '') AS l, replace({reference}::text, '-', '') AS r \
                     FROM {mapping} WHERE {local} = ANY($2::uuid[])"
                );
                let mut binds = Binds::new(ctx);
                binds.bind(Param::TextArray(ids), "uuid[]");
                let pairs: Vec<(String, String)> = binds
                    .query(&query)
                    .fetch_all(db)
                    .await?
                    .into_iter()
                    .map(|r| Ok((r.try_get("l")?, r.try_get("r")?)))
                    .collect::<std::result::Result<_, sqlx::Error>>()?;
                let ref_ids: Vec<String> = {
                    let mut seen = HashSet::new();
                    pairs.iter().map(|(_, r)| r.clone()).filter(|r| seen.insert(r.clone())).collect()
                };
                let found = fetch_by_ids(db, target, &sub, ref_ids, ctx).await?;
                for row in rows.iter_mut() {
                    let id = row["id"].as_str().unwrap_or_default().to_string();
                    let linked: HashSet<&str> =
                        pairs.iter().filter(|(l, _)| *l == id).map(|(_, r)| r.as_str()).collect();
                    let mine: Vec<Value> = found
                        .iter()
                        .filter(|v| v["id"].as_str().is_some_and(|i| linked.contains(i)))
                        .cloned()
                        .collect();
                    row[name.as_str()] = Value::Array(mine);
                }
            }
            _ => unreachable!("filtered to associations"),
        }
    }
    Ok(())
}

async fn fetch_by_ids(
    db: &Db,
    def: &'static EntityDefinition,
    sub: &Criteria,
    ids: Vec<String>,
    ctx: &Context,
) -> Result<Vec<Value>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut criteria = sub.clone();
    criteria.ids = Some(ids);
    Ok(search_def(db, def, &criteria, ctx).await?.data)
}
