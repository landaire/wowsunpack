use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::{self, Cursor, Read, SeekFrom, Write},
    ops::Deref,
    path::{Component, Path, PathBuf},
    rc::{Rc, Weak},
    sync::Arc,
};

use binrw::{BinRead, NullString, PosValue};
use rayon::prelude::*;
use thiserror::Error;

use crate::pkg::PkgFileLoader;

#[derive(Debug, Error)]
pub enum IdxError {
    #[error("File has incorrect endian markers")]
    IncorrectEndian,
    #[error("File not found")]
    FileNotFound,
    #[error("I/O error")]
    IoError(#[from] io::Error),
    #[error("PKG loader error")]
    PkgError(#[from] crate::pkg::PkgError),
    #[error("BinRead error")]
    BinReadErr(#[from] binrw::Error),
}

/// Main struct describing an `.idx` file
#[derive(Debug, BinRead)]
pub struct IdxFile {
    /// The offset at which parsing began for this structure
    resources_meta_offset: PosValue<()>,
    /// Metadata describing resource/file/volume tables
    resources_metadata: ResourceMetadata,
    /// Files and directories
    #[br(count = resources_metadata.resources_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.resources_table_pointer))]
    resources: Vec<PackedFileMetadata>,
    /// File metadata
    #[br(count = resources_metadata.file_infos_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.file_infos_table_pointer))]
    file_infos: Vec<FileInfo>,
    /// Where resource data is actually stored
    #[br(count = resources_metadata.volumes_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.volumes_table_pointer))]
    volumes: Vec<Volume>,
}

#[derive(Debug, BinRead)]
#[br(magic = 0x50465349u32)]
struct Header {
    /// Helps identify if we're loading data on a system with incorrect endianness
    endianness: u32,
    /// Murmur3 hash of the whole file starting from the end of the header
    murmur_hash: u32,
    /// Format revision
    version: u32,
}

#[derive(Debug, BinRead)]
struct ResourceMetadata {
    /// Number of resources in the resources table
    resources_count: u32,
    /// Number of files in the file table
    file_infos_count: u32,
    /// Number of volumes in the volume table
    volumes_count: u32,
    unused: u32,
    /// Offset from the base of this struct to the resources table
    resources_table_pointer: u64,
    /// Offset from the base of this struct to the file infos table
    file_infos_table_pointer: u64,
    /// Offset from the base of this struct to the volumes table
    volumes_table_pointer: u64,
}
#[derive(Debug, Clone, BinRead)]
pub struct PackedFileMetadata {
    /// This struct's offset in the IDX file it was read from
    this_offset: PosValue<()>,
    /// ???? why did I name it this? possibly drunk and can't remember what this field
    /// represents
    resource_ptr: u64,

    /// The offset relative to this struct where the filename is stored
    filename_ptr: u64,
    /// This resource's ID
    id: u64,
    /// This resource's parent ID
    parent_id: u64,

    /// This resource's filename
    #[br(seek_before = SeekFrom::Start(this_offset.pos + filename_ptr), restore_position)]
    filename: NullString,
}

/// Meta information about a file
#[derive(Debug, Clone, BinRead)]
pub struct FileInfo {
    /// The resource ID which this file info belongs to
    pub resource_id: u64,
    /// The volume ID which this resource resides in
    pub volume_id: u64,
    /// This resource's offset within the volume
    pub offset: u64,
    /// How the file is compressed
    pub compression_info: u64,
    /// Compressed data size
    pub size: u32,
    /// Uncompressed file CRC32
    pub crc32: u32,
    /// Uncompressed file size
    pub unpacked_size: u32,
    pub padding: u32,
}

/// Meta informationa bout a volume
#[derive(Debug, Clone, BinRead)]
pub struct Volume {
    this_offset: PosValue<()>,
    /// The length of this volume's name
    pub len: u64,
    /// Where this volume's name is located relative to this struct (or perhaps the table?)
    pub name_ptr: u64,
    /// The volume's ID
    pub volume_id: u64,
    /// The volume's filename
    #[br(seek_before = SeekFrom::Start(this_offset.pos + name_ptr), restore_position)]
    pub filename: NullString,
}

/// A parsed resource
#[derive(Debug)]
pub struct Resource {
    pub filename: PathBuf,
    pub file_info: FileInfo,
    pub volume_info: Volume,
}

/// Represents a node in a FileTree
#[derive(Debug, Default, Clone)]
pub struct FileNode(pub Rc<RefCell<FileTree>>);
/// SAFETY: we're definitely skirting the rules a bit and this is NOT safe for
/// a public library, but is safe for how the main app uses FileNodes
///
/// 1. Do not clone within a rayon task
/// 2. Do not borrow mutably within a rayon task
unsafe impl Send for FileNode {}
unsafe impl Sync for FileNode {}

impl Deref for FileNode {
    type Target = Rc<RefCell<FileTree>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FileNode {
    /// Finds a node with the given path starting from this node
    pub fn find<P: AsRef<Path>>(&self, path: P) -> Result<FileNode, IdxError> {
        let path = path.as_ref();
        let mut current_tree_ptr = self.clone();
        let mut components = path.components();
        while let Some(component) = components.next() {
            match component {
                // This might be an absolute path
                Component::RootDir => {
                    if current_tree_ptr.borrow().is_root {
                        continue;
                    }
                }
                // This might be a relative path
                Component::Normal(name) => {
                    let mut found_child = None;
                    {
                        if let Some(child) = current_tree_ptr
                            .borrow()
                            .children
                            .get(name.to_str().expect("path could not be converted"))
                        {
                            found_child = Some(child.clone());
                        }
                    }

                    if let Some(found_child) = found_child {
                        current_tree_ptr = found_child;
                    } else {
                        return Err(IdxError::FileNotFound);
                    }
                }
                other => {
                    panic!("unexpected path component: {:?}", other);
                }
            }
        }

        Ok(current_tree_ptr)
    }

    /// Given a `path` and `pkg_loader`, this will first find the node at the given path and
    /// then copy its data to the given writer.
    pub fn read_file_at_path<P: AsRef<Path>, W: Write>(
        &self,
        path: P,
        pkg_loader: &PkgFileLoader,
        out_data: &mut W,
    ) -> Result<(), IdxError> {
        self.find(path)?.read_file(pkg_loader, out_data)
    }

    /// Copy this file's data to the given writer.
    pub fn read_file<W: Write>(
        &self,
        pkg_loader: &PkgFileLoader,
        out_data: &mut W,
    ) -> Result<(), IdxError> {
        // Build this file's full path
        //file.extract(path, destination)
        let this = self.0.borrow();
        pkg_loader.read(
            &this.volume_info.as_ref().unwrap().filename.to_string(),
            this.file_info.as_ref().unwrap(),
            out_data,
        )?;

        Ok(())
    }

    /// This file's full path.
    /// TODO: Cache for optimization. Currently O(n) with path components.
    pub fn path(&self) -> Result<PathBuf, IdxError> {
        // Assume at least 4 parts to the path
        let mut path_parts = Vec::with_capacity(4);
        let mut node = Some(self.clone());
        while let Some(current_node) = node {
            let current_node = current_node.0.borrow();
            path_parts.push(current_node.filename.clone());

            node = current_node
                .parent
                .as_ref()
                .map(|p| FileNode(p.upgrade().expect("failed to get parent node")));
        }

        Ok(path_parts.as_slice().iter().rev().collect())
    }

    /// Extract this file to disk at the given `path`
    pub fn extract_to<P: AsRef<Path>>(
        &self,
        path: P,
        pkg_loader: &PkgFileLoader,
    ) -> Result<(), IdxError> {
        self.extract_to_path_with_callback(path, pkg_loader, || {
            // do nothing
        })
    }

    /// Extract this file to disk at the given `path`, executing the given `callback` after
    /// a file is written.
    pub fn extract_to_path_with_callback<P: AsRef<Path>, F>(
        &self,
        path: P,
        pkg_loader: &PkgFileLoader,
        callback: F,
    ) -> Result<(), IdxError>
    where
        F: Fn() -> () + Send + Sync,
    {
        let out_path = path.as_ref();
        if !out_path.exists() {
            std::fs::create_dir_all(out_path).unwrap();
        }

        let child_nodes_count = { self.0.borrow().children.len() };

        let mut nodes = Vec::with_capacity(1 + child_nodes_count);
        nodes.push((Arc::new(out_path.to_owned()), self.clone()));

        let mut files = Vec::new();

        // Enumerate all nodes that will need to be handled
        let mut idx = 0;
        while idx < nodes.len() {
            let (target_path, node) = nodes[idx].clone();

            // TODO: cleanup borrows
            let is_file;
            let this_node_path;
            {
                let node = node.0.borrow();
                is_file = node.file_info.is_some();
                if node.is_root {
                    this_node_path = Arc::clone(&target_path);
                } else {
                    this_node_path = Arc::new(target_path.join(&node.filename));
                }
            };

            if is_file {
                files.push((this_node_path, node))
            } else {
                let node = node.0.borrow();
                let this_node_path = this_node_path;
                if !this_node_path.exists() {
                    std::fs::create_dir(&*this_node_path)?;
                }

                for (_child_name, child) in &node.children {
                    nodes.push((Arc::clone(&this_node_path), child.clone()));
                }
            }

            idx += 1;
        }

        files.par_iter().try_for_each(|(this_node_path, node)| {
            let mut out_file = File::create(this_node_path.as_ref())?;
            let res = node.read_file(pkg_loader, &mut out_file);

            (callback)();

            res
        })?;

        Ok(())
    }

    /// Returns a `Vec<(full path, node)>` of this node and all of its children
    pub fn paths(&self) -> Vec<(Rc<PathBuf>, FileNode)> {
        let mut out_nodes = Vec::new();
        let mut pending_nodes: Vec<(Rc<PathBuf>, FileNode)> = vec![(
            Rc::new(PathBuf::from(&self.0.borrow().filename)),
            self.clone(),
        )];

        while let Some((parent_path, node_ptr)) = pending_nodes.pop() {
            let full_path = {
                let node = node_ptr.0.borrow();
                let full_path = Rc::new(parent_path.join(&node.filename));
                for (_child_name, child) in &node.children {
                    pending_nodes.push((Rc::clone(&full_path), child.clone()));
                }

                full_path
            };

            out_nodes.push((full_path, node_ptr));
        }

        out_nodes
    }
}

/// Weak file node to avoid cyclic references
#[derive(Debug, Clone, Default)]
pub struct WeakFileNode(Weak<RefCell<FileTree>>);
/// SAFETY: see comment above -- not really safe for generic library usage
unsafe impl Send for WeakFileNode {}
unsafe impl Sync for WeakFileNode {}
impl Deref for WeakFileNode {
    type Target = Weak<RefCell<FileTree>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// A FileTree is essentially a lightweight filesystem tree but with a clunkier API
#[derive(Debug)]
pub struct FileTree {
    pub filename: String,
    pub children: BTreeMap<String, FileNode>,
    pub parent: Option<WeakFileNode>,
    pub is_file: bool,
    pub file_info: Option<FileInfo>,
    pub volume_info: Option<Volume>,
    pub is_root: bool,
}

impl FileTree {
    fn new() -> FileTree {
        FileTree {
            filename: "".to_owned(),
            children: Default::default(),
            parent: None,
            is_file: false,
            file_info: None,
            volume_info: None,
            is_root: false,
        }
    }
}

impl Default for FileTree {
    fn default() -> Self {
        Self::new()
    }
}

/// Parse an `idx` file
pub fn parse(data: &mut Cursor<&[u8]>) -> Result<IdxFile, IdxError> {
    let header = Header::read_ne(data)?;
    if header.endianness != 0x20000000 && header.version != 0x40 {
        return Err(IdxError::IncorrectEndian.into());
    }

    IdxFile::read_ne(data).map_err(IdxError::from)
}

/// Builds a file tree from a slice of index files
pub fn build_file_tree(idx_files: &[IdxFile]) -> FileNode {
    // Create hash lookups for each resource, file info, and volume
    let mut packed_resources = BTreeMap::new();
    let mut file_infos = BTreeMap::new();
    let mut volumes = BTreeMap::new();

    for idx_file in idx_files {
        for resource in &idx_file.resources {
            packed_resources.insert(resource.id, resource.clone());
        }

        for file_info in &idx_file.file_infos {
            file_infos.insert(file_info.resource_id, file_info);
        }

        for volume_info in &idx_file.volumes {
            volumes.insert(volume_info.volume_id, volume_info);
        }
    }

    let file_tree = FileNode::default();
    {
        file_tree.borrow_mut().is_root = true;
    }
    let mut cached_nodes = BTreeMap::<u64, WeakFileNode>::new();

    // Now that we have lookup tables, let's build a list of Resources by path
    for (id, packed_file) in &packed_resources {
        if cached_nodes.contains_key(id) {
            continue;
        }

        let mut root_node = file_tree.clone();
        let mut file_chain = vec![*id];

        // Build the file's path based on its parents
        {
            let mut parent_id = packed_file.parent_id;
            while parent_id != 0xdbb1a1d1b108b927 {
                let parent = packed_resources
                    .get(&parent_id)
                    .expect("Failed to find parent packed resource!");

                // As we go along in this algorithm we cache each parent's filename if it
                // hasn't been cached yet. If cached, we can save time by just using what's
                // already been computed.
                if let Some(cached_path) = cached_nodes.get(&parent_id) {
                    root_node = FileNode(
                        cached_path
                            .0
                            .upgrade()
                            .expect("failed to upgrade weak node ptr"),
                    );
                    break;
                }

                file_chain.push(parent_id);
                parent_id = parent.parent_id;
            }
        }

        // Go through each of the files in the chain and:
        // 1. Ensure their nodes are cached
        // 2. Build the rest of the chain starting from the `root_node`
        for file_in_chain_id in file_chain.drain(..).rev() {
            let filename;
            let file_metadata = packed_resources.get(&file_in_chain_id).unwrap();
            {
                filename = file_metadata.filename.to_string();
            }

            let this_node_ptr = FileNode::default();

            root_node
                .borrow_mut()
                .children
                .insert(filename.clone(), this_node_ptr.clone());

            {
                let mut this_node = this_node_ptr.borrow_mut();
                {
                    cached_nodes.insert(
                        file_in_chain_id,
                        WeakFileNode(Rc::downgrade(&this_node_ptr.0)),
                    );
                }

                let file_info = file_infos.get(&file_in_chain_id).cloned();
                let volume_info = file_info.and_then(|file_info| volumes.get(&file_info.volume_id));

                this_node.is_file = file_info.is_some();
                this_node.file_info = file_info.cloned();
                this_node.volume_info = volume_info.map(|v| *v).cloned();

                this_node.parent = Some(WeakFileNode(Rc::downgrade(&root_node.0)));
                this_node.filename = filename;
            }

            root_node = this_node_ptr;
        }
    }

    file_tree
}
