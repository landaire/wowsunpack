# WoWs .geometry File Format

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

The `.geometry` file is a **BigWorld/Wargaming Moo engine** binary format that stores
merged 3D mesh data: vertex buffers, index buffers, and associated metadata. The file
is designed to be memory-mapped; internal pointers are stored as **relative offsets**
that are resolved at load time.

## Pointer Convention

All pointer fields are stored as **`i64` relative offsets**. Resolution depends on
context:

- **Header-level** fields: resolved as `struct_base + value`. Since the header is at
  file offset 0, these are effectively absolute file offsets.
- **Sub-struct** fields: resolved as `sub_struct_base + value`.
- **PackedString** text pointers: resolved as `packed_string_base + value`.

A value of `0` represents a null pointer.

## Top-Level Structure: `MergedGeometryPrototype`

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    mergedVerticesCount    # number of VerticesPrototype entries
0x04    4     u32    mergedIndicesCount     # number of IndicesPrototype entries
0x08    4     u32    verticesMappingCount   # number of MappingEntry entries (vertex)
0x0C    4     u32    indicesMappingCount    # number of MappingEntry entries (index)
0x10    4     u32    collisionModelCount    # number of CollisionModelPrototype entries
0x14    4     u32    armorModelCount        # number of ArmorModelPrototype entries
0x18    8     i64    verticesMappingPtr     # -> MappingEntry[] (relative to file start)
0x20    8     i64    indicesMappingPtr      # -> MappingEntry[] (relative to file start)
0x28    8     i64    mergedVerticesPtr      # -> VerticesPrototype[] (relative to file start)
0x30    8     i64    mergedIndicesPtr       # -> IndicesPrototype[] (relative to file start)
0x38    8     i64    collisionModelsPtr     # -> CollisionModelPrototype[] (relative to file start)
0x40    8     i64    armorModelsPtr         # -> ArmorModelPrototype[] (relative to file start)
```

Total header size: **0x48 (72) bytes**.

## MappingEntry (0x10 bytes each)

Maps a named resource (identified by hash) to a slice of a merged vertex/index buffer.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    mappingId            # hash identifier for this render group
0x04    2     u16    mergedBufferIndex    # which merged buffer this maps to
0x06    2     u16    packedTexelDensity   # encoded texel density value
0x08    4     u32    itemsOffset          # start offset (in items) within the merged buffer
0x0C    4     u32    itemsCount           # number of items (vertices or indices)
```

## PackedString

A variable-length string stored as a counted reference.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    charCount      # number of characters (including null terminator)
0x04    4     ---    (padding)
0x08    8     i64    textPtr        # relative to this struct's base -> char[]
```

Total struct size: **0x10 (16) bytes**. The text data is stored out-of-line, typically
after the associated data blob.

## VerticesPrototype (0x20 bytes each)

Describes a merged vertex buffer.

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          verticesDataPtr    # relative to this struct -> raw data blob
0x08    16    PackedString formatName         # e.g. "set3/xyznuvtbpc"
0x18    4     u32          sizeInBytes        # total byte size of the data blob
0x1C    2     u16          strideInBytes      # per-vertex stride (e.g. 28, 32)
0x1E    1     u8           isSkinned          # 1 if skinned mesh
0x1F    1     u8           isBumped           # 1 if bump-mapped
```

### Vertex Data Blob

The data pointed to by `verticesDataPtr` may be either:

