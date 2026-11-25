use crate::ir::types::ValType;
use crate::ir::{
    CoreInstance, CoreModule, ExportNameString, Func, GlobalIdx, ImportNameString, Instance,
    ResourceId, TypeId, Value,
};
use std::collections::HashMap;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct Component {
    pub(crate) imports: HashMap<ImportNameString, ComponentImport>,
    pub(crate) exports: HashMap<ExportNameString, ComponentExport>,
    /// The core and component instances *defined* in this component
    /// (the instantiate and inline-export forms of both kinds), in
    /// definition order across both kinds. Imported and aliased
    /// instances are not listed: imported instances are supplied by the
    /// linker, aliased instances share the instance they alias, and
    /// aliased core instances are never materialised. Instantiating a
    /// component materialises these eagerly in this order — the runtime
    /// resolves each one, in the environment of each instance of this
    /// component, when that instance is created, so `(with ...)`
    /// instantiation arguments are honoured (#316) and core start
    /// functions observe the definition order of the instances whose
    /// creation runs them (#333), even when none of their exports are
    /// ever referenced.
    pub(crate) defined_instances: Vec<DefinedInstance>,
}

/// A core or component instance defined in a [`Component`]; see
/// [`Component::defined_instances`].
#[derive(Debug, Clone, Copy)]
pub(crate) enum DefinedInstance {
    Core(GlobalIdx<CoreInstance>),
    Component(GlobalIdx<Instance>),
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    Module,
    Component,
    // The declared type is retained so the runtime can map the instance's
    // resource exports back to their `ResourceId`s (needed to route
    // `canon resource.drop` for host-provided resources).
    Instance(TypeId),
    // `engine.rs` identifies callable imports by this variant but does not
    // currently need its type ID.
    #[allow(dead_code)]
    Func(TypeId),
    // Retained conservatively; the current decoder does not construct resource
    // imports.
    #[allow(dead_code)]
    Resource,
    Value(ValType),
}

#[derive(Debug, Clone)]
pub enum ComponentExport {
    Module(GlobalIdx<CoreModule>),
    Component(GlobalIdx<Component>),
    Instance(GlobalIdx<Instance>),
    Func {
        idx: GlobalIdx<Func>,
        type_id: TypeId,
    },
    // `runtime/env.rs` still matches resource exports during lookup.
    #[allow(dead_code)]
    Resource(ResourceId),
    Value {
        idx: GlobalIdx<Value>,
        ty: ValType,
    },
}
