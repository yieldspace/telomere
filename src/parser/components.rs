use crate::binary::BinaryReader;
use crate::parser::{BinaryParser, ParseResult};
use crate::parser::constant::{LAYER, MAGIC, VERSION};
use crate::parser::error::ParseError;

pub struct Components {

}

impl<R: BinaryReader> BinaryParser<R> for Components {
    fn parse(binary: &mut R) -> ParseResult<Self> {
        let magic = binary.read_exact::<4>()?;
        if magic != MAGIC {
            return Err(ParseError::InvalidSignature(format!("Magic is not correct: {:?} != {:?}", magic, MAGIC)));
        }
        let version = binary.read_exact::<2>()?;
        if version != VERSION {
            return Err(ParseError::InvalidSignature(format!("Version is not correct: {:?} != {:?}", version, VERSION)));
        }
        let layer = binary.read_exact::<2>()?;
        if layer != LAYER {
            return Err(ParseError::InvalidSignature(format!("Layer is not correct: {:?} != {:?}", layer, LAYER)));
        }
        // todo: parse sections
        Ok(Self {
        })
    }
}