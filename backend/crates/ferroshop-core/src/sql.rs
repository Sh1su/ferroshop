//! Compiles entity definitions + criteria into PostgreSQL.
//!
//! All values are bound as text (or text[]) and cast to the field's type in SQL, which keeps
//! dynamic queries simple. `$1` is always the context language id.

use serde_json::Value;
use sqlx::AssertSqlSafe;
use sqlx::postgres::{PgArguments, Postgres};
use sqlx::query::Query;

use crate::context::Context;
use crate::criteria::{Criteria, Filter, Sort};
use crate::defaults;
use crate::definition::{EntityDefinition, Field, FieldKind, SEARCHABLE};
use crate::entities;
use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub(crate) enum Param {
    Text(Option<String>),
    TextArray(Vec<String>),
}

/// Collects bind parameters and hands out unique table aliases while SQL is generated.
pub(crate) struct Binds {
    params: Vec<Param>,
    aliases: usize,
}

impl Binds {
    pub fn new(ctx: &Context) -> Self {
        Self { params: vec![Param::Text(Some(ctx.language_id.to_string()))], aliases: 0 }
    }

    /// Binds a value and returns its placeholder, e.g. `$3::uuid`.
    pub fn bind(&mut self, param: Param, cast: &str) -> String {
        self.params.push(param);
        format!("${}::{}", self.params.len(), cast)
    }

    pub fn text(&mut self, value: impl Into<String>, cast: &str) -> String {
        self.bind(Param::Text(Some(value.into())), cast)
    }

    pub fn alias(&mut self) -> String {
        self.aliases += 1;
        format!("t{}", self.aliases)
    }

    /// Builds the query. Identifiers in `sql` only ever come from static entity
    /// definitions; every user value goes through a bind parameter.
    pub fn query(&self, sql: &str) -> Query<'static, Postgres, PgArguments> {
        let mut query = sqlx::query(AssertSqlSafe(sql.to_string()));
        for param in &self.params {
            query = match param {
                Param::Text(v) => query.bind(v.clone()),
                Param::TextArray(v) => query.bind(v.clone()),
            };
        }
        query
    }
}

/// Whether translated fields fall back to the system language.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lang {
    /// Only the context language (what the Admin edits).
    Raw,
    /// Context language, falling back to the system language (`translated`, filters, sorting).
    Resolved,
}

fn column_expr(def: &EntityDefinition, alias: &str, field: &Field, lang: Lang) -> String {
    if let Some(sql) = field.kind.computed_sql() {
        return sql.replace("{a}", alias);
    }
    if !field.translated() {
        return format!("{alias}.{}", field.column);
    }
    let table = def.translation_table.expect("translated field without translation table");
    let fk = def.translation_fk();
    let col = field.column;
    let current = format!("(SELECT tr.{col} FROM {table} tr WHERE tr.{fk} = {alias}.id AND tr.language_id = $1::uuid)");
    match lang {
        Lang::Raw => current,
        Lang::Resolved => format!(
            "COALESCE({current}, (SELECT tr.{col} FROM {table} tr WHERE tr.{fk} = {alias}.id \
             AND tr.language_id = '{}'))",
            defaults::LANGUAGE_SYSTEM
        ),
    }
}

/// SQL expression for a scalar field, including variant -> parent inheritance.
pub(crate) fn field_expr(def: &EntityDefinition, alias: &str, field: &Field, ctx: &Context, lang: Lang) -> String {
    let own = column_expr(def, alias, field, lang);
    let inherit_fk = def.inherit_from.and_then(|name| def.field(name));
    match inherit_fk {
        Some(fk) if ctx.inheritance && field.has(crate::definition::INHERITED) => {
            let parent_alias = format!("{alias}_p");
            let parent = column_expr(def, &parent_alias, field, lang);
            format!(
                "COALESCE({own}, (SELECT {parent} FROM {table} {parent_alias} \
                 WHERE {parent_alias}.id = {alias}.{fk_col}))",
                table = def.table,
                fk_col = fk.column
            )
        }
        _ => own,
    }
}

/// Wraps an expression so it is emitted in API format (ids without dashes).
fn output_expr(field: &Field, expr: String) -> String {
    match field.kind {
        FieldKind::Id | FieldKind::Fk(_) => format!("replace(({expr})::text, '-', '')"),
        _ => expr,
    }
}

