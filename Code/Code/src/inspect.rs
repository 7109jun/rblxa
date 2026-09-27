use std::{fs::File, io::BufReader, path::Path};

use crate::{document::RblxaDocument, error::{Result, RblxaError}, parse::parse_str};

pub fn inspect(doc: &RblxaDocument) -> Result<String> {
    let name = doc
        .source
        .get("place")
        .and_then(|v| v.get("meta"))
        .and_then(|v| v.get("data"))
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("Unnamed Place");

    let scripts = doc.objects.iter().filter(|o| matches!(o.class_name.as_str(), "Script" | "LocalScript" | "ModuleScript")).count();
    let assets = doc.objects.iter().filter(|o| o.table == "3D.Object.File").count();

    let mut out = String::with_capacity(128 + doc.objects.len() * 64);
    out.push_str(&format!("RBLXA v1.0\nPlace: {name}\nObjects: {}\nScripts: {scripts}\n3D assets: {assets}\n", doc.objects.len()));
    for object in &doc.objects {
        out.push_str(&format!(
            "- {} [{}] id={} parent={}\n",
            object.name,
            object.class_name,
            object.id,
            object.parent.as_deref().unwrap_or("<root>")
        ));
    }
    Ok(out)
}

pub fn inspect_rbxl(path: impl AsRef<Path>) -> Result<String> {
    let path = path.as_ref();
    let file = File::open(path)?;
    let dom = rbx_binary::from_reader(BufReader::new(file))
        .map_err(|e| RblxaError::Compile(format!("RBLXA503 RBXL read failed: {e}")))?;

    let root = dom.root();
    let mut out = String::from("RBXL\n");
    out.push_str(&format!("Root: {}\n", root.name));
    out.push_str(&format!("Root objects: {}\n", root.children().len()));
    for &referent in root.children() {
        if let Some(instance) = dom.get_by_ref(referent) {
            out.push_str(&format!("- {} [{}]\n", instance.name, instance.class));
        }
    }
    Ok(out)
}
