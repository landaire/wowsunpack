use winnow::Parser;
use winnow::binary::{le_i64, le_u8, le_u16, le_u32};
use winnow::combinator::repeat;
use winnow::error::ContextError;
use winnow::token::take;

const ENCD_MAGIC: u32 = 0x44434E45;

type WResult<T> = Result<T, winnow::error::ErrMode<ContextError>>;

#[derive(Debug)]
pub struct MergedGeometry<'a> {
    pub vertices_mapping: Vec<MappingEntry>,
    pub indices_mapping: Vec<MappingEntry>,
    pub merged_vertices: Vec<VerticesPrototype<'a>>,
    pub merged_indices: Vec<IndicesPrototype<'a>>,
    pub collision_models: Vec<ModelPrototype<'a>>,
    pub armor_models: Vec<ModelPrototype<'a>>,
}

#[derive(Debug, Clone)]
pub struct MappingEntry {
    pub mapping_id: u32,
    pub merged_buffer_index: u16,
    pub packed_texel_density: u16,
    pub items_offset: u32,
    pub items_count: u32,
}

#[derive(Debug)]
pub struct VerticesPrototype<'a> {
    pub data: VertexData<'a>,
    pub format_name: String,
    pub size_in_bytes: u32,
    pub stride_in_bytes: u16,
    pub is_skinned: bool,
    pub is_bumped: bool,
}

#[derive(Debug)]
pub struct IndicesPrototype<'a> {
    pub data: IndexData<'a>,
    pub size_in_bytes: u32,
    pub index_size: u16,
}

#[derive(Debug)]
pub struct ModelPrototype<'a> {
    pub data: &'a [u8],
    pub name: String,
    pub size_in_bytes: u32,
}

#[derive(Debug)]
pub enum VertexData<'a> {
    Encoded {
        element_count: u32,
        payload: &'a [u8],
        stride: u16,
    },
    Raw(&'a [u8]),
}

#[derive(Debug)]
pub enum IndexData<'a> {
    Encoded {
        element_count: u32,
        payload: &'a [u8],
        index_size: u16,
    },
    Raw(&'a [u8]),
}

/// Decode a meshoptimizer-encoded vertex buffer with a runtime-known stride.
///
/// meshopt_rs requires `size_of::<Vertex>() == stride`, but our stride is only known at
/// runtime. We dispatch to a monomorphized call for each supported stride value.
fn decode_vertex_buffer_dynamic(
    count: usize,
    stride: usize,
    encoded: &[u8],
) -> eyre::Result<Vec<u8>> {
    eyre::ensure!(
        stride > 0 && stride <= 256 && stride % 4 == 0,
        "invalid vertex stride: {stride}"
    );

    let total_bytes = count * stride;
    let mut output = vec![0u8; total_bytes];

    macro_rules! decode_with_stride {
        ($stride:literal, $count:expr, $encoded:expr, $output:expr) => {{
            #[repr(C, align(4))]
            #[derive(Copy, Clone)]
            struct Vertex([u8; $stride]);
            // Safety: output buffer has exactly count * stride bytes, and Vertex has size = stride.
            // The decode function reads/writes through the slice as raw bytes internally.
            let vertex_slice: &mut [Vertex] = unsafe {
                std::slice::from_raw_parts_mut(
                    $output.as_mut_ptr() as *mut Vertex,
                    $count,
                )
            };
            meshopt_rs::vertex::buffer::decode_vertex_buffer(vertex_slice, $encoded)
                .map_err(|e| eyre::eyre!("meshopt vertex decode error: {e:?}"))?;
        }};
    }

    match stride {
        4 => decode_with_stride!(4, count, encoded, output),
        8 => decode_with_stride!(8, count, encoded, output),
        12 => decode_with_stride!(12, count, encoded, output),
        16 => decode_with_stride!(16, count, encoded, output),
        20 => decode_with_stride!(20, count, encoded, output),
        24 => decode_with_stride!(24, count, encoded, output),
        28 => decode_with_stride!(28, count, encoded, output),
        32 => decode_with_stride!(32, count, encoded, output),
        36 => decode_with_stride!(36, count, encoded, output),
        40 => decode_with_stride!(40, count, encoded, output),
        44 => decode_with_stride!(44, count, encoded, output),
        48 => decode_with_stride!(48, count, encoded, output),
        52 => decode_with_stride!(52, count, encoded, output),
        56 => decode_with_stride!(56, count, encoded, output),
        60 => decode_with_stride!(60, count, encoded, output),
        64 => decode_with_stride!(64, count, encoded, output),
        _ => eyre::bail!("unsupported vertex stride: {stride} (add to dispatch table)"),
    }

    Ok(output)
}

