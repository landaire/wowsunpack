//! Parser for `camouflages.xml` — camouflage definitions including color schemes.
//!
//! The game's camouflage system defines texture overrides in a large XML file
//! (`camouflages.xml` in the VFS root). Each `<camouflage>` entry maps a name
//! (e.g. `mat_Steel`) to per-part albedo texture paths. Tiled camouflages also
//! reference a `colorScheme` that provides 4 RGBA colors used to colorize a
//! repeating tile pattern texture.

use std::collections::HashMap;
use std::io::Read;

use vfs::VfsPath;

/// A color scheme with 4 RGBA colors (linear space).
///
/// The tile texture acts as a color-indexed mask: Black/R/G/B zones map to
/// color0/color1/color2/color3 respectively.
pub struct ColorScheme {
    pub name: String,
    pub colors: [[f32; 4]; 4],
}

/// A parsed camouflage entry from `camouflages.xml`.
pub struct CamouflageEntry {
    /// Name, e.g. "mat_Steel" or "camo_CN_NY_2018_02_tile".
    pub name: String,
    /// Whether this camo uses UV tiling (tile texture + colorScheme).
    pub tiled: bool,
    /// Per-part albedo texture paths. Key = part category (lowercase, e.g. "hull"),
    /// Value = VFS path to the albedo DDS. For tiled camos, typically just "tile".
    pub textures: HashMap<String, String>,
    /// Name of the color scheme (for tiled camos).
    pub color_scheme: Option<String>,
}

/// Parsed camouflage database from `camouflages.xml`.
pub struct CamouflageDb {
    entries: HashMap<String, CamouflageEntry>,
    color_schemes: HashMap<String, ColorScheme>,
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

        // Parse color schemes first.
        let mut color_schemes = HashMap::new();
        for cs_node in doc.descendants().filter(|n| n.has_tag_name("colorScheme")) {
            let Some(name) = child_text(&cs_node, "name").map(|s| s.trim()) else {
                continue;
            };
            if name.is_empty() {
                continue;
            }

            let mut colors = [[0.0f32; 4]; 4];
            for i in 0..4 {
                let tag = format!("color{i}");
                if let Some(text) = child_text(&cs_node, &tag) {
                    let parts: Vec<f32> = text
                        .split_whitespace()
                        .filter_map(|s| s.parse().ok())
                        .collect();
                    if parts.len() >= 4 {
                        colors[i] = [parts[0], parts[1], parts[2], parts[3]];
                    }
                }
            }

            color_schemes.insert(
                name.to_string(),
                ColorScheme {
                    name: name.to_string(),
                    colors,
                },
            );
        }

        // Parse camouflage entries.
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
                    // Skip MGN (metallic/gloss/normal) and animmap entries.
                    if tag.ends_with("_mgn") || tag.ends_with("_animmap") {
                        continue;
                    }
                    if let Some(path) = child.text().map(|t| t.trim().to_string()) {
                        if !path.is_empty() {
                            textures.insert(tag.to_lowercase(), path);
                        }
                    }
                }
            }

            // Parse colorSchemes reference (take first word if multiple).
            let color_scheme = child_text(&camo_node, "colorSchemes")
                .map(|s| s.trim())
                .and_then(|s| s.split_whitespace().next())
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());

            entries.insert(
                name.to_string(),
                CamouflageEntry {
                    name: name.to_string(),
                    tiled,
                    textures,
                    color_scheme,
                },
            );
        }

        Some(Self {
            entries,
            color_schemes,
        })
    }

    /// Look up a camouflage by name (e.g. "mat_Steel").
    pub fn get(&self, name: &str) -> Option<&CamouflageEntry> {
        self.entries.get(name)
    }

    /// Look up a color scheme by name.
    pub fn color_scheme(&self, name: &str) -> Option<&ColorScheme> {
        self.color_schemes.get(name)
    }

    /// Number of camouflage entries in the database.
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