/// `jsonb_build_object(...)` for a whole row, including the `translated` object.
pub(crate) fn select_json(def: &EntityDefinition, alias: &str, ctx: &Context) -> String {
    let mut pairs: Vec<String> = Vec::new();
    for field in def.fields {
        if field.kind.is_association() || field.kind == FieldKind::Password {
            continue;
        }
        let expr = output_expr(field, field_expr(def, alias, field, ctx, Lang::Raw));
        pairs.push(format!("'{}', {expr}", field.name));
    }
    pairs.push(format!("'apiAlias', '{}'", def.name));
    if def.translation_table.is_some() {
        let translated: Vec<String> = def
            .translated_fields()
            .map(|f| format!("'{}', {}", f.name, field_expr(def, alias, f, ctx, Lang::Resolved)))
            .collect();
        pairs.push(format!("'translated', jsonb_build_object({})", translated.join(", ")));
    }
    // jsonb_build_object accepts at most 100 arguments, so build in chunks.
    pairs.chunks(40).map(|chunk| format!("jsonb_build_object({})", chunk.join(", "))).collect::<Vec<_>>().join(" || ")
}

/// Splits `product.manufacturer.name` into segments, dropping a leading entity name.
fn split_path<'a>(def: &EntityDefinition, path: &'a str) -> Vec<&'a str> {
    let mut segments: Vec<&str> = path.split('.').collect();
    if segments.len() > 1 && segments[0] == def.name {
        segments.remove(0);
    }
    segments
}

fn unknown_field(def: &EntityDefinition, name: &str) -> Error {
    Error::criteria(format!("field `{name}` does not exist on `{}`", def.name))
}

/// Resolves a path made of to-one hops to a scalar expression.
pub(crate) fn scalar_path(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    path: &[&str],
    ctx: &Context,
) -> Result<(String, &'static Field)> {
    let (lang, path) = match path {
        ["translated", rest @ ..] if !rest.is_empty() => (Lang::Resolved, rest),
        _ => (Lang::Resolved, path),
    };
    let field = def.field(path[0]).ok_or_else(|| unknown_field(def, path[0]))?;
    if path.len() == 1 {
        if field.kind.is_association() || field.kind == FieldKind::Password {
            return Err(Error::criteria(format!("`{}` is not a scalar field", field.name)));
        }
        return Ok((field_expr(def, alias, field, ctx, lang), field));
    }
    match field.kind {
        FieldKind::ManyToOne { entity, fk } => {
            let target = entities::require(entity)?;
            let fk_field = def.field(fk).expect("validated fk");
            let fk_expr = field_expr(def, alias, fk_field, ctx, lang);
            if path[1..] == ["id"] {
                return Ok((fk_expr, fk_field));
            }
            let t = binds.alias();
            let (inner, leaf) = scalar_path(binds, target, &t, &path[1..], ctx)?;
            Ok((format!("(SELECT {inner} FROM {} {t} WHERE {t}.id = {fk_expr})", target.table), leaf))
        }
        _ => Err(Error::criteria(format!("`{}` must be a to-one association to be used here", field.name))),
    }
}

