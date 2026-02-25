//! Parser for space-level `models.bin` (MergedModels) files.
//!
//! These files contain all model instances for a space/map. Each record packs a
//! ModelPrototype, VisualPrototype, and SkeletonProto into a flat 0xA8-byte
//! record with struct-base-relative relptrs.
//!
//! See MODELS.md § "MergedModels (`models.bin`) Format" for full field layout.

use rootcause::Report;
use thiserror::Error;

use crate::data::parser_utils::resolve_relptr;
use crate::models::model::{ModelPrototype, parse_model};
use crate::models::visual::{BoundingBox, Lod, Matrix4x4, RenderSet, VisualNodes, VisualPrototype};

/// Errors during `models.bin` parsing.
#[derive(Debug, Error)]
pub enum MergedModelsError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort {
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// Parsed `models.bin` file.
#[derive(Debug)]
pub struct MergedModels {
    pub models: Vec<MergedModelRecord>,
    pub skeletons: Vec<SkeletonProto>,
    pub model_bone_count: u16,
}

/// A single model record from the merged array.
#[derive(Debug)]
pub struct MergedModelRecord {
    /// selfId identifying this model in pathsStorage.
    pub path_id: u64,
    /// Inlined ModelPrototype fields.
    pub model_proto: ModelPrototype,
    /// Inlined VisualPrototype (includes inline SkeletonProto).
    pub visual_proto: VisualPrototype,
    /// Index into the shared skeletons array.
    pub skeleton_proto_index: u32,
    /// First geometry mapping index for this model's render sets.
    pub render_set_geometry_start_idx: u16,
    /// Number of geometry mappings for this model.
    pub render_set_geometry_count: u16,
}

/// Shared skeleton prototype (stride 0x30).
#[derive(Debug)]
pub struct SkeletonProto {
    pub nodes: VisualNodes,
}

/// A single model instance from `space.bin`, combining a world transform
/// with a reference to the model prototype via `path_id`.
#[derive(Debug)]
pub struct SpaceInstance {
    /// 4×4 world transform matrix (column-major, row 3 = translation + w=1).
    pub transform: Matrix4x4,
    /// selfId matching a `MergedModelRecord::path_id` in the sibling `models.bin`.
    pub path_id: u64,
}

/// Parsed `space.bin` instance placements.
#[derive(Debug)]
pub struct SpaceInstances {
    pub instances: Vec<SpaceInstance>,
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn read_u8(data: &[u8], offset: usize) -> u8 {
    data[offset]
}

fn read_u16(data: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(data[offset..offset + 2].try_into().unwrap())
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_f32(data: &[u8], offset: usize) -> f32 {
    f32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_i64(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn bounds_check(data: &[u8], offset: usize, need: usize) -> Result<(), Report<MergedModelsError>> {
    if offset + need > data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset,
            need,
            have: data.len(),
        }));
    }
    Ok(())
}

fn read_u32_array(
    data: &[u8],
    base: usize,
    relptr_offset: usize,
    count: usize,
) -> Result<Vec<u32>, Report<MergedModelsError>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let relptr = read_i64(data, base + relptr_offset);
    let abs = resolve_relptr(base, relptr);
    bounds_check(data, abs, count * 4)?;
    Ok((0..count).map(|i| read_u32(data, abs + i * 4)).collect())
}

fn read_u16_array(
    data: &[u8],
    base: usize,
    relptr_offset: usize,
    count: usize,
) -> Result<Vec<u16>, Report<MergedModelsError>> {
    if count == 0 {
        return Ok(Vec::new());
    }
    let relptr = read_i64(data, base + relptr_offset);
    let abs = resolve_relptr(base, relptr);
    bounds_check(data, abs, count * 2)?;
    Ok((0..count).map(|i| read_u16(data, abs + i * 2)).collect())
}

// ── Header ───────────────────────────────────────────────────────────────────

const HEADER_SIZE: usize = 0x18;
const MODEL_RECORD_SIZE: usize = 0xA8;
const SKELETON_SIZE: usize = 0x30;
const RENDER_SET_SIZE: usize = 0x28;
const LOD_SIZE: usize = 0x10;

