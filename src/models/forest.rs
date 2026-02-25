//! Parser for space-level `forest.bin` (SpeedTree vegetation placement) files.
//!
//! These files store per-instance placement data for SpeedTree vegetation
//! (trees, bushes, algae, etc.) across a map space. The file contains a species
//! string table referencing `.stsdk` asset paths, followed by a dense array of
//! 16-byte instance records `(f32 x, f32 y, f32 z, f32 w)`.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_f32, le_i64, le_u64};
use winnow::combinator::repeat;

use winnow::error::{ContextError, ErrMode};

use crate::data::parser_utils::{WResult, resolve_relptr};

const INSTANCE_SIZE: usize = 16;

#[derive(Debug, Error)]
pub enum ForestError {
    #[error("data too short: need {need} bytes at offset 0x{offset:X}, have {have}")]
    DataTooShort {
        offset: usize,
        need: usize,
        have: usize,
    },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// A single vegetation instance.
#[derive(Debug, Clone, Copy)]
pub struct ForestInstance {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    /// Per-instance parameter (likely rotation/scale seed).
    pub w: f32,
}

/// Parsed `forest.bin` file.
#[derive(Debug)]
pub struct Forest {
    /// SpeedTree species asset paths (`.stsdk` files).
    pub species: Vec<String>,
    /// All vegetation instances (flat array across all blocks/LODs).
    pub instances: Vec<ForestInstance>,
}

/// Parse a single string table entry: `(u64 len, i64 relptr)`.
fn parse_string_table_entry(input: &mut &[u8]) -> WResult<(u64, i64)> {
    let len = le_u64.parse_next(input)?;
    let relptr = le_i64.parse_next(input)?;
    Ok((len, relptr))
}

/// Parse a single vegetation instance: `(f32 x, f32 y, f32 z, f32 w)`.
fn parse_forest_instance(input: &mut &[u8]) -> WResult<ForestInstance> {
    let x = le_f32.parse_next(input)?;
    let y = le_f32.parse_next(input)?;
    let z = le_f32.parse_next(input)?;
    let w = le_f32.parse_next(input)?;
    Ok(ForestInstance { x, y, z, w })
}

/// Parse a `forest.bin` file.
pub fn parse_forest(file_data: &[u8]) -> Result<Forest, Report<ForestError>> {
    if file_data.len() < 32 {
        return Err(Report::new(ForestError::DataTooShort {
            offset: 0,
            need: 32,
            have: file_data.len(),
        }));
    }

    // Parse header: num_species and string_table_offset.
    let header_input = &mut &file_data[0x00..];
    let num_species = le_u64
        .parse_next(header_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?
        as usize;
    let string_table_offset = le_u64
        .parse_next(header_input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?
        as usize;

    if num_species > 1000 {
        return Err(Report::new(ForestError::ParseError(format!(
            "unreasonable species count: {num_species}"
        ))));
    }

    // Validate string table bounds.
    let string_table_end = string_table_offset + num_species * 16;
    if string_table_end > file_data.len() {
        return Err(Report::new(ForestError::DataTooShort {
            offset: string_table_offset,
            need: num_species * 16,
            have: file_data.len() - string_table_offset,
        }));
    }

    // Parse species string table: `num_species` entries of (u64 len, i64 relptr).
    // Each entry's relptr is resolved relative to that entry's file offset.
    let mut species = Vec::with_capacity(num_species);
    let mut data_start = string_table_end;

    for i in 0..num_species {
        let entry_off = string_table_offset + i * 16;
        let input = &mut &file_data[entry_off..];
        let (str_len, str_relptr) =
            parse_string_table_entry(input).map_err(|e: ErrMode<ContextError>| {
                Report::new(ForestError::ParseError(format!("{e}")))
            })?;

        let str_len = str_len as usize;
        let str_abs = resolve_relptr(entry_off, str_relptr);

        if str_len == 0 || str_abs + str_len > file_data.len() {
            species.push(format!("species_{i}"));
            continue;
        }

        // Exclude null terminator.
        let name_bytes = &file_data[str_abs..str_abs + str_len - 1];
        let name = String::from_utf8_lossy(name_bytes).into_owned();
        species.push(name);

        // Track end of string pool to find where instance data starts.
        let str_end = str_abs + str_len;
        if str_end > data_start {
            data_start = str_end;
        }
    }

    // Parse instance records from data_start to EOF.
    let remaining = file_data.len() - data_start;
    let instance_count = remaining / INSTANCE_SIZE;

    let input = &mut &file_data[data_start..data_start + instance_count * INSTANCE_SIZE];
    let instances: Vec<ForestInstance> = repeat(instance_count, parse_forest_instance)
        .parse_next(input)
        .map_err(|e: ErrMode<ContextError>| Report::new(ForestError::ParseError(format!("{e}"))))?;

    Ok(Forest { species, instances })
}
