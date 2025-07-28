use crate::parser::error::ParseError;
use binary_reader::BinaryReader;

pub type ParseResult<T> = Result<T, ParseError>;

pub mod core;
mod error;
pub mod leb128;

pub trait BinaryParser<R>
where
    R: BinaryReader,
{
    fn parse(binary: &mut R) -> ParseResult<Self>
    where
        Self: Sized;
}
pub use crate::common::Module;
