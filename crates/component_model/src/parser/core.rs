use crate::parser::ComponentParser;
use crate::{ComponentParseError, Result};
use binary_reader::BinaryReader;

impl<T> ComponentParser<'_, '_, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_core_instance(&mut self) -> Result<()> {
        match self.reader.read_exact_one()? {
            0x00 => self.parse_core_instance_with_arg()?,
            0x01 => self.parse_core_instance_inline_export()?,
            x => return Err(ComponentParseError::InvalidCoreInstanceType(x)),
        }
        Ok(())
    }

    fn parse_core_instance_with_arg(&mut self) -> Result<()> {
        Ok(())
    }

    fn parse_core_instance_inline_export(&mut self) -> Result<()> {
        Ok(())
    }
}
