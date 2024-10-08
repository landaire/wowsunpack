use std::{path::PathBuf, rc::Rc};

use crate::data::idx::FileNode;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct SerializedFile {
    pub path: PathBuf,
    is_directory: bool,
    compressed_size: usize,
    compression_info: u64,
    unpacked_size: usize,
    crc32: u32,
}

#[derive(Debug, Serialize)]
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

        for (_child_name, child) in node.children() {
            nodes.push((Rc::clone(&this_path), child.clone()));
        }
    }

    // Sort the files for consistency since the ordering isn't guaranteed otherwise
    out.sort_by(|a, b| a.path.cmp(&b.path));

    out
}
