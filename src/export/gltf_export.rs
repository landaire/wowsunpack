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
    /// Collision material ID (0–255) from the BVH node header.
    pub material_id: u8,
    /// Human-readable material name (e.g. "Cit_Belt", "Bow_Bottom").
    pub material_name: String,
    /// Zone classification (e.g. "Citadel", "Bow", "Superstructure").
    pub zone: String,
    /// Armor thickness in millimeters from GameParams (0.0 if unassigned).
    pub thickness_mm: f32,
    /// `true` when the triangle's material ID was unknown and its zone was
    /// determined via splash-box spatial lookup rather than the material table.
    /// These plates are not shown in the game's armor viewer.
    pub hidden: bool,
    /// RGBA color [0.0–1.0] encoding the thickness via the game's color scale.
    pub color: [f32; 4],
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
    /// Optional world-space transform (column-major 4×4).
    /// Used for turret armor instances positioned at mount points.
    /// Hull armor meshes have `None` (already in world space).
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub transform: Option<[f32; 16]>,
}

impl InteractiveArmorMesh {
    /// Build an `InteractiveArmorMesh` from a parsed `ArmorModel`.
    ///
    /// `armor_map` is the GameParams armor thickness data:
    ///   key = `(model_index << 16) | material_id`, value = thickness in mm.
    /// `model_index` is the 1-based index of this armor model in the geometry file.
    pub fn from_armor_model(
        armor: &crate::models::geometry::ArmorModel,
        armor_map: Option<&std::collections::HashMap<u32, f32>>,
        model_index: u32,
        splash_boxes: Option<&[crate::models::geometry::SplashBox]>,
        hit_locations: Option<&HashMap<String, crate::game_params::types::HitLocation>>,
    ) -> Self {
        let tri_count = armor.triangles.len();
        let vert_count = tri_count * 3;
        let mut positions = Vec::with_capacity(vert_count);
        let mut normals = Vec::with_capacity(vert_count);
        let mut indices = Vec::with_capacity(vert_count);
        let mut colors = Vec::with_capacity(vert_count);
        let mut triangle_info = Vec::with_capacity(tri_count);

        for (ti, tri) in armor.triangles.iter().enumerate() {
            let key = (model_index << 16) | (tri.material_id as u32);
            let thickness_mm = armor_map.and_then(|m| m.get(&key).copied()).unwrap_or(0.0);
            let color = thickness_to_color(thickness_mm);
            let mat_name = collision_material_name(tri.material_id);
            let mut zone = zone_from_material_name(mat_name).to_string();
            let mut hidden = false;

            // Splash-box fallback for unknown materials — these are hidden plates.
            if zone == "Other" {
                if let (Some(sbs), Some(hls)) = (splash_boxes, hit_locations) {
                    let centroid = triangle_centroid(&tri.vertices);
                    if let Some(z) = zone_from_splash_boxes(centroid, sbs, hls) {
                        zone = z.to_string();
                        hidden = true;
                    }
                }
            }

            for v in 0..3 {
                positions.push(tri.vertices[v]);
                normals.push(tri.normals[v]);
                indices.push((ti * 3 + v) as u32);
                colors.push(color);
            }

            triangle_info.push(ArmorTriangleInfo {
                material_id: tri.material_id,
                material_name: mat_name.to_string(),
                zone,
                thickness_mm,
                hidden,
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
    /// Optional world-space transform (column-major 4×4).
    /// Used for turret armor instances positioned at mount points.
    pub transform: Option<[f32; 16]>,
}

impl ArmorSubModel {
    /// Build an `ArmorSubModel` from a parsed `ArmorModel`.
    ///
    /// `armor_map` is the GameParams armor thickness data:
    ///   key = `(model_index << 16) | material_id`, value = thickness in mm.
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
            // Look up thickness by material ID (not triangle index).
            let key = (model_index << 16) | (tri.material_id as u32);
            let thickness_mm = armor_map.and_then(|m| m.get(&key).copied()).unwrap_or(0.0);

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
    // Collision material prefixes.
    if mat_name.starts_with("Bow") {
        return "Bow";
    }
    if mat_name.starts_with("St_") || mat_name == "St" {
        return "Stern";
    }
    if mat_name.starts_with("Cit") || mat_name.starts_with("Ammo") {
        return "Citadel";
    }
    if mat_name.starts_with("Cas") {
        return "Casemate";
    }
    if mat_name.starts_with("SS") {
        return "Superstructure";
    }
    if mat_name.starts_with("Tur") {
        return "Turret";
    }
    if mat_name.starts_with("Rudder") || mat_name == "SG" {
        return "SteeringGear";
    }
    if mat_name.starts_with("Bulge") {
        return "TorpedoProtection";
    }
    match mat_name {
        "Deck" | "ConstrSide" | "Hull" => "Hull",
        "common" | "zero" => "Default",
        _ => "Other",
    }
}

/// Classify an armor triangle into a hit-location zone using splash-box AABBs.
///
/// For each splash box, tests whether `centroid` lies inside the AABB.
/// If a match is found, looks up which `HitLocation` lists that box name
/// and returns the normalized zone name. The hit location key from GameParams
/// (e.g. "Cit", "Cas", "SS") is passed through `zone_from_material_name` to
/// produce standard zone names (e.g. "Citadel", "Casemate", "Superstructure").
pub fn zone_from_splash_boxes(
    centroid: [f32; 3],
    splash_boxes: &[crate::models::geometry::SplashBox],
    hit_locations: &HashMap<String, crate::game_params::types::HitLocation>,
) -> Option<&'static str> {
    // Armor triangles on splash-box boundaries (bulkheads, hull transitions) have
    // centroids exactly on the AABB face. Use a small tolerance so they still match.
    const EPS: f32 = 0.15;
    for sb in splash_boxes {
        let inside =
            (0..3).all(|i| centroid[i] >= sb.min[i] - EPS && centroid[i] <= sb.max[i] + EPS);
        if !inside {
            continue;
        }
        // Found a splash box containing this point — find which hit location owns it.
        for (zone_key, hl) in hit_locations {
            if hl.splash_boxes().iter().any(|name| name == &sb.name) {
                let normalized = zone_from_material_name(zone_key);
                if normalized != "Other" {
                    return Some(normalized);
                }
                // If the key itself doesn't match prefix rules, try hl_type.
                let from_type = zone_from_material_name(hl.hl_type());
                if from_type != "Other" {
                    return Some(from_type);
                }
                return Some(normalized);
            }
        }
        // No hit location claims this box — infer zone from the splash box name.
        // Names follow the pattern "CM_SB_<zone>_<rest>".
        if let Some(zone) = zone_from_splash_box_name(&sb.name) {
            return Some(zone);
        }
    }
    None
}

/// Infer a zone from a splash box name like `CM_SB_bow_1`, `CM_SB_gk_2_1`, etc.
fn zone_from_splash_box_name(name: &str) -> Option<&'static str> {
    let suffix = name.strip_prefix("CM_SB_")?;
    let tag = suffix.split('_').next()?;
    match tag {
        "bow" => Some("Bow"),
        "stern" => Some("Stern"),
        "cit" => Some("Citadel"),
        "cas" => Some("Casemate"),
        "ss" => Some("Superstructure"),
        "ruder" => Some("SteeringGear"),
        "gk" => Some("Turret"),
        "engine" => Some("Citadel"),
        _ => None,
    }
}

/// Compute the centroid of a triangle from its three vertices.
fn triangle_centroid(verts: &[[f32; 3]; 3]) -> [f32; 3] {
    [
        (verts[0][0] + verts[1][0] + verts[2][0]) / 3.0,
        (verts[0][1] + verts[1][1] + verts[2][1]) / 3.0,
        (verts[0][2] + verts[1][2] + verts[2][2]) / 3.0,
    ]
}

/// The built-in collision material name table.
///
/// Index = material ID (u8), value = name string. Extracted from the game client.
/// Only includes entries actually used by ship armor models.
const COLLISION_MATERIAL_NAMES: &[(u8, &str)] = &[
    (0, "common"),
    (1, "zero"),
    (2, "Dual_SSC_Bow_Side"),
    (3, "Dual_SSC_Bow_Top"),
    (4, "Dual_SSC_Bow_Bottom"),
    (5, "Dual_SSC_St_Side"),
    (6, "Dual_SSC_St_Top"),
    (7, "Dual_SSC_St_Bottom"),
    (8, "Dual_Cas_OCit_Belt"),
    (9, "Dual_Cas_OCit_Deck"),
    (10, "Dual_Cas_OCit_Bottom"),
    (11, "Dual_Cas_OCit_Trans"),
    (12, "Dual_SSC_Cas_Side"),
    (13, "Dual_SSC_Cas_Top"),
    (14, "Dual_SSC_Cas_Bottom"),
    (15, "Dual_Cit_OCas_Belt"),
    (16, "Dual_CasT_OCitT"),
    (17, "Dual_Cit_OCas_Deck"),
    (18, "Dual_Cit_OCas_Bottom"),
    (19, "Dual_Cit_OCas_Trans"),
    (20, "Dual_Cit_OCas_FwdTrans"),
    (21, "Dual_Cit_OCas_AftTrans"),
    (22, "Dual_SSC_Cit_Side"),
    (23, "Dual_SSC_Cit_Top"),
    (47, "Bow_Bottom"),
    (48, "Bow_ConstrSide"),
    (49, "Bow_Deck"),
    (50, "Bow_Side"),
    (51, "Bow_Trans"),
    (52, "Bow_Top"),
    (53, "BridgeSide"),
    (54, "BridgeTop"),
    (55, "Cas_AftTrans"),
    (56, "Cas_Belt"),
    (57, "Cas_Deck"),
    (58, "Cas_FwdTrans"),
    (59, "Cit_AftTrans"),
    (60, "Cit_AftWall"),
    (61, "Cit_Belt"),
    (62, "Cit_Bottom"),
    (63, "Cit_Bulge"),
    (64, "Cit_Deck"),
    (65, "Cit_FwdTrans"),
    (66, "Cit_FwdWall"),
    (67, "Cit_Side"),
    (68, "Cit_Top"),
    (69, "ConstrSide"),
    (70, "DD_Belt"),
    (71, "DD_Bottom"),
    (72, "DD_Deck"),
    (73, "DD_Side"),
    (74, "DD_Top"),
    (75, "Deck"),
    (76, "Mid_Belt"),
    (77, "Mid_Bottom"),
    (78, "Mid_Deck"),
    (79, "Mid_Side"),
    (80, "RudderAft"),
    (81, "RudderFwd"),
    (82, "RudderSide"),
    (83, "RudderTop"),
    (84, "Side"),
    (85, "Bottom"),
    (86, "Top"),
    (87, "SSC_Belt"),
    (88, "SSC_Deck"),
    (89, "SS_Side"),
    (90, "SS_Top"),
    (91, "SS_Bottom"),
    (92, "St_Bottom"),
    (93, "St_ConstrSide"),
    (94, "St_Deck"),
    (95, "St_Side"),
    (96, "St_Trans"),
    (97, "St_Top"),
    (98, "CV_Belt"),
    (99, "CV_Deck"),
    (100, "CV_Bottom"),
    (101, "Bulge"),
    (102, "Cas_Bottom"),
    (103, "Deck"),
    (104, "Cas_Top"),
    (105, "Cit_AftBulge"),
    (106, "Cit_FwdBulge"),
    (107, "SS_BridgeTop"),
    (108, "SS_BridgeSide"),
    (109, "SS_BridgeBottom"),
    // Turret barbettes (GkBar), undersides (GkDown), tops (GkTop) for up to 20 turrets.
    // Pattern: base + turret_number, where base is 134/174/214 for Bar/Down/Top.
    (134, "Tur1GkBar"),
    (135, "Tur2GkBar"),
    (136, "Tur3GkBar"),
    (137, "Tur4GkBar"),
    (138, "Tur5GkBar"),
    (174, "Tur1GkDown"),
    (175, "Tur2GkDown"),
    (176, "Tur3GkDown"),
    (177, "Tur4GkDown"),
    (178, "Tur5GkDown"),
    (214, "Tur1GkTop"),
    (215, "Tur2GkTop"),
    (216, "Tur3GkTop"),
    (217, "Tur4GkTop"),
    (218, "Tur5GkTop"),
];

/// Look up the collision material name for a given material ID.
pub fn collision_material_name(id: u8) -> &'static str {
    COLLISION_MATERIAL_NAMES
        .iter()
        .find(|(mid, _)| *mid == id)
        .map(|(_, name)| *name)
        .unwrap_or("unknown")
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
    armor_map: Option<&std::collections::HashMap<u32, f32>>,
    model_index: u32,
    splash_boxes: Option<&[crate::models::geometry::SplashBox]>,
    hit_locations: Option<&HashMap<String, crate::game_params::types::HitLocation>>,
) -> Vec<ArmorSubModel> {
    // Group triangles by zone name.
    let mut zone_tris: std::collections::BTreeMap<
        String,
        Vec<(&crate::models::geometry::ArmorTriangle, [f32; 4])>,
    > = std::collections::BTreeMap::new();

    for tri in &armor.triangles {
        let key = (model_index << 16) | (tri.material_id as u32);
        let thickness_mm = armor_map.and_then(|m| m.get(&key).copied()).unwrap_or(0.0);

        let mat_name = collision_material_name(tri.material_id);
        let mut zone_name = zone_from_material_name(mat_name).to_string();

        // Splash-box fallback for unknown materials.
        if zone_name == "Other" {
            if let (Some(sbs), Some(hls)) = (splash_boxes, hit_locations) {
                let centroid = triangle_centroid(&tri.vertices);
                if let Some(z) = zone_from_splash_boxes(centroid, sbs, hls) {
                    zone_name = z.to_string();
                }
            }
        }

        let color = thickness_to_color(thickness_mm);

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
