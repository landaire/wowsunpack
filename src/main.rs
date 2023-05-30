use eyre::{Result, WrapErr};
use flate2::read::{DeflateDecoder, ZlibDecoder};
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
    /// default relative to the given idx directory as "../../../../res_packages"
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

    let resources = Mutex::new(Vec::new());
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

    let packages_dir = args.pkg_dir.or_else(|| {
        Some(
            paths[0]
                .parent()?
                .parent()?
                .parent()?
                .parent()?
                .join("res_packages"),
        )
    });

    paths.into_par_iter().try_for_each(|path| {
        resources.lock().unwrap().append(&mut load_idx_file(path)?);

        Ok::<(), eyre::Error>(())
    })?;

    let resources = resources.into_inner().unwrap();

    for file in resources {
        if file.filename.file_name().unwrap() == "GameParams.data" {
            println!("{:#X?}", file);
            if let Some(packages_dir) = packages_dir {
                println!(
                    "{:?}",
                    packages_dir.join(file.volume_info.filename.to_string())
                );
                let pkg_file = File::open(packages_dir.join(file.volume_info.filename.to_string()))
                    .expect("Input file does not exist");

                let mmap = unsafe { MmapOptions::new().map(&pkg_file)? };
                let end_offset = (file.file_info.offset + (file.file_info.size as u64)) as usize;

                let cursor = Cursor::new(&mmap[(file.file_info.offset as usize)..end_offset]);
                let mut decoder = DeflateDecoder::new(cursor);
                let mut out_data = Vec::new();
                decoder
                    .read_to_end(&mut out_data)
                    .expect("failed to decompress data");
                std::fs::write("out.bin", out_data).unwrap();
            }
            break;
        }
    }

    //println!("{}", resources.len());

    // let mut decoder = ZlibDecoder::new(File::open(&args.pkg).expect("Input file does not exist"));
    // let mut out_data = Vec::new();
    // decoder
    //     .read_to_end(&mut out_data)
    //     .expect("failed to decompress data");
    // std::fs::write("out.bin", out_data).unwrap();

    Ok(())
}
