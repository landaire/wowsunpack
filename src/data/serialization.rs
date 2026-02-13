use std::{path::PathBuf, rc::Rc};

use crate::data::idx::FileNode;
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SerializedFile {
    pub path: PathBuf,
    is_directory: bool,
    compressed_size: usize,
    compression_info: u64,
    unpacked_size: usize,
    crc32: u32,
}

impl SerializedFile {
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn is_directory(&self) -> bool {
        self.is_directory
    }

    pub fn compressed_size(&self) -> usize {
        self.compressed_size
    }

    pub fn compression_info(&self) -> u64 {
        self.compression_info
    }

    pub fn unpacked_size(&self) -> usize {
        self.unpacked_size
    }

    pub fn crc32(&self) -> u32 {
        self.crc32
    }
}

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct SerializedFileInfo {}

pub fn tree_to_serialized_files(node: FileNode) -> Vec<SerializedFile> {
    let mut out = Vec::new();

    let mut nodes = vec![(Rc::new(PathBuf::new()), node)];
    while let Some((path, node)) = nodes.pop() {
        let this_path = path.join(node.filename());
        let (compressed_size, compression_info, unpacked_size, crc32) = node
            .file_info()
            .map(|file_info| {
                (
                    file_info.size as usize,
                    file_info.compression_info,
                    file_info.unpacked_size as usize,
                    file_info.crc32,
                )
            })
            .unwrap_or_default();

        let file = SerializedFile {
            path: this_path.clone(),
            is_directory: !node.is_file(),
            compressed_size,
            compression_info,
            unpacked_size,
            crc32,
        };

        out.push(file);

        let this_path = Rc::new(this_path);

        for child in node.children().values() {
            nodes.push((Rc::clone(&this_path), child.clone()));
        }
    }

    // Sort the files for consistency since the ordering isn't guaranteed otherwise
    out.sort_by(|a, b| a.path.cmp(&b.path));

    out
}
