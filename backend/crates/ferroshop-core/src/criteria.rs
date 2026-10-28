//! Shopware's search criteria, as sent to `/api/search/{entity}` and the Store-API.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Criteria {
    pub page: Option<u64>,
    pub limit: Option<u64>,
    pub ids: Option<Vec<String>>,
    pub filter: Vec<Filter>,
    pub post_filter: Vec<Filter>,
    pub sort: Vec<Sort>,
    pub term: Option<String>,
    pub associations: BTreeMap<String, Criteria>,
    pub aggregations: Vec<Aggregation>,
    pub includes: Option<HashMap<String, Vec<String>>>,
    #[serde(rename = "total-count-mode", deserialize_with = "total_count_mode")]
    pub total_count_mode: TotalCountMode,
}

impl Criteria {
    pub fn with_ids(ids: Vec<String>) -> Self {
        Self { ids: Some(ids), ..Default::default() }
    }

    /// Turns `{"manufacturer.media": {}}` into `{"manufacturer": {"associations": {"media": {}}}}`.
    pub fn nested_associations(&self) -> BTreeMap<String, Criteria> {
        let mut out: BTreeMap<String, Criteria> = BTreeMap::new();
        for (name, sub) in &self.associations {
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

    pub fn offset(&self) -> u64 {
        match (self.page, self.limit) {
            (Some(page), Some(limit)) if page > 0 => (page - 1) * limit,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TotalCountMode {
    #[default]
    None,
    Exact,
    NextPages,
}

fn total_count_mode<'de, D: Deserializer<'de>>(de: D) -> Result<TotalCountMode, D::Error> {
    Ok(match Value::deserialize(de)? {
        Value::Number(n) if n.as_u64() == Some(1) => TotalCountMode::Exact,
        Value::Number(n) if n.as_u64() == Some(2) => TotalCountMode::NextPages,
        Value::String(s) if s == "exact" || s == "1" => TotalCountMode::Exact,
        Value::String(s) if s == "next-pages" || s == "2" => TotalCountMode::NextPages,
        _ => TotalCountMode::None,
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Filter {
    Equals {
        field: String,
        value: Value,
    },
    EqualsAny {
        field: String,
        value: Value,
    },
    Contains {
        field: String,
        value: Value,
    },
    Prefix {
        field: String,
        value: Value,
    },
    Suffix {
        field: String,
        value: Value,
    },
    Range {
        field: String,
        parameters: RangeParameters,
    },
    Not {
        #[serde(default = "and")]
        operator: String,
        queries: Vec<Filter>,
    },
    Multi {
        #[serde(default = "and")]
        operator: String,
        queries: Vec<Filter>,
    },
}

fn and() -> String {
    "and".into()
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RangeParameters {
    pub gte: Option<Value>,
    pub lte: Option<Value>,
    pub gt: Option<Value>,
    pub lt: Option<Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sort {
    pub field: String,
    #[serde(default = "asc")]
    pub order: String,
    #[serde(default)]
    pub natural_sorting: bool,
}

fn asc() -> String {
    "ASC".into()
}

impl Sort {
    pub fn descending(&self) -> bool {
        self.order.eq_ignore_ascii_case("desc")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Aggregation {
    pub name: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub field: String,
    #[serde(default)]
    pub limit: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_admin_criteria() {
        let c: Criteria = serde_json::from_value(serde_json::json!({
            "page": 2, "limit": 25, "term": "shirt",
            "filter": [
                {"type": "equals", "field": "active", "value": true},
                {"type": "multi", "operator": "or", "queries": [
                    {"type": "range", "field": "stock", "parameters": {"gte": 5}},
                    {"type": "equalsAny", "field": "id", "value": ["a", "b"]}
                ]}
            ],
            "sort": [{"field": "name", "order": "DESC"}],
            "associations": {"manufacturer": {}},
            "aggregations": [{"name": "m", "type": "terms", "field": "manufacturerId"}],
            "total-count-mode": 1
        }))
        .unwrap();
        assert_eq!(c.offset(), 25);
        assert_eq!(c.total_count_mode, TotalCountMode::Exact);
        assert!(c.sort[0].descending());
        assert!(matches!(c.filter[1], Filter::Multi { .. }));
    }
}
