//! Generic create / update / upsert / delete, including translations, nested
//! associations and the audit log. One call runs in one transaction.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;

use serde_json::{Map, Value};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::Db;
use crate::context::Context;
use crate::defaults;
use crate::definition::{EntityDefinition, Field, FieldKind, INHERITED, READ_ONLY};
use crate::entities;
use crate::error::{Error, Result, WriteError};
use crate::sql::{Binds, Param};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteMode {
    /// Fails if the id already exists.
    Create,
    /// Fails if the id does not exist.
    Update,
    Upsert,
}

/// Keys the API emits on read that are ignored on write.
const IGNORED_KEYS: &[&str] = &["apiAlias", "translated", "extensions", "_uniqueIdentifier", "versionId"];

pub async fn write(db: &Db, entity: &str, payload: &Value, mode: WriteMode, ctx: &Context) -> Result<Uuid> {
    let mut ids = write_many(db, entity, std::slice::from_ref(payload), mode, ctx).await?;
    Ok(ids.remove(0))
}

pub async fn write_many(
    db: &Db,
    entity: &str,
    payloads: &[Value],
    mode: WriteMode,
    ctx: &Context,
) -> Result<Vec<Uuid>> {
    let def = entities::require(entity)?;
    let mut tx = db.begin().await?;
    let mut writer = Writer { tx: &mut tx, ctx, errors: Vec::new() };
    let mut ids = Vec::with_capacity(payloads.len());
    for (i, payload) in payloads.iter().enumerate() {
        let pointer = if payloads.len() == 1 { String::new() } else { format!("/{i}") };
        ids.push(writer.write_entity(def, payload, mode, pointer).await?);
    }
    if !writer.errors.is_empty() {
        return Err(Error::Validation(writer.errors));
    }
    tx.commit().await?;
    Ok(ids)
}

/// Like [`write`], inside a transaction the caller commits, so several writes and other
/// statements (e.g. stock updates during checkout) succeed or fail together.
pub async fn write_in(
    tx: &mut Transaction<'static, Postgres>,
    entity: &str,
    payload: &Value,
    mode: WriteMode,
    ctx: &Context,
) -> Result<Uuid> {
    let def = entities::require(entity)?;
    let mut writer = Writer { tx, ctx, errors: Vec::new() };
    let id = writer.write_entity(def, payload, mode, String::new()).await?;
    if !writer.errors.is_empty() {
        return Err(Error::Validation(writer.errors));
    }
    Ok(id)
}

pub async fn delete(db: &Db, entity: &str, ids: &[String], ctx: &Context) -> Result<()> {
    let def = entities::require(entity)?;
    let mut tx = db.begin().await?;
    for id in ids {
        let not_found = || Error::NotFound { entity: def.name.into(), id: id.clone() };
        let uuid = crate::id::parse(id).ok_or_else(not_found)?;
        let mut binds = Binds::new(ctx);
        let placeholder = binds.text(uuid.to_string(), "uuid");
        let sql = format!("DELETE FROM {} WHERE id = {placeholder}", def.table);
        let done = binds.query(&sql).execute(&mut *tx).await.map_err(Error::from_db)?;
        if done.rows_affected() == 0 {
            return Err(not_found());
        }
        audit(&mut tx, def, uuid, "delete", &Value::Null, ctx).await?;
    }
    tx.commit().await?;
    Ok(())
}

/// Removes one link of a many-to-many association, e.g. a product from a category.
pub async fn delete_mapping(
    db: &Db,
    entity: &str,
    id: &str,
    association: &str,
    reference_id: &str,
    ctx: &Context,
) -> Result<()> {
    let def = entities::require(entity)?;
    let Some(FieldKind::ManyToMany { mapping, local, reference, .. }) = def.field(association).map(|f| f.kind) else {
        return Err(Error::criteria(format!("`{association}` is not a many-to-many association")));
    };
    let (Some(id), Some(ref_id)) = (crate::id::parse(id), crate::id::parse(reference_id)) else {
        return Err(Error::NotFound { entity: def.name.into(), id: id.into() });
    };
    let mut binds = Binds::new(ctx);
    let a = binds.text(id.to_string(), "uuid");
    let b = binds.text(ref_id.to_string(), "uuid");
    let sql = format!("DELETE FROM {mapping} WHERE {local} = {a} AND {reference} = {b}");
    binds.query(&sql).execute(db).await?;
    Ok(())
}

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

