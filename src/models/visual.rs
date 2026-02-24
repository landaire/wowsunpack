use rootcause::Report;
use thiserror::Error;

use crate::data::parser_utils::resolve_relptr;
use crate::models::assets_bin::{PrototypeDatabase, StringsSection};

/// Errors that can occur during VisualPrototype parsing.
#[derive(Debug, Error)]
pub enum VisualError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort {
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Item size for VisualPrototype records in the database blob.
pub const VISUAL_ITEM_SIZE: usize = 0x70;

/// A parsed VisualPrototype record.
#[derive(Debug)]
pub struct VisualPrototype {
    pub nodes: VisualNodes,
    pub merged_geometry_path_id: u64,
    pub underwater_model: bool,
    pub abovewater_model: bool,
    pub bounding_box: BoundingBox,
    pub render_sets: Vec<RenderSet>,
    pub lods: Vec<Lod>,
}

/// Scene graph node hierarchy.
#[derive(Debug)]
pub struct VisualNodes {
    pub name_map_name_ids: Vec<u32>,
    pub name_map_node_ids: Vec<u16>,
    pub name_ids: Vec<u32>,
    pub matrices: Vec<Matrix4x4>,
    pub parent_ids: Vec<u16>,
}

/// 4x4 transformation matrix (column-major, 64 bytes).
#[derive(Debug, Clone)]
pub struct Matrix4x4(pub [f32; 16]);

/// Axis-aligned bounding box.
#[derive(Debug, Clone)]
pub struct BoundingBox {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// A render set binding a mesh to a material.
#[derive(Debug)]
pub struct RenderSet {
    pub name_id: u32,
    pub material_name_id: u32,
    /// Unknown u64 at offset +0x08 in the RenderSet struct.
    /// Hypothesis: low32=vertices_mapping_id, high32=indices_mapping_id.
    pub unknown_u64: u64,
    /// selfId of the .mfm file in pathsStorage (u64 path hash).
    pub material_mfm_path_id: u64,
    pub skinned: bool,
    pub node_name_ids: Vec<u32>,
}

/// A level-of-detail entry.
#[derive(Debug)]
pub struct Lod {
    pub extent: f32,
    pub casts_shadow: bool,
    pub render_set_names: Vec<u32>,
}

/// Read a little-endian u16 from a byte slice.
fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

/// Read a little-endian u32 from a byte slice.
fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Read a little-endian u64 from a byte slice.
fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

/// Read a little-endian f32 from a byte slice.
fn read_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

/// Read a little-endian i64 from a byte slice.
fn read_i64(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

/// Read `count` little-endian u32 values from a relptr.
///
/// `base` is the absolute offset in the blob data where the containing struct starts.
/// `relptr_offset` is the offset within the struct where the i64 relptr is stored.
fn read_u32_array(
    blob_data: &[u8],
    base: usize,
    relptr_offset: usize,
    count: usize,
) -> Result<Vec<u32>, Report<VisualError>> {
    let relptr = read_i64(blob_data, base + relptr_offset);
    let abs = resolve_relptr(base, relptr);
    let need = count * 4;
    if abs + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort {
            offset: abs,
            need,
            have: blob_data.len(),
        }));
    }
    Ok((0..count)
        .map(|i| read_u32(blob_data, abs + i * 4))
        .collect())
}

/// Read `count` little-endian u16 values from a relptr.
fn read_u16_array(
    blob_data: &[u8],
    base: usize,
    relptr_offset: usize,
    count: usize,
) -> Result<Vec<u16>, Report<VisualError>> {
    let relptr = read_i64(blob_data, base + relptr_offset);
    let abs = resolve_relptr(base, relptr);
    let need = count * 2;
    if abs + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort {
            offset: abs,
            need,
            have: blob_data.len(),
        }));
    }
    Ok((0..count)
        .map(|i| read_u16(blob_data, abs + i * 2))
        .collect())
}