1. **ENCD-encoded** (compressed): starts with magic `0x44434E45` (`"ENCD"` in ASCII).
   Uses [meshoptimizer](https://github.com/zeux/meshoptimizer) vertex buffer encoding.
2. **Raw**: uncompressed vertex data, `sizeInBytes` total.

#### ENCD Header (8 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    magic          # 0x44434E45 = "ENCD"
0x04    4     u32    elementCount   # number of vertices/indices
```

Followed by the meshoptimizer-encoded payload. Decode with:
```
meshopt_decodeVertexBuffer(output, elementCount, strideInBytes,
                           encoded_data + 8, sizeInBytes - 8)
```

### Vertex Format Names

The `formatName` string encodes the vertex attribute layout. Known format:
`"set3/xyznuvtbpc"` where each group of characters after the `/` describes
vertex components. Format strings use the BigWorld vertex declaration naming
convention.

## IndicesPrototype (0x10 bytes each)

Describes a merged index buffer.

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    8     i64    indicesDataPtr   # relative to this struct -> raw data blob
0x08    4     u32    sizeInBytes      # total byte size of the index data blob
0x0C    2     u16    (reserved)
0x0E    2     u16    indexSize        # bytes per index: 2 = u16, 4 = u32
```

The index data blob follows the same ENCD encoding scheme as vertex data.
Decode with:
```
meshopt_decodeIndexBuffer(output, elementCount, indexSize,
                          encoded_data + 8, sizeInBytes - 8)
```

Where `elementCount = EncodedBufferHeader.elementCount` from the ENCD header.

## CollisionModelPrototype (0x20 bytes each)

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          cmDataPtr            # relative to this struct -> raw data blob
0x08    16    PackedString collisionModelName   # e.g. "CM_something"
0x18    4     u32          sizeInBytes          # total byte size
0x1C    4     ---          (padding)
```

## ArmorModelPrototype (0x20 bytes each)

Same layout as CollisionModelPrototype:

```
Offset  Size  Type         Field
------  ----  ----         -----
0x00    8     i64          armorDataPtr       # relative to this struct -> raw data blob
0x08    16    PackedString armorModelName     # e.g. "CM_PA_united.armor"
0x18    4     u32          sizeInBytes        # total byte size
0x1C    4     ---          (padding)
```

## File Layout Example

For a typical ship model (`BSA013_Colossus_1945.geometry`, 192,311 bytes):

```
0x00000-0x00047  Header (72 bytes)
0x00048-0x00067  verticesMapping[2] (32 bytes)
0x00068-0x00087  indicesMapping[2] (32 bytes)
0x00088-0x000A7  VerticesPrototype[1] (32 bytes)
0x000A8-0x0119E  vertexData[0] blob (4343 bytes, ENCD-encoded)
0x0119F-0x011AE  formatName[0] text (16 bytes: "set3/xyznuvtbpc\0")
0x011AF-0x011BE  IndicesPrototype[1] (16 bytes)
0x011BF-0x012C3  indexData[0] blob (261 bytes, ENCD-encoded)
0x012C4-0x012E3  ArmorModelPrototype[1] (32 bytes)
0x012E4-0x2EF03  (unmapped region - 187,424 bytes)
0x2EF04-0x2EF23  armorData[0] blob (32 bytes)
0x2EF24-0x2EF36  armorModelName[0] text (19 bytes: "CM_PA_united.armor\0")
```

The large unmapped region likely contains additional mesh data (primitive groups,
bounding boxes, etc.) referenced by the `.visual` file system rather than the
`.geometry` header.

## Binary Ninja Annotations

The following functions and types have been annotated in the Binary Ninja database:

### Functions
| Address        | Name                                    | Purpose                              |
|----------------|-----------------------------------------|--------------------------------------|
| `0x140483660`  | `MergedGeometryPrototype_deserialize`   | Deserializes the top-level structure |
| `0x1404841e0`  | `VerticesPrototype_deserialize`         | Deserializes vertex buffer metadata  |
| `0x140484590`  | `IndicesPrototype_deserialize`          | Deserializes index buffer metadata   |
| `0x1404847c0`  | `CollisionModelPrototype_deserialize`   | Deserializes collision model metadata|
| `0x140484a00`  | `ArmorModelPrototype_deserialize`       | Deserializes armor model metadata    |
| `0x140483dc0`  | `MappingArray_deserialize`              | Deserializes mapping arrays          |
| `0x140483f40`  | `MappingEntry_deserialize`              | Deserializes individual mapping entry|
| `0x140484c40`  | `PackedString_deserialize`              | Deserializes packed string structs   |
| `0x140456c50`  | `Moo_Vertices_loadFromPrototype`        | Loads vertex buffer from prototype   |
| `0x140457390`  | `Moo_Primitive_loadFromPrototype`       | Loads index buffer from prototype    |
| `0x140459240`  | `Moo_GeometryManager_createManagedObjects` | Creates GPU resources from prototypes |
| `0x14047f590`  | `Moo_GeometryData_fetchVertices`        | Fetches vertices by mapping ID       |
| `0x140a5a940`  | `MeshDataOptimizer_decodeVertexData`    | Decodes ENCD vertex data             |
| `0x140a5ab20`  | `MeshDataOptimizer_decodeIndexData_u16` | Decodes ENCD index data (u16)        |
| `0x140a5ad00`  | `MeshDataOptimizer_decodeIndexData_u32` | Decodes ENCD index data (u32)        |
| `0x140a5a880`  | `EncodedBufferHeader_checkHeaderData`   | Validates ENCD magic/count           |
| `0x1413fa420`  | `meshopt_decodeVertexBuffer`            | meshoptimizer vertex decode          |
| `0x1413fa610`  | `meshopt_decodeIndexBuffer`             | meshoptimizer index decode           |

### Source Paths (from debug strings)
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\vertices.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\primitive.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\geometry_data.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\moo\geometry_manager.cpp`
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\mesh_data_optimizer\coder.cpp`

---

# WoWs `assets.bin` File Format (PrototypeDatabase)

Reverse-engineered from `WorldOfWarships64.exe` using Binary Ninja.

The `assets.bin` file (located at `res/content/assets.bin`) is a **BigWorld engine
PrototypeDatabase** binary format. It serves as the master asset index, mapping
resource identifiers to prototype data blobs. The file is designed for memory-mapping
with relative pointers resolved at load time.

Source file: `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\resmgr\resmgr_prototype_database.cpp`

## Pointer Convention

All pointer fields are stored as **`i64` relative offsets**. Each offset is resolved
relative to the **start of its containing structure** (`arg1[2]` in the deserialization
code). Specifically:

- **Body-level** fields (strings, databases count/relptr): resolved as `body_base + value`
  where `body_base` is the first byte after the 16-byte header.
- **Sub-section** fields (resourceToPrototypeMap, pathsStorage): resolved as
  `section_base + value` where `section_base = body_base + section_offset`.
- **Entry-level** fields (database data relptr, path name relptr): resolved as
  `entry_base + value`.

A value of `0` represents a null pointer.

## Header (16 bytes)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    magic           # 0x42574442 = "BWDB" (BigWorld DataBase)
0x04    4     u32    version         # 0x01010000
0x08    4     u32    checksum        # CRC32 of the body (everything after header)
0x0C    2     u16    architecture    # 0x0040 = 64-bit
0x0E    2     u16    endianness      # 0x0000 = little-endian
```

## Body Header (0x60 = 96 bytes, starting at file offset 0x10)

The body contains five logical sections packed into a 96-byte header:

```
Offset  Size  Type   Field                              Section
------  ----  ----   -----                              -------
+0x00   4     u32    offsetsMap.capacity                 strings
+0x04   4     ---    (padding)
+0x08   8     i64    offsetsMap.buckets_relptr           strings (rel. to body_base)
+0x10   8     i64    offsetsMap.values_relptr            strings (rel. to body_base)
+0x18   4     u32    stringData.size                     strings
+0x1C   4     ---    (padding)
+0x20   8     i64    stringData.relptr                   strings (rel. to body_base)
+0x28   4     u32    resourceToPrototypeMap.capacity     r2p
+0x2C   4     ---    (padding)
+0x30   8     i64    resourceToPrototypeMap.buckets_relptr  r2p (rel. to body_base+0x28)
+0x38   8     i64    resourceToPrototypeMap.values_relptr   r2p (rel. to body_base+0x28)
+0x40   4     u32    pathsStorage.count                  paths
+0x44   4     ---    (padding)
+0x48   8     i64    pathsStorage.data_relptr            paths (rel. to body_base+0x40)
+0x50   4     u32    databasesCount                      databases
+0x54   4     ---    (padding)
+0x58   8     i64    databases.relptr                    databases (rel. to body_base)
```

## Strings Section (offsetsMap + string data)

A hashmap-based string deduplication table. The `offsetsMap` maps string content
hashes to offsets within the `stringData` byte array.

### OffsetsMap Hashmap

Uses open addressing with linear probing. Slot = `name_id % capacity`.

- **capacity**: Number of hash buckets
- **buckets**: Array of `capacity` entries, each 8 bytes: `(u32 key, u32 sentinel)`.
  - `key`: The 32-bit string name hash (MurmurHash3). 0 when slot is empty.
  - `sentinel`: Has bit 31 set (0x80000000+) when occupied. 0 when empty.
- **values**: Array of `capacity` entries, each 4 bytes (`u32`).
  Contains offsets into the string data array.

Prototype records use `u32` name IDs (e.g. `nameId`, `materialNameId` in RenderSet)
that are looked up through this hashmap to get the string data offset.

### String Data

A contiguous pool of null-terminated UTF-8 strings. Strings are referenced by
offset into this pool. Typical content includes vertex format names, material
names, and other text identifiers.

## ResourceToPrototypeMap

A hashmap mapping resource IDs (64-bit hashes) to prototype locations.
Uses open addressing with linear probing. Slot = `selfId % capacity`.

- **capacity**: Number of hash buckets
- **buckets**: Array of `capacity` entries, each **16 bytes**.
  - Bytes 0-7 (`u64`): The key (`selfId` from pathsStorage)
  - Bytes 8-15 (`u64`): Occupancy sentinel (1 = occupied, 0 = empty)
- **values**: Array of `capacity` entries, each 4 bytes (`u32`).
  Encoded prototype location:
  ```
  value = (record_index << 8) | (blob_index * 4)
  ```
  - Low byte (`value & 0xFF`): `blob_index * 4` (type tag)
  - Upper 24 bits (`value >> 8`): record index within that database blob

## PathsStorage

An array of path metadata entries. Each entry associates a unique resource ID with
a parent ID and a display name.

### PathEntry (32 bytes each)

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    8     u64    selfId          # unique resource identifier (hash)
0x08    8     u64    parentId        # parent resource identifier (hash or index)
0x10    4     u32    name.size       # length of name string (including null terminator)
0x14    4     ---    (padding)
0x18    8     i64    name.data_relptr  # relative to entry_base + 0x10 -> char[]
```

The name strings are stored in a separate contiguous pool located between the
pathsStorage entries and the database entries. Typical names include:
`"OGB202_Dunkirk_dead.model"`, `"JSB023_Izumo_1945.visual"`, etc.

## Database Entries

An array of `databasesCount` database descriptors, each 0x18 (24) bytes:

```
Offset  Size  Type   Field
------  ----  ----   -----
0x00    4     u32    prototypeMagic      # prototype type hash (validated at load time)
0x04    4     u32    prototypeChecksum   # prototype checksum (validated at load time)
0x08    4     u32    size                # size of the data blob in bytes
0x0C    4     ---    (padding)
0x10    8     i64    data_relptr         # relative to entry_base -> u8[] data blob
```

The data blobs are contiguous and collectively consume the remainder of the file.
Each database represents a different prototype type (e.g., visual, model, geometry,
skeleton, material). The `prototypeMagic` and `prototypeChecksum` values are
validated against a static table during loading.

## Prototype Types

The PrototypeDatabase contains 10 registered prototype types. Each type has a
magic value (MurmurHash3_x86_32 of the type name string), a fixed item size,
and a corresponding database blob.

| Idx | Type Name                  | Magic      | Item Size   | Registration Fn  |
|-----|----------------------------|------------|-------------|------------------|
| 0   | MaterialPrototype          | 0x5069C471 | 0x78 (120B) | sub_140026de0    |
| 1   | VisualPrototype            | 0x480DC57B | 0x70 (112B) | sub_140026f40    |
| 2   | SkeletonExtenderPrototype  | 0x1AE023FF | 0x20 (32B)  | sub_140035cb0    |
| 3   | ModelPrototype             | 0xA9576F28 | 0x28 (40B)  | sub_140035b20    |
| 4   | PointLightPrototype        | 0x0D3665A4 | 0x70 (112B) | sub_1400658e0    |
| 5   | EffectPrototype            | 0xEB23E0AF | 0x10 (16B)  | sub_140033cc0    |
| 6   | VelocityFieldPrototype     | 0xAFD4A63F | 0x18 (24B)  | sub_140034190    |
| 7   | EffectPresetPrototype      | 0x42E15336 | 0x10 (16B)  | sub_140033e50    |
| 8   | EffectMetadataPrototype    | 0xDFC8F8E0 | 0x10 (16B)  | sub_140033b30    |
| 9   | AtlasContourProto          | 0xF64359AA | 0x10 (16B)  | sub_140033fb0    |

### Database Blob Structure

Each blob has a 16-byte header followed by fixed-size records and out-of-line data:

```
Offset  Size          Content
------  ----          -------
+0x00   8             count (u64 — number of records)
+0x08   8             header_size (u64 — always 16)
+0x10   count*item    Fixed-size records (item_size bytes each)
+...    remainder     Out-of-line (OOL) data: variable-length arrays, strings
```

Relative pointers (i64) in records point into the OOL region. The base for
resolving relptrs is always the start of the containing struct:
- Top-level record fields: base = record start
- Sub-struct fields (e.g. RenderSet, LOD): base = sub-struct start

## File Layout Example

For a typical `assets.bin` (170,699,420 bytes):

```
0x00000000 - 0x00000010          16 bytes  Header (BWDB magic, version, checksum, arch)
0x00000010 - 0x00000070          96 bytes  Body Header (section descriptors)
0x00000070 - 0x00600078   6,291,464 bytes  offsetsMap.buckets (786,433 x 8)
0x00600078 - 0x0090007C   3,145,732 bytes  offsetsMap.values (786,433 x 4)
0x0090007C - 0x010507A4   7,669,544 bytes  strings.data (null-terminated string pool)
0x010507A4 - 0x01650934   6,291,856 bytes  r2p.buckets (393,241 x 16)
0x01650934 - 0x017D0998   1,572,964 bytes  r2p.values (393,241 x 4)
0x017D0998 - 0x01F52FB8   7,874,080 bytes  pathsStorage entries (246,065 x 32)
0x01F52FB8 - 0x0260379E   7,014,374 bytes  path name strings pool
0x0260379E - 0x0260388E         240 bytes  database entries (10 x 24)
0x0260388E - 0x0A2CAA9C 131,408,654 bytes  database data blobs (10 databases)
```

No gaps or overlaps; every byte is accounted for.

## Binary Ninja Annotations

### Functions
| Address        | Name                                              | Purpose                                |
|----------------|---------------------------------------------------|----------------------------------------|
| `0x140a15980`  | `PrototypeDatabase_load`                          | Loads and validates BWDB file          |
| `0x140a16210`  | `PrototypeDatabase_initStaticDatabase`            | Initializes static type registry       |
| `0x140a178c0`  | `PrototypeDatabase_deserialize`                   | Top-level body deserialization         |
| `0x140a17c50`  | `PrototypeDatabase_deserialize_strings`            | Deserializes strings section           |
| `0x140a17ec0`  | `PrototypeDatabase_deserialize_resourceToPrototypeMap` | Deserializes r2p hashmap         |
| `0x140a18180`  | `PrototypeDatabase_deserialize_pathsStorage`       | Deserializes path entries array        |
| `0x140a18380`  | `PrototypeDatabase_deserialize_database`           | Deserializes a single database entry   |
| `0x140a18660`  | `PrototypeDatabase_deserialize_offsetsMap`         | Deserializes offsetsMap hashmap        |
| `0x140a18930`  | `PrototypeDatabase_deserialize_pathEntry`          | Deserializes a single path entry       |
| `0x140a18ae0`  | `PrototypeDatabase_deserialize_packedString`       | Deserializes packed string struct      |

### Source Paths (from debug strings)
- `D:\Source\Build\SOURCE\WOWS_GIT_SPARSE\client\source\lib\resmgr\resmgr_prototype_database.cpp`

---

## Dependencies

- **meshoptimizer** (`meshopt-rs` crate): Required for decoding ENCD-compressed vertex
  and index buffers.
- **winnow**: Used for binary parsing in the Rust implementation.
