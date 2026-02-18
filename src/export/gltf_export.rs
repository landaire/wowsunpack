//! Export ship visual + geometry to glTF/GLB format.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use gltf_json as json;
use json::validation::Checked::Valid;
use json::validation::USize64;
use rootcause::Report;
use thiserror::Error;

use crate::game_params::types::ArmorMap;
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
            base_color_factor: json::material::PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
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

/// Per-triangle metadata for interactive armor viewers.
///
/// Each entry describes one triangle's collision material, zone classification,
/// armor thickness, and display color. Consumers can use this for hover/click
/// tooltips in a 3D viewer.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArmorTriangleInfo {
    /// 1-based armor model index (matches GameParams key prefix).
    pub model_index: u32,
    /// 0-based triangle index within the armor model.
    pub triangle_index: u32,
    /// Collision material ID (0–255) from the BVH node header.
    pub material_id: u8,
    /// Human-readable material name (e.g. "Cit_Belt", "Bow_Bottom").
    pub material_name: String,
    /// Zone classification (e.g. "Citadel", "Bow", "Superstructure").
    pub zone: String,
    /// Total armor thickness in millimeters (sum of all layers).
    pub thickness_mm: f32,
    /// Per-layer thicknesses in mm, ordered by model_index.
    /// Single-layer materials have one entry; `Dual_*` materials have two or more.
    pub layers: Vec<f32>,
    /// RGBA color [0.0–1.0] encoding the total thickness via the game's color scale.
    pub color: [f32; 4],
}

/// Look up all non-zero armor layers for a material from mount armor (priority) or hull armor.
///
/// Returns `(layers, total)` where `layers` contains all non-zero thickness values
/// across model_indices, and `total` is their sum. This is used when we want to show
/// the per-plate thickness for the outermost layer and include all layers in the tooltip.
fn lookup_all_layers(
    mat_id: u32,
    mount_armor: Option<&ArmorMap>,
    armor_map: Option<&ArmorMap>,
) -> (Vec<f32>, f32) {
    let layers_map = mount_armor
        .and_then(|m| m.get(&mat_id))
        .or_else(|| armor_map.and_then(|m| m.get(&mat_id)));
    let layers: Vec<f32> = layers_map
        .map(|m| m.values().copied().filter(|&v| v > 0.0).collect())
        .unwrap_or_default();
    let total: f32 = layers.iter().sum();
    (layers, total)
}

/// Look up the armor thickness for a specific (material_id, model_index) pair.
/// Checks mount armor first, then hull armor as fallback.
fn lookup_thickness(
    mat_id: u32,
    model_index: u32,
    mount_armor: Option<&ArmorMap>,
    armor_map: Option<&ArmorMap>,
) -> f32 {
    mount_armor
        .and_then(|m| m.get(&mat_id))
        .and_then(|layers| layers.get(&model_index))
        .copied()
        .or_else(|| {
            armor_map
                .and_then(|m| m.get(&mat_id))
                .and_then(|layers| layers.get(&model_index))
                .copied()
        })
        .unwrap_or(0.0)
}

/// An indexed armor mesh with per-triangle metadata for interactive viewers.
///
/// Unlike `ArmorSubModel` (which groups by zone and loses per-triangle material info),
/// this type preserves full metadata for every triangle. Consumers can render the mesh
/// with `positions`/`normals`/`indices`/`colors`, then look up `triangle_info[face_index]`
/// on hover/click to display material name, thickness, and zone.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InteractiveArmorMesh {
    /// Armor model name (e.g. "CM_PA_united").
    pub name: String,
    /// Vertex positions (3 per triangle, triangle-soup layout).
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (same length as positions).
    pub normals: Vec<[f32; 3]>,
    /// Triangle indices into positions/normals (length = triangle_count * 3).
    pub indices: Vec<u32>,
    /// Per-vertex RGBA color encoding armor thickness.
    /// All 3 vertices of a triangle share the same color.
    pub colors: Vec<[f32; 4]>,
    /// Per-triangle metadata. `triangle_info[i]` corresponds to
    /// `indices[i*3..i*3+3]`. Length = `indices.len() / 3`.
    pub triangle_info: Vec<ArmorTriangleInfo>,
    /// Optional world-space transform (column-major 4x4).
    /// Used for turret armor instances positioned at mount points.
    /// Hull armor meshes have `None` (already in world space).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub transform: Option<[f32; 16]>,
}

