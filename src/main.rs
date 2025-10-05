use eyre::{Result, WrapErr};

use memmap::MmapOptions;
use pickled::HashableValue;
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Cursor, Write, stdout},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use thread_local::ThreadLocal;
use wowsunpack::data::{
    idx::{self, FileNode},
    serialization,
};
use wowsunpack::{data::pkg::PkgFileLoader, game_params::convert::game_params_to_pickle};

use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use indicatif::{ParallelProgressIterator, ProgressBar};
use rayon::prelude::*;

/// Utility for interacting with World of Warships game assets
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Game directory. This option can be used instead of pkg_dir / idx_files
    /// and will automatically use the latest version of the game. If none of these
    /// args are provided, the executable's directory is assumed to be the game dir.
    ///
    /// This option will use the latest build of WoWs in the `bin` directory, which
    /// may not necessarily be the latest _playable_ version of the game e.g. when the
    /// game launcher preps an update to the game which has not yet gone live.
    ///
    /// Overrides `--pkg-dir`, `--idx-files`, and `--bin-dir`
    #[clap(short, long)]
    game_dir: Option<PathBuf>,

    /// Directory where pkg files are located. If not provided, this will
    /// default relative to the given idx directory as "../../../../res_packages"
    ///
    /// Ignored if `--game-dir` is specified.
    #[clap(short, long)]
    pkg_dir: Option<PathBuf>,

    /// .idx file(s) or their containing directory.
    ///
    /// Ignored if `--game-dir` is specified.
    #[clap(short, long)]
    idx_files: Vec<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List files in a directory
    List {
        /// Directory name to list
        dir: Option<String>,
    },
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

        /// Dump the full GameParams file. This causes `--id` to be ignored.
        #[clap(short, long)]
        full: bool,

        /// Print the top-level GameParams IDs to stdout.
        #[clap(long)]
        print_ids: bool,

        /// Which GameParams identifier to dump
        #[clap(long, default_value = "")]
        id: String,

        #[clap(default_value = "GameParams.json")]
        out_file: PathBuf,
    },
    /// Grep files for the given regex. Only prints a binary match.
    Grep {
        /// Path filter
        #[clap(long)]
        path: Option<String>,

        /// The pattern to look for
        pattern: String,
    },
    DiffDump {
        out_dir: PathBuf,
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

    Ok(idx::parse(&mut reader)?)
}

