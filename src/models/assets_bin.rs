use rootcause::Report;
use thiserror::Error;
use winnow::Parser;
use winnow::binary::{le_i64, le_u16, le_u32};

use crate::data::parser_utils::{WResult, parse_packed_string_fields, resolve_relptr};

const BWDB_MAGIC: u32 = 0x42574442;
const BWDB_VERSION: u32 = 0x01010000;

/// Errors that can occur during assets.bin (PrototypeDatabase) parsing.
#[derive(Debug, Error)]
pub enum AssetsBinError {
    #[error("invalid magic: 0x{actual:08X} (expected 0x{expected:08X})")]
    InvalidMagic { actual: u32, expected: u32 },
    #[error("unsupported version: 0x{actual:08X} (expected 0x{expected:08X})")]
    UnsupportedVersion { actual: u32, expected: u32 },
    #[error("data extends beyond file at 0x{offset:X}")]
    OutOfBounds { offset: usize },
    #[error("parse error: {0}")]
    ParseError(String),
}

/// The top-level parsed assets.bin (PrototypeDatabase) file.
#[derive(Debug)]
pub struct PrototypeDatabase<'a> {
    pub header: Header,
    pub strings: StringsSection<'a>,
    pub resource_to_prototype_map: HashmapSection<'a>,
    pub paths_storage: Vec<PathEntry>,
    pub databases: Vec<DatabaseEntry<'a>>,
}

#[derive(Debug)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub checksum: u32,
    pub architecture: u16,
    pub endianness: u16,
}

/// The strings section: an offsetsMap hashmap + a raw string data pool.
#[derive(Debug)]
pub struct StringsSection<'a> {
    pub offsets_map: HashmapSection<'a>,
    pub string_data: &'a [u8],
}

/// A generic hashmap section with bucket and value arrays.
#[derive(Debug)]
pub struct HashmapSection<'a> {
    pub capacity: u32,
    pub buckets: &'a [u8],
    pub values: &'a [u8],
    /// Size of each bucket entry in bytes.
    pub bucket_stride: usize,
    /// Size of each value entry in bytes.
    pub value_stride: usize,
}

/// A path entry from the pathsStorage array.
#[derive(Debug)]
pub struct PathEntry {
    pub self_id: u64,
    pub parent_id: u64,
    pub name: String,
}

/// A database entry describing a prototype data blob.
#[derive(Debug)]
pub struct DatabaseEntry<'a> {
    pub prototype_magic: u32,
    pub prototype_checksum: u32,
    pub size: u32,
    pub data: &'a [u8],
}

impl StringsSection<'_> {
    /// Look up a string by its offset into the string data pool.
    pub fn get_string(&self, offset: u32) -> Option<&str> {
        let start = offset as usize;
        if start >= self.string_data.len() {
            return None;
        }
        let remaining = &self.string_data[start..];
        let end = remaining
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(remaining.len());
        std::str::from_utf8(&remaining[..end]).ok()
    }
}

fn parse_header_fields(input: &mut &[u8]) -> WResult<Header> {
    let magic = le_u32.parse_next(input)?;
    let version = le_u32.parse_next(input)?;
    let checksum = le_u32.parse_next(input)?;
    let architecture = le_u16.parse_next(input)?;
    let endianness = le_u16.parse_next(input)?;
    Ok(Header {
        magic,
        version,
        checksum,
        architecture,
        endianness,
    })
}

/// Raw body header fields before resolution.
struct BodyHeaderFields {
    // strings/offsetsMap
    offsets_map_capacity: u32,
    offsets_map_buckets_relptr: i64,
    offsets_map_values_relptr: i64,
    string_data_size: u32,
    string_data_relptr: i64,
    // resourceToPrototypeMap
    r2p_capacity: u32,
    r2p_buckets_relptr: i64,
    r2p_values_relptr: i64,
    // pathsStorage
    paths_count: u32,
    paths_data_relptr: i64,
    // databases
    databases_count: u32,
    databases_relptr: i64,
}

