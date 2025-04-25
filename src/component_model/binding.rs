use crate::component_model::{ComponentFunction, CoreFunction, CoreGlobalRef, CoreInstance, CoreMemoryRef, CoreModule, CoreTableRef, CoreType, ImportName, InlineComponent, Instance, Type};

#[derive(Debug)]
pub enum Binding<T, R = ()> {
    Real(T),
    Alias(usize),
    Reference(T, R),
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
    Imported(ImportName),
}
pub enum InlineComponentReference {
    Imported(ImportName),
}
pub enum InstanceReference {
    Imported(ImportName),
}

pub type CoreModuleBinding = Binding<CoreModule, CoreModuleReference>;
pub type CoreInstanceBinding = Binding<CoreInstance>;
pub type CoreFunctionBinding = Binding<CoreFunction>;
pub type CoreTypeBinding = Binding<CoreType>;
pub type CoreMemoryBinding = Binding<CoreMemoryRef>;
pub type CoreTableBinding = Binding<CoreTableRef>;
pub type CoreGlobalBinding = Binding<CoreGlobalRef>;
pub type TypeBinding = Binding<Type>;
pub type FunctionBinding = Binding<ComponentFunction>;
pub type ComponentBinding = Binding<InlineComponent, InlineComponentReference>;
pub type InstanceBinding = Binding<Instance, InstanceReference>;
