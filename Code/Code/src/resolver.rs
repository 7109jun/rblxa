use std::collections::{HashMap, HashSet};

use crate::{
    document::{RblxaDocument, RblxaObject},
    error::{Result, RblxaError},
    schema::is_service,
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ParentTarget {
    Service(String),
    Object(String),
}

#[derive(Debug, Clone)]
pub struct ResolvedParents {
    targets: HashMap<String, ParentTarget>,
}

struct Resolver<'a> {
    doc: &'a RblxaDocument,
    by_id: HashMap<&'a str, usize>,
    by_name: HashMap<&'a str, Vec<usize>>,
    memo: HashMap<String, ParentTarget>,
    visiting: HashSet<String>,
}

impl<'a> Resolver<'a> {
    fn new(doc: &'a RblxaDocument) -> Result<Self> {
        let mut by_id = HashMap::with_capacity(doc.objects.len());
        let mut by_name = HashMap::<&str, Vec<usize>>::new();

        for (index, object) in doc.objects.iter().enumerate() {
            if by_id.insert(object.id.as_str(), index).is_some() {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA102 duplicate object id '{}'",
                    object.id
                )));
            }
            by_name.entry(object.name.as_str()).or_default().push(index);
        }

        Ok(Self {
            doc,
            by_id,
            by_name,
            memo: HashMap::with_capacity(doc.objects.len()),
            visiting: HashSet::with_capacity(doc.objects.len()),
        })
    }

    fn resolve_all(mut self) -> Result<ResolvedParents> {
        let mut targets = HashMap::with_capacity(self.doc.objects.len());
        for object in &self.doc.objects {
            let target = self.resolve_object_parent(object)?;
            targets.insert(object.id.clone(), target);
        }
        detect_cycles(&targets)?;
        Ok(ResolvedParents { targets })
    }

    fn resolve_object_parent(&mut self, object: &RblxaObject) -> Result<ParentTarget> {
        if let Some(target) = self.memo.get(&object.id) {
            return Ok(target.clone());
        }

        if !self.visiting.insert(object.id.clone()) {
            return Err(RblxaError::Invalid(format!(
                "RBLXA107 cyclic parent reference involving '{}'",
                object.id
            )));
        }

        let result = match object.parent.as_deref() {
            None => Ok(ParentTarget::Service("Workspace".to_owned())),
            Some(parent) => self.resolve_reference(parent),
        };

        self.visiting.remove(&object.id);

        if let Ok(target) = &result {
            self.memo.insert(object.id.clone(), target.clone());
        }

        result
    }

    fn resolve_reference(&mut self, reference: &str) -> Result<ParentTarget> {
        if reference.trim().is_empty() {
            return Err(RblxaError::Invalid(
                "RBLXA106 parent reference cannot be empty".into(),
            ));
        }

        if is_service(reference) {
            return Ok(ParentTarget::Service(reference.to_owned()));
        }

        // Exact object ID always has precedence, preserving RBLXA's direct-reference rule.
        if let Some(&index) = self.by_id.get(reference) {
            let object = &self.doc.objects[index];
            if self.visiting.contains(&object.id) {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA107 cyclic parent reference involving '{}'",
                    object.id
                )));
            }
            return Ok(ParentTarget::Object(object.id.clone()));
        }

        let parts: Vec<&str> = reference.split('.').filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            return Err(RblxaError::Invalid(
                "RBLXA106 parent reference cannot be empty".into(),
            ));
        }

        let mut cursor = if is_service(parts[0]) {
            ParentTarget::Service(parts[0].to_owned())
        } else if let Some(&index) = self.by_id.get(parts[0]) {
            let object = &self.doc.objects[index];
            if self.visiting.contains(&object.id) {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA107 cyclic parent reference involving '{}'",
                    object.id
                )));
            }
            ParentTarget::Object(object.id.clone())
        } else {
            let candidates = self
                .by_name
                .get(parts[0])
                .into_iter()
                .flatten()
                .copied()
                .collect::<Vec<_>>();
            match candidates.as_slice() {
                [] => {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA106 unknown parent root '{}' in '{}'",
                        parts[0], reference
                    )));
                }
                [index] => ParentTarget::Object(self.doc.objects[*index].id.clone()),
                _ => {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA108 parent path '{}' is ambiguous at '{}'",
                        reference, parts[0]
                    )));
                }
            }
        };

        for segment in &parts[1..] {
            let candidates = self.direct_children(&cursor, segment)?;
            match candidates.as_slice() {
                [] => {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA106 parent path '{}' has no child '{}'",
                        reference, segment
                    )));
                }
                [child] => {
                    cursor = ParentTarget::Object((*child).clone());
                }
                _ => {
                    return Err(RblxaError::Invalid(format!(
                        "RBLXA108 parent path '{}' is ambiguous at '{}'",
                        reference, segment
                    )));
                }
            }
        }

        Ok(cursor)
    }

    fn direct_children(&mut self, parent: &ParentTarget, segment: &str) -> Result<Vec<String>> {
        let mut candidate_indexes = Vec::new();

        if let Some(&index) = self.by_id.get(segment) {
            candidate_indexes.push(index);
        }
        if let Some(indexes) = self.by_name.get(segment) {
            candidate_indexes.extend(indexes.iter().copied());
        }

        candidate_indexes.sort_unstable();
        candidate_indexes.dedup();

        let mut matches = Vec::with_capacity(candidate_indexes.len());
        for index in candidate_indexes {
            let object = &self.doc.objects[index];
            let resolved_parent = self.resolve_object_parent(object)?;
            if &resolved_parent == parent {
                matches.push(object.id.clone());
            }
        }

        Ok(matches)
    }
}

