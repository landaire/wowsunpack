//! Export ship visual + geometry to glTF/GLB format.

use std::borrow::Cow;
use std::collections::BTreeMap;
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
}

/// Export a visual + geometry pair to a GLB binary and write it.
pub fn export_glb(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: usize,
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
    let primitives = collect_primitives(visual, geometry, db, lod_entry)?;

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

    for prim in &primitives {
        let gltf_prim = add_primitive_to_root(&mut root, &mut bin_data, prim)?;
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

/// Collect and decode all render set primitives for a given LOD.
fn collect_primitives(
    visual: &VisualPrototype,
    geometry: &MergedGeometry,
    db: &PrototypeDatabase<'_>,
    lod: &crate::models::visual::Lod,
) -> Result<Vec<DecodedPrimitive>, Report<ExportError>> {
    let mut result = Vec::new();

    for &rs_name_id in &lod.render_set_names {
        // Find the render set with this name_id.
        let rs = visual
            .render_sets
            .iter()
            .find(|rs| rs.name_id == rs_name_id)
            .ok_or_else(|| Report::new(ExportError::RenderSetNotFound(rs_name_id)))?;

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

        result.push(DecodedPrimitive {
            positions,
            normals,
            uvs,
            indices,
            material_name,
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

/// Add a decoded primitive's data to the glTF root and binary buffer.
/// Returns the glTF Primitive JSON object.
fn add_primitive_to_root(
    root: &mut json::Root,
    bin_data: &mut Vec<u8>,
    prim: &DecodedPrimitive,
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
            buffer: json::Index::new(0), // will be updated later
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

    // Create material stub.
    let material = root.push(json::Material {
        name: Some(prim.material_name.clone()),
        ..Default::default()
    });

    Ok(json::mesh::Primitive {
        attributes,
        indices: indices_accessor,
        material: Some(material),
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
        extensions: Default::default(),
        extras: Default::default(),
    })
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
pub fn export_ship_glb(
    sub_models: &[SubModel<'_>],
    db: &PrototypeDatabase<'_>,
    lod: usize,
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
        let primitives = collect_primitives(sub.visual, sub.geometry, db, lod_entry)?;

        if primitives.is_empty() {
            eprintln!(
                "Warning: sub-model '{}' has no primitives for LOD {lod}",
                sub.name
            );
            continue;
        }

        let mut gltf_primitives = Vec::new();
        for prim in &primitives {
            let gltf_prim = add_primitive_to_root(&mut root, &mut bin_data, prim)?;
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
