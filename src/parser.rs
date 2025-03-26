use crate::binary::BinaryReader;
use crate::parser::error::ParseError;

pub type ParseResult<T> = Result<T, ParseError>;

pub(crate) mod core;
pub(crate) mod component;
mod error;
mod leb128;

pub trait BinaryParser<R>
where
    R: BinaryReader,
{
    fn parse(binary: &mut R) -> ParseResult<Self>
    where
        Self: Sized;
}
pub use crate::common::Module;