/// Builds a condition on a path; to-many hops become `EXISTS` sub-queries.
fn condition(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    path: &[&str],
    ctx: &Context,
    build: &mut dyn FnMut(&mut Binds, &str, &Field) -> Result<String>,
) -> Result<String> {
    let field = def.field(path[0]);
    match field.map(|f| f.kind) {
        Some(FieldKind::OneToMany { entity, reference }) if path.len() > 1 => {
            let target = entities::require(entity)?;
            let ref_col = target.field(reference).expect("validated reference").column;
            let t = binds.alias();
            let inner = condition(binds, target, &t, &path[1..], ctx, build)?;
            Ok(format!("EXISTS (SELECT 1 FROM {} {t} WHERE {t}.{ref_col} = {alias}.id AND {inner})", target.table))
        }
        Some(FieldKind::ManyToMany { entity, mapping, local, reference }) if path.len() > 1 => {
            let target = entities::require(entity)?;
            let m = binds.alias();
            if path[1..] == ["id"] {
                let id_field = target.field("id").expect("id");
                let inner = build(binds, &format!("{m}.{reference}"), id_field)?;
                return Ok(format!("EXISTS (SELECT 1 FROM {mapping} {m} WHERE {m}.{local} = {alias}.id AND {inner})"));
            }
            let t = binds.alias();
            let inner = condition(binds, target, &t, &path[1..], ctx, build)?;
            Ok(format!(
                "EXISTS (SELECT 1 FROM {mapping} {m} JOIN {} {t} ON {t}.id = {m}.{reference} \
                 WHERE {m}.{local} = {alias}.id AND {inner})",
                target.table
            ))
        }
        Some(FieldKind::ManyToOne { entity, fk }) if !is_to_one_path(def, path) => {
            let target = entities::require(entity)?;
            let fk_field = def.field(fk).expect("validated fk");
            let fk_expr = field_expr(def, alias, fk_field, ctx, Lang::Resolved);
            let t = binds.alias();
            let inner = condition(binds, target, &t, &path[1..], ctx, build)?;
            Ok(format!("EXISTS (SELECT 1 FROM {} {t} WHERE {t}.id = {fk_expr} AND {inner})", target.table))
        }
        _ => {
            let (expr, leaf) = scalar_path(binds, def, alias, path, ctx)?;
            build(binds, &expr, leaf)
        }
    }
}

fn is_to_one_path(def: &EntityDefinition, path: &[&str]) -> bool {
    let path = if path.first() == Some(&"translated") { &path[1..] } else { path };
    match def.field(path[0]).map(|f| f.kind) {
        Some(FieldKind::ManyToOne { entity, .. }) if path.len() > 1 => {
            entities::get(entity).is_some_and(|t| is_to_one_path(t, &path[1..]))
        }
        Some(FieldKind::OneToMany { .. } | FieldKind::ManyToMany { .. }) => false,
        _ => true,
    }
}

fn scalar_to_string(value: &Value) -> Result<String> {
    match value {
        Value::String(s) => Ok(s.clone()),
        Value::Number(n) => Ok(n.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        other => Err(Error::criteria(format!("expected a scalar value, got {other}"))),
    }
}

fn cast_for(field: &Field) -> Result<&'static str> {
    match field.kind {
        FieldKind::Json | FieldKind::Price => {
            Err(Error::criteria(format!("cannot filter on json field `{}`", field.name)))
        }
        kind => Ok(kind.sql_type()),
    }
}