impl InteractiveArmorMesh {
    /// Build an `InteractiveArmorMesh` from a parsed `ArmorModel`.
    ///
    /// `armor_map` is the hull-wide [`ArmorMap`] (`A_Hull.armor`).
    /// `mount_armor` is the optional per-mount [`ArmorMap`] (`A_Artillery.HP_XXX.armor`).
    /// Mount armor is checked first, then hull armor as fallback.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&ArmorMap>,
        mount_armor: Option<&ArmorMap>,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);
        let mut triangle_info = Vec::with_capacity(tri_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            let mat_name = collision_material_name(tri.material_id);
            let zone = zone_from_material_name(mat_name).to_string();

            let mat_id = tri.material_id as u32;
            let layer = tri.layer_index as u32;
            // Use the per-triangle layer_index for the specific plate thickness.
            let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
            // Collect all non-zero layers for the tooltip (shows stacked plates).
            let (all_layers, _) = lookup_all_layers(mat_id, mount_armor, armor_map);
            let color = thickness_to_color(thickness_mm);

            for v in 0..3 {
                positions.push(tri.vertices[v]);
                normals.push(tri.normals[v]);
                indices.push((ti * 3 + v) as u32);
                colors.push(color);
            }

            triangle_info.push(ArmorTriangleInfo {
                model_index: layer,
                triangle_index: ti as u32,
                material_id: tri.material_id,
                material_name: mat_name.to_string(),
                zone,
                thickness_mm,
                layers: if all_layers.len() > 1 {
                    all_layers
                } else {
                    vec![thickness_mm]
                },
                color,
            });
        }

        Self {
            name: armor.name.clone(),
            positions,
            normals,
            indices,
            colors,
            triangle_info,
            transform: None,
        }
    }
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
    /// Optional world-space transform (column-major 4x4).
    /// Used for turret armor instances positioned at mount points.
    pub transform: Option<[f32; 16]>,
}

impl ArmorSubModel {
    /// Build an `ArmorSubModel` from a parsed `ArmorModel`.
    ///
    /// See [`InteractiveArmorMesh::from_armor_model`] for parameter docs.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&ArmorMap>,
        mount_armor: Option<&ArmorMap>,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            let mat_id = tri.material_id as u32;
            let layer = tri.layer_index as u32;
            let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
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
            transform: None,
        }
    }
}

/// A hull visual mesh for interactive viewers (positions, normals, indices + render set name).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct InteractiveHullMesh {
    /// Render set name (e.g. "Hull", "Superstructure").
    pub name: String,
    /// Vertex positions.
    pub positions: Vec<[f32; 3]>,
    /// Vertex normals (same length as positions).
    pub normals: Vec<[f32; 3]>,
    /// Vertex UVs (same length as positions).
    pub uvs: Vec<[f32; 2]>,
    /// Triangle indices into positions/normals/uvs.
    pub indices: Vec<u32>,
    /// Full VFS path to the .mfm material file (for texture lookup).
    pub mfm_path: Option<String>,
    /// Baked per-vertex colors from albedo texture (same length as positions, or empty for fallback).
    pub colors: Vec<[f32; 4]>,
    /// Optional world-space transform (column-major 4x4) for turret mounts.
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub transform: Option<[f32; 16]>,
}

