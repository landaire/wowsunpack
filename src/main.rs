use eyre::{Result, WrapErr};
use flate2::read::ZlibDecoder;
use memmap::MmapOptions;
use std::{
    fs::{self, File, FileType},
    io::{Cursor, Read},
    path::PathBuf,
    sync::Mutex,
};

use clap::Parser;
use rayon::prelude::*;

mod idx;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory where pkg files are located. If not provided, this will
    /// default relative to the given idx directory as "../../../res_packages"
    #[clap(short, long)]
    pkg_dir: Option<PathBuf>,

    /// .idx file(s)
    idx: Vec<PathBuf>,
}

fn load_idx_file(path: PathBuf) -> Result<Vec<idx::Resource>> {
    let input_file = File::open(path).wrap_err("Failed to open idx file")?;
    let mmap = unsafe { MmapOptions::new().map(&input_file)? };

    let mut reader = Cursor::new(&mmap[..]);

    crate::idx::parse(&mut reader)
}

fn main() -> Result<()> {
    let args = Args::parse();

    let mut resources = Mutex::new(Vec::new());
    let mut paths = Vec::with_capacity(args.idx.len());
    for path in args.idx {
        if path.is_dir() {
            for path in fs::read_dir(path)? {
                let path = path?;
                if path.file_type()?.is_file() {
                    paths.push(path.path());
                }
            }
        } else {
            paths.push(path);
        }
    }
    paths.into_par_iter().try_for_each(|path| {
        resources.lock().unwrap().append(&mut load_idx_file(path)?);

        Ok::<(), eyre::Error>(())
    })?;

    let resources = resources.into_inner().unwrap();

    println!("{}", resources.len());

    // let mut decoder = ZlibDecoder::new(File::open(&args.pkg).expect("Input file does not exist"));
    // let mut out_data = Vec::new();
    // decoder
    //     .read_to_end(&mut out_data)
    //     .expect("failed to decompress data");
    // std::fs::write("out.bin", out_data).unwrap();

    Ok(())
}
