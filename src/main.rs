use eyre::{Result, WrapErr};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use glob::glob;
use memmap::MmapOptions;
use std::{
    collections::HashSet,
    convert,
    fs::{self, File, FileType},
    io::{BufWriter, Cursor, Read},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Mutex,
    time::Instant,
};
use wowsunpack::pkg::PkgFileLoader;
use wowsunpack::{idx, serialization};

use clap::{Parser, Subcommand, ValueEnum};
use rayon::prelude::*;

/// Utility for interacting with World of Warships game assets
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
    /// Write meta information about the game assets to the specified output file.
    /// This may be useful for diffing contents between builds at a glance. Output
    /// data includes file name, size, CRC32, unpacked size, compression info,
    /// and a flag indicating if the file is a directory.
    ///
    /// The output data will always be sorted by filename.
    Metadata {
        #[clap(short, long)]
        format: MetadataFormat,

        out_file: PathBuf,
    },
    /// Special command for directly reading the `content/GameParams.data` file,
    /// converting it to JSON, and writing to the specified output file path.
    GameParams {
        /// Don't pretty-print the JSON (may make serialization/deserialization faster)
        #[clap(short, long)]
        ugly: bool,
        #[clap(default_value = "GameParams.json")]
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

    Ok(wowsunpack::idx::parse(&mut reader)?)
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

    match args.command {
        Commands::Extract {
            flatten,
            files,
            mut out_dir,
        } => {
            let paths = file_tree.paths();
            let globs = files
                .iter()
                .map(|file_name| glob::Pattern::new(&file_name).expect("invalid glob pattern"))
                .collect::<Vec<_>>();

            let mut extracted_paths = HashSet::<&Path>::new();

            for (path, node) in &paths {
                let mut matches = false;

                for glob in &globs {
                    if glob.matches_path(&*path) {
                        matches = true;
                        break;
                    }
                }

                // Skip this node if the file path doesn't match OR we're told to
                // flatten the file system
                if !matches || (flatten && !node.0.borrow().is_file) {
                    continue;
                }

                // Also skip this path if its parent directory has already been extracted
                if let Some(parent) = path.parent() {
                    if extracted_paths.contains(parent) {
                        continue;
                    }
                }

                extracted_paths.insert((&*path).as_ref());

                match pkg_loader.as_mut() {
                    Some(pkg_loader) => {
                        node.extract_to(&out_dir, pkg_loader)?;
                    }
                    None => {
                        return Err(eyre::eyre!(
                            "Package file loader is unavailable. Check that the pkg_dir exists."
                        ));
                    }
                }
            }
        }
        Commands::Metadata { format, out_file } => {
            let data = serialization::tree_to_serialized_files(file_tree.clone());
            let out_file = File::create(out_file)?;
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
        Commands::GameParams { out_file, ugly } => match pkg_loader.as_mut() {
            Some(pkg_loader) => {
                let mut writer = BufWriter::new(File::create(out_file)?);
                wowsunpack::game_params::read_game_params_as_json(
                    !ugly,
                    file_tree.clone(),
                    pkg_loader,
                    &mut writer,
                )?;
            }
            None => {
                return Err(eyre::eyre!(
                    "Package file loader is unavailable. Check that the pkg_dir exists."
                ));
            }
        },
    }

    println!(
        "Parsed resources in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    Ok(())
}