fn escape_like(value: &str) -> String {
    value.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub(crate) fn compile_filter(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    filter: &Filter,
    ctx: &Context,
) -> Result<String> {
    let like = |binds: &mut Binds, field: &str, value: &Value, pattern: fn(&str) -> String| {
        let value = escape_like(&scalar_to_string(value)?);
        let path = split_path(def, field);
        condition(binds, def, alias, &path, ctx, &mut |binds, expr, _| {
            let placeholder = binds.text(pattern(&value), "text");
            Ok(format!("({expr})::text ILIKE {placeholder}"))
        })
    };
    match filter {
        Filter::Equals { field, value } => {
            let path = split_path(def, field);
            condition(binds, def, alias, &path, ctx, &mut |binds, expr, leaf| {
                if value.is_null() {
                    return Ok(format!("({expr}) IS NULL"));
                }
                let placeholder = binds.text(scalar_to_string(value)?, cast_for(leaf)?);
                Ok(format!("({expr}) = {placeholder}"))
            })
        }
        Filter::EqualsAny { field, value } => {
            let values: Vec<String> = match value {
                Value::Array(items) => items.iter().map(scalar_to_string).collect::<Result<_>>()?,
                Value::String(s) => s.split('|').map(str::to_string).collect(),
                other => vec![scalar_to_string(other)?],
            };
            let path = split_path(def, field);
            condition(binds, def, alias, &path, ctx, &mut |binds, expr, leaf| {
                let placeholder = binds.bind(Param::TextArray(values.clone()), &format!("{}[]", cast_for(leaf)?));
                Ok(format!("({expr}) = ANY({placeholder})"))
            })
        }
        Filter::Contains { field, value } => like(binds, field, value, |v| format!("%{v}%")),
        Filter::Prefix { field, value } => like(binds, field, value, |v| format!("{v}%")),
        Filter::Suffix { field, value } => like(binds, field, value, |v| format!("%{v}")),
        Filter::Range { field, parameters } => {
            let path = split_path(def, field);
            condition(binds, def, alias, &path, ctx, &mut |binds, expr, leaf| {
                let cast = cast_for(leaf)?;
                let mut parts = Vec::new();
                for (op, value) in
                    [(">=", &parameters.gte), ("<=", &parameters.lte), (">", &parameters.gt), ("<", &parameters.lt)]
                {
                    if let Some(value) = value {
                        let placeholder = binds.text(scalar_to_string(value)?, cast);
                        parts.push(format!("({expr}) {op} {placeholder}"));
                    }
                }
                Ok(if parts.is_empty() { "TRUE".into() } else { parts.join(" AND ") })
            })
        }
        Filter::Multi { operator, queries } => join_filters(binds, def, alias, operator, queries, ctx),
        Filter::Not { operator, queries } => {
            Ok(format!("NOT ({})", join_filters(binds, def, alias, operator, queries, ctx)?))
        }
    }
}

fn join_filters(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    operator: &str,
    queries: &[Filter],
    ctx: &Context,
) -> Result<String> {
    if queries.is_empty() {
        return Ok("TRUE".into());
    }
    let glue = if operator.eq_ignore_ascii_case("or") { " OR " } else { " AND " };
    let parts = queries
        .iter()
        .map(|q| compile_filter(binds, def, alias, q, ctx).map(|sql| format!("({sql})")))
        .collect::<Result<Vec<_>>>()?;
    Ok(parts.join(glue))
}

/// WHERE clause for a criteria. Post filters are excluded for aggregations.
pub(crate) fn where_clause(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    criteria: &Criteria,
    ctx: &Context,
    with_post_filter: bool,
) -> Result<String> {
    let mut parts = vec!["TRUE".to_string()];
    if let Some(ids) = &criteria.ids {
        for id in ids {
            if crate::id::parse(id).is_none() {
                return Err(Error::criteria(format!("`{id}` is not a valid id")));
            }
        }
        let placeholder = binds.bind(Param::TextArray(ids.clone()), "uuid[]");
        parts.push(format!("{alias}.id = ANY({placeholder})"));
    }
    for filter in &criteria.filter {
        parts.push(format!("({})", compile_filter(binds, def, alias, filter, ctx)?));
    }
    if with_post_filter {
        for filter in &criteria.post_filter {
            parts.push(format!("({})", compile_filter(binds, def, alias, filter, ctx)?));
        }
    }
    if let Some(term) = criteria.term.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let placeholder = binds.text(format!("%{}%", escape_like(term)), "text");
        let ors: Vec<String> = def
            .fields
            .iter()
            .filter(|f| f.has(SEARCHABLE))
            .map(|f| format!("({})::text ILIKE {placeholder}", field_expr(def, alias, f, ctx, Lang::Resolved)))
            .collect();
        if !ors.is_empty() {
            parts.push(format!("({})", ors.join(" OR ")));
        }
    }
    Ok(parts.join(" AND "))
}

pub(crate) fn order_by(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    sorts: &[Sort],
    ctx: &Context,
) -> Result<String> {
    let mut parts = Vec::new();
    for sort in sorts {
        let path = split_path(def, &sort.field);
        let (expr, _) = scalar_path(binds, def, alias, &path, ctx)?;
        let dir = if sort.descending() { "DESC NULLS LAST" } else { "ASC NULLS FIRST" };
        parts.push(format!("({expr}) {dir}"));
    }
    parts.push(format!("{alias}.id ASC"));
    Ok(parts.join(", "))
}

/// Expression + leaf field for aggregations (to-one paths only).
pub(crate) fn aggregation_expr(
    binds: &mut Binds,
    def: &EntityDefinition,
    alias: &str,
    field: &str,
    ctx: &Context,
) -> Result<(String, &'static Field)> {
    let path = split_path(def, field);
    let (expr, leaf) = scalar_path(binds, def, alias, &path, ctx)?;
    Ok((output_expr(leaf, expr), leaf))
}