/// Parse a `models.bin` file.
pub fn parse_merged_models(file_data: &[u8]) -> Result<MergedModels, Report<MergedModelsError>> {
    bounds_check(file_data, 0, HEADER_SIZE)?;

    let models_count = read_u32(file_data, 0x00) as usize;
    let skeletons_count = read_u16(file_data, 0x04) as usize;
    let model_bone_count = read_u16(file_data, 0x06);
    let models_relptr = read_i64(file_data, 0x08);
    let skeletons_relptr = read_i64(file_data, 0x10);

    let models_offset = resolve_relptr(0, models_relptr);
    let skeletons_offset = resolve_relptr(0, skeletons_relptr);

    // Parse model records
    bounds_check(file_data, models_offset, models_count * MODEL_RECORD_SIZE)?;
    let mut models = Vec::with_capacity(models_count);
    for i in 0..models_count {
        let rec_base = models_offset + i * MODEL_RECORD_SIZE;
        let record = parse_model_record(file_data, rec_base)
            .map_err(|e| MergedModelsError::ParseError(format!("model[{i}]: {e}")))?;
        models.push(record);
    }

    // Parse shared skeleton prototypes
    bounds_check(file_data, skeletons_offset, skeletons_count * SKELETON_SIZE)?;
    let mut skeletons = Vec::with_capacity(skeletons_count);
    for i in 0..skeletons_count {
        let skel_base = skeletons_offset + i * SKELETON_SIZE;
        let skeleton = parse_skeleton_proto(file_data, skel_base)
            .map_err(|e| MergedModelsError::ParseError(format!("skeleton[{i}]: {e}")))?;
        skeletons.push(skeleton);
    }

    Ok(MergedModels {
        models,
        skeletons,
        model_bone_count,
    })
}

// ── space.bin parser ─────────────────────────────────────────────────────────

const SPACE_HEADER_SIZE: usize = 0x60;
const SPACE_INSTANCE_SIZE: usize = 0x70;

/// Parse a `space.bin` file to extract instance placements (world transforms).
pub fn parse_space_instances(
    file_data: &[u8],
) -> Result<SpaceInstances, Report<MergedModelsError>> {
    bounds_check(file_data, 0, SPACE_HEADER_SIZE)?;

    let instance_count = read_u32(file_data, 0x00) as usize;

    bounds_check(
        file_data,
        SPACE_HEADER_SIZE,
        instance_count * SPACE_INSTANCE_SIZE,
    )?;

    let mut instances = Vec::with_capacity(instance_count);
    for i in 0..instance_count {
        let base = SPACE_HEADER_SIZE + i * SPACE_INSTANCE_SIZE;

        // 4×4 f32 matrix at +0x00 (64 bytes)
        let mut m = [0f32; 16];
        for (j, val) in m.iter_mut().enumerate() {
            *val = read_f32(file_data, base + j * 4);
        }
        let transform = Matrix4x4(m);

        // path_id at +0x50
        let path_id = read_u64(file_data, base + 0x50);

        instances.push(SpaceInstance { transform, path_id });
    }

    Ok(SpaceInstances { instances })
}

// ── Model Record ─────────────────────────────────────────────────────────────

fn parse_model_record(
    data: &[u8],
    rec: usize,
) -> Result<MergedModelRecord, Report<MergedModelsError>> {
    bounds_check(data, rec, MODEL_RECORD_SIZE)?;

    let path_id = read_u64(data, rec + 0x00);

    // ModelProto at rec+0x08 (0x28 bytes). We reuse the existing model.rs parser
    // by slicing from the ModelProto base to end of file.
    let model_proto_base = rec + 0x08;
    if model_proto_base >= data.len() {
        return Err(Report::new(MergedModelsError::DataTooShort {
            offset: model_proto_base,
            need: 0x28,
            have: data.len(),
        }));
    }
    let model_proto = parse_model(&data[model_proto_base..]).map_err(|e| {
        MergedModelsError::ParseError(format!("ModelProto at 0x{model_proto_base:X}: {e}"))
    })?;

    // VisualProto at rec+0x30 (0x70 bytes). Parse inline.
    let vp_base = rec + 0x30;
    let visual_proto = parse_visual_proto_inline(data, vp_base)?;

    let skeleton_proto_index = read_u32(data, rec + 0xA0);
    let render_set_geometry_start_idx = read_u16(data, rec + 0xA4);
    let render_set_geometry_count = read_u16(data, rec + 0xA6);

    Ok(MergedModelRecord {
        path_id,
        model_proto,
        visual_proto,
        skeleton_proto_index,
        render_set_geometry_start_idx,
        render_set_geometry_count,
    })
}

// ── VisualProto (inline at rec+0x30, size 0x70) ─────────────────────────────

