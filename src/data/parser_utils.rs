//! Shared winnow-based parsing utilities used across idx, geometry, and assets_bin parsers.

use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_i64, le_u32};
use winnow::error::ContextError;

/// Common result type for winnow parsers.
pub type WResult<T> = Result<T, winnow::error::ErrMode<ContextError>>;

/// Errors that can occur during shared parsing operations.
#[derive(Debug, Error)]
pub enum ParseError {
    #[error(
        "packed string at 0x{offset:X} extends beyond file (need 0x{needed:X}, have 0x{available:X})"
    )]
    PackedStringOutOfBounds {
        offset: usize,
        needed: usize,
        available: usize,
    },
    #[error("winnow parse error at 0x{offset:X}: {detail}")]
    WinnowError { offset: usize, detail: String },
}

/// Resolve a relative pointer: base_offset + rel_value = absolute file offset.
pub fn resolve_relptr(base_offset: usize, rel_value: i64) -> usize {
    (base_offset as i64 + rel_value) as usize
}

/// Parse packed string fields: (char_count, padding, text_relptr).
pub fn parse_packed_string_fields(input: &mut &[u8]) -> WResult<(u32, u32, i64)> {
    let char_count = le_u32.parse_next(input)?;
    let padding = le_u32.parse_next(input)?;
    let text_relptr = le_i64.parse_next(input)?;
    Ok((char_count, padding, text_relptr))
}

/// Resolve a packed string from file data given the struct base offset.
///
/// Packed strings are stored as: char_count (u32), padding (u32), text_relptr (i64).
/// The actual string data is at `struct_base + text_relptr`.
pub fn parse_packed_string(
    file_data: &[u8],
    struct_base: usize,
) -> Result<String, Report<ParseError>> {
    let input = &mut &file_data[struct_base..];
    let (char_count, _padding, text_relptr) = parse_packed_string_fields(input).map_err(|e| {
        Report::new(ParseError::WinnowError {
            offset: struct_base,
            detail: format!("{e}"),
        })
    })?;

    if char_count == 0 {
        return Ok(String::new());
    }

    let text_offset = resolve_relptr(struct_base, text_relptr);
    let text_end = text_offset + char_count as usize;
    if text_end > file_data.len() {
        return Err(Report::new(ParseError::PackedStringOutOfBounds {
            offset: text_offset,
            needed: text_end,
            available: file_data.len(),
        }));
    }

    let text_bytes = &file_data[text_offset..text_end];
    let text_bytes = text_bytes.strip_suffix(&[0]).unwrap_or(text_bytes);
    Ok(String::from_utf8_lossy(text_bytes).into_owned())
}

/// Read a null-terminated string from `file_data` starting at `offset`.
pub fn read_null_terminated_string(file_data: &[u8], offset: usize) -> &str {
    let remaining = &file_data[offset..];
    let end = remaining
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(remaining.len());
    std::str::from_utf8(&remaining[..end]).expect("invalid UTF-8 in null-terminated string")
}
