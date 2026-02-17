//! Parser for `camouflages.xml` — material-based camouflage definitions.
//!
//! The game's camouflage system defines texture overrides in a large XML file
//! (`camouflages.xml` in the VFS root). Each `<camouflage>` entry maps a name
//! (e.g. `mat_Steel`) to per-part albedo texture paths.

use std::collections::HashMap;
use std::io::Read;

use vfs::VfsPath;

/// A parsed camouflage entry from `camouflages.xml`.
pub struct CamouflageEntry {
    /// Name, e.g. "mat_Steel".
    pub name: String,
    /// Whether this camo uses UV tiling (can't be represented in glTF).
    pub tiled: bool,
    /// Per-part albedo texture paths. Key = part category (lowercase, e.g. "hull"),
    /// Value = VFS path to the albedo DDS.
    pub textures: HashMap<String, String>,
}

/// Parsed camouflage database from `camouflages.xml`.
pub struct CamouflageDb {
    entries: HashMap<String, CamouflageEntry>,
}

impl CamouflageDb {
    /// Load and parse `camouflages.xml` from the VFS.
    pub fn load(vfs: &VfsPath) -> Option<Self> {
        let mut xml_bytes = Vec::new();
        vfs.join("camouflages.xml")
            .ok()?
            .open_file()
            .ok()?
            .read_to_end(&mut xml_bytes)
            .ok()?;
        let xml_str = String::from_utf8_lossy(&xml_bytes);
        Self::parse(&xml_str)
    }

    fn parse(xml: &str) -> Option<Self> {
        let doc = roxmltree::Document::parse(xml).ok()?;
        let mut entries = HashMap::new();

        for camo_node in doc.descendants().filter(|n| n.has_tag_name("camouflage")) {
            let Some(name) = child_text(&camo_node, "name").map(|s| s.trim()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }
            let tiled = child_text(&camo_node, "tiled")
                .map(|s| s.trim() == "true")
                .unwrap_or(false);

            let mut textures = HashMap::new();
            if let Some(tex_node) = camo_node.children().find(|n| n.has_tag_name("Textures")) {
                for child in tex_node.children().filter(|n| n.is_element()) {
                    let tag = child.tag_name().name();
                    // Skip MGN (metallic/gloss/normal) entries — we only load albedo.
                    if tag.ends_with("_mgn") {
                        continue;
                    }
                    if let Some(path) = child.text().map(|t| t.trim().to_string()) {
                        if !path.is_empty() {
                            textures.insert(tag.to_lowercase(), path);
                        }
                    }
                }
            }

            entries.insert(
                name.to_string(),
                CamouflageEntry {
                    name: name.to_string(),
                    tiled,
                    textures,
                },
            );
        }

        Some(Self { entries })
    }

    /// Look up a camouflage by name (e.g. "mat_Steel").
    pub fn get(&self, name: &str) -> Option<&CamouflageEntry> {
        self.entries.get(name)
    }

    /// Number of entries in the database.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the database is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn child_text<'a>(node: &'a roxmltree::Node, tag: &str) -> Option<&'a str> {
    node.children().find(|n| n.has_tag_name(tag))?.text()
}
