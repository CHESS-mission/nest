#[derive(Debug)]
pub enum ParserError {
    ParseError,
    InvalidFormat,
    MissingField(&'static str),
    InvalidType(&'static str),
    ValidationError(&'static str),
}
