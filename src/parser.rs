use crate::binary::BinaryReader;
use crate::parser::error::ParseError;

pub type ParseResult<T> = Result<T, ParseError>;

mod components;
mod error;
mod constant;

pub trait BinaryParser<R> where R: BinaryReader {
    fn parse(binary: &mut R) -> ParseResult<Self> where Self: Sized;
}
