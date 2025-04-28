use crate::component_model::{
    CoreFunc, CoreGlobalRef, CoreInstance, CoreMemoryRef, CoreModule, CoreTableRef, CoreType, Func,
    ImportName, InlineComponent, Instance, InstanceIdx, Type,
};

#[derive(Debug)]
pub enum Binding<T, R = ()> {
    Real(T),
    Alias(usize),
    Reference(T, R),
}

impl<T, R> From<T> for Binding<T, R> {
    fn from(value: T) -> Self {
        Self::Real(value)
    }
}

impl<T, R> Binding<T, R> {
    pub fn real(value: T) -> Self {
        Binding::Real(value)
    }

    pub fn alias(idx: usize) -> Self {
        Binding::Alias(idx)
    }

    pub fn reference(value: T, reference: R) -> Self {
        Binding::Reference(value, reference)
    }
}

pub enum CoreModuleReference {
    Alias(InstanceIdx, String),
    Imported(ImportName),
}
pub enum FuncReference {
    Alias(InstanceIdx, String),
}
pub enum InlineComponentReference {
    Alias(InstanceIdx, String),
    Imported(ImportName),
}
pub enum InstanceReference {
    Alias(InstanceIdx, String),
    Imported(ImportName),
}

pub type CoreModuleBinding = Binding<CoreModule, CoreModuleReference>;
pub type CoreInstanceBinding = Binding<CoreInstance>;
pub type CoreFunctionBinding = Binding<CoreFunc>;
pub type CoreTypeBinding = Binding<CoreType>;
pub type CoreMemoryBinding = Binding<CoreMemoryRef>;
pub type CoreTableBinding = Binding<CoreTableRef>;
pub type CoreGlobalBinding = Binding<CoreGlobalRef>;
pub type TypeBinding = Binding<Type>;
pub type FunctionBinding = Binding<Func, FuncReference>;
pub type ComponentBinding = Binding<InlineComponent, InlineComponentReference>;
pub type InstanceBinding = Binding<Instance, InstanceReference>;