fn run() -> Result<()> {
    let mut args = Args::parse();

    let mut game_dir = PathBuf::from(std::env::args().next().expect("failed to get first arg"))
        .parent()
        .expect("failed to get executable parent dir")
        .to_owned();

    if let Some(game_dir_arg) = args.game_dir.take() {
        game_dir = game_dir_arg;
    }

    let mut game_version = None;

    // If we didn't get any idx dirs/files passed to us, try auto-detecting the
    // WoWs directory
    if args.idx_files.is_empty() {
        let bin_dir = game_dir.join("bin");
        if game_dir.join("WorldOfWarships.exe").exists() {
            // Maybe we are? Try enumerating the `bin` directory
            let paths = fs::read_dir(&bin_dir)
                .wrap_err("No index files were provided and could not enumerate `bin` directory")?;
            for path in paths {
                let path = path.wrap_err("could not enumerate path")?;
                if path.file_type()?.is_dir()
                    && let Ok(version) = u64::from_str_radix(path.file_name().to_str().unwrap(), 10)
                {
                    match game_version {
                        Some(other_version) => {
                            if other_version < version {
                                game_version = Some(version);
                            }
                        }
                        None => game_version = Some(version),
                    }
                }
            }
        }

        if let Some(latest_version) = game_version {
            let latest_version_str = format!("{latest_version}");

            args.idx_files
                .push(bin_dir.join(latest_version_str.as_str()).join("idx"));
        }

        if game_version.is_none() || !args.idx_files[0].exists() {
            Args::command().print_help()?;

            eprintln!();

            return Err(eyre::eyre!(
                "Could not find game idx files. Either provide the path(s) manually or make sure your game installation folder is well-formed"
            ));
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

    let mut pkg_loader = packages_dir.as_ref().map(PkgFileLoader::new);

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
                .map(|file_name| glob::Pattern::new(file_name).expect("invalid glob pattern"))
                .collect::<Vec<_>>();

            let mut extracted_paths = HashSet::<&Path>::new();
            let files_written = AtomicUsize::new(0);

            for (path, node) in &paths {
                let mut matches = false;

                for glob in &globs {
                    if glob.matches_path(path) {
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
                if let Some(parent) = path.parent()
                    && extracted_paths.contains(parent)
                {
                    continue;
                }

                extracted_paths.insert((*path).as_ref());

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
        Commands::GameParams {
            out_file,
            ugly,
            id,
            print_ids: ids,
            full,
        } => match pkg_loader.as_mut() {
            Some(pkg_loader) => {
                let Ok(game_params_file) = file_tree.find("content/GameParams.data") else {
                    return Err(eyre::eyre!(
                        "Could not find GameParams.data in WoWs package"
                    ));
                };

                let mut game_params_data: Vec<u8> = Vec::with_capacity(
                    game_params_file.file_info().unwrap().unpacked_size as usize,
                );

                game_params_file
                    .read_file(pkg_loader, &mut game_params_data)
                    .expect("failed to read GameParams");

                let pickle = game_params_to_pickle(game_params_data)
                    .expect("failed to deserialize GameParams");

                fn print_ids(params_dict: &BTreeMap<pickled::HashableValue, pickled::Value>) {
                    for key in params_dict.keys() {
                        if let HashableValue::String(s) = key {
                            let s = s.inner();
                            if s.is_empty() {
                                println!("(empty string)");
                            } else {
                                println!("{s}");
                            }
                        } else {
                            println!("Non-string Key: {key:?}")
                        }
                    }
                }

                let params_dict = if !full {
                    match pickle {
                        pickled::Value::Dict(params_dict) => {
                            if ids {
                                print_ids(&params_dict.inner());
                                return Ok(());
                            }

                            let params_dict = params_dict.inner();
                            let Some(dict) =
                                params_dict.get(&HashableValue::String(id.clone().into()))
                            else {
                                return Err(eyre::eyre!("Could not find GameParams ID {id:?}"));
                            };

                            dict.clone()
                        }
                        pickled::Value::List(params_list) => {
                            if ids {
                                println!("GameParams format does not have IDs");

                                return Ok(());
                            }
                            params_list.inner_mut().remove(0)
                        }
                        other => {
                            panic!("Unexpected GameParams root element type {}", other);
                        }
                    }
                } else {
                    pickle
                };

                let writer = BufWriter::new(File::create(&out_file)?);
                if ugly {
                    let mut serializer = serde_json::Serializer::new(writer);
                    params_dict.serialize(&mut serializer)?;
                } else {
                    let mut serializer = serde_json::Serializer::pretty(writer);
                    params_dict.serialize(&mut serializer)?;
                }

                println!("GameParams written to {out_file:?}");
            }
            None => {
                return Err(eyre::eyre!(
                    "Package file loader is unavailable. Check that the pkg_dir exists."
                ));
            }
        },
        Commands::List { dir } => {
            let paths = file_tree.paths();
            for (path, node) in &paths {
                let matches = dir
                    .as_ref()
                    .map(|dir| path.starts_with(dir))
                    .unwrap_or(true);
                if !matches {
                    continue;
                }

                if node.is_file() {
                    print!("(F)")
                } else {
                    print!("(D)")
                }

                print!(
                    " {}",
                    path.as_os_str()
                        .to_str()
                        .expect("could not convert path to string")
                );

                if let Some(info) = node.file_info() {
                    println!(" {} bytes", info.unpacked_size);
                }
            }
        }
        Commands::Grep { pattern, path } => {
            let Some(pkg_loader) = pkg_loader.as_mut() else {
                return Err(eyre::eyre!(
                    "Package file loader is unavailable. Check that the pkg_dir exists."
                ));
            };

            let regex = regex::bytes::Regex::new(pattern.as_str())?;

            let glob =
                path.map(|glob| glob::Pattern::new(glob.as_str()).expect("invalid glob pattern"));

            let files = file_tree.paths();

            let buffer = ThreadLocal::<RefCell<Vec<u8>>>::new();

            let bar = ProgressBar::new(files.iter().len() as u64);

            files
                .into_par_iter()
                .progress_with(bar.clone())
                .for_each(|(path, node)| {
                    if let Some(glob) = &glob
                        && !glob.matches_path(&path)
                    {
                        return;
                    }

                    if path.is_dir() {
                        return;
                    }

                    let buffer = buffer.get_or_default();
                    let mut buffer = buffer.borrow_mut();

                    buffer.clear();

                    let Some(file_info) = node.file_info() else {
                        return;
                    };
                    let bytes_needed =
                        (file_info.unpacked_size as usize).saturating_sub(buffer.capacity());
                    if bytes_needed > 0 {
                        buffer.reserve(bytes_needed);
                    }

                    node.read_file(pkg_loader, &mut *buffer)
                        .expect("failed to read file");

                    if let Some(matched) = regex.find(buffer.as_slice()) {
                        let file_path = path.as_os_str().to_string_lossy();

                        if let Ok(data) = std::str::from_utf8(matched.as_bytes()) {
                            bar.println(format!("{} matched: {}", file_path, data));
                        } else {
                            bar.println(format!("{} matched", file_path));
                        }
                    }
                });
        }
        Commands::DiffDump { out_dir } => {
            let game_version = game_version.expect("could not determine latest game version");
            std::fs::write(out_dir.join("version.txt"), game_version.to_string())?;

            let file_info_path = out_dir.join("pkg_files");

            // Dump file info
            for file in serialization::tree_to_serialized_files(file_tree.clone()) {
                let mut dest = file_info_path.join(&file.path);
                let mut new_name = dest.file_name().expect("file has no name?").to_os_string();
                new_name.push(".txt");
                dest.set_file_name(new_name);

                if file.is_directory() {
                    continue;
                }

                std::fs::create_dir_all(dest.parent().expect("file has no parent?"))
                    .expect("failed to create parent dir");

                let out_file = BufWriter::new(
                    std::fs::File::create(dest).expect("failed to create file metadata file"),
                );
                serde_json::to_writer_pretty(out_file, &file)
                    .expect("failed to serialize file metadata");
            }

            let game_params_path = out_dir.join("game_params");

            // Dump params info
            match pkg_loader.as_mut() {
                Some(pkg_loader) => {
                    let pickle = load_game_params(pkg_loader, &file_tree)?;

                    // Dump the base params first
                    let base_path = game_params_path.join("base");

                    match pickle {
                        pickled::Value::Dict(params_dict) => {
                            let params_dict = params_dict.inner();

                            let base_data = params_dict
                                .get(&HashableValue::String("".to_string().into()))
                                .expect("failed to find base GameParams");

                            let base_data = base_data
                                .dict_ref()
                                .expect("params are not a dictionary")
                                .inner();

                            for (key, value) in base_data.iter() {
                                let key = key.to_string_key().expect("key is not stringable");

                                dump_param(key.as_ref(), value, base_path.to_owned());
                            }

                            for (region, params) in params_dict.iter().filter(|(key, _value)| {
                                key.string_ref()
                                    .map(|s| !s.inner().is_empty())
                                    .unwrap_or_default()
                            }) {
                                let pickled::Value::Dict(params) = params else {
                                    continue;
                                };

                                let region_key = region
                                    .to_string_key()
                                    .expect("could not convert region to string");
                                let region_path = game_params_path.join(region_key.as_ref());

                                let params = params.inner();
                                for (key, value) in params.iter() {
                                    let key_str =
                                        key.to_string_key().expect("key is not stringable");

                                    dump_param(&key_str, value, region_path.to_owned());
                                }
                            }
                        }
                        pickled::Value::List(params_list) => {
                            let params = params_list.inner_mut().remove(0);

                            let pickled::Value::Dict(params) = params else {
                                return Err(eyre::eyre!("Params are not a dictionary"));
                            };

                            let region_path = out_dir.join("base");

                            let params = params.inner();
                            for (key, value) in params.iter() {
                                let key_str = key.to_string_key().expect("key is not stringable");

                                dump_param(&key_str, value, region_path.to_owned());
                            }
                        }
                        other => {
                            panic!("Unexpected GameParams root element type {}", other);
                        }
                    };
                }
                None => {
                    return Err(eyre::eyre!(
                        "Package file loader is unavailable. Check that the pkg_dir exists."
                    ));
                }
            }
        }
    }

    Ok(())
}

fn param_path(stem: &str, param: &pickled::Value, mut base: PathBuf) -> Option<PathBuf> {
    let value = param.dict_ref()?;
    let value = value.inner();
    let type_info = value.get(&HashableValue::String("typeinfo".to_string().into()))?;
    let type_info = type_info.dict_ref()?.inner();

    let (nation, species, typ) = (
        type_info.get(&HashableValue::String("nation".to_string().into()))?,
        type_info.get(&HashableValue::String("species".to_string().into()))?,
        type_info.get(&HashableValue::String("type".to_string().into()))?,
    );
    if let pickled::Value::String(typ) = typ {
        base = base.join(typ.inner().as_str());
    }
    if let pickled::Value::String(nation) = nation {
        base = base.join(nation.inner().as_str());
    }
    if let pickled::Value::String(species) = species {
        base = base.join(species.inner().as_str());
    }
    base = base.join(format!("{stem}.json"));

    Some(base)
}

fn dump_param(
    file_stem: &str,
    value: &pickled::Value,
    mut out_path: PathBuf,
) -> Option<()> {
    out_path = param_path(file_stem, value, out_path)?;

    // Dump this file
    let parent = out_path.parent().expect("no parent dir?");
    std::fs::create_dir_all(parent).expect("failed to create parent dir");

    // Doesn't work well with vcs

    // if let Some((base, path)) = &base
    //     && *base == value
    // {
    //     if std::fs::symlink_metadata(&out_path).ok()?.is_symlink() {
    //         symlink::remove_symlink_file(&out_path).ok()?;
    //     } else if out_path.is_file() {
    //         std::fs::remove_file(&out_path).ok()?;
    //     }

    //     // Create a symlink
    //     symlink::symlink_file(path, &out_path)
    //         .with_context(|| format!("path={path:?}, out_path={out_path:?}"))
    //         .expect("failed to create symlink");
    //     return None;
    // } else

    let file =
        BufWriter::new(std::fs::File::create(out_path).expect("failed to create output file"));

    let mut serializer = serde_json::Serializer::pretty(file);
    value
        .serialize(&mut serializer)
        .expect("failed to serialize data");

    None
}

fn load_game_params(pkg_loader: &PkgFileLoader, file_tree: &FileNode) -> Result<pickled::Value> {
    let Ok(game_params_file) = file_tree.find("content/GameParams.data") else {
        return Err(eyre::eyre!(
            "Could not find GameParams.data in WoWs package"
        ));
    };

    let mut game_params_data: Vec<u8> =
        Vec::with_capacity(game_params_file.file_info().unwrap().unpacked_size as usize);

    game_params_file
        .read_file(pkg_loader, &mut game_params_data)
        .expect("failed to read GameParams");

    let pickle = game_params_to_pickle(game_params_data).expect("failed to deserialize GameParams");

    Ok(pickle)
}

fn main() -> Result<()> {
    let timestamp = Instant::now();

    run()?;

    println!(
        "Finished in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    Ok(())
}