fn parse_visual_proto_inline(
    data: &[u8],
    vp_base: usize,
) -> Result<VisualPrototype, Report<MergedModelsError>> {
    bounds_check(data, vp_base, 0x70)?;

    // SkeletonProto is the first 0x30 bytes of VisualProto
    let nodes = parse_skeleton_nodes(data, vp_base)?;

    let merged_geometry_path_id = read_u64(data, vp_base + 0x30);
    let underwater_model = read_u8(data, vp_base + 0x38) != 0;
    let abovewater_model = read_u8(data, vp_base + 0x39) != 0;
    let render_sets_count = read_u16(data, vp_base + 0x3A) as usize;
    let lods_count = read_u16(data, vp_base + 0x3C) as usize;

    let bounding_box = BoundingBox {
        min: [
            read_f32(data, vp_base + 0x40),
            read_f32(data, vp_base + 0x44),
            read_f32(data, vp_base + 0x48),
        ],
        max: [
            read_f32(data, vp_base + 0x50),
            read_f32(data, vp_base + 0x54),
            read_f32(data, vp_base + 0x58),
        ],
    };

    // RenderSets: relptr at vp_base+0x60, base = vp_base
    let render_sets = if render_sets_count > 0 {
        let rs_relptr = read_i64(data, vp_base + 0x60);
        let rs_abs = resolve_relptr(vp_base, rs_relptr);
        parse_render_sets_merged(data, rs_abs, render_sets_count)?
    } else {
        Vec::new()
    };

    // LODs: relptr at vp_base+0x68, base = vp_base
    let lods = if lods_count > 0 {
        let lod_relptr = read_i64(data, vp_base + 0x68);
        let lod_abs = resolve_relptr(vp_base, lod_relptr);
        parse_lods_merged(data, lod_abs, lods_count)?
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

// ── Skeleton nodes (shared between inline SkeletonProto and shared skeletons)

fn parse_skeleton_nodes(
    data: &[u8],
    skel_base: usize,
) -> Result<VisualNodes, Report<MergedModelsError>> {
    bounds_check(data, skel_base, 0x30)?;

    let nodes_count = read_u32(data, skel_base) as usize;

    if nodes_count == 0 {
        return Ok(VisualNodes {
            name_map_name_ids: Vec::new(),
            name_map_node_ids: Vec::new(),
            name_ids: Vec::new(),
            matrices: Vec::new(),
            parent_ids: Vec::new(),
        });
    }

    let name_map_name_ids = read_u32_array(data, skel_base, 0x08, nodes_count)?;
    let name_map_node_ids = read_u16_array(data, skel_base, 0x10, nodes_count)?;
    let name_ids = read_u32_array(data, skel_base, 0x18, nodes_count)?;

    // Matrices: 64 bytes each (16 × f32)
    let matrices_relptr = read_i64(data, skel_base + 0x20);
    let matrices_abs = resolve_relptr(skel_base, matrices_relptr);
    let matrices_need = nodes_count * 64;
    bounds_check(data, matrices_abs, matrices_need)?;
    let matrices = (0..nodes_count)
        .map(|i| {
            let mat_base = matrices_abs + i * 64;
            let mut m = [0f32; 16];
            for (j, val) in m.iter_mut().enumerate() {
                *val = read_f32(data, mat_base + j * 4);
            }
            Matrix4x4(m)
        })
        .collect();

    let parent_ids = read_u16_array(data, skel_base, 0x28, nodes_count)?;

    Ok(VisualNodes {
        name_map_name_ids,
        name_map_node_ids,
        name_ids,
        matrices,
        parent_ids,
    })
}

fn parse_skeleton_proto(
    data: &[u8],
    skel_base: usize,
) -> Result<SkeletonProto, Report<MergedModelsError>> {
    let nodes = parse_skeleton_nodes(data, skel_base)?;
    Ok(SkeletonProto { nodes })
}

// ── RenderSet (stride 0x28) ─────────────────────────────────────────────────

fn parse_render_sets_merged(
    data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<RenderSet>, Report<MergedModelsError>> {
    bounds_check(data, offset, count * RENDER_SET_SIZE)?;

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let rs_base = offset + i * RENDER_SET_SIZE;

        let name_id = read_u32(data, rs_base);
        let material_name_id = read_u32(data, rs_base + 0x04);
        let vertices_mapping_index = read_u32(data, rs_base + 0x08);
        let indices_mapping_index = read_u32(data, rs_base + 0x0C);
        let material_mfm_path_id = read_u64(data, rs_base + 0x10);
        let skinned = read_u8(data, rs_base + 0x18) != 0;
        let nodes_count = read_u8(data, rs_base + 0x19) as usize;

        let node_name_ids = read_u32_array(data, rs_base, 0x20, nodes_count)?;

        // Pack the two u32 indices into the u64 field for compatibility with
        // the existing VisualPrototype RenderSet type.
        let unknown_u64 = ((indices_mapping_index as u64) << 32) | (vertices_mapping_index as u64);

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

// ── LOD (stride 0x10) ───────────────────────────────────────────────────────

fn parse_lods_merged(
    data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<Lod>, Report<MergedModelsError>> {
    bounds_check(data, offset, count * LOD_SIZE)?;

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let lod_base = offset + i * LOD_SIZE;

        let extent = read_f32(data, lod_base);
        let casts_shadow = read_u8(data, lod_base + 0x04) != 0;
        let render_set_names_count = read_u16(data, lod_base + 0x06) as usize;
        let render_set_names = read_u32_array(data, lod_base, 0x08, render_set_names_count)?;

        result.push(Lod {
            extent,
            casts_shadow,
            render_set_names,
        });
    }

    Ok(result)
}