struct Writer<'t, 'c> {
    tx: &'t mut Transaction<'static, Postgres>,
    ctx: &'c Context,
    errors: Vec<WriteError>,
}

/// Values collected for one entity row.
#[derive(Default)]
struct Row_ {
    columns: Vec<(&'static Field, Option<String>)>,
    translations: BTreeMap<Uuid, Vec<(&'static Field, Option<String>)>>,
    one_to_many: Vec<(&'static Field, Vec<Value>, String)>,
    many_to_many: Vec<(&'static Field, Vec<Value>, String)>,
}

impl Writer<'_, '_> {
    fn error(&mut self, pointer: &str, code: &'static str, detail: impl Into<String>) {
        let pointer = if pointer.is_empty() { "/".to_string() } else { pointer.to_string() };
        self.errors.push(WriteError::new(pointer, code, detail));
    }

    fn write_entity<'a>(
        &'a mut self,
        def: &'static EntityDefinition,
        payload: &'a Value,
        mode: WriteMode,
        pointer: String,
    ) -> BoxFuture<'a, Result<Uuid>> {
        Box::pin(async move {
            let Some(obj) = payload.as_object() else {
                self.error(&pointer, "FRAMEWORK__INVALID_TYPE", "expected an object");
                return Ok(Uuid::nil());
            };
            let id = match obj.get("id") {
                None | Some(Value::Null) => crate::id::new_id(),
                Some(v) => match v.as_str().and_then(crate::id::parse) {
                    Some(id) => id,
                    None => {
                        self.error(&format!("{pointer}/id"), "FRAMEWORK__INVALID_UUID", "invalid id");
                        return Ok(Uuid::nil());
                    }
                },
            };
            let exists = self.exists(def, id).await?;
            match (mode, exists) {
                (WriteMode::Create, true) => {
                    self.error(
                        &format!("{pointer}/id"),
                        "FRAMEWORK__DUPLICATE_ENTRY",
                        format!("{} {} already exists", def.name, crate::id::format(id)),
                    );
                    return Ok(id);
                }
                (WriteMode::Update, false) => {
                    return Err(Error::NotFound { entity: def.name.into(), id: crate::id::format(id) });
                }
                _ => {}
            }
            let creating = !exists;

            let mut row = Row_::default();
            for (key, value) in obj {
                let ptr = format!("{pointer}/{key}");
                if key == "id" || IGNORED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                if key == "translations" {
                    self.collect_translations(def, value, &ptr, &mut row);
                    continue;
                }
                let Some(field) = def.field(key) else {
                    self.error(&ptr, "FRAMEWORK__WRITE_EXTRA_FIELD", format!("`{key}` does not exist on {}", def.name));
                    continue;
                };
                if field.has(READ_ONLY) || field.kind.computed_sql().is_some() {
                    continue;
                }
                match field.kind {
                    FieldKind::ManyToOne { entity, fk } => {
                        if value.is_object() {
                            let target = entities::require(entity)?;
                            let nested = self.write_entity(target, value, WriteMode::Upsert, ptr).await?;
                            let fk_field = def.field(fk).expect("validated fk");
                            row.columns.retain(|(f, _)| f.name != fk);
                            row.columns.push((fk_field, Some(nested.to_string())));
                        }
                    }
                    FieldKind::OneToMany { .. } | FieldKind::ManyToMany { .. } => match value.as_array() {
                        Some(items) => {
                            let entry = (field, items.clone(), ptr);
                            if matches!(field.kind, FieldKind::OneToMany { .. }) {
                                row.one_to_many.push(entry);
                            } else {
                                row.many_to_many.push(entry);
                            }
                        }
                        None => self.error(&ptr, "FRAMEWORK__INVALID_TYPE", "expected an array"),
                    },
                    _ => {
                        if let Some(converted) = self.convert(field, value, &ptr) {
                            if field.translated() {
                                row.translations.entry(self.ctx.language_id).or_default().push((field, converted));
                            } else if !row.columns.iter().any(|(f, _)| f.name == field.name) {
                                row.columns.push((field, converted));
                            }
                        }
                    }
                }
            }

            self.check_required(def, obj, creating, &mut row, &pointer);
            if !self.errors.is_empty() {
                return Ok(id);
            }

            // Foreign keys pointing at children created in this same payload (e.g. `coverId` ->
            // one of `media`) can only be set once those children exist.
            let child_entities: Vec<&str> = row
                .one_to_many
                .iter()
                .filter_map(|(f, _, _)| match f.kind {
                    FieldKind::OneToMany { entity, .. } => Some(entity),
                    _ => None,
                })
                .collect();
            let (deferred, columns): (Vec<_>, Vec<_>) =
                std::mem::take(&mut row.columns).into_iter().partition(|(f, v)| {
                    v.is_some() && matches!(f.kind, FieldKind::Fk(target) if child_entities.contains(&target))
                });
            row.columns = columns;

            self.save_row(def, id, creating, &row).await?;
            for (lang, values) in &row.translations {
                self.save_translation(def, id, *lang, values).await?;
            }
            for (field, items, ptr) in &row.one_to_many {
                let FieldKind::OneToMany { entity, reference } = field.kind else { unreachable!() };
                let target = entities::require(entity)?;
                for (i, item) in items.iter().enumerate() {
                    let mut item = item.clone();
                    if let Some(map) = item.as_object_mut() {
                        map.insert(reference.to_string(), Value::String(crate::id::format(id)));
                    }
                    self.write_entity(target, &item, WriteMode::Upsert, format!("{ptr}/{i}")).await?;
                }
            }
            for (field, items, ptr) in &row.many_to_many {
                let FieldKind::ManyToMany { entity, mapping, local, reference } = field.kind else { unreachable!() };
                let target = entities::require(entity)?;
                for (i, item) in items.iter().enumerate() {
                    let item_ptr = format!("{ptr}/{i}");
                    let only_id = item.as_object().is_some_and(|m| m.len() == 1 && m.contains_key("id"));
                    let ref_id = if only_id {
                        match item["id"].as_str().and_then(crate::id::parse) {
                            Some(ref_id) => ref_id,
                            None => {
                                self.error(&format!("{item_ptr}/id"), "FRAMEWORK__INVALID_UUID", "invalid id");
                                continue;
                            }
                        }
                    } else {
                        self.write_entity(target, item, WriteMode::Upsert, item_ptr).await?
                    };
                    if !self.errors.is_empty() {
                        continue;
                    }
                    let mut binds = Binds::new(self.ctx);
                    let a = binds.text(id.to_string(), "uuid");
                    let b = binds.text(ref_id.to_string(), "uuid");
                    let sql = format!(
                        "INSERT INTO {mapping} ({local}, {reference}) VALUES ({a}, {b}) ON CONFLICT DO NOTHING"
                    );
                    binds.query(&sql).execute(&mut **self.tx).await.map_err(Error::from_db)?;
                }
            }
            if !deferred.is_empty() {
                let row = Row_ { columns: deferred, ..Row_::default() };
                self.save_row(def, id, false, &row).await?;
            }
            let action = if creating { "insert" } else { "update" };
            audit(self.tx, def, id, action, payload, self.ctx).await?;
            Ok(id)
        })
    }

    async fn exists(&mut self, def: &EntityDefinition, id: Uuid) -> Result<bool> {
        let mut binds = Binds::new(self.ctx);
        let placeholder = binds.text(id.to_string(), "uuid");
        let sql = format!("SELECT EXISTS (SELECT 1 FROM {} WHERE id = {placeholder}) AS e", def.table);
        Ok(binds.query(&sql).fetch_one(&mut **self.tx).await?.try_get("e")?)
    }

    fn collect_translations(&mut self, def: &EntityDefinition, value: &Value, pointer: &str, row: &mut Row_) {
        if def.translation_table.is_none() {
            self.error(pointer, "FRAMEWORK__WRITE_EXTRA_FIELD", format!("{} is not translatable", def.name));
            return;
        }
        let entries: Vec<(String, Map<String, Value>, String)> = match value {
            Value::Object(map) => map
                .iter()
                .filter_map(|(lang, v)| v.as_object().map(|o| (lang.clone(), o.clone(), format!("{pointer}/{lang}"))))
                .collect(),
            Value::Array(items) => items
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    let o = v.as_object()?;
                    let lang = o.get("languageId")?.as_str()?.to_string();
                    Some((lang, o.clone(), format!("{pointer}/{i}")))
                })
                .collect(),
            _ => {
                self.error(pointer, "FRAMEWORK__INVALID_TYPE", "expected an object or array");
                return;
            }
        };
        for (lang, values, ptr) in entries {
            let Some(lang_id) = crate::id::parse(&lang) else {
                self.error(&ptr, "FRAMEWORK__INVALID_UUID", "invalid language id");
                continue;
            };
            for (key, v) in values {
                if key == "languageId" || IGNORED_KEYS.contains(&key.as_str()) {
                    continue;
                }
                match def.field(&key).filter(|f| f.translated()) {
                    Some(field) => {
                        if let Some(converted) = self.convert(field, &v, &format!("{ptr}/{key}")) {
                            let list = row.translations.entry(lang_id).or_default();
                            list.retain(|(f, _)| f.name != field.name);
                            list.push((field, converted));
                        }
                    }
                    None => self.error(
                        &format!("{ptr}/{key}"),
                        "FRAMEWORK__WRITE_EXTRA_FIELD",
                        format!("`{key}` is not translatable"),
                    ),
                }
            }
        }
    }