/// Collect hull visual meshes (render sets) from a visual prototype and geometry.
///
/// Each render set becomes one `InteractiveHullMesh` with decoded
/// positions, normals, UVs, and indices. The caller is responsible for
/// baking textures into vertex colors using the `mfm_path` field.
pub fn collect_hull_meshes(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: usize,
    damaged: bool,
) -> Result<Vec<InteractiveHullMesh>, Report<ExportError>> {
    let mut result = Vec::new();

    if visual.lods.is_empty() || lod >= visual.lods.len() {
        return Ok(result);
    }
    let lod_entry = &visual.lods[lod];

    let exclude = if damaged {
        DAMAGED_EXCLUDE
    } else {
        INTACT_EXCLUDE
    };

    let self_id_index = db.build_self_id_index();

    for &rs_name_id in &lod_entry.render_set_names {
        let rs = visual
            .render_sets
            .iter()
            .find(|rs| rs.name_id == rs_name_id)
            .ok_or_else(|| Report::new(ExportError::RenderSetNotFound(rs_name_id)))?;

        let rs_name = db
            .strings
            .get_string_by_id(rs_name_id)
            .unwrap_or("<unknown>");

        if exclude.iter().any(|sub| rs_name.contains(sub)) {
            continue;
        }

        let vertices_mapping_id = (rs.unknown_u64 & 0xFFFFFFFF) as u32;
        let indices_mapping_id = (rs.unknown_u64 >> 32) as u32;

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

        let vbuf_idx = vert_mapping.merged_buffer_index as usize;
        if vbuf_idx >= geometry.merged_vertices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: vbuf_idx,
                count: geometry.merged_vertices.len(),
            }));
        }
        let vert_proto = &geometry.merged_vertices[vbuf_idx];

        let ibuf_idx = idx_mapping.merged_buffer_index as usize;
        if ibuf_idx >= geometry.merged_indices.len() {
            return Err(Report::new(ExportError::BufferIndexOutOfRange {
                index: ibuf_idx,
                count: geometry.merged_indices.len(),
            }));
        }
        let idx_proto = &geometry.merged_indices[ibuf_idx];

        let decoded_vertices = vert_proto
            .data
            .decode()
            .map_err(|e| Report::new(ExportError::VertexDecode(format!("{e:?}"))))?;
        let decoded_indices = idx_proto
            .data
            .decode()
            .map_err(|e| Report::new(ExportError::IndexDecode(format!("{e:?}"))))?;

        let format = vertex_format::parse_vertex_format(&vert_proto.format_name);
        let stride = vert_proto.stride_in_bytes as usize;

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

        let (positions, normals, uvs) = unpack_vertices(vert_slice, stride, &format);

        // Resolve full MFM path for texture lookup.
        let mfm_path = if rs.material_mfm_path_id != 0 {
            self_id_index
                .get(&rs.material_mfm_path_id)
                .map(|&idx| db.reconstruct_path(idx, &self_id_index))
        } else {
            None
        };

        result.push(InteractiveHullMesh {
            name: rs_name.to_string(),
            positions,
            normals,
            uvs,
            indices,
            mfm_path,
            colors: Vec::new(),
            transform: None,
        });
    }

    Ok(result)
}

/// Game's exact armor thickness color scale.
///
/// 10 color buckets matching the in-game visualization from `ArmorConstants.py`.
/// Each entry: (max_thickness_mm, r, g, b).
/// Assignment uses `bisect_left` — a thickness of exactly a breakpoint value
/// falls into that breakpoint's bucket.
const ARMOR_COLOR_SCALE: &[(f32, f32, f32, f32)] = &[
    (14.0, 110.0 / 255.0, 209.0 / 255.0, 176.0 / 255.0), // teal
    (16.0, 149.0 / 255.0, 210.0 / 255.0, 127.0 / 255.0), // light green
    (24.0, 170.0 / 255.0, 201.0 / 255.0, 102.0 / 255.0), // yellow-green
    (26.0, 192.0 / 255.0, 193.0 / 255.0, 80.0 / 255.0),  // olive
    (28.0, 226.0 / 255.0, 195.0 / 255.0, 62.0 / 255.0),  // gold
    (33.0, 225.0 / 255.0, 171.0 / 255.0, 54.0 / 255.0),  // orange-gold
    (75.0, 227.0 / 255.0, 144.0 / 255.0, 49.0 / 255.0),  // orange
    (160.0, 230.0 / 255.0, 115.0 / 255.0, 49.0 / 255.0), // dark orange
    (399.0, 220.0 / 255.0, 78.0 / 255.0, 48.0 / 255.0),  // red-orange
    (999.0, 185.0 / 255.0, 47.0 / 255.0, 48.0 / 255.0),  // dark red
];

/// Map armor thickness (mm) to an RGBA color matching the game's visualization.
///
/// Uses the exact 10-bucket color scale from the game's `ArmorConstants.py`.
/// Thickness ≤ 0 is treated as unknown (faint blue).
pub fn thickness_to_color(thickness_mm: f32) -> [f32; 4] {
    if thickness_mm <= 0.0 {
        return [0.8, 0.8, 0.8, 0.5]; // light gray for plates with no assigned thickness
    }

    // bisect_left: find first bucket where breakpoint >= thickness
    let idx = ARMOR_COLOR_SCALE
        .iter()
        .position(|&(bp, _, _, _)| thickness_mm <= bp)
        .unwrap_or(ARMOR_COLOR_SCALE.len() - 1);

    let (_, r, g, b) = ARMOR_COLOR_SCALE[idx];
    [r, g, b, 0.8]
}

