use std::io::Write;

use crate::{
    data::{idx::FileNode, pkg::PkgFileLoader},
    error::ErrorKind,
};

#[cfg(feature = "cbor")]
fn hashable_pickle_to_cbor(pickled: pickled::HashableValue) -> serde_cbor::Value {
    match pickled {
        pickled::HashableValue::None => serde_cbor::Value::Null,
        pickled::HashableValue::Bool(v) => serde_cbor::Value::Bool(v),
        pickled::HashableValue::I64(v) => serde_cbor::Value::Integer(v.into()),
        pickled::HashableValue::Int(_v) => todo!("Hashable int -> JSON"),
        pickled::HashableValue::F64(v) => serde_cbor::Value::Float(v),
        pickled::HashableValue::Bytes(v) => serde_cbor::Value::Bytes(v.into_raw_or_cloned()),
        pickled::HashableValue::String(v) => serde_cbor::Value::Text(v.into_raw_or_cloned()),
        pickled::HashableValue::Tuple(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(hashable_pickle_to_cbor)
                .collect(),
        ),
        pickled::HashableValue::FrozenSet(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(hashable_pickle_to_cbor)
                .collect(),
        ),
    }
}

#[cfg(feature = "cbor")]
pub fn pickle_to_cbor(pickled: pickled::Value) -> serde_cbor::Value {
    use std::collections::BTreeMap;

    match pickled {
        pickled::Value::None => serde_cbor::Value::Null,
        pickled::Value::Bool(v) => serde_cbor::Value::Bool(v),
        pickled::Value::I64(v) => serde_cbor::Value::Integer(v.into()),
        pickled::Value::Int(_v) => todo!("Int -> JSON"),
        pickled::Value::F64(v) => serde_cbor::Value::Float(v),
        pickled::Value::Bytes(v) => serde_cbor::Value::Bytes(v.into_raw_or_cloned()),
        pickled::Value::String(v) => serde_cbor::Value::Text(v.into_raw_or_cloned()),
        pickled::Value::List(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(pickle_to_cbor)
                .collect(),
        ),
        pickled::Value::Tuple(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(pickle_to_cbor)
                .collect(),
        ),
        pickled::Value::Set(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(hashable_pickle_to_cbor)
                .collect(),
        ),
        pickled::Value::FrozenSet(v) => serde_cbor::Value::Array(
            v.into_raw_or_cloned()
                .into_iter()
                .map(hashable_pickle_to_cbor)
                .collect(),
        ),
        pickled::Value::Dict(v) => {
            let mut map = BTreeMap::new();
            let v = v.into_raw_or_cloned();
            for (key, value) in &v {
                let converted_key = hashable_pickle_to_cbor(key.clone());
                let string_key = match converted_key {
                    serde_cbor::Value::Integer(num) => num.to_string(),
                    serde_cbor::Value::Text(s) => s,
                    _other => {
                        continue;
                        // panic!(
                        //     "Unsupported key type: {:?} (original: {:#?}, {:#?})",
                        //     other, key, v
                        // );
                    }
                };

                let converted_value = pickle_to_cbor(value.clone());

                map.insert(serde_cbor::Value::Text(string_key), converted_value);
            }

            serde_cbor::Value::Map(map)
        }
    }
}

#[cfg(feature = "cbor")]
pub fn read_game_params_as_cbor<W: Write>(
    file_tree: FileNode,
    pkg_loader: &PkgFileLoader,
    writer: &mut W,
) -> Result<(), crate::error::ErrorKind> {
    use super::game_params_to_pickle;

    let game_params = file_tree.find("content/GameParams.data")?;
    let mut game_params_data = Vec::new();
    game_params.read_file(pkg_loader, &mut game_params_data)?;

    let decoded = game_params_to_pickle(game_params_data)?;

    let converted = if let pickled::Value::List(list) = decoded {
        pickle_to_cbor(list.into_raw_or_cloned().into_iter().next().unwrap())
    } else {
        return Err(ErrorKind::InvalidGameParamsData);
    };

    serde_cbor::to_writer(writer, &converted).map_err(|e| e.into())
}
