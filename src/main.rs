use eyre::{Result, WrapErr};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use memmap::MmapOptions;
use pkg::PkgFileLoader;
use std::{
    fs::{self, File, FileType},
    io::{Cursor, Read},
    path::PathBuf,
    sync::Mutex,
    time::Instant,
};

use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;

mod idx;
mod pkg;
mod serialization;

/// Utility for interacting with World of Warships `.idx` and `.pkg` files
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Directory where pkg files are located. If not provided, this will
    /// default relative to the given idx directory as "../../../../res_packages"
    #[clap(short, long)]
    pkg_dir: Option<PathBuf>,

    /// .idx file(s) or their containing directory
    #[clap(short, long)]
    idx_files: Vec<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Extract files to an output directory
    Extract {
        /// Flatten the file structure when writing files. For example, if the
        /// given output directory is `res` and the target file is `content/GameParams.data`,
        /// the file will normally be written to `res/content/GameParams.data`.
        /// When this flag is set, the file will be written to `res/GameParams.data`
        #[clap(long)]
        flatten: bool,

        /// Files to extract. Glob patterns such as `content/**/*.xml` are accepted
        files: Vec<String>,

        /// Where to write files to
        out_dir: PathBuf,
    },
    Metadata {
        #[clap(short, long)]
        format: MetadataFormat,

        out_file: PathBuf,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
enum MetadataFormat {
    Json,
    Csv,
}

fn load_idx_file(path: PathBuf) -> Result<idx::IdxFile> {
    let input_file = File::open(&path).wrap_err("Failed to open idx file")?;
    let mmap = unsafe { MmapOptions::new().map(&input_file)? };

    let mut reader = Cursor::new(&mmap[..]);

    Ok(crate::idx::parse(&mut reader)?)
}

fn main() -> Result<()> {
    let timestamp = Instant::now();
    let args = Args::parse();

    let resources = Mutex::new(Vec::new());
    let mut paths = Vec::with_capacity(args.idx_files.len());

    for path in args.idx_files {
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

    let mut pkg_loader = packages_dir.as_ref().map(|dir| PkgFileLoader::new(&dir));

    paths.into_par_iter().try_for_each(|path| {
        resources.lock().unwrap().push(load_idx_file(path)?);

        Ok::<(), eyre::Error>(())
    })?;

    let idx_files = resources.into_inner().unwrap();
    let file_tree = idx::build_file_tree(&idx_files);

    match &args.command {
        Commands::Extract {
            flatten,
            files,
            out_dir,
        } => todo!(),
        Commands::Metadata { format, out_file } => {
            let mut data = serialization::tree_to_serialized_files(file_tree.clone());
            let mut out_file = File::create(out_file)?;
            match format {
                MetadataFormat::Json => {
                    serde_json::to_writer(out_file, &data)?;
                }
                MetadataFormat::Csv => {
                    let mut writer = csv::Writer::from_writer(out_file);
                    for record in data {
                        writer.serialize(record)?;
                    }
                }
            };
        }
    }

    // if let Some(pkg_loader) = pkg_loader.as_mut() {
    //     file_tree.extract_to("res", pkg_loader)?;
    // }

    // if let Ok(node) = resource.find("content/GameParams.data") {
    //     if let Some(pkg_loader) = pkg_loader.as_mut() {
    //         let mut file = File::create("out.bin")?;
    //         node.read_file(pkg_loader, &mut file)?;
    //         panic!("{:#X?}", node.path());
    //     }
    // }

    // for file in &resources {
    //     if file.filename.file_name().unwrap() == "GameParams.data" {
    //         println!("{:#X?}", file);
    //         if let Some(packages_dir) = packages_dir {
    //             println!(
    //                 "{:?}",
    //                 packages_dir.join(file.volume_info.filename.to_string())
    //             );
    //             let pkg_file = File::open(packages_dir.join(file.volume_info.filename.to_string()))
    //                 .expect("Input file does not exist");

    //             let mmap = unsafe { MmapOptions::new().map(&pkg_file)? };
    //             let end_offset = (file.file_info.offset + (file.file_info.size as u64)) as usize;

    //             let cursor = Cursor::new(&mmap[(file.file_info.offset as usize)..end_offset]);
    //             let mut decoder = DeflateDecoder::new(cursor);
    //             let mut out_data = Vec::new();
    //             decoder
    //                 .read_to_end(&mut out_data)
    //                 .expect("failed to decompress data");
    //             std::fs::write("out.bin", out_data).unwrap();
    //         }
    //         break;
    //     }
    // }

    println!(
        "Parsed resources in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    // let mut decoder = ZlibDecoder::new(File::open(&args.pkg).expect("Input file does not exist"));
    // let mut out_data = Vec::new();
    // decoder
    //     .read_to_end(&mut out_data)
    //     .expect("failed to decompress data");
    // std::fs::write("out.bin", out_data).unwrap();

    Ok(())
}
