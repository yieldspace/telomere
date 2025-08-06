use crate::parser::idx::{RawComponentIdx, RawCoreInstanceIdx, RawCoreModuleIdx, RawInstanceIdx};
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CoreSort {
    Func(u32),
    Table(u32),
    Memory(u32),
    Global(u32),
    Type(u32),
    Module(RawCoreModuleIdx),
    Instance(RawCoreInstanceIdx),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    Core(CoreSort),
    Func(u32),
    #[cfg(feature = "value-imports-exports")]
    Value(u32),
    Type(u32),
    Component(RawComponentIdx),
    Instance(RawInstanceIdx),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CoreSortType {
    Func,
    Table,
    Memory,
    Global,
    Type,
    Module,
    Instance,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum SortType {
    Core(CoreSortType) = 0,
    Func = 1,
    #[cfg(feature = "value-imports-exports")]
    Value = 2,
    Type = 3,
    Component = 4,
    Instance = 5,
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

    pub fn parse_core_sort_type(&mut self) -> Result<CoreSortType> {
        let sort_type = match self.reader.read_exact_one()? {
            0x00 => CoreSortType::Func,
            0x01 => CoreSortType::Table,
            0x02 => CoreSortType::Memory,
            0x03 => CoreSortType::Global,
            0x04 => CoreSortType::Type,
            0x05 => CoreSortType::Module,
            0x06 => CoreSortType::Instance,
            x => return Err(ComponentParseError::InvalidCoreSortType(x)),
        };
        Ok(sort_type)
    }

    pub fn parse_sort_type(&mut self) -> Result<SortType> {
        let sort = match self.reader.read_exact_one()? {
            0x00 => SortType::Core(self.parse_core_sort_type()?),
            0x01 => SortType::Func,
            #[cfg(feature = "value-imports-exports")]
            0x02 => SortType::Value,
            0x03 => SortType::Type,
            0x04 => SortType::Component,
            0x05 => SortType::Instance,
            x => return Err(ComponentParseError::InvalidSortType(x)),
        };
        Ok(sort)
    }
}
