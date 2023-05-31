use std::{
    collections::HashMap,
    fs::File,
    io::{self, Cursor, Read, Write},
    path::{Path, PathBuf},
};

use flate2::read::DeflateDecoder;
use memmap::{Mmap, MmapOptions};
use thiserror::Error;

use crate::idx::FileInfo;

#[derive(Debug)]
pub struct PkgFileLoader {
    pkgs_dir: PathBuf,
    pkgs: HashMap<PathBuf, (File, memmap::Mmap)>,
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
            pkgs: HashMap::new(),
        }
    }

    fn ensure_pkg_loaded<P: AsRef<Path>>(&mut self, pkg: P) -> Result<&Mmap, PkgError> {
        let pkg = pkg.as_ref().to_owned();
        if !self.pkgs.contains_key(&pkg) {
            let pkg_path = self.pkgs_dir.join(&pkg);
            if !pkg_path.exists() {
                return Err(PkgError::PkgNotFound(pkg));
            }

            let pkg_file = File::open(pkg_path).expect("Input file does not exist");

            let mmap = unsafe { MmapOptions::new().map(&pkg_file)? };

            self.pkgs.insert(pkg.clone(), (pkg_file, mmap));
        }

        Ok(&self.pkgs.get(&pkg).unwrap().1)
    }

    pub fn read<P: AsRef<Path>, W: Write>(
        &mut self,
        pkg: P,
        file_info: &FileInfo,
        out_data: &mut W,
    ) -> Result<(), PkgError> {
        let mmap = self.ensure_pkg_loaded(pkg)?;

        let start_offset = file_info.offset as usize;
        let end_offset = start_offset + (file_info.size as usize);

        let cursor = Cursor::new(&mmap[start_offset..end_offset]);
        let mut decoder = DeflateDecoder::new(cursor);

        std::io::copy(&mut decoder, out_data)?;

        Ok(())
    }
}
