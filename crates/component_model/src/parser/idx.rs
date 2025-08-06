use crate::parser::vec::RawIdx;
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;
use telomere_wasm::parser::core::parse_u32;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RawIndex {
    Index(u32),
    /// outer index
    Relative(u32, u32),
}

macro_rules! raw_index {
    ($name:ident) => {
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
        pub struct $name(RawIndex);

        impl RawIdx for $name {
            fn new(index: u32) -> Self {
                Self(RawIndex::Index(index))
            }

            fn new_outer(outer: u32, index: u32) -> Self {
                Self(RawIndex::Relative(outer, index))
            }

            fn index(&self) -> Result<usize> {
                match self.0 {
                    RawIndex::Index(idx) => Ok(idx as usize),
                    RawIndex::Relative(_, _) => Err(ComponentParseError::IndexError(
                        "this idx cannot get index".into(),
                    )),
                }
            }
        }
    };
}

raw_index!(RawComponentIdx);
raw_index!(RawInstanceIdx);
raw_index!(RawFuncIdx);
raw_index!(RawTypeIdx);
raw_index!(RawCoreModuleIdx);
raw_index!(RawCoreInstanceIdx);
raw_index!(RawCoreFuncIdx);
raw_index!(RawCoreMemoryIdx);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawImportIdx(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawExportIdx(pub u32);

impl<T> ComponentParser<'_, T>
where
    T: BinaryReader,
{
    pub fn parse_component_idx(&mut self) -> Result<RawComponentIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawComponentIdx::new(index);
        if self.components.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid component index: {}",
                index
            )))
        }
    }

    pub fn parse_core_module_idx(&mut self) -> Result<RawCoreModuleIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreModuleIdx::new(index);
        if self.core_modules.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core module index: {}",
                index
            )))
        }
    }

    pub fn parse_instance_idx(&mut self) -> Result<RawInstanceIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawInstanceIdx::new(index);
        if self.instances.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid instance index: {}",
                index
            )))
        }
    }

    pub fn parse_type_idx(&mut self) -> Result<RawTypeIdx> {
        todo!()
    }

    pub fn parse_func_idx(&mut self) -> Result<RawFuncIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawFuncIdx::new(index);
        if self.funcs.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid func index: {}",
                index
            )))
        }
    }

    pub fn parse_core_instance_idx(&mut self) -> Result<RawCoreInstanceIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreInstanceIdx::new(index);
        if self.core_instances.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core instance index: {}",
                index
            )))
        }
    }

    pub fn parse_core_memory_idx(&mut self) -> Result<RawCoreMemoryIdx> {
        todo!()
    }

    pub fn parse_core_func_idx(&mut self) -> Result<RawCoreFuncIdx> {
        todo!()
    }
}
