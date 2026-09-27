use std::{collections::HashMap, fs::{self, OpenOptions}, io::{BufWriter, Write}, path::{Path, PathBuf}};

use rbx_binary::to_writer;
use rbx_dom::{InstanceBuilder, WeakDom};
use rbx_types::{Color3uint8, Content, EnumItem, Ref, Vector3, Variant};
use toml::Value;

use crate::{
    document::RblxaObject,
    error::{Result, RblxaError},
    parse::{ensure_rblxa_input, parse_file, parse_hex_color},
    resolver::{ParentTarget, ResolvedParents},
    schema::SERVICES,
    script::{normalize_project_relative_path, resolve_script},
    RblxaDocument,
};

pub fn build_file(input: impl AsRef<Path>, output: impl AsRef<Path>) -> Result<()> {
    let input = input.as_ref();
    let output = output.as_ref();
    ensure_rblxa_input(input)?;
    ensure_rbxl_output(output)?;
    let doc = parse_file(input)?;
    let dom = build_dom(&doc, input.parent().unwrap_or(Path::new(".")))?;

    write_rbxl_atomic(output, &dom)
}

fn write_rbxl_atomic(output: &Path, dom: &WeakDom) -> Result<()> {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;

    let stem = output
        .file_name()
        .and_then(|x| x.to_str())
        .unwrap_or("output.rbxl");
    let pid = std::process::id();
    let mut temp_path = PathBuf::from(parent);
    let mut created = None;

    for attempt in 0..100u32 {
        temp_path.set_file_name(format!(".{stem}.rblxa-tmp-{pid}-{attempt}"));
        match OpenOptions::new().write(true).create_new(true).open(&temp_path) {
            Ok(file) => {
                created = Some(file);
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }

    let file = created.ok_or_else(|| RblxaError::Compile(
        "RBLXA504 could not allocate a temporary RBXL output file".into()
    ))?;

    let result = (|| -> Result<()> {
        let mut writer = BufWriter::new(file);
        to_writer(&mut writer, dom, &[dom.root_ref()])
            .map_err(|e| RblxaError::Compile(format!("RBLXA501 RBXL serialization failed: {e}")))?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|e| RblxaError::Io(e.into_error()))?;
        file.sync_all()?;

        replace_output_file(&temp_path, output)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    result
}


fn replace_output_file(temp: &Path, output: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
        const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;

        extern "system" {
            fn MoveFileExW(
                existing: *const u16,
                new: *const u16,
                flags: u32,
            ) -> i32;
        }

        let old: Vec<u16> = temp.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let new: Vec<u16> = output.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let ok = unsafe { MoveFileExW(old.as_ptr(), new.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) };
        if ok == 0 {
            return Err(RblxaError::Compile(format!(
                "RBLXA505 could not finalize RBXL output '{}': {}",
                output.display(),
                std::io::Error::last_os_error()
            )));
        }
        return Ok(());
    }

    #[cfg(not(windows))]
    {
        fs::rename(temp, output).map_err(|e| RblxaError::Compile(format!(
            "RBLXA505 could not finalize RBXL output '{}': {e}",
            output.display()
        )))
    }
}

fn ensure_rbxl_output(output: &Path) -> Result<()> {
    match output.extension().and_then(|x| x.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("rbxl") => Ok(()),
        Some(ext) => Err(RblxaError::Invalid(format!(
            "RBLXA502 output extension '.{ext}' is not supported; output must be .rbxl"
        ))),
        None => Err(RblxaError::Invalid(
            "RBLXA502 output path must have the .rbxl extension".into(),
        )),
    }
}

pub fn build_dom(doc: &RblxaDocument, base: &Path) -> Result<WeakDom> {
    let mut root = InstanceBuilder::new("DataModel").with_name(
        doc.source
            .get("place")
            .and_then(|v| v.get("meta"))
            .and_then(|v| v.get("data"))
            .and_then(|v| v.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("Game"),
    );
    let mut dom = WeakDom::new(root);
    let resolved = ResolvedParents::resolve(doc)?;
    let mut refs: HashMap<String, Ref> = HashMap::with_capacity(SERVICES.len() + doc.objects.len());

    for &service in SERVICES {
        let mut builder = InstanceBuilder::new(service);
        if service == "Workspace" {
            apply_workspace_settings(doc, &mut builder)?;
        }
        let r = dom.insert(dom.root_ref(), builder);
        refs.insert(service.to_owned(), r);
    }

    let mut children: HashMap<String, Vec<&RblxaObject>> = HashMap::with_capacity(doc.objects.len());
    for obj in &doc.objects {
        let target = resolved.get(&obj.id).ok_or_else(|| {
            RblxaError::Compile(format!("RBLXA106 unresolved parent for '{}'", obj.id))
        })?;
        let key = match target {
            ParentTarget::Service(service) => format!("@service:{service}"),
            ParentTarget::Object(parent_id) => format!("@object:{parent_id}"),
        };
        children.entry(key).or_default().push(obj);
    }

    fn insert_children(
        dom: &mut WeakDom,
        refs: &mut HashMap<String, Ref>,
        children: &HashMap<String, Vec<&RblxaObject>>,
        root_key: String,
        root_ref: Ref,
        base: &Path,
    ) -> Result<()> {
        let mut stack = vec![(root_key, root_ref)];
        while let Some((parent_key, parent_ref)) = stack.pop() {
            let Some(objects) = children.get(&parent_key) else {
                continue;
            };

            // Reverse push preserves source order while using an iterative stack,
            // avoiding recursion depth failures on deeply nested DataModels.
            for obj in objects.iter().rev() {
                let builders = object_builders(obj, base)?;
                let mut first_ref = None;
                for builder in builders {
                    let referent = dom.insert(parent_ref, builder);
                    if first_ref.is_none() {
                        first_ref = Some(referent);
                    }
                }
                let object_ref = first_ref.ok_or_else(|| {
                    RblxaError::Compile(format!(
                        "RBLXA501 object '{}' produced no Instance",
                        obj.id
                    ))
                })?;
                refs.insert(obj.id.clone(), object_ref);
                stack.push((format!("@object:{}", obj.id), object_ref));
            }
        }
        Ok(())
    }

    for &service in SERVICES {
        let service_ref = refs.get(service).copied().ok_or_else(|| {
            RblxaError::Compile(format!("RBLXA501 missing built-in service '{service}'"))
        })?;
        insert_children(
            &mut dom,
            &mut refs,
            &children,
            format!("@service:{service}"),
            service_ref,
            base,
        )?;
    }

    Ok(dom)
}

fn apply_workspace_settings(doc: &RblxaDocument, builder: &mut InstanceBuilder) -> Result<()> {
    let Some(settings) = doc
        .source
        .get("place")
        .and_then(|v| v.get("settings"))
        .and_then(Value::as_table)
    else {
        return Ok(());
    };

    if let Some(v) = settings.get("gravity") {
        let n = number(v, "place.settings.gravity")?;
        builder.add_property("Gravity", n as f32);
    }
    if let Some(v) = settings.get("streaming_enabled") {
        let b = v.as_bool().ok_or_else(|| {
            RblxaError::Invalid("RBLXA401 place.settings.streaming_enabled must be boolean".into())
        })?;
        builder.add_property("StreamingEnabled", b);
    }
    Ok(())
}

fn object_builders(obj: &RblxaObject, base: &Path) -> Result<Vec<InstanceBuilder>> {
    if obj.table == "3D.Object.File" {
        return build_3d_reference(obj, base);
    }

    let mut b = InstanceBuilder::new(obj.class_name.as_str()).with_name(obj.name.clone());

    for (key, value) in &obj.properties {
        if matches!(
            key.as_str(),
            "source" | "path" | "format" | "run_context" | "position" | "size" | "rotation" | "scale" | "color"
        ) {
            continue;
        }

        let property_name = roblox_property_name(key);
        if let Some(variant) = property_variant(key, value)? {
            b = b.with_property(property_name, variant);
        }
    }

    match obj.class_name.as_str() {
        "Script" | "LocalScript" | "ModuleScript" => {
            let script = resolve_script(obj, base)?;
            b = b.with_property("Source", script.text);
            if let Some(run_context) = obj.properties.get("run_context").and_then(Value::as_str) {
                let value = match run_context {
                    "Legacy" => 0,
                    "Server" => 1,
                    "Client" => 2,
                    "Plugin" => 3,
                    _ => {
                        return Err(RblxaError::Invalid(format!(
                            "RBLXA403 script '{}' has invalid run_context '{}'",
                            obj.id, run_context
                        )));
                    }
                };
                b = b.with_property(
                    "RunContext",
                    EnumItem { ty: "RunContext".into(), value }.into(),
                );
            }
        }
        _ => {}
    }

    if matches!(obj.class_name.as_str(), "Part" | "MeshPart" | "SpawnLocation" | "Attachment") {
        if let Some(v) = obj.properties.get("position") {
            b = b.with_property("Position", vector3(v)?);
        }
        if matches!(obj.class_name.as_str(), "Part" | "MeshPart" | "SpawnLocation") {
            if let Some(v) = obj.properties.get("size") {
                b = b.with_property("Size", vector3(v)?);
            }
        }
        if let Some(v) = obj.properties.get("rotation") {
            b = b.with_property("Orientation", vector3(v)?);
        }
        if matches!(obj.class_name.as_str(), "Part" | "MeshPart" | "SpawnLocation") {
            if let Some(v) = obj.properties.get("color") {
                b = b.with_property("Color", color(v)?);
            }
        }
    }

    Ok(vec![b])
}

fn build_3d_reference(obj: &RblxaObject, base: &Path) -> Result<Vec<InstanceBuilder>> {
    let path = obj
        .properties
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| RblxaError::Invalid(format!("RBLXA104 [3D.Object.File] '{}' requires path", obj.id)))?;
    let format = obj
        .properties
        .get("format")
        .and_then(Value::as_str)
        .ok_or_else(|| RblxaError::Invalid(format!("RBLXA104 [3D.Object.File] '{}' requires format", obj.id)))?;

    let relative = normalize_project_relative_path(path, &obj.id)?;
    let resolved = crate::script::resolve_project_path(base, path, &obj.id)?;
    if !resolved.is_file() {
        return Err(RblxaError::Compile(format!(
            "RBLXA300 3D file '{}' does not exist",
            resolved.display()
        )));
    }

    let actual_ext = resolved.extension().and_then(|x| x.to_str()).unwrap_or("");
    let normalized_format = format.trim_start_matches('.').to_ascii_lowercase();
    let supported = matches!(normalized_format.as_str(), "obj" | "glb" | "gltf" | "fbx");
    if !supported {
        return Err(RblxaError::Invalid(format!(
            "RBLXA301 unsupported 3D format '{format}'"
        )));
    }
    if !actual_ext.eq_ignore_ascii_case(&normalized_format) {
        return Err(RblxaError::Invalid(format!(
            "RBLXA302 [3D.Object.File] '{}' format '{}' does not match file extension '.{}'",
            obj.id, format, actual_ext
        )));
    }

    // RBXL can carry the source description, but raw OBJ/GLB/GLTF/FBX geometry
    // is not itself a native RBXL property. Preserve the authoring source as a
    // Model plus explicit metadata so another importer can consume it later.
    // Store the project-relative path instead of the machine-specific absolute path.
    let source_value = InstanceBuilder::new("StringValue")
        .with_name("RBLXA_SourceFile")
        .with_property("Value", relative.to_string_lossy().replace('\\', "/"));
    let format_value = InstanceBuilder::new("StringValue")
        .with_name("RBLXA_SourceFormat")
        .with_property("Value", format.to_owned());

    let model = InstanceBuilder::new("Model")
        .with_name(obj.name.clone())
        .with_child(source_value)
        .with_child(format_value);

    Ok(vec![model])
}

fn property_variant(key: &str, value: &Value) -> Result<Option<Variant>> {
    match key {
        "material" => {
            let name = value.as_str().ok_or_else(|| {
                RblxaError::Invalid("RBLXA401 material must be a string enum name".into())
            })?;
            let value = material_value(name).ok_or_else(|| {
                RblxaError::Invalid(format!("RBLXA400 unsupported Enum.Material '{name}'"))
            })?;
            Ok(Some(EnumItem {
                ty: "Material".into(),
                value,
            }
            .into()))
        }
        "sound_id" | "texture_id" | "texture" | "mesh_id" => {
            let s = value.as_str().ok_or_else(|| {
                RblxaError::Invalid(format!("RBLXA401 {key} must be a string"))
            })?;
            if s.is_empty() {
                return Ok(Some(Content::none().into()));
            }
            Ok(Some(Content::from_uri(s.to_owned()).into()))
        }
        _ => simple_variant(value),
    }
}

fn roblox_property_name(key: &str) -> String {
    match key {
        // RBLXA's canonical snake_case spellings with Roblox's exact property names.
        "can_collide" => "CanCollide".into(),
        "can_touch" => "CanTouch".into(),
        "can_query" => "CanQuery".into(),
        "sound_id" => "SoundId".into(),
        "texture_id" => "TextureID".into(),
        "mesh_id" => "MeshId".into(),
        "run_context" => "RunContext".into(),
        "streaming_enabled" => "StreamingEnabled".into(),
        "action_text" => "ActionText".into(),
        "object_text" => "ObjectText".into(),
        "studs_per_tile_u" => "StudsPerTileU".into(),
        "studs_per_tile_v" => "StudsPerTileV".into(),
        "brick_color" => "BrickColor".into(),
        "playback_speed" => "PlaybackSpeed".into(),
        "roll_off_max_distance" => "RollOffMaxDistance".into(),
        "roll_off_min_distance" => "RollOffMinDistance".into(),
        "max_distance" => "MaxDistance".into(),
        "min_distance" => "MinDistance".into(),
        "render_fidelity" => "RenderFidelity".into(),
        "cast_shadow" => "CastShadow".into(),
        "pivot_offset" => "PivotOffset".into(),
        // Keys already written in Roblox form are kept untouched.
        _ if key.chars().any(|c| c.is_uppercase()) => key.to_owned(),
        _ => key
            .split('_')
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<String>(),
    }
}

fn simple_variant(value: &Value) -> Result<Option<Variant>> {
    Ok(match value {
        Value::Boolean(x) => Some((*x).into()),
        Value::Integer(x) => Some((*x).into()),
        Value::Float(x) => {
            if !x.is_finite() || (*x as f32).is_infinite() || (*x as f32).is_nan() {
                return Err(RblxaError::Invalid(
                    "RBLXA402 floating-point property is not representable as finite f32".into(),
                ));
            }
            Some((*x as f32).into())
        }
        Value::String(s) => Some(s.clone().into()),
        Value::Datetime(dt) => Some(dt.to_string().into()),
        Value::Array(_) | Value::Table(_) => {
            return Err(RblxaError::Invalid(
                "RBLXA401 array/table property requires a dedicated RBLXA value type".into(),
            ));
        }
    })
}

fn number(value: &Value, field: &str) -> Result<f64> {
    let number = value
        .as_float()
        .or_else(|| value.as_integer().map(|x| x as f64))
        .ok_or_else(|| RblxaError::Invalid(format!("RBLXA401 {field} must be a number")))?;
    if !number.is_finite() {
        return Err(RblxaError::Invalid(format!(
            "RBLXA402 {field} must be finite"
        )));
    }
    if (number as f32).is_infinite() || (number as f32).is_nan() {
        return Err(RblxaError::Invalid(format!(
            "RBLXA402 {field} is outside the supported f32 range"
        )));
    }
    Ok(number)
}

fn nums3(value: &Value) -> Result<[f32; 3]> {
    let values = value
        .as_array()
        .ok_or_else(|| RblxaError::Invalid("expected a 3-number array".into()))?;
    if values.len() != 3 {
        return Err(RblxaError::Invalid("expected exactly 3 numbers".into()));
    }

    let mut out = [0.0f32; 3];
    for (index, value) in values.iter().enumerate() {
        out[index] = number(value, "Vector3")? as f32;
    }
    Ok(out)
}

fn vector3(value: &Value) -> Result<Vector3> {
    let n = nums3(value)?;
    Ok(Vector3::new(n[0], n[1], n[2]))
}

fn color(value: &Value) -> Result<Color3uint8> {
    let rgb = if let Some(s) = value.as_str() {
        parse_hex_color(s)?
    } else {
        let values = value
            .as_array()
            .ok_or_else(|| RblxaError::Invalid("invalid color value".into()))?;
        if values.len() != 3 {
            return Err(RblxaError::Invalid("color requires exactly 3 channels".into()));
        }
        let mut rgb = [0u8; 3];
        for (i, channel) in values.iter().enumerate() {
            let n = channel.as_integer().ok_or_else(|| {
                RblxaError::Invalid("color channels must be integer values".into())
            })?;
            if !(0..=255).contains(&n) {
                return Err(RblxaError::Invalid("color channels must be 0..255".into()));
            }
            rgb[i] = n as u8;
        }
        rgb
    };
    Ok(Color3uint8::new(rgb[0], rgb[1], rgb[2]))
}

fn material_value(name: &str) -> Option<u32> {
    Some(match name {
        "Plastic" => 256,
        "SmoothPlastic" => 272,
        "Neon" => 288,
        "Wood" => 512,
        "WoodPlanks" => 528,
        "Marble" => 784,
        "Basalt" => 788,
        "Slate" => 800,
        "CrackedLava" => 804,
        "Concrete" => 816,
        "Limestone" => 820,
        "Granite" => 832,
        "Pavement" => 836,
        "Brick" => 848,
        "Pebble" => 864,
        "Cobblestone" => 880,
        "Rock" => 896,
        "Sandstone" => 912,
        "CorrodedMetal" => 1040,
        "DiamondPlate" => 1056,
        "Foil" => 1072,
        "Metal" => 1088,
        "Grass" => 1280,
        "LeafyGrass" => 1284,
        "Sand" => 1296,
        "Fabric" => 1312,
        "Snow" => 1328,
        "Mud" => 1344,
        "Ground" => 1360,
        "Asphalt" => 1376,
        "Salt" => 1392,
        "Ice" => 1536,
        "Glacier" => 1552,
        "Glass" => 1568,
        "ForceField" => 1584,
        "Air" => 1792,
        "Water" => 2048,
        "Cardboard" => 2304,
        "Carpet" => 2305,
        "CeramicTiles" => 2306,
        "ClayRoofTiles" => 2307,
        "RoofShingles" => 2308,
        "Leather" => 2309,
        "Plaster" => 2310,
        "Rubber" => 2311,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::RblxaObject;
    use std::collections::BTreeMap;

    fn script_object() -> RblxaObject {
        let mut properties = BTreeMap::new();
        properties.insert("run_context".into(), Value::String("Server".into()));
        properties.insert("source".into(), Value::String("print(1)".into()));
        RblxaObject {
            table: "script".into(),
            id: "server_script".into(),
            name: "ServerScript".into(),
            class_name: "Script".into(),
            parent: Some("ServerScriptService".into()),
            properties,
        }
    }

    #[test]
    fn run_context_does_not_duplicate_generic_property() {
        let builders = object_builders(&script_object(), Path::new("."))
            .expect("script builder should succeed");
        let mut dom = WeakDom::new(InstanceBuilder::new("DataModel"));
        let referent = dom.insert(dom.root_ref(), builders.into_iter().next().unwrap());
        let instance = dom.get_by_ref(referent).unwrap();
        assert_eq!(
            instance.properties.keys().filter(|key| key.to_string() == "RunContext").count(),
            1
        );
    }
}