    /// Converts a JSON value into its text representation for binding; `Some(None)` is SQL NULL.
    fn convert(&mut self, field: &Field, value: &Value, pointer: &str) -> Option<Option<String>> {
        if value.is_null() {
            return Some(None);
        }
        let invalid = |this: &mut Self, what: &str| {
            this.error(pointer, "FRAMEWORK__INVALID_TYPE", format!("expected {what}"));
            None
        };
        let converted = match field.kind {
            FieldKind::Id | FieldKind::Fk(_) => match value.as_str().and_then(crate::id::parse) {
                Some(id) => id.to_string(),
                None => {
                    self.error(pointer, "FRAMEWORK__INVALID_UUID", "invalid id");
                    return None;
                }
            },
            FieldKind::String | FieldKind::LongText => match value.as_str() {
                Some(s) => s.to_string(),
                None => return invalid(self, "a string"),
            },
            FieldKind::Int => match value.as_i64() {
                Some(n) => n.to_string(),
                None => return invalid(self, "an integer"),
            },
            FieldKind::Float => match value {
                Value::Number(n) => n.to_string(),
                _ => return invalid(self, "a number"),
            },
            FieldKind::Bool => match value.as_bool() {
                Some(b) => b.to_string(),
                None => return invalid(self, "a boolean"),
            },
            FieldKind::DateTime => {
                let s = value.as_str().unwrap_or_default();
                let valid = chrono::DateTime::parse_from_rfc3339(s).is_ok()
                    || chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").is_ok()
                    || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok();
                if !valid {
                    return invalid(self, "a date (RFC 3339)");
                }
                s.to_string()
            }
            FieldKind::Json => value.to_string(),
            FieldKind::Price => match crate::price::normalize(value) {
                Ok(v) => v.to_string(),
                Err(e) => {
                    self.error(pointer, "FRAMEWORK__INVALID_PRICE", e);
                    return None;
                }
            },
            FieldKind::Password => match value.as_str() {
                Some(s) if s.chars().count() >= crate::password::MIN_LENGTH => crate::password::hash(s),
                _ => {
                    self.error(
                        pointer,
                        "FRAMEWORK__PASSWORD_TOO_SHORT",
                        format!("password needs at least {} characters", crate::password::MIN_LENGTH),
                    );
                    return None;
                }
            },
            FieldKind::Computed(_)
            | FieldKind::ComputedText(_)
            | FieldKind::ManyToOne { .. }
            | FieldKind::OneToMany { .. }
            | FieldKind::ManyToMany { .. } => {
                return None;
            }
        };
        Some(Some(converted))
    }

