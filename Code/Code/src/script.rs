use std::{fs, path::{Path, PathBuf}};
use toml::Value;

use crate::{error::{Result, RblxaError}, document::RblxaObject};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptKind {
    Script,
    LocalScript,
    ModuleScript,
}

impl ScriptKind {
    pub fn parse(value: Option<&str>) -> Result<Self> {
        match value.unwrap_or("Script") {
            "Script" => Ok(Self::Script),
            "LocalScript" => Ok(Self::LocalScript),
            "ModuleScript" => Ok(Self::ModuleScript),
            other => Err(RblxaError::Invalid(format!(
                "RBLXA103 unsupported script type '{other}'"
            ))),
        }
    }

    pub fn class_name(self) -> &'static str {
        match self {
            Self::Script => "Script",
            Self::LocalScript => "LocalScript",
            Self::ModuleScript => "ModuleScript",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptSource {
    pub kind: ScriptKind,
    pub text: String,
    pub path: Option<PathBuf>,
}

pub fn resolve_script(obj: &RblxaObject, base: &Path) -> Result<ScriptSource> {
    let kind = ScriptKind::parse(obj.properties.get("type").and_then(Value::as_str))?;
    let source = obj.properties.get("source").and_then(Value::as_str);
    let raw_path = obj.properties.get("path").and_then(Value::as_str);

    match (source, raw_path) {
        (Some(_), Some(_)) => Err(RblxaError::Invalid(format!(
            "RBLXA200 script '{}' cannot define both source and path",
            obj.id
        ))),
        (None, None) => Err(RblxaError::Invalid(format!(
            "RBLXA104 script '{}' requires source or path",
            obj.id
        ))),
        (Some(text), None) => {
            validate_script_text(text, &obj.id)?;
            Ok(ScriptSource { kind, text: normalize_source(text), path: None })
        }
        (None, Some(path)) => {
            let resolved = resolve_project_path(base, path, &obj.id)?;
            let text = fs::read_to_string(&resolved).map_err(|e| {
                RblxaError::Compile(format!(
                    "RBLXA201 script file '{}' could not be read: {e}",
                    resolved.display()
                ))
            })?;
            validate_script_text(&text, &obj.id)?;
            Ok(ScriptSource {
                kind,
                text: normalize_source(&text),
                path: Some(resolved),
            })
        }
    }
}


pub fn validate_script_references(doc: &crate::document::RblxaDocument, base: &Path) -> Result<()> {
    for obj in &doc.objects {
        if matches!(obj.class_name.as_str(), "Script" | "LocalScript" | "ModuleScript") {
            if let Some(raw_path) = obj.properties.get("path").and_then(Value::as_str) {
                let resolved = resolve_project_path(base, raw_path, &obj.id)?;
                let metadata = fs::metadata(&resolved).map_err(|e| {
                    RblxaError::Compile(format!(
                        "RBLXA201 script file '{}' could not be accessed: {e}",
                        resolved.display()
                    ))
                })?;
                if !metadata.is_file() {
                    return Err(RblxaError::Compile(format!(
                        "RBLXA201 script path '{}' is not a file",
                        resolved.display()
                    )));
                }
            }
        }
    }
    Ok(())
}

pub fn resolve_project_path(base: &Path, raw: &str, object_id: &str) -> Result<PathBuf> {
    if raw.trim().is_empty() {
        return Err(RblxaError::Invalid(format!(
            "RBLXA103 script '{object_id}' path cannot be empty"
        )));
    }

    let clean = normalize_project_relative_path(raw, object_id)?;

    let root = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    let resolved = root.join(&clean);
    let canonical_resolved = if resolved.exists() {
        resolved.canonicalize().map_err(|e| {
            RblxaError::Invalid(format!(
                "RBLXA203 project path '{object_id}' could not be canonicalized: {e}"
            ))
        })?
    } else {
        resolved.clone()
    };

    let root_cmp = path_cmp_key(&root);
    let target_cmp = path_cmp_key(&canonical_resolved);
    if canonical_resolved.exists() {
        if !target_cmp.starts_with(&(root_cmp.clone() + "\\")) && target_cmp != root_cmp {
            return Err(RblxaError::Invalid(format!(
                "RBLXA203 project path '{object_id}' escapes the project directory"
            )));
        }
    } else {
        let parent = canonical_resolved.parent().unwrap_or(&root);
        let canonical_parent = parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf());
        let parent_cmp = path_cmp_key(&canonical_parent);
        if !parent_cmp.starts_with(&(root_cmp.clone() + "\\")) && parent_cmp != root_cmp {
            return Err(RblxaError::Invalid(format!(
                "RBLXA203 project path '{object_id}' escapes the project directory"
            )));
        }
    }
    Ok(canonical_resolved)
}

pub fn normalize_project_relative_path(raw: &str, object_id: &str) -> Result<PathBuf> {
    let normalized = raw.replace('\\', "/");
    let candidate = Path::new(&normalized);
    if candidate.is_absolute() || normalized.starts_with('/') {
        return Err(RblxaError::Invalid(format!(
            "RBLXA203 project path '{object_id}' must be relative to the RBLXA project"
        )));
    }

    let mut clean = PathBuf::new();
    for component in candidate.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !clean.pop() {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA203 project path '{object_id}' escapes the project directory"
                    )));
                }
            }
            std::path::Component::Normal(part) => clean.push(part),
            _ => {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA203 project path '{object_id}' contains an invalid component"
                )))
            }
        }
    }
    if clean.as_os_str().is_empty() {
        return Err(RblxaError::Invalid(format!(
            "RBLXA203 project path '{object_id}' cannot resolve to the project directory"
        )));
    }
    Ok(clean)
}