/// An entry in the armor thickness color legend.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ArmorLegendEntry {
    /// Lower bound of this thickness range (mm), inclusive.
    pub min_mm: f32,
    /// Upper bound of this thickness range (mm), inclusive.
    pub max_mm: f32,
    /// RGBA color used in the GLB export, each component 0.0..1.0.
    pub color: [f32; 4],
    /// Human-readable color name.
    pub color_name: String,
}

/// Return the armor thickness color legend.
///
/// Each entry describes one color bucket: the thickness range (mm) and the
/// color used. External tools can use this to build UI legends, filter by
/// exact mm ranges, or map thickness values to colors programmatically.
pub fn armor_color_legend() -> Vec<ArmorLegendEntry> {
    let color_names = [
        "teal",
        "light green",
        "yellow-green",
        "olive",
        "gold",
        "orange-gold",
        "orange",
        "dark orange",
        "red-orange",
        "dark red",
    ];

    ARMOR_COLOR_SCALE
        .iter()
        .enumerate()
        .map(|(i, &(max_mm, r, g, b))| {
            let min_mm = if i == 0 {
                0.0
            } else {
                ARMOR_COLOR_SCALE[i - 1].0 + 1.0
            };
            ArmorLegendEntry {
                min_mm,
                max_mm,
                color: [r, g, b, 0.8],
                color_name: color_names[i].to_string(),
            }
        })
        .collect()
}

/// Derive the zone name from a collision material name.
///
/// Material names follow patterns like `Bow_Bottom`, `Cit_Belt`, `SS_Side`,
/// `Tur1GkBar`, `RudderAft`, etc. The prefix before the first `_` determines
/// the zone, with special handling for turret and rudder names.
pub fn zone_from_material_name(mat_name: &str) -> &'static str {
    use std::collections::HashSet;
    use std::sync::Mutex;
    static WARNED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

    // Dual-zone materials: Dual_<primary>_<secondary>_<part>.
    // Use the first zone identifier after "Dual_" as the primary.
    if let Some(rest) = mat_name.strip_prefix("Dual_") {
        if rest.starts_with("Cit") {
            return "Citadel";
        }
        if rest.starts_with("OCit") {
            return "Citadel";
        }
        if rest.starts_with("Cas") {
            return "Casemate";
        }
        if rest.starts_with("SSC") {
            return "Superstructure";
        }
        if rest.starts_with("Bow") {
            return "Bow";
        }
        if rest.starts_with("St_") {
            return "Stern";
        }
        if rest.starts_with("SS_") {
            return "Superstructure";
        }
        {
            let mut warned = WARNED.lock().unwrap();
            let set = warned.get_or_insert_with(HashSet::new);
            if set.insert(mat_name.to_string()) {
                eprintln!(
                    "BUG: unrecognized Dual_ collision material '{mat_name}' — \
                     zone_from_material_name needs updating"
                );
            }
        }
        return "Other";
    }
    // Zone sub-face materials: Side/Deck/Trans/Inclin + zone suffix.
    if mat_name.ends_with("Cit") {
        return "Citadel";
    }
    if mat_name.ends_with("Cas") {
        return "Casemate";
    }
    if mat_name.ends_with("SSC") {
        return "Superstructure";
    }
    if mat_name.ends_with("Bow") {
        return "Bow";
    }
    if mat_name.ends_with("Stern") {
        return "Stern";
    }
    if mat_name.ends_with("SS") && !mat_name.starts_with("Dual_") {
        // SGBarbetteSS, SGDownSS → SteeringGear; DeckSS, SideSS, TransSS → Superstructure
        if mat_name.starts_with("SG") {
            return "SteeringGear";
        }
        return "Superstructure";
    }
    // Collision material prefixes.
    if mat_name.starts_with("Bow") {
        return "Bow";
    }
    if mat_name.starts_with("St_") {
        return "Stern";
    }
    if mat_name.starts_with("Cit") {
        return "Citadel";
    }
    if mat_name.starts_with("OCit") {
        return "Citadel";
    }
    if mat_name.starts_with("Cas") {
        return "Casemate";
    }
    if mat_name.starts_with("SSC") || mat_name == "SSCasemate" {
        return "Superstructure";
    }
    if mat_name.starts_with("SS_") {
        return "Superstructure";
    }
    if mat_name.starts_with("Tur") || mat_name.starts_with("AuTurret") {
        return "Turret";
    }
    if mat_name.starts_with("Art") {
        return "Turret";
    }
    if mat_name.starts_with("Rudder") || mat_name.starts_with("SG") {
        return "SteeringGear";
    }
    if mat_name.starts_with("Bulge") {
        return "TorpedoProtection";
    }
    if mat_name.starts_with("Bridge") || mat_name.starts_with("Funnel") {
        return "Superstructure";
    }
    if mat_name.starts_with("Kdp") {
        return "Hull";
    }
    match mat_name {
        "Deck" | "ConstrSide" | "Hull" | "Side" | "Bottom" | "Top" | "Belt" | "Trans"
        | "Inclin" => "Hull",
        "common" | "zero" => "Default",
        _ => {
            let mut warned = WARNED.lock().unwrap();
            let set = warned.get_or_insert_with(HashSet::new);
            if set.insert(mat_name.to_string()) {
                eprintln!(
                    "BUG: unrecognized collision material '{mat_name}' — \
                     zone_from_material_name needs updating"
                );
            }
            "Other"
        }
    }
}

