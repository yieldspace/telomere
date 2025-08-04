use crate::parser::idx::{RawComponentIdx, RawCoreInstanceIdx, RawCoreModuleIdx, RawInstanceIdx};
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;

pub enum CoreSort {
    Func(u32),
    Table(u32),
    Memory(u32),
    Global(u32),
    Type(u32),
    Module(RawCoreModuleIdx),
    Instance(RawCoreInstanceIdx),
}

pub enum Sort {
    Core(CoreSort),
    Func(u32),
    #[cfg(feature = "value-imports-exports")]
    Value(u32),
    Type(u32),
    Component(RawComponentIdx),
    Instance(RawInstanceIdx),
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_sort(&mut self) -> Result<Sort> {
        let sort = match self.reader.read_exact_one()? {
            0x00 => todo!("core"),
            0x01 => todo!("func"),
            0x02 => todo!("value"),
            0x03 => todo!("type"),
            0x04 => Sort::Component(self.parse_component_idx()?),
            0x05 => Sort::Instance(self.parse_instance_idx()?),
            x => return Err(ComponentParseError::InvalidSortType(x)),
        };
        Ok(sort)
    }
}
