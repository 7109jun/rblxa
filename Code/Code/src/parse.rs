use std::{collections::HashSet, fs, path::Path};
use toml::Value;

use crate::{
    document::{get_str, table_items, RblxaDocument, RblxaObject},
    error::{Result, RblxaError},
    resolver::ResolvedParents,
    schema::{OBJECT_TABLES, RESERVED},
    script::ScriptKind,
};

pub fn parse_file(path: impl AsRef<Path>) -> Result<RblxaDocument> {
    let path = path.as_ref();
    ensure_rblxa_input(path)?;
    let text = fs::read_to_string(path).map_err(|e| {
        RblxaError::Invalid(format!("RBLXA200 could not read '{}': {e}", path.display()))
    })?;
    let doc = parse_str(&text, Some(path.to_path_buf()))?;
    Ok(doc)
}

pub fn ensure_rblxa_input(path: &Path) -> Result<()> {
    match path.extension().and_then(|x| x.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("rblxa") => Ok(()),
        Some(ext) => Err(RblxaError::Invalid(format!(
            "RBLXA502 input extension '.{ext}' is not supported; input must be .rblxa"
        ))),
        None => Err(RblxaError::Invalid(
            "RBLXA502 input path must have the .rblxa extension".into(),
        )),
    }
}

pub fn parse_str(text: &str, source_path: Option<std::path::PathBuf>) -> Result<RblxaDocument> {
    let root: Value = text.parse()?;
    let mut doc = RblxaDocument::empty(Value::Table(toml::map::Map::new()), source_path);
    let mut ids = HashSet::with_capacity(32);

    let meta = root_ref(&root, &["place", "meta", "data"])
        .and_then(Value::as_table)
        .ok_or_else(|| {
            RblxaError::Invalid(
                "RBLXA104 [place.meta.data] is required and must contain a TOML table".into(),
            )
        })?;

    let name = get_str(meta, "name").ok_or_else(|| {
        RblxaError::Invalid("RBLXA104 [place.meta.data].name is required".into())
    })?;
    if name.trim().is_empty() {
        return Err(RblxaError::Invalid(
            "RBLXA104 [place.meta.data].name cannot be empty".into(),
        ));
    }

    for (path, logical, default_class) in OBJECT_TABLES {
        for value in table_items(&root, path) {
            let Some(table) = value.as_table() else {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA100 table {logical} must contain a TOML table"
                )));
            };
            let id = get_str(table, "id").ok_or_else(|| {
                RblxaError::Invalid(format!("RBLXA104 [{logical}] requires id"))
            })?;
            if id.trim().is_empty() {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA103 [{logical}] id cannot be empty"
                )));
            }
            if is_service(&id) {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA109 object id '{id}' conflicts with a reserved Roblox service name"
                )));
            }
            if !ids.insert(id.clone()) {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA102 duplicate object id '{id}'"
                )));
            }

            let name = get_str(table, "name").unwrap_or_else(|| id.clone());
            let class_name = if *logical == "object" {
                get_str(table, "class").ok_or_else(|| {
                    RblxaError::Invalid(format!("RBLXA104 [object] '{id}' requires class"))
                })?
            } else if *logical == "script" {
                ScriptKind::parse(get_str(table, "type").as_deref())?.class_name().to_owned()
            } else if *logical == "3D.Object.File" {
                "Model".to_owned()
            } else {
                (*default_class).to_owned()
            };

            let parent = get_str(table, "parent");
            let mut properties = table.clone();
            for key in RESERVED {
                properties.remove(*key);
            }
            normalize_properties(logical, &mut properties)?;

            if *logical == "script" {
                let has_source = properties.get("source").is_some();
                let has_path = properties.get("path").is_some();
                if has_source && has_path {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA200 script '{id}' cannot define both source and path"
                    )));
                }
                if !has_source && !has_path {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA104 script '{id}' requires source or path"
                    )));
                }
            }

            doc.objects.push(RblxaObject {
                table: (*logical).to_owned(),
                id,
                name,
                class_name,
                parent,
                properties,
            });
        }
    }

    ResolvedParents::resolve(&doc)?;
    doc.source = root;
    Ok(doc)
}

fn root_ref<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cur = value;
    for p in path {
        cur = cur.get(*p)?;
    }
    Some(cur)
}

fn normalize_properties(table: &str, props: &mut toml::map::Map<String, Value>) -> Result<()> {
    for key in ["position", "size", "rotation", "scale"] {
        if let Some(v) = props.get(key) {
            let ok = v
                .as_array()
                .map(|a| {
                    a.len() == 3
                        && a.iter()
                            .all(|n| n.as_float().is_some() || n.as_integer().is_some())
                })
                .unwrap_or(false);
            if !ok {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA401 [{table}] {key} must be a 3-number array"
                )));
            }
        }
    }
    if let Some(v) = props.get("color") {
        let ok = v.as_str().map(|s| parse_hex_color(s).is_ok()).unwrap_or(false)
            || v.as_array()
                .map(|a| {
                    a.len() == 3
                        && a.iter().all(|n| {
                            n.as_integer()
                                .map(|i| (0..=255).contains(&i))
                                .unwrap_or(false)
                        })
                })
                .unwrap_or(false);
        if !ok {
            return Err(RblxaError::Invalid(format!(
                "RBLXA401 [{table}] color must be '#RRGGBB' or [r,g,b]"
            )));
        }
    }
    if let Some(v) = props.get("transparency") {
        let x = v
            .as_float()
            .or_else(|| v.as_integer().map(|i| i as f64))
            .ok_or_else(|| {
                RblxaError::Invalid("RBLXA401 transparency must be a number".into())
            })?;
        if !x.is_finite() || !(0.0..=1.0).contains(&x) {
            return Err(RblxaError::Invalid(
                "RBLXA402 transparency out of range".into(),
            ));
        }
    }
    Ok(())
}

