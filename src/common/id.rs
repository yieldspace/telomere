use crate::component_model::types::Type;

pub enum CoreSortId {
    Func(usize),
    Table(usize),
    Memory(usize),
    Global(usize),
    Type(usize),
    Module(usize),
    Instance(usize),
}

pub enum SortId {
    CoreSort(CoreSortId),
    Func(usize),
    #[cfg(feature = "import_export")]
    Value(usize),
    Type(usize),
    Component(usize),
    Instance(usize),
}

pub struct TypeId<'a> {
    pub ty: &'a Type,
    pub value: usize,
}

pub struct FuncId {
    pub value: usize,
}

