pub struct ModuleId {
    pub value: usize,
}

pub struct InstanceId {
    pub value: usize,
}

pub enum CoreSortId {
    Func(usize),
    Table(usize),
    Memory(usize),
    Global(usize),
    Type(usize),
    Module(usize),
    Instance(usize),
}

pub struct CoreFuncId {
    pub value: usize,
}

pub struct CoreTableId {
    pub value: usize,
}

pub struct CoreMemoryId {
    pub value: usize,
}

pub struct CoreGlobalId {
    pub value: usize,
}

pub struct CoreModuleId {
    pub value: usize,
}

pub struct CoreInstanceId {
    pub value: usize,
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

pub struct ComponentId {
    pub value: usize,
}

pub struct CoreTypeId {
    pub value: usize,
}

pub struct TypeId(pub i32);

pub struct FuncId {
    pub value: usize,
}
