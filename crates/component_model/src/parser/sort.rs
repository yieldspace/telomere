use crate::Result;
use crate::parser::idx::{
    RawComponentIdx, RawCoreFuncIdx, RawCoreGlobalIdx, RawCoreInstanceIdx, RawCoreMemoryIdx,
    RawCoreModuleIdx, RawCoreTableIdx, RawCoreTypeIdx, RawFuncIdx, RawInstanceIdx,
};
use crate::types::{TypeId, TypeIdx};
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CoreSort {
    Func(RawCoreFuncIdx),
    Table(RawCoreTableIdx),
    Memory(RawCoreMemoryIdx),
    Global(RawCoreGlobalIdx),
    Type(RawCoreTypeIdx),
    Module(RawCoreModuleIdx),
    Instance(RawCoreInstanceIdx),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Sort {
    Core(CoreSort),
    Func(RawFuncIdx),
    #[cfg(feature = "value-imports-exports")]
    Value(u32),
    Type(TypeIdx),
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

impl CoreSort {
    pub fn get_type(&self) -> CoreSortType {
        match self {
            CoreSort::Func(_) => CoreSortType::Func,
            CoreSort::Table(_) => CoreSortType::Table,
            CoreSort::Memory(_) => CoreSortType::Memory,
            CoreSort::Global(_) => CoreSortType::Global,
            CoreSort::Type(_) => CoreSortType::Type,
            CoreSort::Module(_) => CoreSortType::Module,
            CoreSort::Instance(_) => CoreSortType::Instance,
        }
    }
}

impl Sort {
    pub fn get_type(&self) -> SortType {
        match self {
            Sort::Core(sort) => SortType::Core(sort.get_type()),
            Sort::Func(_) => SortType::Func,
            #[cfg(feature = "value-imports-exports")]
            Sort::Value(_) => SortType::Value,
            Sort::Type(_) => SortType::Type,
            Sort::Component(_) => SortType::Component,
            Sort::Instance(_) => SortType::Instance,
        }
    }
}

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_core_sort(&mut self) -> Result<CoreSort> {
        let sort = match self.reader.read_exact_one()? {
            0x00 => CoreSort::Func(self.parse_core_func_idx()?),
            0x01 => CoreSort::Table(self.parse_core_table_idx()?),
            0x02 => CoreSort::Memory(self.parse_core_memory_idx()?),
            0x03 => CoreSort::Global(self.parse_core_global_idx()?),
            0x04 => CoreSort::Type(self.parse_core_type_idx()?),
            0x05 => CoreSort::Module(self.parse_core_module_idx()?),
            0x06 => CoreSort::Instance(self.parse_core_instance_idx()?),
            x => return Err(ComponentParseError::InvalidCoreSortType(x)),
        };
        Ok(sort)
    }

    pub fn parse_sort(&mut self) -> Result<Sort> {
        let sort = match self.reader.read_exact_one()? {
            0x00 => Sort::Core(self.parse_core_sort()?),
            0x01 => Sort::Func(self.parse_func_idx()?),
            #[cfg(feature = "value-imports-exports")]
            0x02 => todo!("value"),
            0x03 => Sort::Type(self.parse_type_idx()?),
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

    pub fn get_type_id_from_sort(&self, sort: &Sort) -> Result<Option<TypeId>> {
        match sort {
            Sort::Core(_) => Ok(None),
            Sort::Func(idx) => {
                let func_type = self.validator.locals.get_func_type(idx)?;
                Ok(Some(TypeId::Func(func_type)))
            }
            Sort::Type(idx) => {
                let type_id = self.validator.locals.get_type(idx)?;
                Ok(Some(*type_id))
            }
            Sort::Component(idx) => {
                let component_type = self.validator.locals.get_component_type(idx)?;
                Ok(Some(TypeId::Component(component_type)))
            }
            Sort::Instance(idx) => {
                let instance_type = self.validator.locals.get_instance_type(idx)?;
                Ok(Some(TypeId::Instance(instance_type)))
            }
        }
    }
}
