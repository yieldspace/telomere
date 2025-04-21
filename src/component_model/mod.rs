mod canon;
mod component;
mod core;
mod func;
mod idx;
mod instance;
mod sort;
mod types;

pub use canon::*;
pub use component::*;
pub use core::*;
pub use func::*;
pub use idx::*;
pub use instance::*;
pub use sort::*;
pub use types::*;

#[derive(Clone)]
pub enum Slot<T, I: Idx> {
    Value(T),
    Idx(I),
}

impl<T, I: Idx> From<Slot<T, I>> for Binding<T> {
    fn from(value: Slot<T, I>) -> Self {
        match value {
            Slot::Value(data) => Binding::Real(data),
            Slot::Idx(idx) => Binding::Alias(idx.global()),
        }
    }
}

pub type ExportName = String;
pub type ImportName = String;

#[derive(Debug, Clone)]
pub enum Reference {
    Instance(InstanceIdx, ExportName),
    Component(ComponentIdx, ExportName),
    Imported(ImportName),
    Exported(ExportName),
}

#[derive(Debug)]
pub enum Binding<T> {
    Real(T),
    Alias(usize),
}

pub struct FlattenComponent {
    pub core_modules: Vec<Binding<CoreModule>>,
    pub core_instances: Vec<Binding<CoreInstance>>,
    pub core_functions: Vec<Binding<CoreFunction>>,
    pub functions: Vec<Binding<ComponentFunction>>,
    pub components: Vec<Binding<InlineComponent>>,
    pub instances: Vec<Binding<Instance>>,
    pub core_types: Vec<Binding<CoreType>>,
    pub core_memories: Vec<Binding<CoreMemoryRef>>,
    pub core_tables: Vec<Binding<CoreTableRef>>,
    pub core_globals: Vec<Binding<CoreGlobalRef>>,
    pub types: Vec<Binding<Type>>,
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    pub values: Vec<Binding<ValueBound>>,
}

impl Default for FlattenComponent {
    fn default() -> Self {
        Self::new()
    }
}

impl FlattenComponent {
    pub fn new() -> Self {
        FlattenComponent {
            core_modules: vec![],
            core_instances: vec![],
            core_functions: vec![],
            functions: vec![],
            components: vec![],
            instances: vec![],
            core_types: vec![],
            core_memories: vec![],
            core_tables: vec![],
            core_globals: vec![],
            types: vec![],
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            values: vec![],
        }
    }

    pub(crate) fn get_core_module(&self, idx: usize) -> &CoreModule {
        match self
            .core_modules
            .get(idx)
            .expect("Core Module is not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_module(*idx),
        }
    }

    pub(crate) fn get_core_instance(&self, idx: usize) -> &CoreInstance {
        match self
            .core_instances
            .get(idx)
            .expect("Core Instance is not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_instance(*idx),
        }
    }

    pub fn get_core_function(&self, idx: usize) -> &CoreFunction {
        match self
            .core_functions
            .get(idx)
            .expect("Core function not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_function(*idx),
        }
    }

    pub fn get_function(&self, idx: usize) -> &ComponentFunction {
        match self
            .functions
            .get(idx)
            .expect("Component function not found")
        {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_function(*idx),
        }
    }

    pub fn get_type(&self, idx: usize) -> &Type {
        match self.types.get(idx).expect("Type not found") {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_type(*idx),
        }
    }

    pub fn get_instance(&self, idx: usize) -> &Instance {
        match self.instances.get(idx).expect("Instance not found") {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_instance(*idx),
        }
    }

    pub fn get_component(&self, idx: usize) -> &InlineComponent {
        match self.components.get(idx).expect("Component not found") {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_component(*idx),
        }
    }

    pub fn get_core_type(&self, idx: usize) -> &CoreType {
        match self.core_types.get(idx).expect("Core Type not found") {
            Binding::Real(real) => real,
            Binding::Alias(idx) => self.get_core_type(*idx),
        }
    }
}

#[derive(Debug)]
pub struct InlineExport {
    pub name: String,
    pub sort: SortWithIdx,
}
