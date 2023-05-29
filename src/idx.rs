use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{Cursor, Read, SeekFrom},
    path::PathBuf,
};

use binrw::{BinRead, NullString, PosValue};
use eyre::{Result, WrapErr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum IdxError {
    #[error("File has incorrect endian markers")]
    IncorrectEndian,
}

#[derive(Debug, BinRead)]
#[br(magic = 0x50465349u32)]
struct Header {
    endianness: u32,
    murmur_hash: u32,
    endianness2: u32,
}

#[derive(Debug, BinRead)]
struct ResourceMetadata {
    resources_count: u32,
    file_infos_count: u32,
    volumes_count: u32,
    unused: u32,
    resources_table_pointer: u64,
    file_infos_table_pointer: u64,
    volumes_table_pointer: u64,
}

#[derive(Debug, BinRead)]
#[br(import(offset: u64))]
struct PackedFileMetadata {
    resource_ptr: u64,
    filename_ptr: u64,
    id: u64,
    parent_id: u64,

    #[br(seek_before = SeekFrom::Start(offset + filename_ptr), restore_position)]
    filename: NullString,
    #[br(ignore)]
    cached_path: Option<PathBuf>,
}

#[derive(Debug, BinRead)]
struct FileInfo {
    resource_id: u64,
    volume_id: u64,
    offset: u64,
    compression_info: u64,
    size: u32,
    crc32: u32,
    unpacked_size: u32,
    padding: u32,
}

#[derive(Debug, Clone, BinRead)]
struct Volume {
    this_offset: PosValue<()>,
    len: u64,
    name_ptr: u64,
    volume_id: u64,
    #[br(seek_before = SeekFrom::Start(this_offset.pos + name_ptr), restore_position)]
    filename: NullString,
}

#[derive(Debug)]
pub struct Resource {
    filename: PathBuf,
    file_info: FileInfo,
    volume_info: Volume,
}

pub fn parse(data: &mut Cursor<&[u8]>) -> Result<()> {
    let header = Header::read_ne(data).wrap_err("Failed to parse header")?;
    if header.endianness != 0x20000000 && header.endianness2 != 0x40 {
        return Err(IdxError::IncorrectEndian.into());
    }

    let resources_header_position = data.position();
    let resource_metadata =
        ResourceMetadata::read_ne(data).wrap_err("Failed to parse resource metadata")?;

    let mut packed_resources = BTreeMap::new();
    if resource_metadata.resources_count != 0 {
        let mut metadata_offset =
            resources_header_position + resource_metadata.resources_table_pointer;
        data.set_position(metadata_offset);

        for _ in 0..resource_metadata.resources_count {
            metadata_offset = data.position();
            let file_metadata = PackedFileMetadata::read_ne_args(data, (metadata_offset,))
                .wrap_err("Failed to parse packed file metadata")?;

            packed_resources.insert(file_metadata.id, RefCell::new(file_metadata));
        }
    }

    let mut file_infos = BTreeMap::new();
    if resource_metadata.file_infos_count != 0 {
        let metadata_offset =
            resources_header_position + resource_metadata.file_infos_table_pointer;
        data.set_position(metadata_offset);

        for _ in 0..resource_metadata.file_infos_count {
            let file_info = FileInfo::read_ne(data).wrap_err("Failed to parse file info")?;
            file_infos.insert(file_info.resource_id, file_info);
        }
    }

    let mut volumes = BTreeMap::new();
    if resource_metadata.volumes_count != 0 {
        let metadata_offset = resources_header_position + resource_metadata.volumes_table_pointer;
        data.set_position(metadata_offset);

        for _ in 0..resource_metadata.volumes_count {
            let volume_info = Volume::read_ne(data).wrap_err("Failed to parse volume info")?;
            volumes.insert(volume_info.volume_id, volume_info);
        }
    }

    let mut packed_files = Vec::new();
    for (id, packed_file) in &packed_resources {
        let is_file;
        {
            let packed_file = packed_file.borrow();
            is_file = file_infos.contains_key(id);

            if is_file {
                if let Some(cached_path) = &packed_file.cached_path {
                    let file_info = file_infos
                        .remove(id)
                        .expect("failed to get file info for resource!");

                    let volume_info = volumes
                        .get(&file_info.volume_id)
                        .expect("failed to find volume");

                    packed_files.push(Resource {
                        filename: cached_path.clone(),
                        file_info: file_info,
                        volume_info: volume_info.clone(),
                    });
                    continue;
                }
            }
        }

        let mut partial_cached_path = None;
        let mut file_chain = vec![*id];

        // Build the file's path based on its parents
        {
            let packed_file = packed_file.borrow();

            let mut parent_id = packed_file.parent_id;
            while parent_id != 0xdbb1a1d1b108b927 {
                let parent = packed_resources
                    .get(&parent_id)
                    .expect("Failed to find parent packed resource!");
                let parent = parent.borrow();

                // As we go along in this algorithm we cache each parent's filename if it
                // hasn't been cached yet. If cached, we can save time by just using what's
                // already been computed.
                if let Some(cached_path) = parent.cached_path.as_ref() {
                    partial_cached_path = Some(cached_path.clone());
                    break;
                }

                file_chain.push(parent_id);
                parent_id = parent.parent_id;
            }
        }

        // Go through each of the files in the chain and:
        // 1. Ensure their paths are cached
        // 2. Build the rest of the filename starting from the `partial_cached_path`
        let mut result_path = partial_cached_path.unwrap_or(PathBuf::new());
        for file_id in file_chain.drain(..).rev() {
            let file_metadata = packed_resources.get(&file_id).unwrap();
            let mut file_metadata = file_metadata.borrow_mut();

            result_path = result_path.join(file_metadata.filename.to_string());
            file_metadata.cached_path = Some(result_path.clone());
        }

        // We don't care about directories
        if !is_file {
            continue;
        }

        let file_info = file_infos
            .remove(id)
            .expect("failed to get file info for resource!");

        let volume_info = volumes
            .get(&file_info.volume_id)
            .expect("failed to find volume");

        packed_files.push(Resource {
            filename: result_path,
            file_info: file_info,
            volume_info: volume_info.clone(),
        });
    }

    panic!("{:#X?}", &packed_files[..5]);

    Ok(())
}
