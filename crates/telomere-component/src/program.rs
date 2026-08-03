use crate::ir::types::{CoreType, Type};
use crate::ir::{
    Component, ComponentExport, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule,
    CoreRelation, CoreTable, Func, GlobalIdx, Instance, Relation, TypeId,
};
use std::collections::HashMap;

/// Canonical ABI layout information for one component type.
///
/// The engine computes this metadata during compilation so lifting and lowering
/// values do not have to repeatedly walk the type graph at call time.
#[derive(Clone, Debug)]
pub struct ComponentTypeInfo {
    /// The dense component type index this metadata describes.
    pub id: u32,
    /// Number of core values required by the flattened canonical ABI form.
    pub flat_len: usize,
    /// Byte size of the indirect canonical ABI representation.
    pub indirect_size: u32,
    /// Required byte alignment of the indirect representation.
    pub indirect_align: u32,
    /// Fixed list or flags length when the type encodes one.
    pub fixed_length: Option<u32>,
}

/// Placeholder entries produced for root exports while compiling a component.
///
/// This is not a complete decoded operation stream. The engine currently emits
/// `CanonLift { func_idx: 0 }` once per function export, where `0` is a
/// placeholder rather than a local function index, and an [`Self::Export`] for
/// each non-function export. It does not currently construct [`Self::Instantiate`],
/// [`Self::Alias`], or [`Self::CanonLower`] entries.
#[derive(Clone, Debug)]
pub enum ComponentOp {
    /// Instantiates the component at the given local component index.
    Instantiate {
        /// The local component index to instantiate.
        component_idx: u32,
    },
    /// Creates an alias from one local index to another.
    Alias {
        /// The source local index.
        source_idx: u32,
        /// The target local index.
        target_idx: u32,
    },
    /// Lowers a component function for a core module call.
    CanonLower {
        /// The local component function index.
        func_idx: u32,
    },
    /// Lifts a core function for a component export call.
    CanonLift {
        /// The local core function index.
        func_idx: u32,
    },
    /// Records a non-function export by name.
    Export {
        /// The export name.
        name: String,
    },
}

/// A validated, reusable representation of a Component Model binary.
///
/// Programs are returned by ComponentEngine::compile and retain both the source
/// bytes and the decoded stores needed for instantiation. The fields are public
/// to support inspection tools; typical embedders use the lookup helpers and
/// ComponentEngine::instantiate instead.
#[derive(Clone, Debug)]
pub struct ComponentProgram {
    /// Canonical ABI metadata indexed by component type index.
    pub type_infos: Vec<ComponentTypeInfo>,
    /// Names of all root imports in the compiled component.
    pub imports: Vec<String>,
    /// Root imports that are callable functions.
    pub callable_imports: Vec<String>,
    /// Names of all root exports in the compiled component.
    pub exports: Vec<String>,
    /// Root exports that are callable functions.
    pub callable_exports: Vec<String>,
    /// An incomplete placeholder summary of root exports; see [`ComponentOp`].
    pub ops: Vec<ComponentOp>,
    /// The original Component Model binary bytes.
    pub bytes: Vec<u8>,
    /// The decoded root component declaration.
    pub root: Component,
    /// Dense component type storage indexed by TypeId.
    pub types: Box<[Type]>,
    /// Decoded component declarations and their source relations.
    pub component_store: HashMap<GlobalIdx<Component>, Relation<Component>>,
    /// Decoded component-instance declarations and relations.
    pub instance_store: HashMap<GlobalIdx<Instance>, Relation<Instance>>,
    /// Decoded component-function declarations and relations.
    pub func_store: HashMap<GlobalIdx<Func>, Relation<Func>>,
    /// Decoded core-module declarations and relations.
    pub core_module_store: HashMap<GlobalIdx<CoreModule>, CoreRelation<CoreModule>>,
    /// Decoded core-type declarations and relations.
    pub core_type_store: HashMap<GlobalIdx<CoreType>, CoreRelation<CoreType>>,
    /// Decoded core-instance declarations and relations.
    pub core_instance_store: HashMap<GlobalIdx<CoreInstance>, CoreRelation<CoreInstance>>,
    /// Decoded core-function declarations and relations.
    pub core_func_store: HashMap<GlobalIdx<CoreFunc>, CoreRelation<CoreFunc>>,
    /// Decoded core-memory declarations and relations.
    pub core_memory_store: HashMap<GlobalIdx<CoreMemory>, CoreRelation<CoreMemory>>,
    /// Decoded core-global declarations and relations.
    pub core_global_store: HashMap<GlobalIdx<CoreGlobal>, CoreRelation<CoreGlobal>>,
    /// Decoded core-table declarations and relations.
    pub core_table_store: HashMap<GlobalIdx<CoreTable>, CoreRelation<CoreTable>>,
}