fn path_cmp_key(path: &Path) -> String {
    // Windows path comparisons are case-insensitive. Canonicalization resolves
    // junctions/symlinks; lower-casing the remaining path closes casing-only
    // escapes that `Path::starts_with` would otherwise miss.
    path.to_string_lossy().replace('/', "\\").to_lowercase().trim_end_matches('\\').to_owned()
}

fn validate_script_text(text: &str, object_id: &str) -> Result<()> {
    if text.contains('\0') {
        return Err(RblxaError::Invalid(format!(
            "RBLXA204 script '{object_id}' contains a NUL byte"
        )));
    }
    Ok(())
}

fn normalize_source(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RblxaObject;
    use std::collections::BTreeMap;

    fn obj(props: &[(&str, Value)]) -> RblxaObject {
        RblxaObject {
            table: "script".into(),
            id: "s".into(),
            name: "S".into(),
            class_name: "Script".into(),
            parent: Some("ServerScriptService".into()),
            properties: props.iter().map(|(k,v)| ((*k).into(), v.clone())).collect::<BTreeMap<_,_>>(),
        }
    }

    #[test]
    fn normalizes_inline_source() {
        let s = resolve_script(&obj(&[("source", Value::String("a\r\nb".into()))]), Path::new(".")).unwrap();
        assert_eq!(s.text, "a\nb");
    }

    #[test]
    fn rejects_absolute_path() {
        let err = resolve_script(&obj(&[("path", Value::String("C:/x.luau".into()))]), Path::new(".")).unwrap_err();
        assert!(err.to_string().contains("RBLXA203"));
    }

    #[test]
    fn parses_script_kinds() {
        assert_eq!(ScriptKind::parse(Some("ModuleScript")).unwrap(), ScriptKind::ModuleScript);
        assert!(ScriptKind::parse(Some("BadScript")).is_err());
    }

    #[test]
    fn normalizes_windows_path_for_comparison() {
        assert_eq!(path_cmp_key(Path::new("C:/Game/Assets/../Scripts")), "c:\\game\\assets\\..\\scripts");
    }

    #[test]
    fn normalizes_relative_asset_path() {
        let p = normalize_project_relative_path("models\\..\\models\\tree.glb", "tree").unwrap();
        assert_eq!(p.to_string_lossy().replace('\\', "/"), "models/tree.glb");
    }
}
