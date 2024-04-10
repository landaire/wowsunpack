use eyre::{Result, WrapErr};

use memmap::MmapOptions;
use std::{
    collections::HashSet,
    fs::{self, File},
    io::{stdout, BufWriter, Cursor, Write},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    },
    time::Instant,
};
use wowsunpack::pkg::PkgFileLoader;
use wowsunpack::{idx, serialization};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
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

        /// Do not preserve the matched file path when writing output files. For example,
        /// if `gui/achievements` is passed as a `files` arg and `res_unpacked` is the `out_dir`, it would normally
        /// extract as `res_unpacked/gui/achievements/`. Enabling this option will instead extract as
        /// `res_unpacked/achievements/` -- stripping the matched part which is not part of the filename
        /// or its children.
        #[clap(long)]
        strip_prefix: bool,

        /// Where to extract files to
        #[clap(short, long, default_value = "wowsunpack_extracted")]
        out_dir: PathBuf,

        /// Files to extract. Glob patterns such as `content/**/*.xml` are accepted
        files: Vec<String>,
    },
    /// Write meta information about the game assets to the specified output file.
    /// This may be useful for diffing contents between builds at a glance. Output
    /// data includes file name, size, CRC32, unpacked size, compression info,
    /// and a flag indicating if the file is a directory.
    ///
    /// The output data will always be sorted by filename.
    Metadata {
        #[clap(short, long, default_value_t = MetadataFormat::Plain, value_enum)]
        format: MetadataFormat,

        /// A value of "-" will print to stdout
        #[clap(default_value = "-")]
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
    Plain,
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
    let mut args = Args::parse();
    // If we didn't get any idx dirs/files passed to us, try auto-detecting the
    // WoWs directory
    if args.idx_files.is_empty() {
        let mut latest_version = None;
        if Path::new("WorldOfWarships.exe").exists() {
            // Maybe we are? Try enumerating the `bin` directory
            let paths = fs::read_dir("bin")
                .wrap_err("No index files were provided and could not enumerate `bin` directory")?;
            for path in paths {
                let path = path.wrap_err("could not enumerate path")?;
                if path.file_type()?.is_dir() {
                    if let Ok(version) = u64::from_str_radix(path.file_name().to_str().unwrap(), 10)
                    {
                        match latest_version {
                            Some(other_version) => {
                                if other_version < version {
                                    latest_version = Some(version);
                                }
                            }
                            None => latest_version = Some(version),
                        }
                    }
                }
            }
        }

        if let Some(latest_version) = latest_version {
            let latest_version_str = format!("{}", latest_version);

            args.idx_files
                .push(["bin", latest_version_str.as_str(), "idx"].iter().collect())
        }

        if latest_version.is_none() || !args.idx_files[0].exists() {
            Args::command().print_help()?;

            eprintln!("");

            return Err(eyre::eyre!("Could not find game idx files. Either provide the path(s) manually or make sure your game installation folder is well-formed"));
        }
    }

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
            out_dir,
            strip_prefix,
        } => {
            let paths = file_tree.paths();
            let globs = files
                .iter()
                .map(|file_name| glob::Pattern::new(&file_name).expect("invalid glob pattern"))
                .collect::<Vec<_>>();

            let mut extracted_paths = HashSet::<&Path>::new();
            let files_written = AtomicUsize::new(0);

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
                if !matches || (flatten && !node.is_file()) {
                    continue;
                }

                // Also skip this path if its parent directory has already been extracted
                if let Some(parent) = path.parent() {
                    if extracted_paths.contains(parent) {
                        continue;
                    }
                }

                extracted_paths.insert((&*path).as_ref());

                let out_dir = if !node.is_root() && !strip_prefix {
                    out_dir.join(node.path()?.parent().expect("no parent node"))
                } else {
                    // TODO: optimize -- should be an unnecessary clone
                    out_dir.clone()
                };

                match pkg_loader.as_mut() {
                    Some(pkg_loader) => {
                        node.extract_to_path_with_callback(&out_dir, pkg_loader, || {
                            files_written.fetch_add(1, Ordering::Relaxed);
                        })?;
                    }
                    None => {
                        return Err(eyre::eyre!(
                            "Package file loader is unavailable. Check that the pkg_dir exists."
                        ));
                    }
                }
            }
            println!("Wrote {} files", files_written.load(Ordering::Relaxed));
        }
        Commands::Metadata { format, out_file } => {
            let data = serialization::tree_to_serialized_files(file_tree.clone());
            let out_file = if out_file.to_str().unwrap() != "-" {
                Some(BufWriter::new(File::create(out_file)?))
            } else {
                None
            };
            // TODO: use dynamic dispatch with Box<T>
            match format {
                MetadataFormat::Json => {
                    if let Some(out_file) = out_file {
                        serde_json::to_writer(out_file, &data)?;
                    } else {
                        let stdout = stdout().lock();

                        serde_json::to_writer(stdout, &data)?;
                    }
                }
                MetadataFormat::Csv => {
                    if let Some(out_file) = out_file {
                        let mut writer = csv::Writer::from_writer(out_file);

                        for record in data {
                            writer.serialize(record)?;
                        }
                    } else {
                        let mut writer = csv::Writer::from_writer(stdout().lock());

                        for record in data {
                            writer.serialize(record)?;
                        }
                    };
                }
                MetadataFormat::Plain => {
                    if let Some(out_file) = out_file {
                        let mut writer = out_file;

                        for record in data {
                            writeln!(&mut writer, "{}", record.path.to_str().unwrap())?;
                        }
                    } else {
                        let mut writer = stdout().lock();

                        for record in data {
                            writeln!(&mut writer, "{}", record.path.to_str().unwrap())?;
                        }
                    };
                }
            };
        }
        Commands::GameParams { out_file, ugly } => match pkg_loader.as_mut() {
            Some(pkg_loader) => {
                let mut writer = BufWriter::new(File::create(&out_file)?);
                wowsunpack::game_params::read_game_params_as_json(
                    !ugly,
                    file_tree.clone(),
                    pkg_loader,
                    &mut writer,
                )?;

                println!("GameParams written to {:?}", out_file);
            }
            None => {
                return Err(eyre::eyre!(
                    "Package file loader is unavailable. Check that the pkg_dir exists."
                ));
            }
        },
    }

    println!(
        "Finished in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    Ok(())
}
