#[derive(Debug)]
pub enum ParserError {
    ParseError,
    DocumentFormat,
    MissingField(&'static str),
    InvalidType(&'static str),
    ValidationError(&'static str),
}
