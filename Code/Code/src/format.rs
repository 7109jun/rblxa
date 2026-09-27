use std::{fs, path::Path};

use crate::error::Result;
use crate::parse::parse_str;

pub fn format_file(path: impl AsRef<Path>, in_place: bool) -> Result<String> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)?;
    let doc = parse_str(&text, Some(path.to_path_buf()))?;
    let formatted = toml::to_string_pretty(&doc.source)?;
    if in_place {
        fs::write(path, &formatted)?;
    }
    Ok(formatted)
}