impl ComponentProgram {
    /// Returns the decoded type at an index, if it is in this program's type store.
    pub fn get_type(&self, id: TypeId) -> Option<&Type> {
        self.types.get(id.index() as usize)
    }

    /// Returns canonical ABI metadata for the type at an index, if available.
    pub fn get_type_info(&self, id: TypeId) -> Option<&ComponentTypeInfo> {
        self.type_infos.get(id.index() as usize)
    }

    /// Returns the declared type index of a root function export by name.
    ///
    /// Non-function exports and unknown names return None.
    pub fn get_root_func_type_id(&self, name: &str) -> Option<TypeId> {
        match self.root.exports.get(name) {
            Some(ComponentExport::Func { type_id, .. }) => Some(*type_id),
            _ => None,
        }
    }

    /// Returns the root function's local index and declared type index by name.
    ///
    /// Non-function exports and unknown names return None.
    pub fn get_root_func(&self, name: &str) -> Option<(GlobalIdx<Func>, TypeId)> {
        match self.root.exports.get(name) {
            Some(ComponentExport::Func { idx, type_id }) => Some((*idx, *type_id)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{Component, ResourceId};

    #[test]
    fn get_type_reads_dense_type_storage_by_index() {
        let first = Type::Resource(ResourceId::synthetic());
        let second = Type::Resource(ResourceId::synthetic());
        let program = ComponentProgram {
            type_infos: vec![
                ComponentTypeInfo {
                    id: 0,
                    flat_len: 0,
                    indirect_size: 0,
                    indirect_align: 1,
                    fixed_length: None,
                },
                ComponentTypeInfo {
                    id: 1,
                    flat_len: 0,
                    indirect_size: 0,
                    indirect_align: 1,
                    fixed_length: None,
                },
            ],
            imports: Vec::new(),
            callable_imports: Vec::new(),
            exports: Vec::new(),
            callable_exports: Vec::new(),
            ops: Vec::new(),
            bytes: Vec::new(),
            root: Component {
                imports: HashMap::new(),
                exports: HashMap::new(),
            },
            types: vec![first.clone(), second.clone()].into_boxed_slice(),
            component_store: HashMap::new(),
            instance_store: HashMap::new(),
            func_store: HashMap::new(),
            core_module_store: HashMap::new(),
            core_type_store: HashMap::new(),
            core_instance_store: HashMap::new(),
            core_func_store: HashMap::new(),
            core_memory_store: HashMap::new(),
            core_global_store: HashMap::new(),
            core_table_store: HashMap::new(),
        };

        assert!(matches!(
            program.get_type(TypeId::from_index(0)),
            Some(Type::Resource(_))
        ));
        assert_eq!(program.get_type_info(TypeId::from_index(0)).unwrap().id, 0);
        assert!(matches!(
            program.get_type(TypeId::from_index(1)),
            Some(Type::Resource(_))
        ));
        assert!(program.get_type(TypeId::from_index(2)).is_none());
    }
}
