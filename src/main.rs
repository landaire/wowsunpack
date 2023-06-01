use eyre::{Result, WrapErr};
use flate2::read::{DeflateDecoder, ZlibDecoder};
use memmap::MmapOptions;
use pkg::PkgFileLoader;
use serde_json::Map;
use serde_pickle::DeOptions;
use std::{
    convert,
    fs::{self, File, FileType},
    io::{BufWriter, Cursor, Read},
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

    let game_params = file_tree.find("content/GameParams.data")?;
    let mut game_params_data = Vec::new();
    game_params.read_file(pkg_loader.as_mut().unwrap(), &mut game_params_data)?;
    game_params_data.reverse();

    let mut decompressed_data = Cursor::new(Vec::new());
    let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
    std::io::copy(&mut decoder, &mut decompressed_data)?;
    decompressed_data.set_position(0);

    let decoded: serde_pickle::Value = serde_pickle::from_reader(
        &mut decompressed_data,
        DeOptions::default()
            .replace_unresolved_globals()
            .replace_recursive()
            .decode_strings(),
    )?;

    fn hashable_pickle_to_json(pickled: serde_pickle::HashableValue) -> serde_json::Value {
        match pickled {
            serde_pickle::HashableValue::None => serde_json::Value::Null,
            serde_pickle::HashableValue::Bool(v) => serde_json::Value::Bool(v),
            serde_pickle::HashableValue::I64(v) => {
                serde_json::Value::Number(serde_json::Number::from(v))
            }
            serde_pickle::HashableValue::Int(v) => todo!(),
            serde_pickle::HashableValue::F64(v) => {
                serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64"))
            }
            serde_pickle::HashableValue::Bytes(v) => serde_json::Value::Array(
                v.into_iter()
                    .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                    .collect(),
            ),
            serde_pickle::HashableValue::String(v) => serde_json::Value::String(v),
            serde_pickle::HashableValue::Tuple(v) => {
                serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
            }
            serde_pickle::HashableValue::FrozenSet(v) => {
                serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
            }
        }
    }

    fn pickle_to_json(pickled: serde_pickle::Value) -> serde_json::Value {
        match pickled {
            serde_pickle::Value::None => serde_json::Value::Null,
            serde_pickle::Value::Bool(v) => serde_json::Value::Bool(v),
            serde_pickle::Value::I64(v) => serde_json::Value::Number(serde_json::Number::from(v)),
            serde_pickle::Value::Int(v) => todo!(),
            serde_pickle::Value::F64(v) => {
                serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64"))
            }
            serde_pickle::Value::Bytes(v) => serde_json::Value::Array(
                v.into_iter()
                    .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                    .collect(),
            ),
            serde_pickle::Value::String(v) => serde_json::Value::String(v),
            serde_pickle::Value::List(v) => {
                serde_json::Value::Array(v.into_iter().map(pickle_to_json).collect())
            }
            serde_pickle::Value::Tuple(v) => {
                serde_json::Value::Array(v.into_iter().map(pickle_to_json).collect())
            }
            serde_pickle::Value::Set(v) => {
                serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
            }
            serde_pickle::Value::FrozenSet(v) => {
                serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
            }
            serde_pickle::Value::Dict(v) => {
                let mut map = Map::new();
                for (key, value) in &v {
                    let converted_key = hashable_pickle_to_json(key.clone());
                    let string_key = match converted_key {
                        serde_json::Value::Number(num) => num.to_string(),
                        serde_json::Value::String(s) => s.to_string(),
                        other => {
                            continue;
                            panic!(
                                "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                                other, key, v
                            );
                        }
                    };

                    let converted_value = pickle_to_json(value.clone());

                    map.insert(string_key, converted_value);
                }

                serde_json::Value::Object(map)
            }
        }
    }

    // match decoded {
    //     serde_pickle::Value::None => todo!(),
    //     serde_pickle::Value::Bool(_) => todo!(),
    //     serde_pickle::Value::I64(_) => todo!(),
    //     serde_pickle::Value::Int(_) => todo!(),
    //     serde_pickle::Value::F64(_) => todo!(),
    //     serde_pickle::Value::Bytes(_) => todo!(),
    //     serde_pickle::Value::String(_) => todo!(),
    //     serde_pickle::Value::List(list) => panic!("{}", &list[1]),
    //     serde_pickle::Value::Tuple(_) => todo!(),
    //     serde_pickle::Value::Set(_) => todo!(),
    //     serde_pickle::Value::FrozenSet(_) => todo!(),
    //     serde_pickle::Value::Dict(_) => todo!(),
    // };

    // panic!("{:#?}", decoded);

    let converted = if let serde_pickle::Value::List(list) = decoded {
        pickle_to_json(list.into_iter().next().unwrap())
    } else {
        panic!("");
    };
    println!(
        "converted in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    println!("writing data");
    let mut f = BufWriter::new(File::create("GameParams.json")?);
    //let mut data = Vec::new();
    serde_json::to_writer_pretty(f, &converted)?;
    //std::fs::write("GameParams.data", &data);
    println!(
        "Parsed resources in {} seconds",
        (Instant::now() - timestamp).as_secs_f32()
    );

    panic!("");

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