impl VertexData<'_> {
    pub fn decode(&self) -> eyre::Result<Vec<u8>> {
        match self {
            VertexData::Encoded {
                element_count,
                payload,
                stride,
            } => decode_vertex_buffer_dynamic(*element_count as usize, *stride as usize, payload),
            VertexData::Raw(data) => Ok(data.to_vec()),
        }
    }
}

impl IndexData<'_> {
    pub fn decode(&self) -> eyre::Result<Vec<u8>> {
        match self {
            IndexData::Encoded {
                element_count,
                payload,
                index_size,
            } => {
                let count = *element_count as usize;
                let mut output = vec![0u32; count];
                meshopt_rs::index::buffer::decode_index_buffer(&mut output, payload)
                    .map_err(|e| eyre::eyre!("meshopt index decode error: {e:?}"))?;

                match index_size {
                    2 => Ok(output.iter().flat_map(|i| (*i as u16).to_le_bytes()).collect()),
                    4 => Ok(output.iter().flat_map(|i| i.to_le_bytes()).collect()),
                    other => eyre::bail!("unsupported index size: {other}"),
                }
            }
            IndexData::Raw(data) => Ok(data.to_vec()),
        }
    }
}

/// Resolve a relative pointer: base_offset + rel_value = absolute file offset
fn resolve_relptr(base_offset: usize, rel_value: i64) -> usize {
    (base_offset as i64 + rel_value) as usize
}

fn parse_mapping_entry(input: &mut &[u8]) -> WResult<MappingEntry> {
    let mapping_id = le_u32.parse_next(input)?;
    let merged_buffer_index = le_u16.parse_next(input)?;
    let packed_texel_density = le_u16.parse_next(input)?;
    let items_offset = le_u32.parse_next(input)?;
    let items_count = le_u32.parse_next(input)?;
    Ok(MappingEntry {
        mapping_id,
        merged_buffer_index,
        packed_texel_density,
        items_offset,
        items_count,
    })
}

/// Header fields parsed by winnow, before sub-structure resolution.
struct HeaderFields {
    merged_vertices_count: u32,
    merged_indices_count: u32,
    vertices_mapping_count: u32,
    indices_mapping_count: u32,
    collision_model_count: u32,
    armor_model_count: u32,
    vertices_mapping_ptr: i64,
    indices_mapping_ptr: i64,
    merged_vertices_ptr: i64,
    merged_indices_ptr: i64,
    collision_models_ptr: i64,
    armor_models_ptr: i64,
}

fn parse_header(input: &mut &[u8]) -> WResult<HeaderFields> {
    let merged_vertices_count = le_u32.parse_next(input)?;
    let merged_indices_count = le_u32.parse_next(input)?;
    let vertices_mapping_count = le_u32.parse_next(input)?;
    let indices_mapping_count = le_u32.parse_next(input)?;
    let collision_model_count = le_u32.parse_next(input)?;
    let armor_model_count = le_u32.parse_next(input)?;
    let vertices_mapping_ptr = le_i64.parse_next(input)?;
    let indices_mapping_ptr = le_i64.parse_next(input)?;
    let merged_vertices_ptr = le_i64.parse_next(input)?;
    let merged_indices_ptr = le_i64.parse_next(input)?;
    let collision_models_ptr = le_i64.parse_next(input)?;
    let armor_models_ptr = le_i64.parse_next(input)?;
    Ok(HeaderFields {
        merged_vertices_count,
        merged_indices_count,
        vertices_mapping_count,
        indices_mapping_count,
        collision_model_count,
        armor_model_count,
        vertices_mapping_ptr,
        indices_mapping_ptr,
        merged_vertices_ptr,
        merged_indices_ptr,
        collision_models_ptr,
        armor_models_ptr,
    })
}

