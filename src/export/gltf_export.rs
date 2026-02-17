//! Export ship visual + geometry to glTF/GLB format.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use gltf_json as json;
use json::validation::Checked::Valid;
use json::validation::USize64;
use rootcause::Report;
use thiserror::Error;

use crate::models::assets_bin::PrototypeDatabase;
use crate::models::geometry::MergedGeometry;
use crate::models::vertex_format::{self, AttributeSemantic, VertexFormat};
use crate::models::visual::VisualPrototype;

#[derive(Debug, Error)]
pub enum ExportError {
    #[error("no LOD {0} in visual (max LOD: {1})")]
    LodOutOfRange(usize, usize),
    #[error("no .visual files found in directory: {0}")]
    NoVisualFiles(String),
    #[error("render set name 0x{0:08X} not found among render sets")]
    RenderSetNotFound(u32),
    #[error("vertices mapping id 0x{id:08X} not found in geometry")]
    VerticesMappingNotFound { id: u32 },
    #[error("indices mapping id 0x{id:08X} not found in geometry")]
    IndicesMappingNotFound { id: u32 },
    #[error("buffer index {index} out of range (count: {count})")]
    BufferIndexOutOfRange { index: usize, count: usize },
    #[error("vertex decode error: {0}")]
    VertexDecode(String),
    #[error("index decode error: {0}")]
    IndexDecode(String),
    #[error(
        "vertex format stride mismatch: format says {format_stride}, geometry says {geo_stride}"
    )]
    StrideMismatch {
        format_stride: usize,
        geo_stride: usize,
    },
    #[error("glTF serialization error: {0}")]
    Serialize(String),
    #[error("I/O error: {0}")]
    Io(String),
}

/// Decoded primitive data ready for glTF export.
struct DecodedPrimitive {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
    material_name: String,
    /// MFM stem for texture lookup (e.g. "JSB039_Yamato_1945_Hull").
    mfm_stem: Option<String>,
}

