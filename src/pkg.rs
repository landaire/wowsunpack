use std::{
    collections::HashMap,
    fs::File,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::RwLock,
};

use flate2::read::DeflateDecoder;
use memmap::{Mmap, MmapOptions};
use thiserror::Error;

use crate::idx::FileInfo;

#[derive(Debug)]
pub struct PkgFileLoader {
    pkgs_dir: PathBuf,
    pkgs: RwLock<HashMap<PathBuf, (File, memmap::Mmap)>>,
}

#[derive(Debug, Error)]
pub enum PkgError {
    #[error("PKG file {0} not found")]
    PkgNotFound(PathBuf),
    #[error("I/O error")]
    IoError(#[from] io::Error),
}

impl PkgFileLoader {
    pub fn new<P: AsRef<Path>>(pkgs_dir: P) -> Self {
        PkgFileLoader {
            pkgs_dir: pkgs_dir.as_ref().into(),
            pkgs: Default::default(),
        }
    }

    fn ensure_pkg_loaded<P: AsRef<Path>>(&self, pkg: P) -> Result<(), PkgError> {
        let pkg = pkg.as_ref().to_owned();
        let pkg_loaded = { self.pkgs.read().unwrap().contains_key(&pkg) };
        if !pkg_loaded {
            let pkg_path = self.pkgs_dir.join(&pkg);
            if !pkg_path.exists() {
                return Err(PkgError::PkgNotFound(pkg));
            }

            let pkg_file = File::open(pkg_path).expect("Input file does not exist");

            let mmap = unsafe { MmapOptions::new().map(&pkg_file)? };

            self.pkgs
                .write()
                .unwrap()
                .insert(pkg.clone(), (pkg_file, mmap));
        }

        Ok(())
    }

    pub fn read<P: AsRef<Path>, W: Write>(
        &self,
        pkg: P,
        file_info: &FileInfo,
        out_data: &mut W,
    ) -> Result<(), PkgError> {
        let pkg = pkg.as_ref();
        self.ensure_pkg_loaded(pkg)?;
        let pkgs = self.pkgs.read().unwrap();
        let mmap = &pkgs.get(pkg).unwrap().1;

        let start_offset = file_info.offset as usize;
        let end_offset = start_offset + (file_info.size as usize);

        let mut cursor = Cursor::new(&mmap[start_offset..end_offset]);
        if file_info.compression_info != 0 {
            let mut decoder = DeflateDecoder::new(cursor);

            std::io::copy(&mut decoder, out_data)?;
        } else {
            std::io::copy(&mut cursor, out_data)?;
        }

        Ok(())
    }
}