/// The built-in collision material name table.
///
/// Contiguous array indexed by material ID (0..=250). Extracted from the game
/// client's `py_collisionMaterialName` table at 0x142a569a0.
const COLLISION_MATERIAL_NAMES: &[&str] = &[
    // 0-1: generic
    "common", // 0
    "zero",   // 1
    // 2-31: Dual-zone materials
    "Dual_SSC_Bow_Side",       // 2
    "Dual_SSC_St_Side",        // 3
    "Dual_Cas_OCit_Belt",      // 4
    "Dual_OCit_St_Trans",      // 5
    "Dual_OCit_Bow_Trans",     // 6
    "Dual_Cit_Bow_Side",       // 7
    "Dual_Cit_Bow_Belt",       // 8
    "Dual_Cit_Bow_ArtSide",    // 9
    "Dual_Cit_St_Side",        // 10
    "Dual_Cit_St_Belt",        // 11
    "Bottom",                  // 12
    "Dual_Cit_St_ArtSide",     // 13
    "Dual_Cas_Bow_Belt",       // 14
    "Dual_Cas_St_Belt",        // 15
    "Dual_Cas_SSC_Belt",       // 16
    "Dual_SSC_Bow_ConstrSide", // 17
    "Dual_SSC_St_ConstrSide",  // 18
    "Cas_Inclin",              // 19
    "SSC_Inclin",              // 20
    "Dual_Cas_SSC_Inclin",     // 21
    "Dual_Cas_Bow_Inclin",     // 22
    "Dual_Cas_St_Inclin",      // 23
    "Dual_SSC_Bow_Inclin",     // 24
    "Dual_SSC_St_Inclin",      // 25
    "Dual_Cit_Bow_Bulge",      // 26
    "Dual_Cit_St_Bulge",       // 27
    "Dual_Cas_SS_Belt",        // 28
    "Dual_Cit_Cas_ArtDeck",    // 29
    "Dual_Cit_Cas_ArtSide",    // 30
    "Dual_OCit_OCit_Side",     // 31
    // 32-45: turret/artillery/auxiliary turret
    "TurretSide",       // 32
    "TurretTop",        // 33
    "TurretFront",      // 34
    "TurretAft",        // 35
    "FunnelSide",       // 36
    "ArtBottom",        // 37
    "ArtSide",          // 38
    "ArtTop",           // 39
    "AuTurretAft",      // 40
    "AuTurretBarbette", // 41
    "AuTurretDown",     // 42
    "AuTurretFwd",      // 43
    "AuTurretSide",     // 44
    "AuTurretTop",      // 45
    // 46-51: Bow
    "Bow_Belt",       // 46
    "Bow_Bottom",     // 47
    "Bow_ConstrSide", // 48
    "Bow_Deck",       // 49
    "Bow_Inclin",     // 50
    "Bow_Trans",      // 51
    // 52-54: Bridge
    "BridgeBottom", // 52
    "BridgeSide",   // 53
    "BridgeTop",    // 54
    // 55-58: Casemate
    "Cas_AftTrans", // 55
    "Cas_Belt",     // 56
    "Cas_Deck",     // 57
    "Cas_FwdTrans", // 58
    // 59-68: Citadel
    "Cit_AftTrans",       // 59
    "Cit_Barbette",       // 60
    "Cit_Belt",           // 61
    "Cit_Bottom",         // 62
    "Cit_Bulge",          // 63
    "Cit_Deck",           // 64
    "Cit_FwdTrans",       // 65
    "Cit_Inclin",         // 66
    "Cit_Side",           // 67
    "Dual_Cit_Cas_Bulge", // 68
    // 69-79: Hull/misc
    "ConstrSide",        // 69
    "Dual_Cit_Cas_Belt", // 70
    "Bow_Fdck",          // 71
    "St_Fdck",           // 72
    "KdpBottom",         // 73
    "KdpSide",           // 74
    "KdpTop",            // 75
    "OCit_AftTrans",     // 76
    "OCit_Belt",         // 77
    "OCit_Deck",         // 78
    "OCit_FwdTrans",     // 79
    // 80-83: Rudder
    "RudderAft",  // 80
    "RudderFwd",  // 81
    "RudderSide", // 82
    "RudderTop",  // 83
    // 84-90: Superstructure casemate / Superstructure
    "SSC_AftTrans",   // 84
    "SSCasemate",     // 85
    "SSC_ConstrSide", // 86
    "SSC_Deck",       // 87
    "SSC_FwdTrans",   // 88
    "SS_Side",        // 89
    "SS_Top",         // 90
    // 91-96: Stern
    "St_Belt",       // 91
    "St_Bottom",     // 92
    "St_ConstrSide", // 93
    "St_Deck",       // 94
    "St_Inclin",     // 95
    "St_Trans",      // 96
    // 97-106: Turret generic / hull generic
    "TurretBarbette",     // 97
    "TurretBarbette2",    // 98
    "TurretDown",         // 99
    "TurretFwd",          // 100
    "Bulge",              // 101
    "Trans",              // 102
    "Deck",               // 103
    "Belt",               // 104
    "Dual_Cit_SSC_Bulge", // 105
    "Inclin",             // 106
    // 107-110: SS/Bridge, Casemate bottom
    "SS_BridgeTop",    // 107
    "SS_BridgeSide",   // 108
    "SS_BridgeBottom", // 109
    "Cas_Bottom",      // 110
    // 111-133: Zone sub-face materials (Side/Deck/Trans/Inclin per zone)
    "SideCit",     // 111
    "DeckCit",     // 112
    "TransCit",    // 113
    "InclinCit",   // 114
    "SideCas",     // 115
    "DeckCas",     // 116
    "TransCas",    // 117
    "InclinCas",   // 118
    "SideSSC",     // 119
    "DeckSSC",     // 120
    "TransSSC",    // 121
    "InclinSSC",   // 122
    "SideBow",     // 123
    "DeckBow",     // 124
    "TransBow",    // 125
    "InclinBow",   // 126
    "SideStern",   // 127
    "DeckStern",   // 128
    "TransStern",  // 129
    "InclinStern", // 130
    "SideSS",      // 131
    "DeckSS",      // 132
    "TransSS",     // 133
    // 134-153: Turret barbettes (GkBar) for turrets 1-20
    "Tur1GkBar",  // 134
    "Tur2GkBar",  // 135
    "Tur3GkBar",  // 136
    "Tur4GkBar",  // 137
    "Tur5GkBar",  // 138
    "Tur6GkBar",  // 139
    "Tur7GkBar",  // 140
    "Tur8GkBar",  // 141
    "Tur9GkBar",  // 142
    "Tur10GkBar", // 143
    "Tur11GkBar", // 144
    "Tur12GkBar", // 145
    "Tur13GkBar", // 146
    "Tur14GkBar", // 147
    "Tur15GkBar", // 148
    "Tur16GkBar", // 149
    "Tur17GkBar", // 150
    "Tur18GkBar", // 151
    "Tur19GkBar", // 152
    "Tur20GkBar", // 153
    // 154-173: Dual-zone transitions (Cas/SSC/Bow/St/SS combinations)
    "Dual_Cas_Bow_Trans",  // 154
    "Dual_Cas_Bow_Deck",   // 155
    "Dual_Cas_St_Trans",   // 156
    "Dual_Cas_St_Deck",    // 157
    "Dual_Cas_SSC_Deck",   // 158
    "Dual_Cas_SSC_Trans",  // 159
    "Dual_Cas_SS_Deck",    // 160
    "Dual_Cas_SS_Trans",   // 161
    "Dual_SSC_Bow_Trans",  // 162
    "Dual_SSC_Bow_Deck",   // 163
    "Dual_SSC_St_Trans",   // 164
    "Dual_SSC_St_Deck",    // 165
    "Dual_SSC_SS_Deck",    // 166
    "Dual_SSC_SS_Trans",   // 167
    "Dual_Bow_SS_Deck",    // 168
    "Dual_Bow_SS_Trans",   // 169
    "Dual_St_SS_Deck",     // 170
    "Dual_St_SS_Trans",    // 171
    "Dual_Cit_Bow_Bottom", // 172
    "Dual_Cit_St_Bottom",  // 173
    // 174-193: Turret undersides (GkDown) for turrets 1-20
    "Tur1GkDown",  // 174
    "Tur2GkDown",  // 175
    "Tur3GkDown",  // 176
    "Tur4GkDown",  // 177
    "Tur5GkDown",  // 178
    "Tur6GkDown",  // 179
    "Tur7GkDown",  // 180
    "Tur8GkDown",  // 181
    "Tur9GkDown",  // 182
    "Tur10GkDown", // 183
    "Tur11GkDown", // 184
    "Tur12GkDown", // 185
    "Tur13GkDown", // 186
    "Tur14GkDown", // 187
    "Tur15GkDown", // 188
    "Tur16GkDown", // 189
    "Tur17GkDown", // 190
    "Tur18GkDown", // 191
    "Tur19GkDown", // 192
    "Tur20GkDown", // 193
    // 194-213: Dual same-zone / cross-zone combinations
    "Dual_Cit_Cit_Deck",       // 194
    "Dual_Cit_Cit_Inclin",     // 195
    "Dual_Cit_Cit_Trans",      // 196
    "Dual_Cit_Cit_Side",       // 197
    "Dual_Cas_Cas_Belt",       // 198
    "Dual_Cas_Cas_Deck",       // 199
    "Dual_SSC_SSC_ConstrSide", // 200
    "Dual_SSC_SSC_Deck",       // 201
    "Dual_Bow_Bow_Deck",       // 202
    "Dual_Bow_Bow_ConstrSide", // 203
    "Dual_St_St_Deck",         // 204
    "Dual_St_St_ConstrSide",   // 205
    "Dual_SS_SS_Top",          // 206
    "Dual_SS_SS_Side",         // 207
    "Dual_Cit_Bow_ArtDeck",    // 208
    "Dual_Cit_St_ArtDeck",     // 209
    "Dual_Cas_Bow_Side",       // 210
    "Dual_Cas_St_Side",        // 211
    "Dual_Cit_Cas_Side",       // 212
    "Dual_Cit_SSC_Side",       // 213
    // 214-233: Turret tops (GkTop) for turrets 1-20
    "Tur1GkTop",  // 214
    "Tur2GkTop",  // 215
    "Tur3GkTop",  // 216
    "Tur4GkTop",  // 217
    "Tur5GkTop",  // 218
    "Tur6GkTop",  // 219
    "Tur7GkTop",  // 220
    "Tur8GkTop",  // 221
    "Tur9GkTop",  // 222
    "Tur10GkTop", // 223
    "Tur11GkTop", // 224
    "Tur12GkTop", // 225
    "Tur13GkTop", // 226
    "Tur14GkTop", // 227
    "Tur15GkTop", // 228
    "Tur16GkTop", // 229
    "Tur17GkTop", // 230
    "Tur18GkTop", // 231
    "Tur19GkTop", // 232
    "Tur20GkTop", // 233
    // 234-241: Hangar/forecastle deck, steering gear barbette
    "Cas_Hang",      // 234
    "Cas_Fdck",      // 235
    "SSC_Fdck",      // 236
    "SSC_Hang",      // 237
    "SS_SGBarbette", // 238
    "SS_SGDown",     // 239
    "SGBarbetteSS",  // 240
    "SGDownSS",      // 241
    // 242-254: Dual Citadel zone transitions
    "Dual_Cit_Cas_Deck",   // 242
    "Dual_Cit_Cas_Inclin", // 243
    "Dual_Cit_Cas_Trans",  // 244
    "Dual_Cit_SSC_Deck",   // 245
    "Dual_Cit_SSC_Inclin", // 246
    "Dual_Cit_SSC_Trans",  // 247
    "Dual_Cit_Bow_Trans",  // 248
    "Dual_Cit_Bow_Inclin", // 249
    "Dual_Cit_Bow_Deck",   // 250
    "Dual_Cit_St_Trans",   // 251
    "Dual_Cit_St_Inclin",  // 252
    "Dual_Cit_St_Deck",    // 253
    "Dual_Cit_SS_Deck",    // 254
];

