use crate::parser::vec::RawIdx;

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

            fn index(&self) -> Result<usize, ()> {
                match self.0 {
                    RawIndex::Index(idx) => Ok(idx as usize),
                    RawIndex::Relative(_, _) => Err(()),
                }
            }
        }
    };
}

raw_index!(RawCoreModuleIdx);
raw_index!(RawComponentIdx);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawImportIdx(pub u32);

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RawExportIdx(pub u32);
