use crate::binary::BinaryReader;
use crate::parser::error::ParseError;

pub type ParseResult<T> = Result<T, ParseError>;

pub(crate) mod core;
mod error;

pub trait BinaryParser<R>
where
    R: BinaryReader,
{
    fn parse(binary: &mut R) -> ParseResult<Self>
    where
        Self: Sized;
}
pub use crate::common::Module;
