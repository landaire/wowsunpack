use rootcause::prelude::*;

use pickled::HashableValue;
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashSet},
    fs::{self, File},
    io::{BufWriter, Read, Write, stdout},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};
use thread_local::ThreadLocal;
use vfs::VfsPath;
use vfs::impls::overlay::OverlayFS;
use wowsunpack::data::{
    assets_bin_vfs::AssetsBinVfs,
    idx::{self, VfsEntry},
    idx_vfs::IdxVfs,
    serialization,
    wrappers::mmap::MmapPkgSource,
};
use wowsunpack::game_params::convert::game_params_to_pickle;

use clap::{Parser, Subcommand, ValueEnum};
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
    /// Parse and inspect a .geometry model file
    Geometry {
        /// Path to the .geometry file (VFS path by default, disk path with --no-vfs)
        file: PathBuf,

        /// Decode ENCD-compressed vertex/index buffers and print sizes
        #[clap(long)]
        decode: bool,

        /// Read file from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,
    },
    /// Export a ship sub-model to GLB format
    ExportModel {
        /// Model name (e.g. "JSB039_Yamato_1945_Bow"). Resolves to {name}.visual
        /// in assets.bin and its linked .geometry file.
        name: String,

        /// Output file path
        #[arg(short, long, default_value = "output.glb")]
        output: PathBuf,

        /// LOD level (0 = highest detail)
        #[arg(long, default_value = "0")]
        lod: usize,
    },
    /// Export all sub-models of a ship to a single GLB file.
    /// Each sub-model becomes a separate named object in Blender.
    ExportShip {
        /// Ship name — either a model directory name (e.g. "JSB039_Yamato_1945")
        /// or a translated display name (e.g. "Yamato") for fuzzy lookup
        name: String,

        /// Output file path
        #[arg(short, long, default_value = "output.glb")]
        output: PathBuf,

        /// LOD level (0 = highest detail)
        #[arg(long, default_value = "0")]
        lod: usize,

        /// List available hull upgrades and their components, then exit
        #[arg(long)]
        list_upgrades: bool,

        /// Hull upgrade to use (e.g. "A" for stock, "B" for upgraded).
        /// Accepts a prefix match against upgrade keys.
        #[arg(long)]
        hull: Option<String>,
    },
    /// Parse and inspect an assets.bin (PrototypeDatabase) file
    AssetsBin {
        /// Path to the assets.bin file (VFS path by default, disk path with --no-vfs)
        file: PathBuf,

        /// Filter path entries by name substring
        #[clap(long)]
        filter: Option<String>,

        /// Maximum number of path entries to display
        #[clap(long, default_value = "50")]
        max_paths: usize,

        /// Resolve a path suffix to its prototype location and print record info
        #[clap(long)]
        resolve: Option<String>,

        /// Parse and display a VisualPrototype by path suffix
        #[clap(long)]
        parse_visual: Option<String>,

        /// Read file from disk instead of VFS
        #[clap(long)]
        no_vfs: bool,
    },
}

#[derive(Debug, Clone, Eq, PartialEq, ValueEnum)]
enum MetadataFormat {
    Plain,
    Json,
    Csv,
}

fn load_idx_file(path: PathBuf) -> Result<idx::IdxFile, Report> {
    let file_data = std::fs::read(&path).context("Failed to read idx file")?;
    Ok(idx::parse(&file_data)?)
}

/// Read file data from disk (if `no_vfs`) or from the VFS.
fn read_file_data(path: &Path, no_vfs: bool, vfs: Option<&VfsPath>) -> Result<Vec<u8>, Report> {
    if no_vfs {
        return Ok(std::fs::read(path).context("Failed to read file from disk")?);
    }

    let Some(vfs) = vfs else {
        bail!(
            "No VFS available. Use --game-dir to specify a game install, \
             or --no-vfs to read from disk."
        );
    };

    let vfs_path = path.to_string_lossy().replace('\\', "/");
    let mut data = Vec::new();
    vfs.join(&vfs_path)
        .context("VFS path error")?
        .open_file()
        .context_with(|| format!("File not found in VFS: '{vfs_path}'"))?
        .read_to_end(&mut data)?;
    Ok(data)
}

/// Add entries from an AssetsBinVfs to the IDX file tree so list/extract can see them.
fn add_vfs_entries_to_file_tree(
    assets_vfs: &AssetsBinVfs,
    file_tree: &mut BTreeMap<String, VfsEntry>,
) -> HashSet<String> {
    let mut assets_bin_paths = HashSet::new();

    // Add directory entries (skip empty root).
    for dir_path in assets_vfs.dirs() {
        if !dir_path.is_empty() {
            let path = dir_path.to_string();
            assets_bin_paths.insert(path.clone());
            file_tree.entry(path).or_insert(VfsEntry::Directory);
        }
    }

    // Add file entries with stub FileInfo/Volume.
    let stub_volume = idx::Volume {
        volume_id: 0,
        filename: String::new(),
    };
    for (file_path, size) in assets_vfs.files() {
        let path = file_path.to_string();
        assets_bin_paths.insert(path.clone());
        file_tree.entry(path).or_insert(VfsEntry::File {
            file_info: idx::FileInfo {
                resource_id: 0,
                volume_id: 0,
                offset: 0,
                compression_info: 0,
                size: size as u32,
                crc32: 0,
                unpacked_size: size as u32,
                padding: 0,
            },
            volume: stub_volume.clone(),
        });
    }

    assets_bin_paths
}

