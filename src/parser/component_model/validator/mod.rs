mod idx;
mod state;
mod store;

use crate::component_model::{
    Binding, ComponentBinding, ComponentExport, ComponentFunction, ComponentIdx, ComponentImport,
    ComponentType, CoreFuncIdx, CoreFunction, CoreFunctionBinding, CoreGlobalBinding,
    CoreGlobalIdx, CoreGlobalRef, CoreInstance, CoreInstanceBinding, CoreInstanceIdx,
    CoreMemoryBinding, CoreMemoryIdx, CoreMemoryRef, CoreModule, CoreModuleBinding, CoreModuleIdx,
    CoreTableBinding, CoreTableIdx, CoreTableRef, CoreType, CoreTypeBinding, CoreTypeIdx,
    FlattenComponent, FuncIdx, Idx, InlineComponent, Instance, InstanceBinding, InstanceIdx,
    Resolvable, Resolver, Type, TypeBinding, TypeIdx,
};
#[cfg(feature = "component-gated-feature-value-imports-exports")]
use crate::component_model::{ValueBound, ValueIdx};
use crate::parser::component_model::error::ComponentParseError;
pub use crate::parser::component_model::validator::idx::IdxValidator;
use either::Either;
pub use state::*;
use std::collections::HashMap;
pub use store::LocalStore;

pub struct Validator<T> {
    pub(crate) state: T,
}

impl<T> Validator<T> {
    pub fn new(state: T) -> Self {
        Validator { state }
    }

    pub fn resolve_idx<V>(
        &self,
        idx: &(impl Idx + Resolvable<V>),
    ) -> Result<&V, ComponentParseError>
    where
        T: Resolver<V, Error = ComponentParseError>,
    {
        self.state.resolve(idx)
    }

    pub fn validate_local_idx<I>(&self, local: u32) -> Result<I, ComponentParseError>
    where
        T: IdxValidator<I>,
        I: Idx,
    {
        self.state.validate_local_idx(local)
    }

    pub fn validate_idx_resolved<V, I>(&self, local_idx: u32) -> Result<V, ComponentParseError>
    where
        I: Resolvable<V>,
        T: Resolver<V, Error = ComponentParseError> + IdxValidator<I, Resolved = V> + Sized,
        V: Clone,
    {
        self.state.validate_idx_resolved(local_idx)
    }

    pub fn validate_outer_idx<I>(&self, ct: u32, idx: u32) -> Result<I, ComponentParseError>
    where
        T: IdxValidator<I>,
        I: Idx,
    {
        self.state.validate_outer_idx(ct, idx)
    }

    pub fn validate_outer_idx_resolved<V, I>(
        &self,
        ct: u32,
        idx: u32,
    ) -> Result<V, ComponentParseError>
    where
        I: Resolvable<V>,
        T: Resolver<V, Error = ComponentParseError> + IdxValidator<I, Resolved = V> + Sized,
        V: Clone,
    {
        self.state.validate_outer_idx_resolved(ct, idx)
    }
}