/// Parse a VisualPrototype from blob data.
///
/// `record_data` is a slice starting at the record's offset within the blob,
/// extending to the end of the blob (so relptrs can resolve into OOL data).
/// The first `VISUAL_ITEM_SIZE` bytes are the fixed record fields.
pub fn parse_visual(record_data: &[u8]) -> Result<VisualPrototype, Report<VisualError>> {
    if record_data.len() < VISUAL_ITEM_SIZE {
        return Err(Report::new(VisualError::DataTooShort {
            offset: 0,
            need: VISUAL_ITEM_SIZE,
            have: record_data.len(),
        }));
    }

    // The record base is at offset 0 within record_data.
    // All top-level relptrs are relative to this base.
    let base = 0usize;

    // Node sub-struct (+0x00 to +0x2F)
    let nodes_count = read_u32(record_data, base + 0x00) as usize;

    let nodes = if nodes_count > 0 {
        let name_map_name_ids = read_u32_array(record_data, base, 0x08, nodes_count)?;
        let name_map_node_ids = read_u16_array(record_data, base, 0x10, nodes_count)?;
        let name_ids = read_u32_array(record_data, base, 0x18, nodes_count)?;

        // Matrices: 64 bytes each (16 x f32)
        let matrices_relptr = read_i64(record_data, base + 0x20);
        let matrices_abs = resolve_relptr(base, matrices_relptr);
        let matrices_need = nodes_count * 64;
        if matrices_abs + matrices_need > record_data.len() {
            return Err(Report::new(VisualError::DataTooShort {
                offset: matrices_abs,
                need: matrices_need,
                have: record_data.len(),
            }));
        }
        let matrices = (0..nodes_count)
            .map(|i| {
                let mat_base = matrices_abs + i * 64;
                let mut m = [0f32; 16];
                for j in 0..16 {
                    m[j] = read_f32(record_data, mat_base + j * 4);
                }
                Matrix4x4(m)
            })
            .collect();

        let parent_ids = read_u16_array(record_data, base, 0x28, nodes_count)?;

        VisualNodes {
            name_map_name_ids,
            name_map_node_ids,
            name_ids,
            matrices,
            parent_ids,
        }
    } else {
        VisualNodes {
            name_map_name_ids: Vec::new(),
            name_map_node_ids: Vec::new(),
            name_ids: Vec::new(),
            matrices: Vec::new(),
            parent_ids: Vec::new(),
        }
    };

    let merged_geometry_path_id = read_u64(record_data, base + 0x30);
    let underwater_model = record_data[base + 0x38] != 0;
    let abovewater_model = record_data[base + 0x39] != 0;
    let render_sets_count = read_u16(record_data, base + 0x3A) as usize;
    let lods_count = record_data[base + 0x3C] as usize;

    let bounding_box = BoundingBox {
        min: [
            read_f32(record_data, base + 0x40),
            read_f32(record_data, base + 0x44),
            read_f32(record_data, base + 0x48),
        ],
        max: [
            read_f32(record_data, base + 0x50),
            read_f32(record_data, base + 0x54),
            read_f32(record_data, base + 0x58),
        ],
    };

    // RenderSets: 0x28 bytes each, relptr at +0x60
    let render_sets = if render_sets_count > 0 {
        let rs_relptr = read_i64(record_data, base + 0x60);
        let rs_abs = resolve_relptr(base, rs_relptr);
        parse_render_sets(record_data, rs_abs, render_sets_count)?
    } else {
        Vec::new()
    };

    // LODs: 0x10 bytes each, relptr at +0x68
    let lods = if lods_count > 0 {
        let lod_relptr = read_i64(record_data, base + 0x68);
        let lod_abs = resolve_relptr(base, lod_relptr);
        parse_lods(record_data, lod_abs, lods_count)?
    } else {
        Vec::new()
    };

    Ok(VisualPrototype {
        nodes,
        merged_geometry_path_id,
        underwater_model,
        abovewater_model,
        bounding_box,
        render_sets,
        lods,
    })
}

