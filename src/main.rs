use eyre::{Result, WrapErr};
use flate2::read::ZlibDecoder;
use memmap::MmapOptions;
use std::{
    fs::File,
    io::{Cursor, Read},
    path::PathBuf,
};

use clap::Parser;

mod idx;

/// Simple program to greet a person
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Name of the person to greet
    pkg: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let input_file = File::open(&args.pkg).wrap_err("Failed to open idx file")?;
    let mmap = unsafe { MmapOptions::new().map(&input_file)? };

    let mut reader = Cursor::new(&mmap[..]);

    crate::idx::parse(&mut reader)?;

    let mut decoder = ZlibDecoder::new(File::open(&args.pkg).expect("Input file does not exist"));
    let mut out_data = Vec::new();
    decoder
        .read_to_end(&mut out_data)
        .expect("failed to decompress data");
    std::fs::write("out.bin", out_data).unwrap();

    Ok(())
}