/// Look up the collision material name for a given material ID.
///
/// Logs a warning for unknown IDs — this indicates the game's material table
/// has been extended and our hardcoded copy needs updating.
pub fn collision_material_name(id: u8) -> &'static str {
    use std::sync::Mutex;
    static WARNED: Mutex<[bool; 256]> = Mutex::new([false; 256]);

    let idx = id as usize;
    if idx < COLLISION_MATERIAL_NAMES.len() {
        COLLISION_MATERIAL_NAMES[idx]
    } else {
        let mut warned = WARNED.lock().unwrap();
        if !warned[idx] {
            warned[idx] = true;
            eprintln!(
                "BUG: collision material ID {id} is beyond the known table (max {}). \
                 The game's material table has likely been updated — \
                 see MODELS.md for how to re-extract it.",
                COLLISION_MATERIAL_NAMES.len() - 1
            );
        }
        "unknown"
    }
}

/// Split an armor model into per-zone `ArmorSubModel`s for selective visibility in Blender.
///
/// Each triangle is classified by its collision material ID, which determines both:
/// - The armor thickness (looked up from the GameParams armor dict)
/// - The zone name (derived from the material name pattern)
///
/// Triangles are grouped into one mesh per zone for easy toggling in Blender.
pub fn armor_sub_models_by_zone(
    armor: &crate::models::geometry::ArmorModel,
    armor_map: Option<&ArmorMap>,
    mount_armor: Option<&ArmorMap>,
) -> Vec<ArmorSubModel> {
    // Group triangles by zone name.
    let mut zone_tris: std::collections::BTreeMap<
        String,
        Vec<(&crate::models::geometry::ArmorTriangle, [f32; 4])>,
    > = std::collections::BTreeMap::new();

    for tri in &armor.triangles {
        let mat_id = tri.material_id as u32;
        let layer = tri.layer_index as u32;
        let thickness_mm = lookup_thickness(mat_id, layer, mount_armor, armor_map);
        let color = thickness_to_color(thickness_mm);

        let mat_name = collision_material_name(tri.material_id);
        let zone_name = zone_from_material_name(mat_name).to_string();

        zone_tris.entry(zone_name).or_default().push((tri, color));
    }

    // Build one ArmorSubModel per zone.
    zone_tris
        .into_iter()
        .map(|(zone_name, tris)| {
            let vert_count = tris.len() * 3;
            let mut positions = Vec::with_capacity(vert_count);
            let mut normals = Vec::with_capacity(vert_count);
            let mut indices = Vec::with_capacity(vert_count);
            let mut colors = Vec::with_capacity(vert_count);

            for (vi, (tri, color)) in tris.iter().enumerate() {
                for v in 0..3 {
                    positions.push(tri.vertices[v]);
                    normals.push(tri.normals[v]);
                    indices.push((vi * 3 + v) as u32);
                    colors.push(*color);
                }
            }

            ArmorSubModel {
                name: format!("Armor_{}", zone_name),
                positions,
                normals,
                indices,
                colors,
                transform: None,
            }
        })
        .collect()
}