const RENDER_SET_SIZE: usize = 0x28;

fn parse_render_sets(
    blob_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<RenderSet>, Report<VisualError>> {
    let need = count * RENDER_SET_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort {
            offset,
            need,
            have: blob_data.len(),
        }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let rs_base = offset + i * RENDER_SET_SIZE;

        let name_id = read_u32(blob_data, rs_base + 0x00);
        let material_name_id = read_u32(blob_data, rs_base + 0x04);
        let unknown_u64 = read_u64(blob_data, rs_base + 0x08);
        let material_mfm_path_id = read_u64(blob_data, rs_base + 0x10);
        let skinned = blob_data[rs_base + 0x18] != 0;
        let nodes_count = blob_data[rs_base + 0x19] as usize;

        // nodeNameIds relptr is relative to the RS base
        let node_name_ids = if nodes_count > 0 {
            read_u32_array(blob_data, rs_base, 0x20, nodes_count)?
        } else {
            Vec::new()
        };

        result.push(RenderSet {
            name_id,
            material_name_id,
            unknown_u64,
            material_mfm_path_id,
            skinned,
            node_name_ids,
        });
    }

    Ok(result)
}

const LOD_SIZE: usize = 0x10;

fn parse_lods(
    blob_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<Lod>, Report<VisualError>> {
    let need = count * LOD_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(VisualError::DataTooShort {
            offset,
            need,
            have: blob_data.len(),
        }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let lod_base = offset + i * LOD_SIZE;

        let extent = read_f32(blob_data, lod_base + 0x00);
        let casts_shadow = blob_data[lod_base + 0x04] != 0;
        let render_set_names_count = read_u16(blob_data, lod_base + 0x06) as usize;

        // renderSetNames relptr is relative to the LOD base
        let render_set_names = if render_set_names_count > 0 {
            read_u32_array(blob_data, lod_base, 0x08, render_set_names_count)?
        } else {
            Vec::new()
        };

        result.push(Lod {
            extent,
            casts_shadow,
            render_set_names,
        });
    }

    Ok(result)
}

impl VisualPrototype {
    /// Resolve string IDs and path IDs using the database.
    pub fn print_summary(&self, db: &PrototypeDatabase<'_>) {
        let strings = &db.strings;
        let self_id_index = db.build_self_id_index();

        // Helper to resolve a u64 selfId to a path leaf name
        let resolve_path_leaf = |self_id: u64| -> String {
            if self_id == 0 {
                return "(none)".to_string();
            }
            match self_id_index.get(&self_id) {
                Some(&idx) => db.paths_storage[idx].name.clone(),
                None => format!("0x{self_id:016X}"),
            }
        };

        println!("  Nodes: {}", self.nodes.name_ids.len());
        for (i, &name_id) in self.nodes.name_ids.iter().enumerate() {
            let name = strings.get_string_by_id(name_id).unwrap_or("<unknown>");
            let parent = self.nodes.parent_ids[i];
            let parent_str = if parent == 0xFFFF {
                "root".to_string()
            } else {
                format!("{parent}")
            };
            println!("    [{i}] name=\"{name}\" parent={parent_str}");
        }

        let geom_name = resolve_path_leaf(self.merged_geometry_path_id);
        println!("  MergedGeometry: {geom_name}");
        println!("  UnderwaterModel: {}", self.underwater_model);
        println!("  AbovewaterModel: {}", self.abovewater_model);
        println!(
            "  BoundingBox: min=({:.3}, {:.3}, {:.3}) max=({:.3}, {:.3}, {:.3})",
            self.bounding_box.min[0],
            self.bounding_box.min[1],
            self.bounding_box.min[2],
            self.bounding_box.max[0],
            self.bounding_box.max[1],
            self.bounding_box.max[2],
        );

        println!("  RenderSets: {}", self.render_sets.len());
        for (i, rs) in self.render_sets.iter().enumerate() {
            let name = strings.get_string_by_id(rs.name_id).unwrap_or("<unknown>");
            let mat_name = strings
                .get_string_by_id(rs.material_name_id)
                .unwrap_or("<unknown>");
            let mfm_name = resolve_path_leaf(rs.material_mfm_path_id);
            let low32 = (rs.unknown_u64 & 0xFFFFFFFF) as u32;
            let high32 = (rs.unknown_u64 >> 32) as u32;
            println!(
                "    [{i}] name=\"{name}\" material=\"{mat_name}\" mfm=\"{mfm_name}\" skinned={} nodes={}\n        unknown_u64=0x{:016X} (low32=0x{:08X} high32=0x{:08X})",
                rs.skinned,
                rs.node_name_ids.len(),
                rs.unknown_u64,
                low32,
                high32,
            );
        }

        println!("  LODs: {}", self.lods.len());
        for (i, lod) in self.lods.iter().enumerate() {
            let rs_names: Vec<String> = lod
                .render_set_names
                .iter()
                .map(|&id| {
                    strings
                        .get_string_by_id(id)
                        .unwrap_or("<unknown>")
                        .to_string()
                })
                .collect();
            println!(
                "    [{i}] extent={:.1} shadow={} renderSets=[{}]",
                lod.extent,
                lod.casts_shadow,
                rs_names.join(", ")
            );
        }
    }

    /// Find the world-space transform for a named hardpoint node.
    ///
    /// Looks up `hp_name` in the visual's name_map by resolving each name_map_name_id
    /// via the strings section, then composes transforms walking up the parent chain.
    ///
    /// Returns `None` if the node name is not found.
    pub fn find_hardpoint_transform(
        &self,
        hp_name: &str,
        strings: &StringsSection<'_>,
    ) -> Option<[f32; 16]> {
        // Find the node index for this hardpoint name.
        let node_idx = self.find_node_index_by_name(hp_name, strings)?;

        // Compose transforms walking up the parent chain.
        let mut result = self.nodes.matrices[node_idx as usize].0;
        let mut current = node_idx;
        loop {
            let parent = self.nodes.parent_ids[current as usize];
            if parent == 0xFFFF || parent as usize >= self.nodes.matrices.len() {
                break;
            }
            result = mat4_mul(&self.nodes.matrices[parent as usize].0, &result);
            current = parent as u16;
        }

        Some(result)
    }

    /// Get the local (non-composed) matrix of a named node.
    pub fn find_node_local_matrix(
        &self,
        name: &str,
        strings: &StringsSection<'_>,
    ) -> Option<[f32; 16]> {
        let node_idx = self.find_node_index_by_name(name, strings)?;
        Some(self.nodes.matrices[node_idx as usize].0)
    }

    /// Check whether `node_idx` is a descendant of `ancestor_idx` in the
    /// skeleton hierarchy.
    pub fn is_descendant_of(&self, mut node_idx: u16, ancestor_idx: u16) -> bool {
        loop {
            let parent = self.nodes.parent_ids[node_idx as usize];
            if parent == 0xFFFF || parent as usize >= self.nodes.parent_ids.len() {
                return false;
            }
            if parent == ancestor_idx {
                return true;
            }
            node_idx = parent;
        }
    }

    /// Find the node index for a given node name string.
    pub fn find_node_index_by_name(&self, name: &str, strings: &StringsSection<'_>) -> Option<u16> {
        for (i, &name_id) in self.nodes.name_map_name_ids.iter().enumerate() {
            if let Some(resolved) = strings.get_string_by_id(name_id) {
                if resolved == name {
                    return Some(self.nodes.name_map_node_ids[i]);
                }
            }
        }
        None
    }
}

/// Multiply two 4x4 matrices (column-major order, as stored in BigWorld).
fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for col in 0..4 {
        for row in 0..4 {
            let mut sum = 0.0;
            for k in 0..4 {
                sum += a[k * 4 + row] * b[col * 4 + k];
            }
            out[col * 4 + row] = sum;
        }
    }
    out
}
