use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::{Cursor, Read, SeekFrom, Write},
    ops::Deref,
    path::{Component, Path, PathBuf},
    rc::{Rc, Weak},
    thread::current,
};

use binrw::{BinRead, NullString, PosValue};
use eyre::{Result, WrapErr};
use thiserror::Error;

use crate::pkg::PkgFileLoader;

#[derive(Debug, Error)]
pub enum IdxError {
    #[error("File has incorrect endian markers")]
    IncorrectEndian,
    #[error("File not found")]
    FileNotFound,
}

#[derive(Debug, BinRead)]
struct IdxFile {
    resources_meta_offset: PosValue<()>,
    resources_metadata: ResourceMetadata,
    #[br(count = resources_metadata.resources_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.resources_table_pointer))]
    resources: Vec<PackedFileMetadata>,
    #[br(count = resources_metadata.file_infos_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.file_infos_table_pointer))]
    file_infos: Vec<FileInfo>,
    #[br(count = resources_metadata.volumes_count, seek_before = SeekFrom::Start(resources_meta_offset.pos + resources_metadata.volumes_table_pointer))]
    volumes: Vec<Volume>,
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
struct PackedFileMetadata {
    this_offset: PosValue<()>,
    resource_ptr: u64,
    filename_ptr: u64,
    id: u64,
    parent_id: u64,

    #[br(seek_before = SeekFrom::Start(this_offset.pos + filename_ptr), restore_position)]
    filename: NullString,
    #[br(ignore)]
    cached_tree_node: Option<WeakFileNode>,
}

#[derive(Debug, Clone, BinRead)]
pub struct FileInfo {
    pub resource_id: u64,
    pub volume_id: u64,
    pub offset: u64,
    pub compression_info: u64,
    pub size: u32,
    pub crc32: u32,
    pub unpacked_size: u32,
    pub padding: u32,
}

#[derive(Debug, Clone, BinRead)]
pub struct Volume {
    this_offset: PosValue<()>,
    pub len: u64,
    pub name_ptr: u64,
    pub volume_id: u64,
    #[br(seek_before = SeekFrom::Start(this_offset.pos + name_ptr), restore_position)]
    pub filename: NullString,
}

#[derive(Debug)]
pub struct Resource {
    pub filename: PathBuf,
    pub file_info: FileInfo,
    pub volume_info: Volume,
}

#[derive(Debug, Default, Clone)]
pub struct FileNode(Rc<RefCell<FileTree>>);
unsafe impl Send for FileNode {}
unsafe impl Sync for FileNode {}
impl Deref for FileNode {
    type Target = Rc<RefCell<FileTree>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl FileNode {
    pub fn find<P: AsRef<Path>>(&self, path: P) -> Result<FileNode, IdxError> {
        let path = path.as_ref();
        let mut current_tree_ptr = self.clone();
        let mut components = path.components();
        while let Some(component) = components.next() {
            match component {
                // This might be an absolute path
                Component::RootDir => {
                    if current_tree_ptr.borrow().filename == "/" {
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

    pub fn read_file_at_path<P: AsRef<Path>, W: Write>(
        &self,
        path: P,
        pkg_loader: &mut PkgFileLoader,
        out_data: &mut W,
    ) -> Result<()> {
        self.find(path)?.read_file(pkg_loader, out_data)
    }

    pub fn read_file<W: Write>(
        &self,
        pkg_loader: &mut PkgFileLoader,
        out_data: &mut W,
    ) -> Result<()> {
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

    pub fn path(&self) -> Result<PathBuf> {
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
}

#[derive(Debug, Default)]
pub struct WeakFileNode(Weak<RefCell<FileTree>>);
unsafe impl Send for WeakFileNode {}
unsafe impl Sync for WeakFileNode {}
impl Deref for WeakFileNode {
    type Target = Weak<RefCell<FileTree>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug)]
pub struct FileTree {
    pub filename: String,
    pub children: BTreeMap<String, FileNode>,
    pub parent: Option<WeakFileNode>,
    pub is_file: bool,
    pub file_info: Option<FileInfo>,
    pub volume_info: Option<Volume>,
}

impl FileTree {
    fn new() -> FileTree {
        FileTree {
            filename: "/".to_owned(),
            children: Default::default(),
            parent: None,
            is_file: false,
            file_info: None,
            volume_info: None,
        }
    }
}

impl Default for FileTree {
    fn default() -> Self {
        Self::new()
    }
}

pub fn parse(data: &mut Cursor<&[u8]>) -> Result<FileNode> {
    let header = Header::read_ne(data).wrap_err("Failed to parse header")?;
    if header.endianness != 0x20000000 && header.endianness2 != 0x40 {
        return Err(IdxError::IncorrectEndian.into());
    }

    let idx_file = IdxFile::read_ne(data).wrap_err("Failed to parse IdxFile")?;

    // Create hash lookups for each resource, file info, and volume
    let mut packed_resources = BTreeMap::new();
    for resource in idx_file.resources {
        packed_resources.insert(resource.id, RefCell::new(resource));
    }

    let mut file_infos = BTreeMap::new();
    for file_info in idx_file.file_infos {
        file_infos.insert(file_info.resource_id, file_info);
    }

    let mut volumes = BTreeMap::new();
    for volume_info in idx_file.volumes {
        volumes.insert(volume_info.volume_id, volume_info);
    }

    let file_tree = FileNode::default();

    // Now that we have lookup tables, let's build a list of Resources by path
    for (id, packed_file) in &packed_resources {
        {
            let packed_file = packed_file.borrow();
            if packed_file.cached_tree_node.is_some() {
                continue;
            }
        }

        let mut root_node = file_tree.clone();
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
                if let Some(cached_path) = parent.cached_tree_node.as_ref() {
                    root_node = FileNode(
                        cached_path
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
                filename = file_metadata.borrow().filename.to_string();
            }

            let this_node_ptr = FileNode::default();

            root_node
                .borrow_mut()
                .children
                .insert(filename.clone(), this_node_ptr.clone());

            {
                let mut this_node = this_node_ptr.borrow_mut();
                {
                    file_metadata.borrow_mut().cached_tree_node =
                        Some(WeakFileNode(Rc::downgrade(&this_node_ptr.0)));
                }

                let file_info = file_infos.remove(&file_in_chain_id);
                let volume_info = file_info
                    .as_ref()
                    .and_then(|file_info| volumes.get(&file_info.volume_id));

                this_node.is_file = file_info.is_some();
                this_node.file_info = file_info;
                this_node.volume_info = volume_info.cloned();
                this_node.filename = filename;
                this_node.parent = Some(WeakFileNode(Rc::downgrade(&root_node.0)));
            }

            root_node = this_node_ptr;
        }
    }

    Ok(file_tree)
}