/// A named sub-model for multi-model ship export.
pub struct SubModel<'a> {
    pub name: String,
    pub visual: &'a VisualPrototype,
    pub geometry: &'a MergedGeometry<'a>,
    /// Optional world-space transform (column-major 4x4 matrix).
    /// If `None`, the sub-model is placed at the origin.
    pub transform: Option<[f32; 16]>,
    /// Group name for Blender outliner hierarchy (e.g. "Hull", "Main Battery").
    pub group: &'static str,
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
    let mut mat_cache = MaterialCache::new();

    // Collect mesh nodes grouped by category.
    let mut grouped_nodes: BTreeMap<&str, Vec<json::Index<json::Node>>> = BTreeMap::new();

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

        grouped_nodes.entry(sub.group).or_default().push(node);
    }

    // Add armor meshes grouped under "Armor".
    let mut armor_nodes: Vec<json::Index<json::Node>> = Vec::new();
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
            matrix: armor.transform,
            ..Default::default()
        });

        armor_nodes.push(node);
    }
    if !armor_nodes.is_empty() {
        grouped_nodes.insert("Armor", armor_nodes);
    }

    // Build scene hierarchy: one parent node per group.
    let mut scene_nodes = Vec::new();
    for (group_name, children) in &grouped_nodes {
        let parent = root.push(json::Node {
            children: Some(children.clone()),
            name: Some(group_name.to_string()),
            ..Default::default()
        });
        scene_nodes.push(parent);
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

    // Create scene with group parent nodes.
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
