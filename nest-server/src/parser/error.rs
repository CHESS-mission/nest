use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("Failed to parse topology")]
    ParseError,
    #[error("Invalid format")]
    InvalidFormat,
    #[error("Missing field: \"{0}\"")]
    MissingField(&'static str),
    #[error("Invalid type: \"{0}\"")]
    InvalidType(&'static str),
    #[error("Validation failed: {0}")]
    ValidationError(&'static str),
}
