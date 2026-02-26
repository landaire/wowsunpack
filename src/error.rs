use thiserror::Error;

use crate::data::Version;

#[derive(Debug)]
pub struct Error {
    pub kind: ErrorKind,
}

#[derive(Error, Debug)]
pub enum ErrorKind {
    #[error("Parse error: {detail}")]
    ParseError { detail: String },
    #[cfg(feature = "json")]
    #[error("Error serializing or deserializing json: {err}")]
    SerdeJson {
        #[from]
        err: serde_json::Error,
    },
    #[cfg(feature = "cbor")]
    #[error("Error serializing or deserializing cbor: {err}")]
    SerdeCbor {
        #[from]
        err: serde_cbor::Error,
    },
    #[error("Error interpreting UTF-8 string: {err}")]
    Utf8Error {
        #[from]
        err: std::str::Utf8Error,
    },
    #[error("Error interpreting UTF-8 string: {err}")]
    FromUtf8Error {
        #[from]
        err: std::string::FromUtf8Error,
    },
    #[error("Unsupported replay file version found")]
    UnsupportedReplayVersion(String),
    #[error("Unable to process packet: supertype={supertype}, subtype={subtype}, reason={reason}")]
    UnableToProcessPacket {
        supertype: u32,
        subtype: u32,
        reason: String,
        _packet: Vec<u8>,
    },
    #[error(
        "Could not parse RPC value: method={method}, arg {argnum} (type={argtype}), error={error}"
    )]
    UnableToParseRpcValue {
        method: String,
        argnum: usize,
        argtype: String,
        _packet: Vec<u8>,
        error: String,
    },
    #[error("Unknown FixedDict flag: {flag:#x}")]
    UnknownFixedDictFlag { flag: u8, _packet: Vec<u8> },
    #[error("Internal prop set on unsupported entity: id={entity_id}, type={entity_type}")]
    UnsupportedInternalPropSet {
        entity_id: u32,
        entity_type: String,
        _payload: Vec<u8>,
    },
    #[error("Data file not found: version={version:?}, path={path}")]
    DatafileNotFound { version: Version, path: String },
    #[error("Decoder ring failure")]
    DecoderRingFailure(String),
    #[error("Unable to process packet")]
    ParsingFailure(String),
    #[error("Failed to decode pickled data")]
    PickleError(#[from] pickled::Error),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("Unexpected GameParams data type")]
    InvalidGameParamsData,
    #[error("File tree error")]
    FileTreeError(#[from] crate::data::idx::IdxError),
}

impl From<winnow::error::ErrMode<winnow::error::ContextError>> for Error {
    fn from(e: winnow::error::ErrMode<winnow::error::ContextError>) -> Self {
        Self {
            kind: ErrorKind::ParseError {
                detail: format!("{e}"),
            },
        }
    }
}

impl From<winnow::error::ErrMode<winnow::error::ContextError>> for ErrorKind {
    fn from(e: winnow::error::ErrMode<winnow::error::ContextError>) -> Self {
        ErrorKind::ParseError {
            detail: format!("{e}"),
        }
    }
}

impl std::convert::From<std::str::Utf8Error> for Error {
    fn from(x: std::str::Utf8Error) -> Error {
        Error { kind: x.into() }
    }
}

impl std::convert::From<std::string::FromUtf8Error> for Error {
    fn from(x: std::string::FromUtf8Error) -> Error {
        Error { kind: x.into() }
    }
}

#[cfg(feature = "json")]
impl std::convert::From<serde_json::Error> for Error {
    fn from(x: serde_json::Error) -> Error {
        Error { kind: x.into() }
    }
}

pub type IResult<T> = Result<T, Error>;

pub fn failure_from_kind(kind: ErrorKind) -> Error {
    Error { kind }
}
