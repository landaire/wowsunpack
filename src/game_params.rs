use std::io::{Cursor, Write};

use flate2::read::ZlibDecoder;
use serde_json::Map;
use serde_pickle::DeOptions;
use thiserror::Error;

use crate::{idx::FileNode, pkg::PkgFileLoader};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Pickle deserialization")]
    PickleError(#[from] serde_pickle::Error),
    #[error("JSON serialization")]
    Json(#[from] serde_json::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("Unexpected GameParams data type")]
    InvalidGameParamsData,
    #[error("File tree error")]
    FileTreeError(#[from] crate::idx::IdxError),
}

fn hashable_pickle_to_json(pickled: serde_pickle::HashableValue) -> serde_json::Value {
    match pickled {
        serde_pickle::HashableValue::None => serde_json::Value::Null,
        serde_pickle::HashableValue::Bool(v) => serde_json::Value::Bool(v),
        serde_pickle::HashableValue::I64(v) => {
            serde_json::Value::Number(serde_json::Number::from(v))
        }
        serde_pickle::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
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
        serde_pickle::Value::Int(_v) => todo!("Int -> JSON"),
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
                    _other => {
                        continue;
                        // panic!(
                        //     "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                        //     other, key, v
                        // );
                    }
                };

                let converted_value = pickle_to_json(value.clone());

                map.insert(string_key, converted_value);
            }

            serde_json::Value::Object(map)
        }
    }
}

pub fn read_game_params_as_json<W: Write>(
    pretty_print: bool,
    file_tree: FileNode,
    pkg_loader: &PkgFileLoader,
    writer: &mut W,
) -> Result<(), Error> {
    let game_params = file_tree.find("content/GameParams.data")?;
    let mut game_params_data = Vec::new();
    game_params.read_file(pkg_loader, &mut game_params_data)?;
    game_params_data.reverse();

    let mut decompressed_data = Cursor::new(Vec::new());
    let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
    std::io::copy(&mut decoder, &mut decompressed_data)?;
    decompressed_data.set_position(0);

    let decoded: serde_pickle::Value = serde_pickle::from_reader(
        &mut decompressed_data,
        DeOptions::default()
            .replace_unresolved_globals()
            .decode_strings(),
    )?;

    let converted = if let serde_pickle::Value::List(list) = decoded {
        pickle_to_json(list.into_iter().next().unwrap())
    } else {
        return Err(Error::InvalidGameParamsData);
    };

    if pretty_print {
        serde_json::to_writer_pretty(writer, &converted)?;
    } else {
        serde_json::to_writer(writer, &converted)?;
    }

    Ok(())
}