/// Parse the header and resolve all sub-structures from the full file data.
pub fn parse_geometry(file_data: &[u8]) -> eyre::Result<MergedGeometry<'_>> {
    let input = &mut &file_data[..];

    let hdr = parse_header(input)
        .map_err(|e| eyre::eyre!("failed to parse geometry header: {e}"))?;

    let header_base = 0usize;

    let vm_offset = resolve_relptr(header_base, hdr.vertices_mapping_ptr);
    let vertices_mapping = parse_mapping_array(file_data, vm_offset, hdr.vertices_mapping_count as usize)?;

    let im_offset = resolve_relptr(header_base, hdr.indices_mapping_ptr);
    let indices_mapping = parse_mapping_array(file_data, im_offset, hdr.indices_mapping_count as usize)?;

    let mv_offset = resolve_relptr(header_base, hdr.merged_vertices_ptr);
    let merged_vertices = parse_vertices_array(file_data, mv_offset, hdr.merged_vertices_count as usize)?;

    let mi_offset = resolve_relptr(header_base, hdr.merged_indices_ptr);
    let merged_indices = parse_indices_array(file_data, mi_offset, hdr.merged_indices_count as usize)?;

    let collision_models = if hdr.collision_model_count > 0 {
        let cm_offset = resolve_relptr(header_base, hdr.collision_models_ptr);
        parse_model_array(file_data, cm_offset, hdr.collision_model_count as usize)?
    } else {
        Vec::new()
    };

    let am_offset = resolve_relptr(header_base, hdr.armor_models_ptr);
    let armor_models = parse_model_array(file_data, am_offset, hdr.armor_model_count as usize)?;

    Ok(MergedGeometry {
        vertices_mapping,
        indices_mapping,
        merged_vertices,
        merged_indices,
        collision_models,
        armor_models,
    })
}

fn parse_mapping_array(
    file_data: &[u8],
    offset: usize,
    count: usize,
) -> eyre::Result<Vec<MappingEntry>> {
    let input = &mut &file_data[offset..];
    let entries: Vec<MappingEntry> = repeat(count, parse_mapping_entry)
        .parse_next(input)
        .map_err(|e| eyre::eyre!("failed to parse mapping entries at offset 0x{offset:X}: {e}"))?;
    Ok(entries)
}

fn parse_packed_string(file_data: &[u8], struct_base: usize) -> eyre::Result<String> {
    let input = &mut &file_data[struct_base..];
    let (char_count, _padding, text_relptr) = parse_packed_string_fields(input)
        .map_err(|e| eyre::eyre!("failed to parse packed string at 0x{struct_base:X}: {e}"))?;

    if char_count == 0 {
        return Ok(String::new());
    }

    let text_offset = resolve_relptr(struct_base, text_relptr);
    let text_end = text_offset + char_count as usize;
    eyre::ensure!(
        text_end <= file_data.len(),
        "packed string text extends beyond file: offset=0x{text_offset:X}, count={char_count}, file_len=0x{:X}",
        file_data.len()
    );

    let text_bytes = &file_data[text_offset..text_end];
    let text_bytes = text_bytes.strip_suffix(&[0]).unwrap_or(text_bytes);
    Ok(String::from_utf8_lossy(text_bytes).into_owned())
}

fn parse_packed_string_fields(input: &mut &[u8]) -> WResult<(u32, u32, i64)> {
    let char_count = le_u32.parse_next(input)?;
    let padding = le_u32.parse_next(input)?;
    let text_relptr = le_i64.parse_next(input)?;
    Ok((char_count, padding, text_relptr))
}

fn parse_vertex_data<'a>(
    file_data: &'a [u8],
    data_offset: usize,
    size_in_bytes: u32,
    stride_in_bytes: u16,
) -> eyre::Result<VertexData<'a>> {
    eyre::ensure!(
        data_offset + size_in_bytes as usize <= file_data.len(),
        "vertex data extends beyond file"
    );
    let blob = &file_data[data_offset..data_offset + size_in_bytes as usize];

    if blob.len() >= 8 {
        let magic = u32::from_le_bytes(blob[0..4].try_into().unwrap());
        if magic == ENCD_MAGIC {
            let element_count = u32::from_le_bytes(blob[4..8].try_into().unwrap());
            return Ok(VertexData::Encoded {
                element_count,
                payload: &blob[8..],
                stride: stride_in_bytes,
            });
        }
    }

    Ok(VertexData::Raw(blob))
}

