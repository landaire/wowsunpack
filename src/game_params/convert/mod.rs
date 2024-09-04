#[cfg(feature = "cbor")]
mod cbor;
#[cfg(feature = "json")]
mod json;

#[cfg(feature = "cbor")]
pub use crate::game_params::convert::cbor::*;

#[cfg(feature = "json")]
pub use crate::game_params::convert::json::*;

use std::io::Cursor;

use flate2::read::ZlibDecoder;
use pickled::DeOptions;

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
