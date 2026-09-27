use std::collections::BTreeSet;

/// Built-in RBLXA object tables and their Roblox ClassName mapping.
pub const OBJECT_TABLES: &[(&[&str], &str, &str)] = &[
    (&["part"], "part", "Part"),
    (&["model"], "model", "Model"),
    (&["folder"], "folder", "Folder"),
    (&["meshpart"], "meshpart", "MeshPart"),
    (&["script"], "script", "Script"),
    (&["3D", "Object", "File"], "3D.Object.File", "Model"),
    (&["texture"], "texture", "Texture"),
    (&["decal"], "decal", "Decal"),
    (&["sound"], "sound", "Sound"),
    (&["attachment"], "attachment", "Attachment"),
    (&["spawn"], "spawn", "SpawnLocation"),
    (&["camera"], "camera", "Camera"),
    (&["terrain"], "terrain", "Terrain"),
    (&["object"], "object", "Object"),
];

/// Fields interpreted by the RBLXA compiler rather than copied as Roblox properties.
pub const RESERVED: &[&str] = &[
    "id", "name", "parent", "class", "type", "source", "path", "format", "run_context",
];

/// Roblox services that may be used directly as RBLXA parents.
pub const SERVICES: &[&str] = &[
    "Workspace",
    "Lighting",
    "ReplicatedStorage",
    "ReplicatedFirst",
    "ServerScriptService",
    "ServerStorage",
    "StarterGui",
    "StarterPack",
    "StarterPlayer",
    "SoundService",
    "Teams",
    "TextChatService",
    "Chat",
];

pub fn known_tables() -> BTreeSet<&'static str> {
    let mut s = BTreeSet::new();
    for (_, n, _) in OBJECT_TABLES {
        s.insert(*n);
    }
    s.insert("place.meta.data");
    s.insert("place.settings");
    s
}

pub fn is_service(name: &str) -> bool {
    SERVICES.contains(&name)
}