pub fn parse_hex_color(s: &str) -> Result<[u8; 3]> {
    let raw = s
        .strip_prefix('#')
        .ok_or_else(|| RblxaError::Invalid("RBLXA103 color must use #RRGGBB".into()))?;
    if raw.len() != 6 {
        return Err(RblxaError::Invalid(
            "RBLXA103 color must use #RRGGBB".into(),
        ));
    }
    let r = u8::from_str_radix(&raw[0..2], 16)
        .map_err(|_| RblxaError::Invalid("RBLXA103 invalid color".into()))?;
    let g = u8::from_str_radix(&raw[2..4], 16)
        .map_err(|_| RblxaError::Invalid("RBLXA103 invalid color".into()))?;
    let b = u8::from_str_radix(&raw[4..6], 16)
        .map_err(|_| RblxaError::Invalid("RBLXA103 invalid color".into()))?;
    Ok([r, g, b])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_document() {
        let doc = parse_str(
            r#"
[place.meta.data]
name = "Test"

[part]
id = "p"
name = "Part"
parent = "Workspace"
"#,
            None,
        )
        .expect("minimal RBLXA should parse");

        assert_eq!(doc.objects.len(), 1);
        assert_eq!(doc.objects[0].class_name, "Part");
    }

    #[test]
    fn accepts_dotted_3d_table() {
        let doc = parse_str(
            r#"
[place.meta.data]
name = "Test"

[3D.Object.File]
id = "tree"
name = "Tree"
parent = "Workspace"
path = "tree.glb"
format = "glb"
"#,
            None,
        )
        .expect("3D.Object.File should parse");

        assert_eq!(doc.objects[0].table, "3D.Object.File");
    }

    #[test]
    fn rejects_duplicate_ids() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[[part]]
id = "same"
[[part]]
id = "same"
"#,
            None,
        )
        .expect_err("duplicate IDs must fail");

        assert!(err.to_string().contains("RBLXA102"));
    }

    #[test]
    fn accepts_dotted_parent_path() {
        let doc = parse_str(
            r#"
[place.meta.data]
name = "Test"

[model]
id = "map"
name = "Map"
parent = "Workspace"

[part]
id = "floor"
parent = "Workspace.Map"
"#,
            None,
        )
        .expect("dotted parent should parse");

        assert_eq!(doc.objects.len(), 2);
    }

    #[test]
    fn rejects_parent_cycle() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[part]
id = "a"
parent = "b"

[model]
id = "b"
parent = "a"
"#,
            None,
        )
        .expect_err("parent cycle must fail");

        assert!(err.to_string().contains("RBLXA107"));
    }

    #[test]
    fn rejects_non_rblxa_input_path() {
        let err = ensure_rblxa_input(Path::new("game.rbxl")).expect_err("rbxl is not RBLXA source");
        assert!(err.to_string().contains("RBLXA502"));
    }

    #[test]
    fn accepts_case_insensitive_rblxa_extension() {
        ensure_rblxa_input(Path::new("GAME.RBLXA")).expect("extension is case-insensitive");
    }

    #[test]
    fn rejects_service_name_as_object_id() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[part]
id = "Workspace"
"#,
            None,
        )
        .expect_err("service names cannot be object ids");
        assert!(err.to_string().contains("RBLXA109"));
    }

    #[test]
    fn rejects_non_numeric_transparency() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[part]
id = "p"
transparency = "half"
"#,
            None,
        )
        .expect_err("invalid transparency must fail");

        assert!(err.to_string().contains("RBLXA401"));
    }

    #[test]
    fn rejects_invalid_run_context() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[script]
id = "s"
run_context = "Nope"
source = "print(1)"
"#,
            None,
        )
        .expect_err("invalid run context must fail");

        assert!(err.to_string().contains("RBLXA403"));
    }

    #[test]
    fn rejects_run_context_on_local_script() {
        let err = parse_str(
            r#"
[place.meta.data]
name = "Test"

[script]
id = "s"
type = "LocalScript"
run_context = "Client"
source = "print(1)"
"#,
            None,
        )
        .expect_err("LocalScript cannot define run_context");

        assert!(err.to_string().contains("RBLXA403"));
    }

    #[test]
    fn validates_hex_color() {
        assert_eq!(parse_hex_color("#12aBc3").unwrap(), [0x12, 0xAB, 0xC3]);
        assert!(parse_hex_color("#12345").is_err());
    }
}