fn parse_body_header(input: &mut &[u8]) -> WResult<BodyHeaderFields> {
    // strings section (0x28 bytes)
    let offsets_map_capacity = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let offsets_map_buckets_relptr = le_i64.parse_next(input)?;
    let offsets_map_values_relptr = le_i64.parse_next(input)?;
    let string_data_size = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let string_data_relptr = le_i64.parse_next(input)?;

    // resourceToPrototypeMap section (0x18 bytes)
    let r2p_capacity = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let r2p_buckets_relptr = le_i64.parse_next(input)?;
    let r2p_values_relptr = le_i64.parse_next(input)?;

    // pathsStorage section (0x10 bytes)
    let paths_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let paths_data_relptr = le_i64.parse_next(input)?;

    // databases section (0x10 bytes)
    let databases_count = le_u32.parse_next(input)?;
    let _pad = le_u32.parse_next(input)?;
    let databases_relptr = le_i64.parse_next(input)?;

    Ok(BodyHeaderFields {
        offsets_map_capacity,
        offsets_map_buckets_relptr,
        offsets_map_values_relptr,
        string_data_size,
        string_data_relptr,
        r2p_capacity,
        r2p_buckets_relptr,
        r2p_values_relptr,
        paths_count,
        paths_data_relptr,
        databases_count,
        databases_relptr,
    })
}

fn resolve_hashmap<'a>(
    file_data: &'a [u8],
    base: usize,
    capacity: u32,
    buckets_relptr: i64,
    values_relptr: i64,
    bucket_stride: usize,
    value_stride: usize,
) -> Result<HashmapSection<'a>, Report<AssetsBinError>> {
    let cap = capacity as usize;

    let buckets_offset = resolve_relptr(base, buckets_relptr);
    let buckets_end = buckets_offset + cap * bucket_stride;
    if buckets_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds {
            offset: buckets_offset,
        }));
    }
    let buckets = &file_data[buckets_offset..buckets_end];

    let values_offset = resolve_relptr(base, values_relptr);
    let values_end = values_offset + cap * value_stride;
    if values_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds {
            offset: values_offset,
        }));
    }
    let values = &file_data[values_offset..values_end];

    Ok(HashmapSection {
        capacity,
        buckets,
        values,
        bucket_stride,
        value_stride,
    })
}

/// Parse path entry fields: (selfId, parentId)
fn parse_path_entry_ids(input: &mut &[u8]) -> WResult<(u64, u64)> {
    let self_id = winnow::binary::le_u64.parse_next(input)?;
    let parent_id = winnow::binary::le_u64.parse_next(input)?;
    Ok((self_id, parent_id))
}

fn parse_path_entries(
    file_data: &[u8],
    data_offset: usize,
    count: usize,
) -> Result<Vec<PathEntry>, Report<AssetsBinError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let entry_base = data_offset + i * 32;
        let input = &mut &file_data[entry_base..];

        let (self_id, parent_id) = parse_path_entry_ids(input)
            .map_err(|e| Report::new(AssetsBinError::ParseError(format!("pathEntry[{i}]: {e}"))))?;

        // name is a packed string at entry_base + 0x10
        let name_base = entry_base + 0x10;
        let name_input = &mut &file_data[name_base..];
        let (name_size, _pad, name_relptr) =
            parse_packed_string_fields(name_input).map_err(|e| {
                Report::new(AssetsBinError::ParseError(format!(
                    "pathEntry[{i}] name: {e}"
                )))
            })?;

        let name = if name_size > 0 {
            let name_data_offset = resolve_relptr(name_base, name_relptr);
            let name_end = name_data_offset + name_size as usize;
            if name_end > file_data.len() {
                return Err(Report::new(AssetsBinError::OutOfBounds {
                    offset: name_data_offset,
                }));
            }
            let name_bytes = &file_data[name_data_offset..name_end];
            let name_bytes = name_bytes.strip_suffix(&[0]).unwrap_or(name_bytes);
            String::from_utf8_lossy(name_bytes).into_owned()
        } else {
            String::new()
        };

        result.push(PathEntry {
            self_id,
            parent_id,
            name,
        });
    }

    Ok(result)
}

