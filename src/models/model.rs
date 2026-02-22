//! Parser for ModelPrototype records (blob index 3, item size 0x28).
//!
//! ModelPrototype wraps a VisualPrototype with additional skeleton extension,
//! animation, and dye/tint data. The key field is `visual_resource_id` which
//! is a selfId (path hash) pointing to the corresponding `.visual` entry in
//! pathsStorage.

use rootcause::Report;
use thiserror::Error;

use crate::data::parser_utils::resolve_relptr;

/// Errors that can occur during ModelPrototype parsing.
#[derive(Debug, Error)]
pub enum ModelError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort {
        offset: usize,
        need: usize,
        have: usize,
    },
}

/// Item size for ModelPrototype records in the database blob.
pub const MODEL_ITEM_SIZE: usize = 0x28;

/// A parsed ModelPrototype record.
#[derive(Debug)]
pub struct ModelPrototype {
    /// selfId (path hash) of the referenced `.visual` in pathsStorage.
    pub visual_resource_id: u64,
    /// Unknown byte at offset +0x09; purpose unclear.
    pub misc_type: u8,
    /// Skeleton extension resource IDs (selfIds of skeleton extender prototypes).
    pub skel_ext_res_ids: Vec<u64>,
    /// Animation entries (each has the same layout as a ModelPrototype record).
    pub animations: Vec<ModelPrototype>,
    /// Dye entries for camouflage / cosmetic material replacement.
    pub dyes: Vec<DyeEntry>,
}

/// Material dye/tint replacement entry.
#[derive(Debug)]
pub struct DyeEntry {
    /// String ID of the target material name.
    pub matter_id: u32,
    /// String ID of the replacement material name.
    pub replaces_id: u32,
    /// String IDs of tint variant names.
    pub tint_name_ids: Vec<u32>,
    /// selfIds of tint variant .mfm material files.
    pub tint_material_ids: Vec<u64>,
}

fn read_u8(data: &[u8], offset: usize) -> u8 {
    data[offset]
}

fn read_u32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_i32(data: &[u8], offset: usize) -> i32 {
    i32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

fn read_u64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn read_i64(data: &[u8], offset: usize) -> i64 {
    i64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

/// Parse a ModelPrototype from blob data.
///
/// `record_data` is a slice starting at the record's offset within the blob,
/// extending to the end of the blob (so relptrs can resolve into OOL data).
/// The first `MODEL_ITEM_SIZE` bytes are the fixed record fields.
pub fn parse_model(record_data: &[u8]) -> Result<ModelPrototype, Report<ModelError>> {
    parse_model_at(record_data, 0)
}

/// Parse a ModelPrototype at the given base offset within `blob_data`.
fn parse_model_at(blob_data: &[u8], base: usize) -> Result<ModelPrototype, Report<ModelError>> {
    if base + MODEL_ITEM_SIZE > blob_data.len() {
        return Err(Report::new(ModelError::DataTooShort {
            offset: base,
            need: MODEL_ITEM_SIZE,
            have: blob_data.len(),
        }));
    }

    let visual_resource_id = read_u64(blob_data, base + 0x00);
    let skel_ext_count = read_u8(blob_data, base + 0x08) as usize;
    let misc_type = read_u8(blob_data, base + 0x09);
    let animations_count = read_u8(blob_data, base + 0x0A) as usize;
    let dyes_count = read_u8(blob_data, base + 0x0B) as usize;

    // skelExtResIds: array of u64, relptr at +0x10
    let skel_ext_res_ids = if skel_ext_count > 0 {
        let relptr = read_i64(blob_data, base + 0x10);
        let abs = resolve_relptr(base, relptr);
        let need = skel_ext_count * 8;
        if abs + need > blob_data.len() {
            return Err(Report::new(ModelError::DataTooShort {
                offset: abs,
                need,
                have: blob_data.len(),
            }));
        }
        (0..skel_ext_count)
            .map(|i| read_u64(blob_data, abs + i * 8))
            .collect()
    } else {
        Vec::new()
    };

    // animations: array of ModelPrototype (0x28 each), relptr at +0x18
    let animations = if animations_count > 0 {
        let relptr = read_i64(blob_data, base + 0x18);
        let abs = resolve_relptr(base, relptr);
        let need = animations_count * MODEL_ITEM_SIZE;
        if abs + need > blob_data.len() {
            return Err(Report::new(ModelError::DataTooShort {
                offset: abs,
                need,
                have: blob_data.len(),
            }));
        }
        let mut anims = Vec::with_capacity(animations_count);
        for i in 0..animations_count {
            anims.push(parse_model_at(blob_data, abs + i * MODEL_ITEM_SIZE)?);
        }
        anims
    } else {
        Vec::new()
    };

    // dyes: array of DyeEntry (0x20 each), relptr at +0x20
    let dyes = if dyes_count > 0 {
        let relptr = read_i64(blob_data, base + 0x20);
        let abs = resolve_relptr(base, relptr);
        parse_dye_entries(blob_data, abs, dyes_count)?
    } else {
        Vec::new()
    };

    Ok(ModelPrototype {
        visual_resource_id,
        misc_type,
        skel_ext_res_ids,
        animations,
        dyes,
    })
}

const DYE_ENTRY_SIZE: usize = 0x20;

fn parse_dye_entries(
    blob_data: &[u8],
    offset: usize,
    count: usize,
) -> Result<Vec<DyeEntry>, Report<ModelError>> {
    let need = count * DYE_ENTRY_SIZE;
    if offset + need > blob_data.len() {
        return Err(Report::new(ModelError::DataTooShort {
            offset,
            need,
            have: blob_data.len(),
        }));
    }

    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let dye_base = offset + i * DYE_ENTRY_SIZE;

        let matter_id = read_u32(blob_data, dye_base + 0x00);
        let replaces_id = read_u32(blob_data, dye_base + 0x04);
        let tints_count = read_i32(blob_data, dye_base + 0x08).max(0) as usize;

        // tintNameIds: array of u32, relptr at +0x10
        let tint_name_ids = if tints_count > 0 {
            let relptr = read_i64(blob_data, dye_base + 0x10);
            let abs = resolve_relptr(dye_base, relptr);
            let need = tints_count * 4;
            if abs + need > blob_data.len() {
                return Err(Report::new(ModelError::DataTooShort {
                    offset: abs,
                    need,
                    have: blob_data.len(),
                }));
            }
            (0..tints_count)
                .map(|j| read_u32(blob_data, abs + j * 4))
                .collect()
        } else {
            Vec::new()
        };

        // tintMaterialIds: array of u64, relptr at +0x18
        let tint_material_ids = if tints_count > 0 {
            let relptr = read_i64(blob_data, dye_base + 0x18);
            let abs = resolve_relptr(dye_base, relptr);
            let need = tints_count * 8;
            if abs + need > blob_data.len() {
                return Err(Report::new(ModelError::DataTooShort {
                    offset: abs,
                    need,
                    have: blob_data.len(),
                }));
            }
            (0..tints_count)
                .map(|j| read_u64(blob_data, abs + j * 8))
                .collect()
        } else {
            Vec::new()
        };

        result.push(DyeEntry {
            matter_id,
            replaces_id,
            tint_name_ids,
            tint_material_ids,
        });
    }

    Ok(result)
}