    fn check_required(
        &mut self,
        def: &EntityDefinition,
        obj: &Map<String, Value>,
        creating: bool,
        row: &mut Row_,
        pointer: &str,
    ) {
        let is_variant = def.inherit_from.is_some_and(|parent| obj.get(parent).is_some_and(|v| !v.is_null()));
        let required = def
            .fields
            .iter()
            .filter(|f| f.required() && f.kind != FieldKind::Id)
            .filter(|f| !(is_variant && f.has(INHERITED)));

        let ctx_lang = self.ctx.language_id;
        let system = defaults::LANGUAGE_SYSTEM;
        if creating && ctx_lang != system && !row.translations.contains_key(&system) {
            // A new entity always needs its system-language translation; seed it from the context language.
            if let Some(values) = row.translations.get(&ctx_lang).cloned() {
                row.translations.insert(system, values);
            }
        }

        for field in required {
            let missing = |values: Option<&Vec<(&'static Field, Option<String>)>>| -> Option<bool> {
                values
                    .and_then(|v| v.iter().find(|(f, _)| f.name == field.name))
                    .map(|(_, v)| v.as_deref().is_none_or(str::is_empty))
            };
            let (provided_blank, absent) = if field.translated() {
                let state = missing(row.translations.get(&system));
                let ctx_state = missing(row.translations.get(&ctx_lang));
                (state == Some(true) || ctx_state == Some(true), state.is_none())
            } else {
                let state = missing(Some(&row.columns));
                (state == Some(true), state.is_none())
            };
            let field_pointer = format!("{pointer}/{}", field.name);
            let already_reported = self.errors.iter().any(|e| e.pointer == field_pointer);
            if !already_reported && (provided_blank || (creating && absent)) {
                self.error(&field_pointer, "FRAMEWORK__WRITE_CONSTRAINT_VIOLATION", "This value should not be blank.");
            }
        }
    }