fn run() -> Result<(), Report> {
    let mut args = Args::parse();

    let mut game_dir = PathBuf::from(std::env::args().next().expect("failed to get first arg"))
        .parent()
        .expect("failed to get executable parent dir")
        .to_owned();

    if let Some(game_dir_arg) = args.game_dir.take() {
        game_dir = game_dir_arg;
    }

    let mut game_version = None;

    // Try to set up VFS from game directory / idx files. This is best-effort:
    // if no game dir is provided and no idx files exist, vfs will be None.
    let mut vfs: Option<VfsPath> = None;
    let mut file_tree = BTreeMap::new();
    let mut assets_bin_paths = HashSet::new();

    if args.idx_files.is_empty() {
        let bin_dir = game_dir.join("bin");
        if game_dir.join("WorldOfWarships.exe").exists() {
            let paths = fs::read_dir(&bin_dir).ok();
            if let Some(paths) = paths {
                for path in paths {
                    let Ok(path) = path else { continue };
                    if path.file_type().map(|t| t.is_dir()).unwrap_or(false)
                        && let Ok(version) =
                            u64::from_str_radix(path.file_name().to_str().unwrap(), 10)
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
                let idx_path = bin_dir.join(latest_version_str.as_str()).join("idx");
                if idx_path.exists() {
                    args.idx_files.push(idx_path);
                }
            }
        }
    }

    if !args.idx_files.is_empty() {
        let resources = Mutex::new(Vec::new());
        let mut paths = Vec::with_capacity(args.idx_files.len());

        for path in args.idx_files {
            if path.is_dir() {
                if let Some(parent) = path.parent()
                    && let Some(stem) = parent.file_stem().and_then(|stem| stem.to_str())
                    && let Some(version) = stem.parse::<u64>().ok()
                {
                    game_version = Some(version);
                }

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
            resources.lock().unwrap().push(load_idx_file(path)?);

            Ok::<(), Report>(())
        })?;

        let idx_files = resources.into_inner().unwrap();
        file_tree = idx::build_file_tree(&idx_files);

        // Build VFS for commands that need to read file contents
        if let Some(pkg_dir) = packages_dir.as_ref() {
            let pkg_source = MmapPkgSource::new(pkg_dir);
            let idx_vfs = IdxVfs::new(pkg_source, &idx_files);
            let pkg_vfs = VfsPath::new(idx_vfs);

            // Try to load assets.bin from the PKG VFS and overlay it.
            let mut assets_bin_data = Vec::new();
            let assets_loaded = pkg_vfs
                .join("content/assets.bin")
                .and_then(|p| p.open_file())
                .and_then(|mut f| {
                    f.read_to_end(&mut assets_bin_data)?;
                    Ok(())
                })
                .is_ok();

            if assets_loaded {
                match AssetsBinVfs::new(assets_bin_data) {
                    Ok(assets_vfs) => {
                        // Add assets.bin entries to the file_tree so list/extract can find them.
                        assets_bin_paths =
                            add_vfs_entries_to_file_tree(&assets_vfs, &mut file_tree);

                        let assets_layer = VfsPath::new(assets_vfs);
                        let overlay = OverlayFS::new(&[assets_layer, pkg_vfs]);
                        vfs = Some(VfsPath::new(overlay));
                    }
                    Err(e) => {
                        eprintln!("Warning: failed to parse assets.bin for overlay VFS: {e}");
                        vfs = Some(pkg_vfs);
                    }
                }
            } else {
                vfs = Some(pkg_vfs);
            }
        }
    }

    match args.command {
        Commands::Extract {
            flatten,
            files,
            out_dir,
            strip_prefix,
        } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let globs = files
                .iter()
                .map(|file_name| glob::Pattern::new(file_name).expect("invalid glob pattern"))
                .collect::<Vec<_>>();

            let files_written = AtomicUsize::new(0);

            // Collect matching entries
            let matching: Vec<(&str, &VfsEntry)> = file_tree
                .iter()
                .filter(|(path, entry)| {
                    let path_str = path.as_str();
                    let mut matches = false;
                    for glob in &globs {
                        if glob.matches(path_str) {
                            matches = true;
                            break;
                        }
                    }
                    if !matches {
                        return false;
                    }
                    // Skip directories when flattening
                    if flatten && matches!(entry, VfsEntry::Directory) {
                        return false;
                    }
                    true
                })
                .map(|(p, e)| (p.as_str(), e))
                .collect();

            for (path, entry) in &matching {
                match entry {
                    VfsEntry::File { .. } => {
                        let mut file_data = Vec::new();
                        vfs.join(path)
                            .context("VFS path error")?
                            .open_file()
                            .context_with(|| format!("Failed to open {path}"))?
                            .read_to_end(&mut file_data)?;

                        let out_path = if flatten {
                            let file_name = path.rsplit('/').next().unwrap_or(path);
                            out_dir.join(file_name)
                        } else if strip_prefix {
                            // Strip the matched prefix, keep only the last component
                            let file_name = path.rsplit('/').next().unwrap_or(path);
                            out_dir.join(file_name)
                        } else {
                            out_dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR))
                        };

                        if let Some(parent) = out_path.parent() {
                            fs::create_dir_all(parent)?;
                        }
                        fs::write(&out_path, &file_data)?;
                        files_written.fetch_add(1, Ordering::Relaxed);
                    }
                    VfsEntry::Directory => {
                        // For directories, extract all children recursively
                        let prefix = if path.is_empty() {
                            String::new()
                        } else {
                            format!("{}/", path)
                        };

                        for (child_path, child_entry) in &file_tree {
                            if !child_path.starts_with(&prefix) {
                                continue;
                            }
                            if matches!(child_entry, VfsEntry::Directory) {
                                continue;
                            }

                            let mut file_data = Vec::new();
                            vfs.join(child_path.as_str())
                                .context("VFS path error")?
                                .open_file()
                                .context_with(|| format!("Failed to open {child_path}"))?
                                .read_to_end(&mut file_data)?;

                            let relative = if strip_prefix {
                                // Strip the matched part
                                child_path
                                    .strip_prefix(&prefix)
                                    .unwrap_or(child_path.as_str())
                            } else {
                                child_path.as_str()
                            };

                            let out_path =
                                out_dir.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
                            if let Some(parent) = out_path.parent() {
                                fs::create_dir_all(parent)?;
                            }
                            fs::write(&out_path, &file_data)?;
                            files_written.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            }
            println!("Wrote {} files", files_written.load(Ordering::Relaxed));
        }
        Commands::Metadata { format, out_file } => {
            let data = serialization::tree_to_serialized_files(&file_tree);
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
                            writeln!(&mut writer, "{}", record.path())?;
                        }
                    } else {
                        let mut writer = stdout().lock();

                        for record in data {
                            writeln!(&mut writer, "{}", record.path())?;
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
        } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let pickle = load_game_params(vfs)?;

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
                        let Some(dict) = params_dict.get(&HashableValue::String(id.clone().into()))
                        else {
                            bail!("Could not find GameParams ID {id:?}");
                        };

                        dict.clone()
                    }
                    pickled::Value::Tuple(params_tuple) => {
                        if ids {
                            println!("GameParams format does not have IDs");

                            return Ok(());
                        }
                        params_tuple
                            .inner()
                            .first()
                            .expect("params_list has no items?")
                            .clone()
                    }
                    pickled::Value::List(params_list) => {
                        if ids {
                            println!("GameParams format does not have IDs");

                            return Ok(());
                        }
                        params_list
                            .inner()
                            .first()
                            .expect("params_list has no items?")
                            .clone()
                    }
                    _ => {
                        panic!("Unexpected GameParams root element type");
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
        Commands::List { dir } => {
            for (path, entry) in &file_tree {
                let matches = dir
                    .as_ref()
                    .map(|dir| path.starts_with(dir.as_str()))
                    .unwrap_or(true);
                if !matches {
                    continue;
                }

                let from_assets_bin = assets_bin_paths.contains(path.as_str());
                match entry {
                    VfsEntry::File { file_info, .. } => {
                        let tag = if from_assets_bin { "A" } else { "F" };
                        println!("({tag}) {} {} bytes", path, file_info.unpacked_size);
                    }
                    VfsEntry::Directory => {
                        let tag = if from_assets_bin { "A" } else { "D" };
                        println!("({tag}) {path}");
                    }
                }
            }
        }
        Commands::Grep { pattern, path } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let regex = regex::bytes::Regex::new(pattern.as_str())?;

            let glob =
                path.map(|glob| glob::Pattern::new(glob.as_str()).expect("invalid glob pattern"));

            let files: Vec<(&String, &VfsEntry)> = file_tree.iter().collect();

            let buffer = ThreadLocal::<RefCell<Vec<u8>>>::new();

            let bar = ProgressBar::new(files.len() as u64);

            files
                .into_par_iter()
                .progress_with(bar.clone())
                .for_each(|(path, entry)| {
                    if let Some(glob) = &glob
                        && !glob.matches(path.as_str())
                    {
                        return;
                    }

                    if matches!(entry, VfsEntry::Directory) {
                        return;
                    }

                    let file_info = match entry {
                        VfsEntry::File { file_info, .. } => file_info,
                        _ => return,
                    };

                    let buffer = buffer.get_or_default();
                    let mut buffer = buffer.borrow_mut();

                    buffer.clear();

                    let bytes_needed =
                        (file_info.unpacked_size as usize).saturating_sub(buffer.capacity());
                    if bytes_needed > 0 {
                        buffer.reserve(bytes_needed);
                    }

                    let Ok(mut file) = vfs.join(path.as_str()).and_then(|p| p.open_file()) else {
                        return;
                    };

                    if file.read_to_end(&mut *buffer).is_err() {
                        return;
                    }

                    if let Some(matched) = regex.find(buffer.as_slice()) {
                        if let Ok(data) = std::str::from_utf8(matched.as_bytes()) {
                            bar.println(format!("{path} matched: {data}"));
                        } else {
                            bar.println(format!("{path} matched"));
                        }
                    }
                });
        }
        Commands::DiffDump { out_dir } => {
            let Some(vfs) = &vfs else {
                bail!("Package file loader is unavailable. Check that the pkg_dir exists.");
            };

            let game_version = game_version.expect("could not determine latest game version");
            std::fs::write(out_dir.join("version.txt"), game_version.to_string())?;

            let file_info_path = out_dir.join("pkg_files");

            // Dump file info
            for file in serialization::tree_to_serialized_files(&file_tree) {
                let file_path_on_disk =
                    file_info_path.join(file.path().replace('/', std::path::MAIN_SEPARATOR_STR));
                let mut dest = file_path_on_disk;
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
            let pickle = load_game_params(vfs)?;

            // Dump the base params first
            let base_path = game_params_path.join("base");

            let handle_params_from_listish = |params: &pickled::Value| -> Result<(), Report> {
                let pickled::Value::Dict(params) = params else {
                    bail!("Params are not a dictionary");
                };

                let params = params.inner();
                for (key, value) in params.iter() {
                    let key_str = key.to_string_key().expect("key is not stringable");

                    dump_param(&key_str, value, base_path.to_owned());
                }

                Ok(())
            };

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
                            let key_str = key.to_string_key().expect("key is not stringable");

                            dump_param(&key_str, value, region_path.to_owned());
                        }
                    }
                }
                pickled::Value::Tuple(params_tuple) => {
                    handle_params_from_listish(
                        params_tuple
                            .inner()
                            .first()
                            .expect("params tuple does not have any items?"),
                    )?;
                }
                pickled::Value::List(params_list) => {
                    let params = params_list.inner();
                    let params = params
                        .first()
                        .expect("params list does not have any items?");

                    handle_params_from_listish(params)?;
                }
                _ => {
                    panic!("Unexpected GameParams root element type");
                }
            };
        }
        Commands::Geometry {
            file,
            decode,
            no_vfs,
        } => {
            let file_data = read_file_data(&file, no_vfs, vfs.as_ref())?;
            run_geometry(&file_data, &file.to_string_lossy(), decode)?;
        }
        Commands::ExportModel { name, output, lod } => {
            let Some(vfs) = &vfs else {
                bail!("VFS required for export-model. Use --game-dir to specify a game install.");
            };

            run_export_model(vfs, &name, &output, lod)?;
        }
        Commands::ExportShip {
            name,
            output,
            lod,
            list_upgrades,
            hull,
        } => {
            let Some(vfs) = &vfs else {
                bail!("VFS required for export-ship. Use --game-dir to specify a game install.");
            };

            run_export_ship(
                vfs,
                &name,
                &output,
                lod,
                &game_dir,
                game_version,
                list_upgrades,
                hull.as_deref(),
            )?;
        }
        Commands::AssetsBin {
            file,
            filter,
            max_paths,
            resolve,
            parse_visual,
            no_vfs,
        } => {
            let file_data = read_file_data(&file, no_vfs, vfs.as_ref())?;
            run_assets_bin(
                &file_data,
                &file.to_string_lossy(),
                filter.as_deref(),
                max_paths,
                resolve.as_deref(),
                parse_visual.as_deref(),
            )?;
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

fn dump_param(file_stem: &str, value: &pickled::Value, mut out_path: PathBuf) -> Option<()> {
    out_path = param_path(file_stem, value, out_path)?;

    // Dump this file
    let parent = out_path.parent().expect("no parent dir?");
    std::fs::create_dir_all(parent).expect("failed to create parent dir");

    let file =
        BufWriter::new(std::fs::File::create(out_path).expect("failed to create output file"));

    let mut serializer = serde_json::Serializer::pretty(file);
    value
        .serialize(&mut serializer)
        .expect("failed to serialize data");

    None
}

fn load_game_params(vfs: &VfsPath) -> Result<pickled::Value, Report> {
    let mut game_params_data: Vec<u8> = Vec::new();
    vfs.join("content/GameParams.data")
        .context("VFS path error")?
        .open_file()
        .context("Could not find GameParams.data in WoWs package")?
        .read_to_end(&mut game_params_data)
        .context("Failed to read GameParams")?;

    let pickle =
        game_params_to_pickle(game_params_data).context("Failed to deserialize GameParams")?;

    Ok(pickle)
}

fn run_geometry(file_data: &[u8], name: &str, decode: bool) -> Result<(), Report> {
    use wowsunpack::models::geometry;

    let geom = geometry::parse_geometry(file_data)?;

    println!("=== .geometry file: {} ===", name);
    println!("File size: {} bytes", file_data.len());
    println!();

    println!("Vertices mappings: {}", geom.vertices_mapping.len());
    for (i, m) in geom.vertices_mapping.iter().enumerate() {
        println!(
            "  [{i}] id=0x{:08X} buf={} offset={} count={} texelDensity=0x{:04X}",
            m.mapping_id,
            m.merged_buffer_index,
            m.items_offset,
            m.items_count,
            m.packed_texel_density
        );
    }
    println!();

    println!("Indices mappings: {}", geom.indices_mapping.len());
    for (i, m) in geom.indices_mapping.iter().enumerate() {
        println!(
            "  [{i}] id=0x{:08X} buf={} offset={} count={} texelDensity=0x{:04X}",
            m.mapping_id,
            m.merged_buffer_index,
            m.items_offset,
            m.items_count,
            m.packed_texel_density
        );
    }
    println!();

    println!("Merged vertices: {}", geom.merged_vertices.len());
    for (i, v) in geom.merged_vertices.iter().enumerate() {
        let encoding = match &v.data {
            geometry::VertexData::Encoded { element_count, .. } => {
                format!("ENCD ({element_count} elements)")
            }
            geometry::VertexData::Raw(_) => "raw".to_string(),
        };
        println!(
            "  [{i}] format=\"{}\" size={} stride={} skinned={} bumped={} encoding={}",
            v.format_name, v.size_in_bytes, v.stride_in_bytes, v.is_skinned, v.is_bumped, encoding
        );

        if decode {
            match v.data.decode() {
                Ok(decoded) => println!("    -> decoded {} bytes of vertex data", decoded.len()),
                Err(e) => println!("    -> decode error: {e:?}"),
            }
        }
    }
    println!();

    println!("Merged indices: {}", geom.merged_indices.len());
    for (i, idx) in geom.merged_indices.iter().enumerate() {
        let encoding = match &idx.data {
            geometry::IndexData::Encoded { element_count, .. } => {
                format!("ENCD ({element_count} elements)")
            }
            geometry::IndexData::Raw(_) => "raw".to_string(),
        };
        println!(
            "  [{i}] size={} indexSize={} encoding={}",
            idx.size_in_bytes, idx.index_size, encoding
        );

        if decode {
            match idx.data.decode() {
                Ok(decoded) => println!("    -> decoded {} bytes of index data", decoded.len()),
                Err(e) => println!("    -> decode error: {e:?}"),
            }
        }
    }
    println!();

    println!("Collision models: {}", geom.collision_models.len());
    for (i, cm) in geom.collision_models.iter().enumerate() {
        println!("  [{i}] name=\"{}\" size={}", cm.name, cm.size_in_bytes);
    }
    println!();

    println!("Armor models: {}", geom.armor_models.len());
    for (i, am) in geom.armor_models.iter().enumerate() {
        println!("  [{i}] name=\"{}\" size={}", am.name, am.size_in_bytes);
    }

    Ok(())
}

fn run_assets_bin(
    file_data: &[u8],
    name: &str,
    filter: Option<&str>,
    max_paths: usize,
    resolve: Option<&str>,
    parse_visual: Option<&str>,
) -> Result<(), Report> {
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::visual;

    let db = assets_bin::parse_assets_bin(file_data)?;

    // If --resolve is given, do a targeted lookup instead of the full dump.
    if let Some(path_suffix) = resolve {
        let self_id_index = db.build_self_id_index();
        let (location, full_path) = db.resolve_path(path_suffix, &self_id_index)?;

        println!("Resolved: {full_path}");
        println!(
            "  blob_index={}, record_index={}",
            location.blob_index, location.record_index
        );
        let blob = &db.databases[location.blob_index];
        println!(
            "  blob: magic=0x{:08X}, record_count={}, size={}",
            blob.prototype_magic, blob.record_count, blob.size
        );

        // Print the first 64 bytes of the record as hex
        let item_sizes: &[usize] = &[0x78, 0x70, 0x20, 0x28, 0x70, 0x10, 0x18, 0x10, 0x10, 0x10];
        if location.blob_index < item_sizes.len() {
            let item_size = item_sizes[location.blob_index];
            match db.get_prototype_data(location, item_size) {
                Ok(data) => {
                    let show_len = item_size.min(data.len()).min(128);
                    println!("  item_size=0x{item_size:X} ({item_size} bytes)");
                    println!("  record hex ({show_len} bytes):");
                    for row in 0..(show_len + 15) / 16 {
                        let start = row * 16;
                        let end = (start + 16).min(show_len);
                        let hex: String = data[start..end]
                            .iter()
                            .map(|b| format!("{b:02X}"))
                            .collect::<Vec<_>>()
                            .join(" ");
                        println!("    +0x{start:02X}: {hex}");
                    }
                }
                Err(e) => println!("  error reading prototype data: {e}"),
            }
        }

        return Ok(());
    }

    // --parse-visual: resolve path, extract record, parse and print VisualPrototype
    if let Some(path_suffix) = parse_visual {
        let self_id_index = db.build_self_id_index();
        let (location, full_path) = db.resolve_path(path_suffix, &self_id_index)?;

        if location.blob_index != 1 {
            bail!(
                "Path '{}' resolved to blob {} (not VisualPrototype blob 1)",
                path_suffix,
                location.blob_index
            );
        }

        let record_data = db
            .get_prototype_data(location, visual::VISUAL_ITEM_SIZE)
            .context("Failed to get visual prototype data")?;

        let vp = visual::parse_visual(record_data).context("Failed to parse VisualPrototype")?;

        println!("=== VisualPrototype: {full_path} ===");
        println!(
            "  blob_index={}, record_index={}",
            location.blob_index, location.record_index
        );
        vp.print_summary(&db);

        return Ok(());
    }

    println!("=== assets.bin (PrototypeDatabase): {} ===", name);
    println!("File size: {} bytes", file_data.len());
    println!();

    println!("Header:");
    println!("  magic:        0x{:08X}", db.header.magic);
    println!("  version:      0x{:08X}", db.header.version);
    println!("  checksum:     0x{:08X}", db.header.checksum);
    println!("  architecture: 0x{:04X}", db.header.architecture);
    println!("  endianness:   0x{:04X}", db.header.endianness);
    println!();

    println!("Strings:");
    println!(
        "  offsetsMap: capacity={}, buckets={} bytes, values={} bytes",
        db.strings.offsets_map.capacity,
        db.strings.offsets_map.buckets.len(),
        db.strings.offsets_map.values.len()
    );
    println!("  string data: {} bytes", db.strings.string_data.len());
    // Show a few sample strings
    let mut sample_count = 0;
    let mut pos = 0;
    while pos < db.strings.string_data.len() && sample_count < 5 {
        if db.strings.string_data[pos] == 0 {
            pos += 1;
            continue;
        }
        if let Some(s) = db.strings.get_string(pos as u32) {
            if !s.is_empty() {
                println!("    [offset={pos}] \"{s}\"");
                pos += s.len() + 1;
                sample_count += 1;
                continue;
            }
        }
        pos += 1;
    }
    println!();

    println!("ResourceToPrototypeMap:");
    println!(
        "  capacity={}, buckets={} bytes, values={} bytes",
        db.resource_to_prototype_map.capacity,
        db.resource_to_prototype_map.buckets.len(),
        db.resource_to_prototype_map.values.len()
    );
    println!();

    println!("PathsStorage: {} entries", db.paths_storage.len());
    let mut shown = 0;
    for entry in &db.paths_storage {
        let matches = filter.map(|f| entry.name.contains(f)).unwrap_or(true);
        if !matches {
            continue;
        }
        println!(
            "  selfId=0x{:016X} parentId=0x{:016X} name=\"{}\"",
            entry.self_id, entry.parent_id, entry.name
        );
        shown += 1;
        if shown >= max_paths {
            let remaining = if let Some(f) = filter {
                db.paths_storage
                    .iter()
                    .filter(|e| e.name.contains(f))
                    .count()
                    - shown
            } else {
                db.paths_storage.len() - shown
            };
            if remaining > 0 {
                println!("  ... and {remaining} more (use --max-paths to show more)");
            }
            break;
        }
    }
    println!();

    println!("Databases: {} entries", db.databases.len());
    for (i, entry) in db.databases.iter().enumerate() {
        println!(
            "  [{i}] magic=0x{:08X} checksum=0x{:08X} records={} size={} bytes",
            entry.prototype_magic, entry.prototype_checksum, entry.record_count, entry.size
        );
    }

    Ok(())
}

fn run_export_model(vfs: &VfsPath, name: &str, output: &Path, lod: usize) -> Result<(), Report> {
    use wowsunpack::export::gltf_export;
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::geometry;
    use wowsunpack::models::visual;

    // Load assets.bin from VFS.
    let mut assets_bin_data = Vec::new();
    vfs.join("content/assets.bin")
        .context("VFS path error")?
        .open_file()
        .context("Could not find content/assets.bin in VFS")?
        .read_to_end(&mut assets_bin_data)?;

    let db = assets_bin::parse_assets_bin(&assets_bin_data)?;
    let self_id_index = db.build_self_id_index();

    // Resolve {name}.visual
    let visual_suffix = format!("{name}.visual");
    let (vis_location, vis_full_path) = db
        .resolve_path(&visual_suffix, &self_id_index)
        .context_with(|| format!("Could not resolve visual: {visual_suffix}"))?;

    if vis_location.blob_index != 1 {
        bail!(
            "Path '{}' resolved to blob {} (expected VisualPrototype blob 1)",
            visual_suffix,
            vis_location.blob_index
        );
    }

    let vis_data = db
        .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
        .context("Failed to get visual prototype data")?;
    let vp = visual::parse_visual(vis_data).context("Failed to parse VisualPrototype")?;

    println!("Visual: {vis_full_path}");
    println!(
        "  {} render sets, {} LODs, {} nodes",
        vp.render_sets.len(),
        vp.lods.len(),
        vp.nodes.name_ids.len()
    );

    // Resolve geometry path from mergedGeometryPathId.
    let geom_path_idx = self_id_index
        .get(&vp.merged_geometry_path_id)
        .ok_or_else(|| {
            rootcause::report!(
                "Could not resolve mergedGeometryPathId 0x{:016X}",
                vp.merged_geometry_path_id
            )
        })?;
    let geom_full_path = db.reconstruct_path(*geom_path_idx, &self_id_index);

    println!("Geometry: {geom_full_path}");

    // Load and parse geometry from VFS.
    let mut geom_data = Vec::new();
    vfs.join(&geom_full_path)
        .context("VFS path error")?
        .open_file()
        .context_with(|| format!("Could not open geometry file: {geom_full_path}"))?
        .read_to_end(&mut geom_data)?;

    let geom = geometry::parse_geometry(&geom_data).context("Failed to parse geometry")?;

    println!(
        "  {} vertex buffers, {} index buffers, {} vertices mappings, {} indices mappings",
        geom.merged_vertices.len(),
        geom.merged_indices.len(),
        geom.vertices_mapping.len(),
        geom.indices_mapping.len(),
    );

    // Export to GLB.
    let mut out_file = std::fs::File::create(output).context("Failed to create output file")?;
    gltf_export::export_glb(&vp, &geom, &db, lod, &mut out_file).context("Failed to export GLB")?;

    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!(
        "Exported LOD {} to {} ({} bytes)",
        lod,
        output.display(),
        file_size
    );

    Ok(())
}

fn run_export_ship(
    vfs: &VfsPath,
    name: &str,
    output: &Path,
    lod: usize,
    game_dir: &Path,
    game_version: Option<u64>,
    list_upgrades: bool,
    hull_selection: Option<&str>,
) -> Result<(), Report> {
    use std::collections::HashMap;
    use wowsunpack::export::gltf_export::{self, SubModel};
    use wowsunpack::models::assets_bin;
    use wowsunpack::models::geometry;
    use wowsunpack::models::visual;

    // Load assets.bin from VFS.
    let mut assets_bin_data = Vec::new();
    vfs.join("content/assets.bin")
        .context("VFS path error")?
        .open_file()
        .context("Could not find content/assets.bin in VFS")?
        .read_to_end(&mut assets_bin_data)?;

    let db = assets_bin::parse_assets_bin(&assets_bin_data)?;
    let self_id_index = db.build_self_id_index();

    // Strategy 1: Try direct resolution — scan paths_storage for .visual files
    // in a directory matching the user's name.
    let needle = format!("/{name}/");
    let mut visual_paths: Vec<(String, String)> = Vec::new(); // (sub_model_name, full_path)

    for (i, entry) in db.paths_storage.iter().enumerate() {
        if !entry.name.ends_with(".visual") {
            continue;
        }
        let full_path = db.reconstruct_path(i, &self_id_index);
        if full_path.contains(&needle) {
            let sub_name = entry
                .name
                .strip_suffix(".visual")
                .unwrap_or(&entry.name)
                .to_string();
            visual_paths.push((sub_name, full_path));
        }
    }

    // Strategy 2: If no direct match, try name lookup via GameParams.
    if visual_paths.is_empty() {
        let model_dir = resolve_ship_name_to_model_dir(vfs, name, game_dir, game_version)?;

        let dir_needle = format!("/{}/", model_dir.rsplit('/').next().unwrap_or(&model_dir));
        for (i, entry) in db.paths_storage.iter().enumerate() {
            if !entry.name.ends_with(".visual") {
                continue;
            }
            let full_path = db.reconstruct_path(i, &self_id_index);
            if full_path.contains(&dir_needle) {
                let sub_name = entry
                    .name
                    .strip_suffix(".visual")
                    .unwrap_or(&entry.name)
                    .to_string();
                visual_paths.push((sub_name, full_path));
            }
        }
    }

    if visual_paths.is_empty() {
        bail!("No .visual files found for '{name}'. Try using the model directory name directly.");
    }

    visual_paths.sort_by(|a, b| a.0.cmp(&b.0));

    println!("Found {} sub-models:", visual_paths.len());
    for (sub_name, _) in &visual_paths {
        println!("  {sub_name}");
    }

    // Parse each visual and its geometry.
    struct SubModelData {
        name: String,
        visual: visual::VisualPrototype,
        geom_bytes: Vec<u8>,
    }

    let mut sub_model_data: Vec<SubModelData> = Vec::new();

    for (sub_name, _vis_full_path) in &visual_paths {
        let visual_suffix = format!("{sub_name}.visual");
        let (vis_location, _) = db
            .resolve_path(&visual_suffix, &self_id_index)
            .context_with(|| format!("Could not resolve visual: {visual_suffix}"))?;

        if vis_location.blob_index != 1 {
            eprintln!(
                "Warning: '{visual_suffix}' resolved to blob {} (not VisualPrototype), skipping",
                vis_location.blob_index
            );
            continue;
        }

        let vis_data = db
            .get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE)
            .context("Failed to get visual prototype data")?;
        let vp = visual::parse_visual(vis_data).context("Failed to parse VisualPrototype")?;

        // Resolve geometry path from mergedGeometryPathId.
        let geom_path_idx = self_id_index
            .get(&vp.merged_geometry_path_id)
            .ok_or_else(|| {
                rootcause::report!(
                    "Could not resolve mergedGeometryPathId 0x{:016X} for {}",
                    vp.merged_geometry_path_id,
                    sub_name
                )
            })?;
        let geom_full_path = db.reconstruct_path(*geom_path_idx, &self_id_index);

        let mut geom_bytes = Vec::new();
        vfs.join(&geom_full_path)
            .context("VFS path error")?
            .open_file()
            .context_with(|| format!("Could not open geometry: {geom_full_path}"))?
            .read_to_end(&mut geom_bytes)?;

        println!(
            "  {sub_name}: {} render sets, geometry={}",
            vp.render_sets.len(),
            geom_full_path,
        );

        sub_model_data.push(SubModelData {
            name: sub_name.clone(),
            visual: vp,
            geom_bytes,
        });
    }

    // --- Load component mounts (turrets, AA, etc.) from GameParams ---

    /// Info about a single mount point from GameParams.
    struct MountInfo {
        hp_name: String,    // e.g. "HP_JGM_1"
        model_path: String, // e.g. "content/gameplay/.../JGM178.model"
    }

    let mut mount_infos: Vec<MountInfo> = Vec::new();
    let mut ship_display_name = String::new();

    // We need GameParams to know which turrets go on which hull.
    // Load raw pickle data — this is the same approach as load_game_params().
    let pickle = load_game_params(vfs)?;
    let params_dict = match &pickle {
        pickled::Value::Dict(d) => d
            .inner()
            .get(&HashableValue::String("".to_string().into()))
            .and_then(|v| v.dict_ref())
            .map(|d| d.inner().clone()),
        pickled::Value::List(l) => l
            .inner()
            .first()
            .and_then(|v| v.dict_ref())
            .map(|d| d.inner().clone()),
        _ => None,
    };

    if let Some(ref params_dict) = params_dict {
        // Find the ship param by matching the model directory name.
        // The model_dir from our visual_paths contains the dir name like "JSB039_Yamato_1945".
        let ship_dir_name = visual_paths
            .first()
            .and_then(|(_, path)| {
                // Extract dir name: "content/gameplay/.../JSB039_Yamato_1945/Foo.visual" -> "JSB039_Yamato_1945"
                let parent = path.rsplit_once('/')?.0;
                parent.rsplit_once('/').map(|(_, d)| d.to_string())
            })
            .unwrap_or_default();

        // Find the ship entity in GameParams by matching hull model path.
        let mut ship_param_key = String::new();
        let mut ship_data_opt: Option<pickled::Value> = None;

        for (key, value) in params_dict.iter() {
            let Some(key_str) = key.string_ref() else {
                continue;
            };
            let Some(entity_dict) = value.dict_ref() else {
                continue;
            };
            let entity = entity_dict.inner();

            // Check typeinfo.type == "Ship"
            let is_ship = entity
                .get(&HashableValue::String("typeinfo".to_string().into()))
                .and_then(|v| v.dict_ref())
                .map(|ti| {
                    ti.inner()
                        .get(&HashableValue::String("type".to_string().into()))
                        .and_then(|v| v.string_ref())
                        .map(|s| s.inner() == "Ship")
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if !is_ship {
                continue;
            }

            // Check if any hull component's model path contains our ship_dir_name.
            let has_matching_hull = entity.iter().any(|(ek, ev)| {
                let Some(ek_str) = ek.string_ref() else {
                    return false;
                };
                if !ek_str.inner().contains("Hull") {
                    return false;
                }
                let Some(hull_dict) = ev.dict_ref() else {
                    return false;
                };
                hull_dict
                    .inner()
                    .get(&HashableValue::String("model".to_string().into()))
                    .and_then(|v| v.string_ref())
                    .map(|s| s.inner().contains(&ship_dir_name))
                    .unwrap_or(false)
            });

            if has_matching_hull {
                ship_param_key = key_str.inner().clone();
                ship_data_opt = Some(value.clone());
                break;
            }
        }

        if let Some(ship_data_val) = ship_data_opt {
            let ship_data = ship_data_val.dict_ref();
            let Some(ship_data) = ship_data else {
                bail!("Ship data is not a dict");
            };
            let ship_data = ship_data.inner();
            ship_display_name = ship_param_key.clone();

            // Read ShipUpgradeInfo to find _Hull upgrades.
            let upgrade_info = ship_data
                .get(&HashableValue::String("ShipUpgradeInfo".to_string().into()))
                .and_then(|v| v.dict_ref());

            if let Some(upgrade_info) = upgrade_info {
                // Collect all _Hull upgrades.
                let mut hull_upgrades: Vec<(
                    String,
                    std::collections::BTreeMap<HashableValue, pickled::Value>,
                )> = Vec::new();

                for (uk, uv) in upgrade_info.inner().iter() {
                    let Some(uk_str) = uk.string_ref() else {
                        continue;
                    };
                    let Some(ud) = uv.dict_ref() else {
                        continue;
                    };
                    let uc_type = ud
                        .inner()
                        .get(&HashableValue::String("ucType".to_string().into()))
                        .and_then(|v| v.string_ref())
                        .map(|s| s.inner().clone())
                        .unwrap_or_default();
                    if uc_type == "_Hull" {
                        hull_upgrades.push((uk_str.inner().clone(), ud.inner().clone()));
                    }
                }

                hull_upgrades.sort_by(|a, b| a.0.cmp(&b.0));

                // Handle --list-upgrades
                if list_upgrades {
                    println!("Hull upgrades for {ship_display_name}:");
                    let component_types = [
                        "hull",
                        "artillery",
                        "atba",
                        "airDefense",
                        "directors",
                        "finders",
                        "radars",
                        "torpedoes",
                    ];
                    for (i, (upgrade_name, upgrade_dict)) in hull_upgrades.iter().enumerate() {
                        println!("  [{}] {upgrade_name}", i + 1);

                        let components = upgrade_dict
                            .get(&HashableValue::String("components".to_string().into()))
                            .and_then(|v| v.dict_ref());

                        if let Some(components) = components {
                            for ct in &component_types {
                                let comp_name = components
                                    .inner()
                                    .get(&HashableValue::String(ct.to_string().into()))
                                    .and_then(|v| v.list_ref())
                                    .and_then(|l| {
                                        l.inner()
                                            .first()
                                            .and_then(|v| v.string_ref())
                                            .map(|s| s.inner().clone())
                                    });
                                if let Some(ref comp_name) = comp_name {
                                    // Count HP_ entries in the component
                                    let mount_count = ship_data
                                        .get(&HashableValue::String(comp_name.clone().into()))
                                        .and_then(|v| v.dict_ref())
                                        .map(|d| {
                                            d.inner()
                                                .keys()
                                                .filter(|k| {
                                                    k.string_ref()
                                                        .map(|s| s.inner().starts_with("HP_"))
                                                        .unwrap_or(false)
                                                })
                                                .count()
                                        })
                                        .unwrap_or(0);
                                    println!("      {ct}: {comp_name} ({mount_count} mounts)");
                                } else {
                                    println!("      {ct}: (none)");
                                }
                            }
                        }
                    }
                    return Ok(());
                }

                // Select hull upgrade.
                let selected_upgrade = if let Some(sel) = hull_selection {
                    hull_upgrades
                        .iter()
                        .find(|(name, _)| {
                            name == sel || name.to_lowercase().contains(&sel.to_lowercase())
                        })
                        .or_else(|| {
                            // Try matching by hull letter prefix (A, B, C)
                            let prefix = format!("{sel}_");
                            hull_upgrades.iter().find(|(_, dict)| {
                                dict.get(&HashableValue::String("components".to_string().into()))
                                    .and_then(|v| v.dict_ref())
                                    .map(|c| {
                                        c.inner()
                                            .get(&HashableValue::String("hull".to_string().into()))
                                            .and_then(|v| v.list_ref())
                                            .map(|l| {
                                                l.inner()
                                                    .first()
                                                    .and_then(|v| v.string_ref())
                                                    .map(|s| s.inner().starts_with(&prefix))
                                                    .unwrap_or(false)
                                            })
                                            .unwrap_or(false)
                                    })
                                    .unwrap_or(false)
                            })
                        })
                } else {
                    hull_upgrades.first()
                };

                if let Some((_upgrade_name, upgrade_dict)) = selected_upgrade {
                    let components = upgrade_dict
                        .get(&HashableValue::String("components".to_string().into()))
                        .and_then(|v| v.dict_ref());

                    if let Some(components) = components {
                        // Component types that have 3D models.
                        let model_component_types = [
                            "artillery",
                            "atba",
                            "airDefense",
                            "directors",
                            "finders",
                            "radars",
                            "torpedoes",
                        ];

                        for ct in &model_component_types {
                            let comp_name = components
                                .inner()
                                .get(&HashableValue::String(ct.to_string().into()))
                                .and_then(|v| v.list_ref())
                                .and_then(|l| {
                                    l.inner()
                                        .first()
                                        .and_then(|v| v.string_ref())
                                        .map(|s| s.inner().clone())
                                });
                            let Some(comp_name) = comp_name else {
                                continue;
                            };

                            let comp_data = ship_data
                                .get(&HashableValue::String(comp_name.into()))
                                .and_then(|v| v.dict_ref());
                            let Some(comp_data) = comp_data else {
                                continue;
                            };

                            for (mk, mv) in comp_data.inner().iter() {
                                let Some(mk_str) = mk.string_ref() else {
                                    continue;
                                };
                                if !mk_str.inner().starts_with("HP_") {
                                    continue;
                                }
                                let Some(mount_dict) = mv.dict_ref() else {
                                    continue;
                                };
                                let model_path = mount_dict
                                    .inner()
                                    .get(&HashableValue::String("model".to_string().into()))
                                    .and_then(|v| v.string_ref())
                                    .map(|s| s.inner().clone());
                                let Some(model_path) = model_path else {
                                    continue;
                                };

                                mount_infos.push(MountInfo {
                                    hp_name: mk_str.inner().clone(),
                                    model_path,
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // --- Parse and deduplicate turret models ---

    // Collect unique model paths for turrets.
    let mut unique_turret_models: HashMap<String, usize> = HashMap::new(); // model_path -> index
    let mut turret_model_data: Vec<SubModelData> = Vec::new();

    for mi in &mount_infos {
        if unique_turret_models.contains_key(&mi.model_path) {
            continue;
        }

        // Convert .model path to .visual path
        let visual_path = mi.model_path.replace(".model", ".visual");
        let visual_suffix = visual_path
            .rsplit('/')
            .next()
            .unwrap_or(&visual_path)
            .to_string();

        let vis_location = match db.resolve_path(&visual_suffix, &self_id_index) {
            Ok((loc, _)) => loc,
            Err(_) => {
                eprintln!(
                    "Warning: could not resolve turret visual '{}', skipping",
                    visual_suffix
                );
                continue;
            }
        };

        if vis_location.blob_index != 1 {
            continue;
        }

        let vis_data = match db.get_prototype_data(vis_location, visual::VISUAL_ITEM_SIZE) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Warning: failed to get turret visual data: {e}");
                continue;
            }
        };
        let vp = match visual::parse_visual(vis_data) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Warning: failed to parse turret visual: {e}");
                continue;
            }
        };

        // Resolve geometry.
        let geom_path_idx = match self_id_index.get(&vp.merged_geometry_path_id) {
            Some(&idx) => idx,
            None => {
                eprintln!(
                    "Warning: could not resolve geometry for turret '{}', skipping",
                    visual_suffix
                );
                continue;
            }
        };
        let geom_full_path = db.reconstruct_path(geom_path_idx, &self_id_index);
        let mut geom_bytes = Vec::new();
        match vfs.join(&geom_full_path).and_then(|p| p.open_file()) {
            Ok(mut f) => {
                f.read_to_end(&mut geom_bytes)?;
            }
            Err(e) => {
                eprintln!(
                    "Warning: could not open turret geometry '{}': {e}",
                    geom_full_path
                );
                continue;
            }
        }

        let idx = turret_model_data.len();
        unique_turret_models.insert(mi.model_path.clone(), idx);
        let model_short_name = mi
            .model_path
            .rsplit('/')
            .next()
            .unwrap_or(&mi.model_path)
            .strip_suffix(".model")
            .unwrap_or(&mi.model_path);
        turret_model_data.push(SubModelData {
            name: model_short_name.to_string(),
            visual: vp,
            geom_bytes,
        });
    }

    if !mount_infos.is_empty() {
        println!(
            "Found {} mount points ({} unique models)",
            mount_infos.len(),
            unique_turret_models.len()
        );
    }

    // --- Build hardpoint transform maps from hull sub-model visuals ---

    // Collect all hardpoint transforms from all hull sub-model visuals.
    let mut hp_transforms: HashMap<String, [f32; 16]> = HashMap::new();
    for smd in &sub_model_data {
        for (_i, &name_id) in smd.visual.nodes.name_map_name_ids.iter().enumerate() {
            if let Some(name) = db.strings.get_string_by_id(name_id) {
                if name.starts_with("HP_") {
                    if let Some(xform) = smd.visual.find_hardpoint_transform(name, &db.strings) {
                        hp_transforms.insert(name.to_string(), xform);
                    }
                }
            }
        }
    }

    // --- Parse geometries and build SubModel refs ---

    // Parse hull geometries.
    let parsed_hull_geoms: Vec<geometry::MergedGeometry<'_>> = sub_model_data
        .iter()
        .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse geometry"))
        .collect();

    // Parse turret geometries.
    let parsed_turret_geoms: Vec<geometry::MergedGeometry<'_>> = turret_model_data
        .iter()
        .map(|d| geometry::parse_geometry(&d.geom_bytes).expect("Failed to parse turret geometry"))
        .collect();

    // Build SubModel list: hull parts + mounted components.
    let mut sub_models: Vec<SubModel<'_>> = Vec::new();

    // Hull sub-models (no transform, placed at origin).
    for (data, geom) in sub_model_data.iter().zip(parsed_hull_geoms.iter()) {
        sub_models.push(SubModel {
            name: data.name.clone(),
            visual: &data.visual,
            geometry: geom,
            transform: None,
        });
    }

    // Mount sub-models (with hardpoint transforms).
    let mut mounted_count = 0;
    let mut skipped_count = 0;
    for mi in &mount_infos {
        let Some(&model_idx) = unique_turret_models.get(&mi.model_path) else {
            continue;
        };

        // Skip compound hardpoints (e.g. "HP_JGM_1_HP_JGA_1") for now.
        let hp_parts: Vec<&str> = mi.hp_name.split("_HP_").collect();
        if hp_parts.len() > 1 {
            skipped_count += 1;
            continue;
        }

        let transform = hp_transforms.get(&mi.hp_name).copied();
        if transform.is_none() {
            skipped_count += 1;
            continue;
        }

        let turret_data = &turret_model_data[model_idx];
        let turret_geom = &parsed_turret_geoms[model_idx];

        sub_models.push(SubModel {
            name: format!("{} ({})", mi.hp_name, turret_data.name),
            visual: &turret_data.visual,
            geometry: turret_geom,
            transform,
        });
        mounted_count += 1;
    }

    if mounted_count > 0 || skipped_count > 0 {
        println!(
            "Mounted {} components ({} skipped — compound hardpoints or missing nodes)",
            mounted_count, skipped_count
        );
    }

    let mut out_file = std::fs::File::create(output).context("Failed to create output file")?;
    gltf_export::export_ship_glb(&sub_models, &db, lod, &mut out_file)
        .context("Failed to export ship GLB")?;

    let file_size = std::fs::metadata(output).map(|m| m.len()).unwrap_or(0);
    println!(
        "Exported {} sub-models (LOD {}) to {} ({} bytes)",
        sub_models.len(),
        lod,
        output.display(),
        file_size
    );

    Ok(())
}

/// Resolve a ship display name to a model directory path via GameParams lookup.
fn resolve_ship_name_to_model_dir(
    vfs: &VfsPath,
    name: &str,
    game_dir: &Path,
    game_version: Option<u64>,
) -> Result<String, Report> {
    use wowsunpack::data::ResourceLoader;
    use wowsunpack::game_params::provider::GameMetadataProvider;
    use wowsunpack::game_params::types::GameParamProvider;

    let mut metadata = GameMetadataProvider::from_vfs(vfs)
        .context("Failed to load GameParams for ship name lookup")?;

    // Try to load translations.
    if let Some(version) = game_version {
        let mo_path = wowsunpack::game_data::translations_path(game_dir, version as u32);
        if let Ok(mo_data) = std::fs::read(&mo_path) {
            if let Ok(catalog) = gettext::Catalog::parse(&*mo_data) {
                metadata.set_translations(catalog);
            }
        }
    }

    let normalized_input = unidecode::unidecode(name).to_lowercase();

    let mut matches: Vec<(String, String, String)> = Vec::new(); // (display_name, index, model_path)

    for param in metadata.params() {
        let vehicle = match param.vehicle() {
            Some(v) => v,
            None => continue,
        };
        let model_path = match vehicle.model_path() {
            Some(p) => p,
            None => continue,
        };

        // Try translated name first, fall back to param index.
        let display_name = metadata
            .localized_name_from_param(param)
            .map(|s| s.to_string())
            .unwrap_or_else(|| param.index().to_string());

        let normalized_display = unidecode::unidecode(&display_name).to_lowercase();

        if normalized_display.contains(&normalized_input) {
            matches.push((
                display_name,
                param.index().to_string(),
                model_path.to_string(),
            ));
        }
    }

    match matches.len() {
        0 => bail!(
            "No ship found matching '{name}'. Try using the model directory name directly \
             (e.g. 'JSB039_Yamato_1945')."
        ),
        1 => {
            let model_path = &matches[0].2;
            let dir = model_path
                .rsplit_once('/')
                .map(|(d, _)| d)
                .unwrap_or(model_path);
            println!("Resolved '{}' -> {} ({})", name, matches[0].0, dir);
            Ok(dir.to_string())
        }
        _ => {
            // If all matches resolve to the same model directory, just use it.
            let dirs: Vec<&str> = matches
                .iter()
                .map(|(_, _, mp)| mp.rsplit_once('/').map(|(d, _)| d).unwrap_or(mp.as_str()))
                .collect();
            let unique_dirs: std::collections::HashSet<&&str> = dirs.iter().collect();
            if unique_dirs.len() == 1 {
                let dir = dirs[0];
                println!(
                    "Resolved '{}' -> {} ({} variants, same model dir)",
                    name,
                    matches[0].0,
                    matches.len()
                );
                return Ok(dir.to_string());
            }

            eprintln!("Multiple ships match '{name}':");
            for (display, index, model_path) in &matches {
                let dir = model_path
                    .rsplit_once('/')
                    .map(|(d, _)| d)
                    .unwrap_or(model_path);
                let dir_name = dir.rsplit('/').next().unwrap_or(dir);
                eprintln!("  {display} ({index}) -> {dir_name}");
            }
            bail!(
                "Multiple ships match '{name}'. Please refine your search or use \
                 the model directory name directly."
            );
        }
    }
}

fn main() -> Result<(), Report> {
    let timestamp = Instant::now();

    run()?;

    println!(
        "Finished in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    Ok(())
}
