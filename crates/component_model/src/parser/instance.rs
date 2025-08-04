use crate::parser::ComponentParser;
use crate::Result;
use binary_reader::BinaryReader;

impl<'a, T> ComponentParser<'a, T>
where
    T: BinaryReader,
{
    pub(crate) fn parse_instance(&mut self) -> Result<()> {
        Ok(())
    }
}