    async fn save_row(&mut self, def: &EntityDefinition, id: Uuid, creating: bool, row: &Row_) -> Result<()> {
        let mut binds = Binds::new(self.ctx);
        let id_placeholder = binds.text(id.to_string(), "uuid");
        let sql = if creating {
            let mut columns = vec!["id".to_string()];
            let mut values = vec![id_placeholder];
            for (field, value) in &row.columns {
                columns.push(field.column.to_string());
                values.push(binds.bind(Param::Text(value.clone()), field.kind.sql_type()));
            }
            format!("INSERT INTO {} ({}) VALUES ({})", def.table, columns.join(", "), values.join(", "))
        } else {
            let mut sets = vec!["updated_at = now()".to_string()];
            for (field, value) in &row.columns {
                let placeholder = binds.bind(Param::Text(value.clone()), field.kind.sql_type());
                sets.push(format!("{} = {placeholder}", field.column));
            }
            format!("UPDATE {} SET {} WHERE id = {id_placeholder}", def.table, sets.join(", "))
        };
        binds.query(&sql).execute(&mut **self.tx).await.map_err(Error::from_db)?;
        Ok(())
    }

    async fn save_translation(
        &mut self,
        def: &EntityDefinition,
        id: Uuid,
        language: Uuid,
        values: &[(&'static Field, Option<String>)],
    ) -> Result<()> {
        let table = def.translation_table.expect("translated entity");
        let fk = def.translation_fk();
        let mut binds = Binds::new(self.ctx);
        let mut columns = vec![fk.clone(), "language_id".to_string()];
        let mut placeholders = vec![binds.text(id.to_string(), "uuid"), binds.text(language.to_string(), "uuid")];
        let mut updates = vec!["updated_at = now()".to_string()];
        for (field, value) in values {
            columns.push(field.column.to_string());
            placeholders.push(binds.bind(Param::Text(value.clone()), field.kind.sql_type()));
            updates.push(format!("{0} = EXCLUDED.{0}", field.column));
        }
        let sql = format!(
            "INSERT INTO {table} ({}) VALUES ({}) ON CONFLICT ({fk}, language_id) DO UPDATE SET {}",
            columns.join(", "),
            placeholders.join(", "),
            updates.join(", ")
        );
        binds.query(&sql).execute(&mut **self.tx).await.map_err(Error::from_db)?;
        Ok(())
    }
}

/// Names of all password fields, scrubbed from audit payloads.
fn secret_keys() -> impl Iterator<Item = &'static str> {
    entities::ALL.iter().flat_map(|d| d.fields.iter()).filter(|f| f.kind == FieldKind::Password).map(|f| f.name)
}

fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for key in secret_keys() {
                if map.contains_key(key) {
                    map.insert(key.to_string(), Value::String("***".into()));
                }
            }
            map.values_mut().for_each(scrub);
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}

async fn audit(
    tx: &mut Transaction<'static, Postgres>,
    def: &EntityDefinition,
    id: Uuid,
    action: &str,
    payload: &Value,
    ctx: &Context,
) -> Result<()> {
    let mut payload = payload.clone();
    scrub(&mut payload);
    let mut binds = Binds::new(ctx);
    let values = [
        binds.text(crate::id::new_id().to_string(), "uuid"),
        binds.text(def.name, "text"),
        binds.text(id.to_string(), "uuid"),
        binds.text(action, "text"),
        binds.text(payload.to_string(), "jsonb"),
        binds.bind(Param::Text(ctx.actor_id.map(|a| a.to_string())), "uuid"),
    ];
    let sql = format!(
        "INSERT INTO audit_log (id, entity, entity_id, action, payload, actor_id) VALUES ({})",
        values.join(", ")
    );
    binds.query(&sql).execute(&mut **tx).await?;
    Ok(())
}
