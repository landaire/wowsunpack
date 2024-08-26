use std::io::{Cursor, Write};

use flate2::read::ZlibDecoder;
use pickled::DeOptions;
use serde_json::Map;
use thiserror::Error;

use crate::{
    data::{idx::FileNode, pkg::PkgFileLoader},
    error::ErrorKind,
};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Pickle deserialization")]
    PickleError(#[from] pickled::Error),
    #[error("JSON serialization")]
    Json(#[from] serde_json::Error),
    #[error("I/O error")]
    Io(#[from] std::io::Error),
    #[error("Unexpected GameParams data type")]
    InvalidGameParamsData,
    #[error("File tree error")]
    FileTreeError(#[from] crate::idx::IdxError),
}

fn hashable_pickle_to_json(pickled: pickled::HashableValue) -> serde_json::Value {
    match pickled {
        pickled::HashableValue::None => serde_json::Value::Null,
        pickled::HashableValue::Bool(v) => serde_json::Value::Bool(v),
        pickled::HashableValue::I64(v) => serde_json::Value::Number(serde_json::Number::from(v)),
        pickled::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        pickled::HashableValue::F64(v) => {
            serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64"))
        }
        pickled::HashableValue::Bytes(v) => serde_json::Value::Array(
            v.into_iter()
                .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                .collect(),
        ),
        pickled::HashableValue::String(v) => serde_json::Value::String(v),
        pickled::HashableValue::Tuple(v) => {
            serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::HashableValue::FrozenSet(v) => {
            serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
        }
    }
}

fn pickle_to_json(pickled: pickled::Value) -> serde_json::Value {
    match pickled {
        pickled::Value::None => serde_json::Value::Null,
        pickled::Value::Bool(v) => serde_json::Value::Bool(v),
        pickled::Value::I64(v) => serde_json::Value::Number(serde_json::Number::from(v)),
        pickled::Value::Int(_v) => todo!("Int -> JSON"),
        pickled::Value::F64(v) => {
            serde_json::Value::Number(serde_json::Number::from_f64(v).expect("invalid f64"))
        }
        pickled::Value::Bytes(v) => serde_json::Value::Array(
            v.into_iter()
                .map(|b| serde_json::Value::Number(serde_json::Number::from(b)))
                .collect(),
        ),
        pickled::Value::String(v) => serde_json::Value::String(v),
        pickled::Value::List(v) => {
            serde_json::Value::Array(v.into_iter().map(pickle_to_json).collect())
        }
        pickled::Value::Tuple(v) => {
            serde_json::Value::Array(v.into_iter().map(pickle_to_json).collect())
        }
        pickled::Value::Set(v) => {
            serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::Value::FrozenSet(v) => {
            serde_json::Value::Array(v.into_iter().map(hashable_pickle_to_json).collect())
        }
        pickled::Value::Dict(v) => {
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
) -> Result<(), crate::error::ErrorKind> {
    let game_params = file_tree.find("content/GameParams.data")?;
    let mut game_params_data = Vec::new();
    game_params.read_file(pkg_loader, &mut game_params_data)?;

    let decoded = game_params_to_pickle(game_params_data)?;

    let decoded: pickled::Value = pickled::from_reader(
        &mut decompressed_data,
        DeOptions::default()
            .replace_unresolved_globals()
            .decode_strings(),
    )?;

    let converted = if let pickled::Value::List(list) = decoded {
        pickle_to_json(list.into_iter().next().unwrap())
    } else {
        return Err(ErrorKind::InvalidGameParamsData);
    };

    if pretty_print {
        serde_json::to_writer_pretty(writer, &converted)?;
    } else {
        serde_json::to_writer(writer, &converted)?;
    }

    Ok(())
}

/// Converts a raw pickled GameParams.data file to its pickled representation. This operation is quite
/// expensive.
pub fn game_params_to_pickle(
    mut game_params_data: Vec<u8>,
) -> Result<pickled::Value, crate::error::ErrorKind> {
    game_params_data.reverse();

    let mut decompressed_data = Cursor::new(Vec::new());
    let mut decoder = ZlibDecoder::new(Cursor::new(game_params_data));
    std::io::copy(&mut decoder, &mut decompressed_data)?;
    decompressed_data.set_position(0);

    pickled::from_reader(
        &mut decompressed_data,
        DeOptions::default()
            .replace_unresolved_globals()
            .decode_strings(),
    )
    .map_err(|err| err.into())
}
