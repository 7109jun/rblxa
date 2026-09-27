use std::{collections::BTreeMap, path::PathBuf};
use toml::Value;

#[derive(Debug, Clone)]
pub struct RblxaDocument {
    pub source: Value,
    pub source_path: Option<PathBuf>,
    pub objects: Vec<RblxaObject>,
}

#[derive(Debug, Clone)]
pub struct RblxaObject {
    pub table: String,
    pub id: String,
    pub name: String,
    pub class_name: String,
    pub parent: Option<String>,
    pub properties: BTreeMap<String, Value>,
}

impl RblxaDocument {
    pub fn empty(source: Value, source_path: Option<PathBuf>) -> Self {
        Self { source, source_path, objects: Vec::new() }
    }
}

pub fn table_items<'a>(root: &'a Value, path: &[&str]) -> Vec<&'a Value> {
    let mut cur = root;
    for key in path {
        match cur.get(*key) {
            Some(v) => cur = v,
            None => return Vec::new(),
        }
    }
    match cur {
        Value::Table(_) => vec![cur],
        Value::Array(a) => a.iter().collect(),
        _ => Vec::new(),
    }
}

pub fn get_str(table: &toml::map::Map<String, Value>, key: &str) -> Option<String> {
    table.get(key).and_then(Value::as_str).map(str::to_owned)
}
