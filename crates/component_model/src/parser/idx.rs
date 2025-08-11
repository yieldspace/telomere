use crate::name::{ExportName, ImportName};
use crate::parser::vec::RawIdx;
use crate::Result;
use crate::{ComponentParseError, ComponentParser};
use binary_reader::BinaryReader;
use std::hash::{DefaultHasher, Hash, Hasher};
use telomere_wasm::parser::core::parse_u32;
use crate::vec::Idx;

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
raw_index!(RawCoreTableIdx);
raw_index!(RawCoreGlobalIdx);
raw_index!(RawCoreMemoryIdx);
raw_index!(RawCoreTypeIdx);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawImportId(u64);

impl RawImportId {
    pub fn new(name: &ImportName) -> Self {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        Self(hasher.finish())
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawExportId(u64);

impl RawExportId {
    pub fn new(name: &ExportName) -> Self {
        let mut hasher = DefaultHasher::new();
        name.hash(&mut hasher);
        Self(hasher.finish())
    }
}

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

    /// indexからtype idに変換したものを返す
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
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreMemoryIdx::new(index);
        if self.core_memories.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core memory index: {}",
                index
            )))
        }
    }

    pub fn parse_core_func_idx(&mut self) -> Result<RawCoreFuncIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreFuncIdx::new(index);
        if self.core_funcs.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core func index: {}",
                index
            )))
        }
    }

    pub fn parse_core_table_idx(&mut self) -> Result<RawCoreTableIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreTableIdx::new(index);
        if self.core_tables.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core table index: {}",
                index
            )))
        }
    }

    pub fn parse_core_global_idx(&mut self) -> Result<RawCoreGlobalIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreGlobalIdx::new(index);
        if self.core_globals.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core global index: {}",
                index
            )))
        }
    }

    pub fn parse_core_type_idx(&mut self) -> Result<RawCoreTypeIdx> {
        let (_, index) = parse_u32(self.reader)?;
        let idx = RawCoreTypeIdx::new(index);
        if self.core_types.is_valid(&idx) {
            Ok(idx)
        } else {
            Err(ComponentParseError::IndexError(format!(
                "Invalid core type index: {}",
                index
            )))
        }
    }
}
