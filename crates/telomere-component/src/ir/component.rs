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
    /// Global indices of the core instances *defined* in this component
    /// (both the instantiate and inline-export forms), in definition order.
    /// Aliased core instances are not listed: they are never materialised.
    /// The runtime materialises these in the environment of each instance
    /// of this component, so that `(with ...)` instantiation arguments are
    /// honoured when the core instances are created (#316).
    pub(crate) core_instances: Vec<GlobalIdx<CoreInstance>>,
    /// Global indices of the component instances *defined* in this
    /// component (the `(instance (instantiate ...))` form), in definition
    /// order. Imported and aliased instances are not listed: imported
    /// instances are supplied by the linker, and aliased instances share
    /// the instance they alias. Instantiating a component instantiates
    /// these eagerly — the runtime resolves each one when an instance of
    /// this component is created, so the core instances they define are
    /// materialised (running any core start functions) even when none of
    /// their exports are ever referenced.
    pub(crate) instances: Vec<GlobalIdx<Instance>>,
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