/// Export a visual + geometry pair to a GLB binary and write it.
///
/// `texture_set` contains base albedo PNGs and optional camouflage variant PNGs.
/// Primitives whose MFM stem matches a key will have the texture applied as
/// `baseColorTexture` on their material. Camo variants are exposed via
/// `KHR_materials_variants` so users can switch in Blender.
pub fn export_glb(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: usize,
    texture_set: &TextureSet,
    damaged: bool,
    writer: &mut impl Write,
) -> Result<(), Report<ExportError>> {
    if visual.lods.is_empty() {
        return Err(Report::new(ExportError::LodOutOfRange(lod, 0)));
    }
    if lod >= visual.lods.len() {
        return Err(Report::new(ExportError::LodOutOfRange(
            lod,
            visual.lods.len() - 1,
        )));
    }

    let lod_entry = &visual.lods[lod];

    // Collect render sets for this LOD by matching LOD render_set_names to RS name_ids.
    let primitives = collect_primitives(visual, geometry, db, lod_entry, damaged)?;

    if primitives.is_empty() {
        eprintln!("Warning: no primitives found for LOD {lod}");
    }

    // Build glTF document.
    let mut root = json::Root::default();
    root.asset = json::Asset {
        version: "2.0".to_string(),
        generator: Some("wowsunpack".to_string()),
        ..Default::default()
    };

    // Accumulate all binary data into a single buffer.
    let mut bin_data: Vec<u8> = Vec::new();
    let mut gltf_primitives = Vec::new();
    let mut mat_cache = MaterialCache::new();

    for prim in &primitives {
        let gltf_prim =
            add_primitive_to_root(&mut root, &mut bin_data, prim, texture_set, &mut mat_cache)?;
        gltf_primitives.push(gltf_prim);
    }

    // Pad binary data to 4-byte alignment.
    while bin_data.len() % 4 != 0 {
        bin_data.push(0);
    }

    // Set the buffer byte_length now that we know the total size.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        // Update all buffer views to reference this buffer.
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    // Create mesh with all primitives.
    let mesh = root.push(json::Mesh {
        primitives: gltf_primitives,
        weights: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    // Create a simple node hierarchy.
    // For now, use a single root node with the mesh attached.
    let root_node = root.push(json::Node {
        mesh: Some(mesh),
        ..Default::default()
    });

    let scene = root.push(json::Scene {
        nodes: vec![root_node],
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    // Add KHR_materials_variants root extension if we have camo schemes.
    add_variants_extension(&mut root, texture_set);

    // Serialize and write GLB.
    let json_string = json::serialize::to_string(&root)
        .map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0, // to_writer computes this
        },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() {
            None
        } else {
            Some(Cow::Owned(bin_data))
        },
    };

    glb.to_writer(writer)
        .map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}

/// Render set name substrings to exclude for intact-state export.
///
/// BigWorld ship visuals contain both intact and damaged geometry in the same
/// file. Crack geometry (`_crack_`) shows jagged fracture edges for the damaged
/// state, while patch geometry (`_patch_`) covers those seams when intact.
/// The `_hide` geometry is context-dependent and hidden by default.
const INTACT_EXCLUDE: &[&str] = &["_crack_", "_hide"];

/// Render set name substrings to exclude for damaged-state export.
///
/// In the damaged state, patch geometry is hidden and crack geometry is shown.
const DAMAGED_EXCLUDE: &[&str] = &["_patch_", "_hide"];

/// Collect and decode all render set primitives for a given LOD.
///
/// When `damaged` is false, crack and hide geometry is excluded (intact hull).
/// When `damaged` is true, patch and hide geometry is excluded (destroyed look).
fn collect_primitives(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: &crate::models::visual::Lod,
    damaged: bool,
) -> Result<Vec<DecodedPrimitive>, Report<ExportError>> {
    let mut result = Vec::new();
    let exclude = if damaged {
        DAMAGED_EXCLUDE
    } else {
        INTACT_EXCLUDE
    };

    for &rs_name_id in &lod.render_set_names {
        // Find the render set with this name_id.
        let rs = visual
            .render_sets
            .iter()
            .find(|rs| rs.name_id == rs_name_id)
            .ok_or_else(|| Report::new(ExportError::RenderSetNotFound(rs_name_id)))?;

        // Skip render sets based on damage state.
        if let Some(rs_name) = db.strings.get_string_by_id(rs_name_id) {
            if exclude.iter().any(|sub| rs_name.contains(sub)) {
                continue;
            }
        }

        // Decode unknown_u64 → (vertices_mapping_id, indices_mapping_id)
        let vertices_mapping_id = (rs.unknown_u64 & 0xFFFFFFFF) as u32;
        let indices_mapping_id = (rs.unknown_u64 >> 32) as u32;

        // Find mapping entries.
        let vert_mapping = geometry
            .vertices_mapping
            .iter()
            .find(|m| m.mapping_id == vertices_mapping_id)
            .ok_or_else(|| {
                Report::new(ExportError::VerticesMappingNotFound {
                    id: vertices_mapping_id,
                })
            })?;

        let idx_mapping = geometry
            .indices_mapping
            .iter()
            .find(|m| m.mapping_id == indices_mapping_id)
            .ok_or_else(|| {
                Report::new(ExportError::IndicesMappingNotFound {
                    id: indices_mapping_id,
                })
            })?;

        // Get vertex buffer.
        let vbuf_idx = vert_mapping.merged_buffer_index as usize;
        if vbuf_idx >= geometry.merged_vertices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: vbuf_idx,
                count: geometry.merged_vertices.len(),
            }));
        }
        let vert_proto = &geometry.merged_vertices[vbuf_idx];

        // Get index buffer.
        let ibuf_idx = idx_mapping.merged_buffer_index as usize;
        if ibuf_idx >= geometry.merged_indices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: ibuf_idx,
                count: geometry.merged_indices.len(),
            }));
        }
        let idx_proto = &geometry.merged_indices[ibuf_idx];

        // Decode buffers.
        let decoded_vertices = vert_proto
            .data
            .decode()
            .map_err(|e| Report::new(ExportError::VertexDecode(format!("{e:?}"))))?;
        let decoded_indices = idx_proto
            .data
            .decode()
            .map_err(|e| Report::new(ExportError::IndexDecode(format!("{e:?}"))))?;

        // Parse vertex format.
        let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
        let stride = vert_proto.stride_in_bytes as usize;

        if format.stride != stride {
            // The parsed format stride doesn't match the geometry's stride.
            // This can happen for formats we don't fully parse. Use the
            // geometry stride and just extract what we can.
            eprintln!(
                "Warning: format \"{}\" parsed stride {} != geometry stride {}; using geometry stride",
                vert_proto.format_name, format.stride, stride
            );
        }

        // Extract vertex slice.
        let vert_offset = vert_mapping.items_offset as usize;
        let vert_count = vert_mapping.items_count as usize;
        let vert_start = vert_offset * stride;
        let vert_end = vert_start + vert_count * stride;

        if vert_end > decoded_vertices.len() {
            return Err(Report::new(ExportError::VertexDecode(format!(
                "vertex range {}..{} exceeds buffer size {}",
                vert_start,
                vert_end,
                decoded_vertices.len()
            ))));
        }
        let vert_slice = &decoded_vertices[vert_start..vert_end];

        // Extract index slice.
        let idx_offset = idx_mapping.items_offset as usize;
        let idx_count = idx_mapping.items_count as usize;
        let index_size = idx_proto.index_size as usize;
        let idx_start = idx_offset * index_size;
        let idx_end = idx_start + idx_count * index_size;

        if idx_end > decoded_indices.len() {
            return Err(Report::new(ExportError::IndexDecode(format!(
                "index range {}..{} exceeds buffer size {}",
                idx_start,
                idx_end,
                decoded_indices.len()
            ))));
        }
        let idx_slice = &decoded_indices[idx_start..idx_end];

        // Parse indices as u32.
        let indices: Vec<u32> = match index_size {
            2 => idx_slice
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]) as u32)
                .collect(),
            4 => idx_slice
                .chunks_exact(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
                .collect(),
            _ => {
                return Err(Report::new(ExportError::IndexDecode(format!(
                    "unsupported index size: {index_size}"
                ))));
            }
        };

        // Indices are already 0-based relative to the vertex slice
        // (items_offset is applied when extracting the vertex slice).

        // Unpack vertex attributes.
        let (positions, normals, uvs) = unpack_vertices(vert_slice, stride, &format);

        // Material name for this render set.
        let material_name = db
            .strings
            .get_string_by_id(rs.material_name_id)
            .unwrap_or("<unknown>")
            .to_string();

        // Resolve MFM stem for texture lookup.
        let self_id_index = db.build_self_id_index();
        let mfm_stem = if rs.material_mfm_path_id != 0 {
            self_id_index.get(&rs.material_mfm_path_id).map(|&idx| {
                db.paths_storage[idx]
                    .name
                    .strip_suffix(".mfm")
                    .unwrap_or(&db.paths_storage[idx].name)
                    .to_string()
            })
        } else {
            None
        };

        result.push(DecodedPrimitive {
            positions,
            normals,
            uvs,
            indices,
            material_name,
            mfm_stem,
        });
    }

    Ok(result)
}