impl ResolvedParents {
    pub fn resolve(doc: &RblxaDocument) -> Result<Self> {
        Resolver::new(doc)?.resolve_all()
    }

    pub fn get(&self, object_id: &str) -> Option<&ParentTarget> {
        self.targets.get(object_id)
    }
}

fn detect_cycles(targets: &HashMap<String, ParentTarget>) -> Result<()> {
    for start in targets.keys() {
        let mut current = start.as_str();
        let mut seen = HashSet::new();

        loop {
            if !seen.insert(current) {
                return Err(RblxaError::Invalid(format!(
                    "RBLXA107 cyclic parent reference involving '{}'",
                    start
                )));
            }

            let Some(ParentTarget::Object(parent)) = targets.get(current) else {
                break;
            };
            current = parent;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse_str;

    fn parse(input: &str) -> RblxaDocument {
        parse_str(input, None).expect("valid RBLXA")
    }

    #[test]
    fn resolves_id_parent() {
        let doc = parse(
            r#"
[place.meta.data]
name = "Test"

[model]
id = "map"
parent = "Workspace"

[part]
id = "floor"
parent = "map"
"#,
        );

        let resolved = ResolvedParents::resolve(&doc).unwrap();
        assert_eq!(resolved.get("floor"), Some(&ParentTarget::Object("map".into())));
    }

    #[test]
    fn resolves_dotted_path_by_id() {
        let doc = parse(
            r#"
[place.meta.data]
name = "Test"

[[model]]
id = "map"
parent = "Workspace"

[[model]]
id = "house"
parent = "map"

[part]
id = "door"
parent = "Workspace.map.house"
"#,
        );

        let resolved = ResolvedParents::resolve(&doc).unwrap();
        assert_eq!(resolved.get("door"), Some(&ParentTarget::Object("house".into())));
    }

    #[test]
    fn resolves_dotted_path_by_name() {
        let doc = parse(
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
        );

        let resolved = ResolvedParents::resolve(&doc).unwrap();
        assert_eq!(resolved.get("floor"), Some(&ParentTarget::Object("map".into())));
    }

    #[test]
    fn reports_ambiguous_name() {
        let doc = parse(
            r#"
[place.meta.data]
name = "Test"

[[model]]
id = "a"
name = "Map"
parent = "Workspace"

[[model]]
id = "b"
name = "Map"
parent = "Workspace"

[part]
id = "floor"
parent = "Workspace.Map"
"#,
        );

        let err = ResolvedParents::resolve(&doc).expect_err("ambiguous path must fail");
        assert!(err.to_string().contains("RBLXA108"));
    }

    #[test]
    fn rejects_parent_cycle() {
        let doc = parse(
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
        );

        let err = ResolvedParents::resolve(&doc).expect_err("cycle must fail");
        assert!(err.to_string().contains("RBLXA107"));
    }
}
