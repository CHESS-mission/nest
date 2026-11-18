use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("Failed to parse topology")]
    ParseError,
    #[error("Invalid format")]
    InvalidFormat,
    #[error("Failed to read topology file {0}: {1}")]
    ReadError(PathBuf, std::io::Error),
    #[error("Missing field: \"{0}\"")]
    MissingField(&'static str),
    #[error("Invalid type: \"{0}\"")]
    InvalidType(&'static str),
    #[error("Validation failed: {0}")]
    ValidationError(String),
}
