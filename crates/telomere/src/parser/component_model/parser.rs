use crate::binary::BinaryReader;
use crate::component_model::{
    Component, CoreFunc, CoreGlobal, CoreInstance, CoreMemory, CoreModule, CoreRelation, CoreTable,
    Func, GlobalIdx, Instance, Relation,
};
use crate::parser::component_model::{parse_component, ParseResult, ParseState, Validator};
use crate::runtime::component_model::instantiate::{
    InstantiateOp, InstantiateResult, InstantiateScope, InstantiateState,
};
use crate::runtime::component_model::ComponentVMError;
use std::collections::HashMap;
use typed_arena::Arena;

type GlobalMap<T> = HashMap<GlobalIdx<T>, Relation<T>>;
type CoreGlobalMap<T> = HashMap<GlobalIdx<T>, CoreRelation<T>>;

pub struct ParsedComponent {
    pub(crate) ops: Vec<InstantiateOp>,
    pub(crate) components: GlobalMap<Component>,
    pub(crate) instances: GlobalMap<Instance>,
    pub(crate) funcs: GlobalMap<Func>,
    pub(crate) core_modules: CoreGlobalMap<CoreModule>,
    pub(crate) core_instances: CoreGlobalMap<CoreInstance>,
    pub(crate) core_funcs: CoreGlobalMap<CoreFunc>,
    pub(crate) core_memories: CoreGlobalMap<CoreMemory>,
    pub(crate) core_globals: CoreGlobalMap<CoreGlobal>,
    pub(crate) core_tables: CoreGlobalMap<CoreTable>,
    // todo: add types for validate import while instantiate
}

pub struct ComponentParser<'a, R>
where
    R: BinaryReader,
{
    reader: &'a mut R,
}

impl<'a, R> ComponentParser<'a, R>
where
    R: BinaryReader,
{
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }

    pub fn parse(&mut self) -> ParseResult<ParsedComponent> {
        let state_arena = typed_arena::Arena::new();
        let mut state = ParseState::new(&state_arena);
        let arena = typed_arena::Arena::new();
        let mut validator = Validator::new(&arena);
        parse_component(self.reader, &mut state, &mut validator)?;

        Ok(state.into())
    }
}

impl<'a> From<ParseState<'a>> for ParsedComponent {
    fn from(value: ParseState<'a>) -> Self {
        let ops = value.scope().ops.clone();
        let ParseState {
            component_store,
            instance_store,
            func_store,
            core_module_store,
            core_instance_store,
            core_func_store,
            core_memory_store,
            core_global_store,
            core_table_store,
            ..
        } = value;
        Self {
            ops,
            components: component_store.into(),
            instances: instance_store.into(),
            funcs: func_store.into(),
            core_modules: core_module_store.into(),
            core_instances: core_instance_store.into(),
            core_funcs: core_func_store.into(),
            core_memories: core_memory_store.into(),
            core_globals: core_global_store.into(),
            core_tables: core_table_store.into(),
        }
    }
}

impl ParsedComponent {
    pub fn resolve_core_module(
        &self,
        idx: GlobalIdx<CoreModule>,
        scope: &InstantiateScope,
        state: &InstantiateState,
    ) -> InstantiateResult<&CoreModule> {
        self.core_modules
            .get(&idx)
            .ok_or_else(|| {
                ComponentVMError::LinkError(format!("Core module with index {:?} not found", idx))
            })
            .and_then(|relation| match relation {
                CoreRelation::Defined(module) => Ok(module),
                CoreRelation::ImportModule(name) => {
                    let module_idx = scope.get_core_module(name)?;
                    self.resolve_core_module(module_idx, scope, state)
                }
                CoreRelation::FromExport(idx, name) => {
                    todo!()
                }
                CoreRelation::FromCoreExport(_, _) => Err(ComponentVMError::LinkError(format!(
                    "Core module with index {:?} is not defined",
                    idx
                ))),
            })
    }

    pub fn resolve_core_instance(
        &self,
        idx: GlobalIdx<CoreInstance>,
    ) -> InstantiateResult<&CoreInstance> {
        self.core_instances
            .get(&idx)
            .ok_or_else(|| {
                ComponentVMError::LinkError(format!("Core instance with index {:?} not found", idx))
            })
            .and_then(|relation| match relation {
                CoreRelation::Defined(instance) => Ok(instance),
                CoreRelation::ImportModule(_) => {
                    panic!("Core instance cannot be imported as a module")
                }
                CoreRelation::FromExport(_, _) => {
                    panic!("Core instance cannot be imported as a module")
                }
                CoreRelation::FromCoreExport(_, _) => Err(ComponentVMError::LinkError(format!(
                    "Core instance with index {:?} is not defined",
                    idx
                ))),
            })
    }

    pub fn resolve_component(
        &self,
        idx: GlobalIdx<Component>,
        scope: &InstantiateScope,
        state: &InstantiateState,
    ) -> InstantiateResult<&Component> {
        self.components
            .get(&idx)
            .ok_or_else(|| {
                ComponentVMError::LinkError(format!("Component with index {:?} not found", idx))
            })
            .and_then(|relation| match relation {
                Relation::Defined(component) => Ok(component),
                Relation::Import(name) => {
                    let component_idx = scope.get_component(name)?;
                    self.resolve_component(component_idx, scope, state)
                }
                Relation::FromExport(_, _) => Err(ComponentVMError::LinkError(format!(
                    "Component with index {:?} is not defined",
                    idx
                ))),
            })
    }

    pub fn resolve_instance(
        &self,
        idx: GlobalIdx<Instance>,
        scope: &InstantiateScope,
        state: &InstantiateState,
    ) -> InstantiateResult<&Instance> {
        self.instances
            .get(&idx)
            .ok_or_else(|| {
                ComponentVMError::LinkError(format!("Instance with index {:?} not found", idx))
            })
            .and_then(|relation| match relation {
                Relation::Defined(instance) => Ok(instance),
                Relation::Import(name) => {
                    let instance_idx = scope.get_instance(name)?;
                    self.resolve_instance(instance_idx, scope, state)
                }
                Relation::FromExport(_, _) => Err(ComponentVMError::LinkError(format!(
                    "Instance with index {:?} is not defined",
                    idx
                ))),
            })
    }
}