/// Parse database entry fields: (magic, checksum, size, pad, data_relptr)
fn parse_database_entry_fields(input: &mut &[u8]) -> WResult<(u32, u32, u32, u32, i64)> {
    let magic = le_u32.parse_next(input)?;
    let checksum = le_u32.parse_next(input)?;
    let size = le_u32.parse_next(input)?;
    let pad = le_u32.parse_next(input)?;
    let relptr = le_i64.parse_next(input)?;
    Ok((magic, checksum, size, pad, relptr))
}

fn parse_database_entries<'a>(
    file_data: &'a [u8],
    entries_offset: usize,
    count: usize,
) -> Result<Vec<DatabaseEntry<'a>>, Report<AssetsBinError>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let entry_base = entries_offset + i * 0x18;
        let input = &mut &file_data[entry_base..];

        let (prototype_magic, prototype_checksum, size, _pad, data_relptr) =
            parse_database_entry_fields(input).map_err(|e| {
                Report::new(AssetsBinError::ParseError(format!("database[{i}]: {e}")))
            })?;

        let data = if size > 0 {
            let data_offset = resolve_relptr(entry_base, data_relptr);
            let data_end = data_offset + size as usize;
            if data_end > file_data.len() {
                return Err(Report::new(AssetsBinError::OutOfBounds {
                    offset: data_offset,
                }));
            }
            &file_data[data_offset..data_end]
        } else {
            &[]
        };

        result.push(DatabaseEntry {
            prototype_magic,
            prototype_checksum,
            size,
            data,
        });
    }

    Ok(result)
}

/// Parse an assets.bin file into a PrototypeDatabase.
pub fn parse_assets_bin(file_data: &[u8]) -> Result<PrototypeDatabase<'_>, Report<AssetsBinError>> {
    let input = &mut &file_data[..];

    let header = parse_header_fields(input)
        .map_err(|e| Report::new(AssetsBinError::ParseError(format!("header: {e}"))))?;

    if header.magic != BWDB_MAGIC {
        return Err(Report::new(AssetsBinError::InvalidMagic {
            actual: header.magic,
            expected: BWDB_MAGIC,
        }));
    }
    if header.version != BWDB_VERSION {
        return Err(Report::new(AssetsBinError::UnsupportedVersion {
            actual: header.version,
            expected: BWDB_VERSION,
        }));
    }

    let body_base = 0x10; // header is 16 bytes

    let body = parse_body_header(&mut &file_data[body_base..])
        .map_err(|e| Report::new(AssetsBinError::ParseError(format!("body header: {e}"))))?;

    // strings section: base = body_base (0x10)
    let strings_base = body_base;
    let offsets_map = resolve_hashmap(
        file_data,
        strings_base,
        body.offsets_map_capacity,
        body.offsets_map_buckets_relptr,
        body.offsets_map_values_relptr,
        8, // u64 buckets
        4, // u32 values
    )?;

    let string_data_offset = resolve_relptr(strings_base, body.string_data_relptr);
    let string_data_end = string_data_offset + body.string_data_size as usize;
    if string_data_end > file_data.len() {
        return Err(Report::new(AssetsBinError::OutOfBounds {
            offset: string_data_offset,
        }));
    }
    let string_data = &file_data[string_data_offset..string_data_end];

    let strings = StringsSection {
        offsets_map,
        string_data,
    };

    // resourceToPrototypeMap: base = body_base + 0x28
    let r2p_base = body_base + 0x28;
    let resource_to_prototype_map = resolve_hashmap(
        file_data,
        r2p_base,
        body.r2p_capacity,
        body.r2p_buckets_relptr,
        body.r2p_values_relptr,
        16, // u128 buckets (key_hash + metadata)
        4,  // u32 values
    )?;

    // pathsStorage: base = body_base + 0x40
    let paths_base = body_base + 0x40;
    let paths_data_offset = resolve_relptr(paths_base, body.paths_data_relptr);
    let paths_storage =
        parse_path_entries(file_data, paths_data_offset, body.paths_count as usize)?;

    // databases: relptr relative to body_base
    let db_entries_offset = resolve_relptr(body_base, body.databases_relptr);
    let databases =
        parse_database_entries(file_data, db_entries_offset, body.databases_count as usize)?;

    Ok(PrototypeDatabase {
        header,
        strings,
        resource_to_prototype_map,
        paths_storage,
        databases,
    })
}