fn parse_index_data<'a>(
    file_data: &'a [u8],
    data_offset: usize,
    size_in_bytes: u32,
    index_size: u16,
) -> eyre::Result<IndexData<'a>> {
    eyre::ensure!(
        data_offset + size_in_bytes as usize <= file_data.len(),
        "index data extends beyond file"
    );
    let blob = &file_data[data_offset..data_offset + size_in_bytes as usize];

    if blob.len() >= 8 {
        let magic = u32::from_le_bytes(blob[0..4].try_into().unwrap());
        if magic == ENCD_MAGIC {
            let element_count = u32::from_le_bytes(blob[4..8].try_into().unwrap());
            return Ok(IndexData::Encoded {
                element_count,
                payload: &blob[8..],
                index_size,
            });
        }
    }

    Ok(IndexData::Raw(blob))
}

/// Parse the struct fields of a VerticesPrototype (0x20 bytes).
/// Returns (data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped).
fn parse_vertices_fields(input: &mut &[u8]) -> WResult<(i64, u32, u16, bool, bool)> {
    let data_relptr = le_i64.parse_next(input)?;
    let _packed_string: &[u8] = take(16usize).parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let stride_in_bytes = le_u16.parse_next(input)?;
    let is_skinned = le_u8.parse_next(input)? != 0;
    let is_bumped = le_u8.parse_next(input)? != 0;
    Ok((data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped))
}

fn parse_vertices_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> eyre::Result<Vec<VerticesPrototype<'a>>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x20;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes, stride_in_bytes, is_skinned, is_bumped) =
            parse_vertices_fields(input)
                .map_err(|e| eyre::eyre!("vertices[{i}] parse error: {e}"))?;

        let packed_string_base = struct_base + 0x08;
        let format_name = parse_packed_string(file_data, packed_string_base)?;
        let data_offset = resolve_relptr(struct_base, data_relptr);
        let data = parse_vertex_data(file_data, data_offset, size_in_bytes, stride_in_bytes)?;

        result.push(VerticesPrototype {
            data,
            format_name,
            size_in_bytes,
            stride_in_bytes,
            is_skinned,
            is_bumped,
        });
    }

    Ok(result)
}

/// Parse the struct fields of an IndicesPrototype (0x10 bytes).
/// Returns (data_relptr, size_in_bytes, index_size).
fn parse_indices_fields(input: &mut &[u8]) -> WResult<(i64, u32, u16)> {
    let data_relptr = le_i64.parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let _reserved = le_u16.parse_next(input)?;
    let index_size = le_u16.parse_next(input)?;
    Ok((data_relptr, size_in_bytes, index_size))
}

fn parse_indices_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> eyre::Result<Vec<IndicesPrototype<'a>>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x10;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes, index_size) = parse_indices_fields(input)
            .map_err(|e| eyre::eyre!("indices[{i}] parse error: {e}"))?;

        let data_offset = resolve_relptr(struct_base, data_relptr);
        let data = parse_index_data(file_data, data_offset, size_in_bytes, index_size)?;

        result.push(IndicesPrototype {
            data,
            size_in_bytes,
            index_size,
        });
    }

    Ok(result)
}

/// Parse the struct fields of a ModelPrototype (0x20 bytes: armor or collision).
/// Returns (data_relptr, size_in_bytes).
fn parse_model_fields(input: &mut &[u8]) -> WResult<(i64, u32)> {
    let data_relptr = le_i64.parse_next(input)?;
    let _packed_string: &[u8] = take(16usize).parse_next(input)?;
    let size_in_bytes = le_u32.parse_next(input)?;
    let _padding = le_u32.parse_next(input)?;
    Ok((data_relptr, size_in_bytes))
}

fn parse_model_array<'a>(
    file_data: &'a [u8],
    offset: usize,
    count: usize,
) -> eyre::Result<Vec<ModelPrototype<'a>>> {
    let mut result = Vec::with_capacity(count);

    for i in 0..count {
        let struct_base = offset + i * 0x20;
        let input = &mut &file_data[struct_base..];

        let (data_relptr, size_in_bytes) = parse_model_fields(input)
            .map_err(|e| eyre::eyre!("model[{i}] parse error: {e}"))?;

        let packed_string_base = struct_base + 0x08;
        let name = parse_packed_string(file_data, packed_string_base)?;
        let data_offset = resolve_relptr(struct_base, data_relptr);

        eyre::ensure!(
            data_offset + size_in_bytes as usize <= file_data.len(),
            "model[{i}] data extends beyond file"
        );
        let data = &file_data[data_offset..data_offset + size_in_bytes as usize];

        result.push(ModelPrototype {
            data,
            name,
            size_in_bytes,
        });
    }

    Ok(result)
}