/// Unpack vertex data into separate position, normal, and UV arrays.
fn unpack_vertices(
    data: &[u8],
    stride: usize,
    format: &VertexFormat,
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>) {
    let count = data.len() / stride;
    let mut positions = Vec::with_capacity(count);
    let mut normals = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);

    // Find attribute offsets.
    let pos_attr = format
        .attributes
        .iter()
        .find(|a| a.semantic == AttributeSemantic::Position);
    let norm_attr = format
        .attributes
        .iter()
        .find(|a| a.semantic == AttributeSemantic::Normal);
    let uv_attr = format
        .attributes
        .iter()
        .find(|a| a.semantic == AttributeSemantic::TexCoord0);

    for i in 0..count {
        let base = i * stride;

        // Position: 3 x f32
        if let Some(attr) = pos_attr {
            let off = base + attr.offset;
            let x = f32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            let y = f32::from_le_bytes(data[off + 4..off + 8].try_into().unwrap());
            let z = f32::from_le_bytes(data[off + 8..off + 12].try_into().unwrap());
            positions.push([x, y, z]);
        }

        // Normal: packed 4 bytes
        if let Some(attr) = norm_attr {
            let off = base + attr.offset;
            let packed = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            normals.push(vertex_format::unpack_normal(packed));
        }

        // UV: packed 4 bytes (2 x float16)
        if let Some(attr) = uv_attr {
            let off = base + attr.offset;
            let packed = u32::from_le_bytes(data[off..off + 4].try_into().unwrap());
            uvs.push(vertex_format::unpack_uv(packed));
        }
    }

    (positions, normals, uvs)
}

/// All texture data for a ship export: base albedo + camouflage variants.
pub struct TextureSet {
    /// Base albedo PNGs keyed by MFM stem — the default ship appearance.
    pub base: HashMap<String, Vec<u8>>,
    /// Camouflage variant PNGs: scheme name → (MFM stem → PNG bytes).
    /// Only stems that have a texture for this scheme are included.
    pub camo_schemes: Vec<(String, HashMap<String, Vec<u8>>)>,
    /// UV scale/offset for tiled camo schemes. Key = `(scheme_index, mfm_stem)`.
    /// Only present for tiled camos; non-tiled camos use default UVs.
    pub tiled_uv_transforms: HashMap<(usize, String), [f32; 4]>,
}

