use crate::binary::BinaryReader;
use crate::parser::error::ParseError;

pub type ParseResult<T> = Result<T, ParseError>;

mod error;
pub(crate) mod core;

pub trait BinaryParser<R> where R: BinaryReader {
    fn parse(binary: &mut R) -> ParseResult<Self> where Self: Sized;
}
pub use core::Module;