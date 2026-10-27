//! Schemas derived from the entity definitions: `entity-schema.json` and OpenAPI components.

use serde_json::{Map, Value, json};

use crate::definition::{EntityDefinition, Field, FieldKind, READ_ONLY};
use crate::entities;

fn field_type(field: &Field) -> &'static str {
    match field.kind {
        FieldKind::Id | FieldKind::Fk(_) => "uuid",
        FieldKind::String | FieldKind::Password => "string",
        FieldKind::LongText => "text",
        FieldKind::Int | FieldKind::Computed(_) => "int",
        FieldKind::Float => "float",
        FieldKind::Bool => "boolean",
        FieldKind::DateTime => "date",
        FieldKind::Json => "json_object",
        FieldKind::Price => "json_object",
        FieldKind::ManyToOne { .. } | FieldKind::OneToMany { .. } | FieldKind::ManyToMany { .. } => "association",
    }
}

/// Shopware-style `entity-schema.json`: entity -> properties with type and flags.
pub fn entity_schema() -> Value {
    let mut out = Map::new();
    for def in entities::ALL {
        let mut properties = Map::new();
        for field in def.fields {
            let mut flags = Map::new();
            if field.required() {
                flags.insert("required".into(), json!(true));
            }
            if field.translated() {
                flags.insert("translatable".into(), json!(true));
            }
            if field.has(crate::definition::INHERITED) {
                flags.insert("inherited".into(), json!(true));
            }
            if field.has(READ_ONLY) || matches!(field.kind, FieldKind::Computed(_)) {
                flags.insert("write_protected".into(), json!(true));
            }
            let mut prop = json!({ "type": field_type(field), "flags": flags });
            match field.kind {
                FieldKind::ManyToOne { entity, fk } => {
                    prop["relation"] = json!("many_to_one");
                    prop["entity"] = json!(entity);
                    prop["localField"] = json!(fk);
                }
                FieldKind::OneToMany { entity, reference } => {
                    prop["relation"] = json!("one_to_many");
                    prop["entity"] = json!(entity);
                    prop["referenceField"] = json!(reference);
                }
                FieldKind::ManyToMany { entity, mapping, .. } => {
                    prop["relation"] = json!("many_to_many");
                    prop["entity"] = json!(entity);
                    prop["mapping"] = json!(mapping);
                }
                _ => {}
            }
            properties.insert(field.name.into(), prop);
        }
        out.insert(def.name.into(), json!({ "entity": def.name, "properties": properties }));
    }
    Value::Object(out)
}

fn openapi_property(field: &Field) -> Value {
    match field.kind {
        FieldKind::Id | FieldKind::Fk(_) => json!({ "type": "string", "pattern": "^[0-9a-f]{32}$" }),
        FieldKind::String | FieldKind::LongText => json!({ "type": "string" }),
        FieldKind::Password => json!({ "type": "string", "writeOnly": true }),
        FieldKind::Int | FieldKind::Computed(_) => json!({ "type": "integer" }),
        FieldKind::Float => json!({ "type": "number" }),
        FieldKind::Bool => json!({ "type": "boolean" }),
        FieldKind::DateTime => json!({ "type": "string", "format": "date-time" }),
        FieldKind::Json => json!({ "type": "object", "additionalProperties": true }),
        FieldKind::Price => json!({ "type": "array", "items": { "$ref": "#/components/schemas/Price" } }),
        FieldKind::ManyToOne { entity, .. } => {
            json!({ "$ref": format!("#/components/schemas/{}", schema_name(entity)) })
        }
        FieldKind::OneToMany { entity, .. } | FieldKind::ManyToMany { entity, .. } => {
            json!({ "type": "array", "items": { "$ref": format!("#/components/schemas/{}", schema_name(entity)) } })
        }
    }
}

/// `product_manufacturer` -> `ProductManufacturer`.
pub fn schema_name(entity: &str) -> String {
    entity
        .split('_')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map(|c| c.to_ascii_uppercase().to_string() + chars.as_str()).unwrap_or_default()
        })
        .collect()
}

fn entity_component(def: &EntityDefinition) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    for field in def.fields {
        let mut prop = openapi_property(field);
        if field.has(READ_ONLY) || matches!(field.kind, FieldKind::Computed(_)) {
            prop["readOnly"] = json!(true);
        }
        if field.required() && field.kind != FieldKind::Id {
            required.push(field.name);
        }
        properties.insert(field.name.into(), prop);
    }
    if def.translation_table.is_some() {
        let translated: Map<String, Value> =
            def.translated_fields().map(|f| (f.name.to_string(), openapi_property(f))).collect();
        properties.insert("translated".into(), json!({ "type": "object", "readOnly": true, "properties": translated }));
    }
    json!({ "type": "object", "properties": properties, "required": required })
}

/// OpenAPI component schemas for every entity, plus shared types.
pub fn openapi_components() -> Map<String, Value> {
    let mut schemas = Map::new();
    for def in entities::ALL {
        schemas.insert(schema_name(def.name), entity_component(def));
    }
    schemas.insert(
        "Price".into(),
        json!({
            "type": "object",
            "required": ["currencyId", "gross", "net", "linked"],
            "properties": {
                "currencyId": { "type": "string" },
                "gross": { "type": "number" },
                "net": { "type": "number" },
                "linked": { "type": "boolean" },
                "listPrice": {
                    "type": "object",
                    "properties": {
                        "gross": { "type": "number" }, "net": { "type": "number" }, "linked": { "type": "boolean" }
                    }
                }
            }
        }),
    );
    schemas
}

#[cfg(test)]
mod tests {
    #[test]
    fn names() {
        assert_eq!(super::schema_name("product_manufacturer"), "ProductManufacturer");
        let schema = super::entity_schema();
        assert_eq!(schema["product"]["properties"]["name"]["flags"]["translatable"], true);
    }
}