impl TextureSet {
    pub fn empty() -> Self {
        Self {
            base: HashMap::new(),
            camo_schemes: Vec::new(),
            tiled_uv_transforms: HashMap::new(),
        }
    }
}

/// Cached material info for a given MFM stem / material name.
struct CachedMaterial {
    /// Default material index (base albedo or untextured).
    default_mat: json::Index<json::Material>,
    /// Variant material indices, one per camo scheme (same order as TextureSet::camo_schemes).
    variant_mats: Vec<Option<json::Index<json::Material>>>,
}

/// Cache for deduplicating materials and textures across primitives.
struct MaterialCache {
    /// Maps cache key (MFM stem or material name) to cached material info.
    materials: HashMap<String, CachedMaterial>,
}

impl MaterialCache {
    fn new() -> Self {
        Self {
            materials: HashMap::new(),
        }
    }
}

/// Embed a PNG image in the glTF binary buffer and create a textured material.
/// Returns the material index.
///
/// `uv_transform` is an optional `[scale_x, scale_y, offset_x, offset_y]` applied
/// via `KHR_texture_transform` for tiled camouflage textures.
fn create_textured_material(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    png_bytes: &[u8],
    material_name: &str,
    image_name: Option<String>,
    uv_transform: Option<[f32; 4]>,
) -> json::Index<json::Material> {
    let byte_offset = bin_data.len();
    bin_data.extend_from_slice(png_bytes);
    pad_to_4(bin_data);
    let byte_length = png_bytes.len();

    let bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: None,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let image = root.push(json::Image {
        buffer_view: Some(bv),
        mime_type: Some(json::image::MimeType("image/png".to_string())),
        uri: None,
        name: image_name,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let sampler = root.push(json::texture::Sampler {
        mag_filter: Some(Valid(json::texture::MagFilter::Linear)),
        min_filter: Some(Valid(json::texture::MinFilter::LinearMipmapLinear)),
        wrap_s: Valid(json::texture::WrappingMode::Repeat),
        wrap_t: Valid(json::texture::WrappingMode::Repeat),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let texture = root.push(json::Texture {
        source: image,
        sampler: Some(sampler),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let tex_transform_ext = uv_transform.map(|t| json::extensions::texture::Info {
        texture_transform: Some(json::extensions::texture::TextureTransform {
            scale: json::extensions::texture::TextureTransformScale(t[0..2].try_into().unwrap()),
            offset: json::extensions::texture::TextureTransformOffset(t[2..4].try_into().unwrap()),
            rotation: Default::default(),
            tex_coord: Some(0),
            extras: Default::default(),
        }),
        ..Default::default()
    });

    let texture_info = json::texture::Info {
        index: texture,
        tex_coord: 0,
        extensions: tex_transform_ext,
        extras: Default::default(),
    };

    root.push(json::Material {
        name: Some(material_name.to_string()),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_texture: Some(texture_info),
            ..Default::default()
        },
        ..Default::default()
    })
}

/// Create an untextured material.
fn create_untextured_material(
    root: &mut json::Root,
    material_name: &str,
) -> json::Index<json::Material> {
    root.push(json::Material {
        name: Some(material_name.to_string()),
        ..Default::default()
    })
}

/// Add a decoded primitive's data to the glTF root and binary buffer.
/// Returns the glTF Primitive JSON object.
fn add_primitive_to_root(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    prim: &DecodedPrimitive,
    texture_set: &TextureSet,
    mat_cache: &mut MaterialCache,
) -> Result<json::mesh::Primitive, Report<ExportError>> {
    let mut attributes = BTreeMap::new();

    // --- Positions ---
    let pos_accessor = if !prim.positions.is_empty() {
        let (min, max) = bounding_coords(&prim.positions);
        let byte_offset = bin_data.len();
        for pos in &prim.positions {
            bin_data.extend_from_slice(&pos[0].to_le_bytes());
            bin_data.extend_from_slice(&pos[1].to_le_bytes());
            bin_data.extend_from_slice(&pos[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.positions.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            type_: Valid(json::accessor::Type::Vec3),
            min: Some(json::Value::from(min.to_vec())),
            max: Some(json::Value::from(max.to_vec())),
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Normals ---
    let norm_accessor = if !prim.normals.is_empty() {
        let byte_offset = bin_data.len();
        for n in &prim.normals {
            bin_data.extend_from_slice(&n[0].to_le_bytes());
            bin_data.extend_from_slice(&n[1].to_le_bytes());
            bin_data.extend_from_slice(&n[2].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.normals.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            type_: Valid(json::accessor::Type::Vec3),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- UVs ---
    let uv_accessor = if !prim.uvs.is_empty() {
        let byte_offset = bin_data.len();
        for uv in &prim.uvs {
            bin_data.extend_from_slice(&uv[0].to_le_bytes());
            bin_data.extend_from_slice(&uv[1].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.uvs.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            type_: Valid(json::accessor::Type::Vec2),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // --- Indices ---
    let indices_accessor = if !prim.indices.is_empty() {
        let byte_offset = bin_data.len();
        for &idx in &prim.indices {
            bin_data.extend_from_slice(&idx.to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        Some(root.push(json::Accessor {
            buffer_view: Some(bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(prim.indices.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::U32,
            )),
            type_: Valid(json::accessor::Type::Scalar),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        }))
    } else {
        None
    };

    // Build attribute map.
    if let Some(pos) = pos_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Positions), pos);
    }
    if let Some(norm) = norm_accessor {
        attributes.insert(Valid(json::mesh::Semantic::Normals), norm);
    }
    if let Some(uv) = uv_accessor {
        attributes.insert(Valid(json::mesh::Semantic::TexCoords(0)), uv);
    }

    // Determine cache key: prefer MFM stem, fall back to material name.
    let cache_key = prim
        .mfm_stem
        .clone()
        .unwrap_or_else(|| prim.material_name.clone());

    if !mat_cache.materials.contains_key(&cache_key) {
        // Create the default material (base albedo or untextured).
        let default_mat = if let Some(png_bytes) = prim
            .mfm_stem
            .as_ref()
            .and_then(|stem| texture_set.base.get(stem))
        {
            create_textured_material(
                root,
                bin_data,
                png_bytes,
                &prim.material_name,
                prim.mfm_stem.clone(),
                None,
            )
        } else {
            create_untextured_material(root, &prim.material_name)
        };

        // Create variant materials for each camo scheme.
        let variant_mats: Vec<Option<json::Index<json::Material>>> = texture_set
            .camo_schemes
            .iter()
            .enumerate()
            .map(|(scheme_idx, (scheme_name, scheme_textures))| {
                prim.mfm_stem.as_ref().and_then(|stem| {
                    let png_bytes = scheme_textures.get(stem)?;
                    let uv_xform = texture_set
                        .tiled_uv_transforms
                        .get(&(scheme_idx, stem.clone()))
                        .copied();
                    Some(create_textured_material(
                        root,
                        bin_data,
                        png_bytes,
                        &format!("{} [{}]", prim.material_name, scheme_name),
                        Some(format!("{stem}_{scheme_name}")),
                        uv_xform,
                    ))
                })
            })
            .collect();

        mat_cache.materials.insert(
            cache_key.clone(),
            CachedMaterial {
                default_mat,
                variant_mats,
            },
        );
    }

    let cached = &mat_cache.materials[&cache_key];

    // Build KHR_materials_variants mappings for this primitive.
    let prim_variants_ext = if !texture_set.camo_schemes.is_empty() {
        let mut mappings = Vec::new();
        for (variant_idx, variant_mat) in cached.variant_mats.iter().enumerate() {
            // Use the variant material if this stem has a camo texture for this scheme,
            // otherwise fall back to the default material.
            let mat_index = variant_mat.unwrap_or(cached.default_mat);
            mappings.push(json::extensions::mesh::Mapping {
                material: mat_index.value() as u32,
                variants: vec![variant_idx as u32],
            });
        }
        Some(json::extensions::mesh::KhrMaterialsVariants { mappings })
    } else {
        None
    };

    Ok(json::mesh::Primitive {
        attributes,
        indices: indices_accessor,
        material: Some(cached.default_mat),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: Some(json::extensions::mesh::Primitive {
            khr_materials_variants: prim_variants_ext,
        }),
        extras: Default::default(),
    })
}

/// Add an armor mesh primitive (positions + normals, untextured) to the glTF root.
fn add_armor_primitive_to_root(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    armor: &ArmorSubModel,
) -> Result<json::mesh::Primitive, Report<ExportError>> {
    let mut attributes = BTreeMap::new();

    // --- Positions ---
    let (min, max) = bounding_coords(&armor.positions);
    let byte_offset = bin_data.len();
    for pos in &armor.positions {
        bin_data.extend_from_slice(&pos[0].to_le_bytes());
        bin_data.extend_from_slice(&pos[1].to_le_bytes());
        bin_data.extend_from_slice(&pos[2].to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let pos_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let pos_acc = root.push(json::Accessor {
        buffer_view: Some(pos_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.positions.len()),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        type_: Valid(json::accessor::Type::Vec3),
        min: Some(json::Value::from(min.to_vec())),
        max: Some(json::Value::from(max.to_vec())),
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    attributes.insert(Valid(json::mesh::Semantic::Positions), pos_acc);

    // --- Normals ---
    let byte_offset = bin_data.len();
    for n in &armor.normals {
        bin_data.extend_from_slice(&n[0].to_le_bytes());
        bin_data.extend_from_slice(&n[1].to_le_bytes());
        bin_data.extend_from_slice(&n[2].to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let norm_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let norm_acc = root.push(json::Accessor {
        buffer_view: Some(norm_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.normals.len()),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        type_: Valid(json::accessor::Type::Vec3),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    attributes.insert(Valid(json::mesh::Semantic::Normals), norm_acc);

    // --- Vertex Colors (COLOR_0) ---
    if !armor.colors.is_empty() {
        let byte_offset = bin_data.len();
        for c in &armor.colors {
            bin_data.extend_from_slice(&c[0].to_le_bytes());
            bin_data.extend_from_slice(&c[1].to_le_bytes());
            bin_data.extend_from_slice(&c[2].to_le_bytes());
            bin_data.extend_from_slice(&c[3].to_le_bytes());
        }
        pad_to_4(bin_data);
        let byte_length = bin_data.len() - byte_offset;

        let color_bv = root.push(json::buffer::View {
            buffer: json::Index::new(0),
            byte_length: USize64::from(byte_length),
            byte_offset: Some(USize64::from(byte_offset)),
            byte_stride: None,
            target: Some(Valid(json::buffer::Target::ArrayBuffer)),
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });

        let color_acc = root.push(json::Accessor {
            buffer_view: Some(color_bv),
            byte_offset: Some(USize64(0)),
            count: USize64::from(armor.colors.len()),
            component_type: Valid(json::accessor::GenericComponentType(
                json::accessor::ComponentType::F32,
            )),
            type_: Valid(json::accessor::Type::Vec4),
            min: None,
            max: None,
            name: None,
            normalized: false,
            sparse: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        attributes.insert(Valid(json::mesh::Semantic::Colors(0)), color_acc);
    }

    // --- Indices ---
    let byte_offset = bin_data.len();
    for &idx in &armor.indices {
        bin_data.extend_from_slice(&idx.to_le_bytes());
    }
    pad_to_4(bin_data);
    let byte_length = bin_data.len() - byte_offset;

    let idx_bv = root.push(json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: USize64::from(byte_length),
        byte_offset: Some(USize64::from(byte_offset)),
        byte_stride: None,
        target: Some(Valid(json::buffer::Target::ElementArrayBuffer)),
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    let idx_acc = root.push(json::Accessor {
        buffer_view: Some(idx_bv),
        byte_offset: Some(USize64(0)),
        count: USize64::from(armor.indices.len()),
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::U32,
        )),
        type_: Valid(json::accessor::Type::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
        extensions: Default::default(),
        extras: Default::default(),
    });

    // Untextured semi-transparent material for armor visualization.
    let material = root.push(json::Material {
        name: Some(format!("armor_{}", armor.name)),
        alpha_mode: Valid(json::material::AlphaMode::Blend),
        pbr_metallic_roughness: json::material::PbrMetallicRoughness {
            base_color_factor: json::material::PbrBaseColorFactor([0.2, 0.6, 1.0, 0.3]),
            metallic_factor: json::material::StrengthFactor(0.0),
            roughness_factor: json::material::StrengthFactor(0.8),
            ..Default::default()
        },
        double_sided: true,
        ..Default::default()
    });

    Ok(json::mesh::Primitive {
        attributes,
        indices: Some(idx_acc),
        material: Some(material),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: None,
        extras: Default::default(),
    })
}

/// Add `KHR_materials_variants` root extension and `extensionsUsed` entry.
///
/// Creates variant definitions at the glTF root so that each camo scheme name
/// appears as a selectable variant in viewers like Blender.
fn add_variants_extension(root: &mut json::Root, texture_set: &TextureSet) {
    if texture_set.camo_schemes.is_empty() {
        return;
    }

    let variants: Vec<json::extensions::scene::khr_materials_variants::Variant> = texture_set
        .camo_schemes
        .iter()
        .map(
            |(name, _)| json::extensions::scene::khr_materials_variants::Variant {
                name: name.clone(),
            },
        )
        .collect();

    let ext = json::extensions::root::KhrMaterialsVariants { variants };
    root.extensions = Some(json::extensions::root::Root {
        khr_materials_variants: Some(ext),
        ..Default::default()
    });

    root.extensions_used
        .push("KHR_materials_variants".to_string());

    if !texture_set.tiled_uv_transforms.is_empty() {
        root.extensions_used
            .push("KHR_texture_transform".to_string());
    }
}

fn pad_to_4(data: &mut Vec<u8>) {
    while data.len() % 4 != 0 {
        data.push(0);
    }
}

fn bounding_coords(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for p in points {
        for i in 0..3 {
            min[i] = f32::min(min[i], p[i]);
            max[i] = f32::max(max[i], p[i]);
        }
    }
    (min, max)
}

/// An armor mesh ready for glTF export (triangle soup, no textures).
pub struct ArmorSubModel {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Per-vertex RGBA color encoding armor thickness.
    /// All 3 vertices of a triangle share the same color.
    pub colors: Vec<[f32; 4]>,
}

impl ArmorSubModel {
    /// Build an `ArmorSubModel` from a parsed `ArmorModel`.
    ///
    /// `armor_map` is the GameParams armor thickness data:
    ///   key = `(model_index << 16) | triangle_index`, value = thickness in mm.
    /// `model_index` is the 1-based index of this armor model in the geometry file.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&std::collections::HashMap<u32, f32>>,
        model_index: u32,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            // Look up thickness for this triangle.
            let key = (model_index << 16) | (ti as u32);
            let thickness_mm = armor_map.and_then(|m| m.get(&key).copied()).unwrap_or(0.0);

            // Map thickness to a color: blue (thin) → green → yellow → red (thick).
            // 0mm = blue, ~200mm = green, ~400mm = red. Clamp at 650mm.
            let color = thickness_to_color(thickness_mm);

            for v in 0..3 {
                positions.push(tri.vertices[v]);
                normals.push(tri.normals[v]);
                indices.push((ti * 3 + v) as u32);
                colors.push(color);
            }
        }

        Self {
            name: armor.name.clone(),
            positions,
            normals,
            indices,
            colors,
        }
    }
}

/// Map armor thickness (mm) to an RGBA color for visualization.
///
/// Uses a blue → cyan → green → yellow → red heat map:
///   0mm = blue, 100mm = cyan, 200mm = green, 400mm = yellow, 650mm+ = red.
/// Alpha is 0.5 for all thicknesses (semi-transparent overlay).
fn thickness_to_color(thickness_mm: f32) -> [f32; 4] {
    let alpha = 0.5;
    if thickness_mm <= 0.0 {
        return [0.2, 0.2, 0.8, 0.3]; // dim blue for unmapped
    }

    // Normalize: 0..650 → 0..1
    let t = (thickness_mm / 650.0).clamp(0.0, 1.0);

    let (r, g, b) = if t < 0.25 {
        // blue → cyan (0..~162mm)
        let s = t / 0.25;
        (0.0, s, 1.0)
    } else if t < 0.5 {
        // cyan → green (~162..~325mm)
        let s = (t - 0.25) / 0.25;
        (0.0, 1.0, 1.0 - s)
    } else if t < 0.75 {
        // green → yellow (~325..~487mm)
        let s = (t - 0.5) / 0.25;
        (s, 1.0, 0.0)
    } else {
        // yellow → red (~487..650mm+)
        let s = (t - 0.75) / 0.25;
        (1.0, 1.0 - s, 0.0)
    };

    [r, g, b, alpha]
}

/// A named sub-model for multi-model ship export.
pub struct SubModel<'a> {
    pub name: String,
    pub visual: &'a VisualPrototype,
    pub geometry: &'a MergedGeometry<'a>,
    /// Optional world-space transform (column-major 4x4 matrix).
    /// If `None`, the sub-model is placed at the origin.
    pub transform: Option<[f32; 16]>,
}

/// Export multiple sub-models as a single GLB with separate named meshes/nodes.
///
/// Each sub-model becomes a separate selectable object in Blender.
/// `texture_set` contains base albedo + camo variant PNGs for material textures.
/// `armor_models` are added as additional untextured semi-transparent meshes.
pub fn export_ship_glb(
    sub_models: &[SubModel<'_>],
    armor_models: &[ArmorSubModel],
    db: &PrototypeDatabase<'_>,
    lod: usize,
    texture_set: &TextureSet,
    damaged: bool,
    writer: &mut impl Write,
) -> Result<(), Report<ExportError>> {
    let mut root = json::Root::default();
    root.asset = json::Asset {
        version: "2.0".to_string(),
        generator: Some("wowsunpack".to_string()),
        ..Default::default()
    };

    let mut bin_data: Vec<u8> = Vec::new();
    let mut scene_nodes = Vec::new();
    let mut mat_cache = MaterialCache::new();

    for sub in sub_models {
        // Validate LOD — skip sub-models that don't have enough LODs.
        if sub.visual.lods.is_empty() || lod >= sub.visual.lods.len() {
            eprintln!(
                "Warning: sub-model '{}' has {} LODs, skipping (requested LOD {})",
                sub.name,
                sub.visual.lods.len(),
                lod
            );
            continue;
        }

        let lod_entry = &sub.visual.lods[lod];
        let primitives = collect_primitives(sub.visual, sub.geometry, db, lod_entry, damaged)?;

        if primitives.is_empty() {
            eprintln!(
                "Warning: sub-model '{}' has no primitives for LOD {lod}",
                sub.name
            );
            continue;
        }

        let mut gltf_primitives = Vec::new();
        for prim in &primitives {
            let gltf_prim =
                add_primitive_to_root(&mut root, &mut bin_data, prim, texture_set, &mut mat_cache)?;
            gltf_primitives.push(gltf_prim);
        }

        // Create a mesh named after the sub-model.
        let mesh = root.push(json::Mesh {
            primitives: gltf_primitives,
            weights: None,
            name: Some(sub.name.clone()),
            extensions: Default::default(),
            extras: Default::default(),
        });

        // Create a node named after the sub-model, referencing the mesh.
        let node = root.push(json::Node {
            mesh: Some(mesh),
            name: Some(sub.name.clone()),
            matrix: sub.transform,
            ..Default::default()
        });

        scene_nodes.push(node);
    }

    // Add armor meshes as separate nodes.
    for armor in armor_models {
        if armor.positions.is_empty() {
            continue;
        }

        let gltf_prim = add_armor_primitive_to_root(&mut root, &mut bin_data, armor)?;

        let mesh = root.push(json::Mesh {
            primitives: vec![gltf_prim],
            weights: None,
            name: Some(armor.name.clone()),
            extensions: Default::default(),
            extras: Default::default(),
        });

        let node = root.push(json::Node {
            mesh: Some(mesh),
            name: Some(armor.name.clone()),
            ..Default::default()
        });

        scene_nodes.push(node);
    }

    // Pad binary data to 4-byte alignment.
    while bin_data.len() % 4 != 0 {
        bin_data.push(0);
    }

    // Set the buffer byte_length.
    if !bin_data.is_empty() {
        let buffer = root.push(json::Buffer {
            byte_length: USize64::from(bin_data.len()),
            uri: None,
            name: None,
            extensions: Default::default(),
            extras: Default::default(),
        });
        for bv in root.buffer_views.iter_mut() {
            bv.buffer = buffer;
        }
    }

    // Create scene with all sub-model nodes.
    let scene = root.push(json::Scene {
        nodes: scene_nodes,
        name: None,
        extensions: Default::default(),
        extras: Default::default(),
    });
    root.scene = Some(scene);

    // Add KHR_materials_variants root extension if we have camo schemes.
    add_variants_extension(&mut root, texture_set);

    // Serialize and write GLB.
    let json_string = json::serialize::to_string(&root)
        .map_err(|e| Report::new(ExportError::Serialize(e.to_string())))?;

    let glb = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: 0,
        },
        json: Cow::Owned(json_string.into_bytes()),
        bin: if bin_data.is_empty() {
            None
        } else {
            Some(Cow::Owned(bin_data))
        },
    };

    glb.to_writer(writer)
        .map_err(|e| Report::new(ExportError::Io(e.to_string())))?;

    Ok(())
}
