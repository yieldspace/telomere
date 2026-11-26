use crate::decoder::{ComponentParseError, ParseResult};
use crate::ir::types::CoreSortType;
use crate::support::common::{
    module_exports, module_function_types, module_functions, module_globals, module_imports,
    module_memories, module_tables, ExportDesc, ImportDesc,
};
pub use crate::support::common::{
    FuncType as CoreFuncType, GlobalType as CoreGlobalType, MemType as CoreMemoryType,
    TableType as CoreTableType,
};
use crate::support::Module;
use std::collections::HashMap;
use std::ptr;
use std::sync::Arc;

/// The canonical identity of a declared core func type: its
/// signature, its finality, the identity of its declared
/// supertype, if any, and its recursion-group context.
///
/// Core WebAssembly type matching is defined over canonical types:
/// two defined types are the same type when their canonical forms
/// agree, and a defined type is a subtype of exactly the types on
/// its declared supertype chain. A bare comptype — like a `sub
/// final` with no declared supertypes — is final and canonically
/// identified by its signature alone; that is the identity produced
/// by every signature-only construction site (a core module's own
/// type space, the core func index space). A declared subtype keeps
/// its ancestry here so that module import/export matching can
/// apply the chain rule instead of comparing signatures alone: a
/// plain `(func)` is not a subtype of a declared `(sub (func))`,
/// even though the signatures agree.
///
/// Canonicalisation treats a recursion group as a unit: a type
/// defined inside an explicit `rec` group is identified by the
/// group's whole canonical form plus its own position in it, so a
/// member of a multi-member group is a different type from a
/// standalone definition of the same shape. `rec_group` carries
/// that context; it is `None` for a standalone definition, and a
/// group of one member is a standalone definition (canonicalisation
/// does not distinguish the two), so singleton groups are
/// normalised to `None` as well.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreFuncIdentity {
    pub signature: CoreFuncType,
    pub is_final: bool,
    pub supertype: Option<Arc<CoreFuncIdentity>>,
    /// The canonical form of the explicit recursion group this
    /// type was defined in, when the group has more than one
    /// member; `None` for a standalone definition.
    pub rec_group: Option<Arc<CoreRecGroup>>,
    /// This type's position within `rec_group` (0 when there is
    /// no group context).
    pub rec_index: u32,
}

/// The canonical form of one explicit recursion group: the
/// structural description of every member, in declaration order.
/// A member's declared supertype is expressed relative to the
/// group when it names another member, so the form is built
/// without referring to the members' own identities (which carry
/// the group itself).
#[derive(Debug, Clone, PartialEq)]
pub struct CoreRecGroup {
    pub members: Vec<CoreRecMember>,
}

/// One member of a `CoreRecGroup`, described structurally.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreRecMember {
    pub signature: CoreFuncType,
    pub is_final: bool,
    pub supertype: Option<CoreRecSuper>,
}

/// A declared supertype as recorded in a `CoreRecMember`.
#[derive(Debug, Clone, PartialEq)]
pub enum CoreRecSuper {
    /// The supertype is an earlier member of the same group, by
    /// position.
    Member(u32),
    /// The supertype is a type defined outside the group; its
    /// canonical identity is carried in full.
    External(Arc<CoreFuncIdentity>),
}

impl CoreFuncIdentity {
    /// The identity of a bare (final, supertype-less) func type
    /// with the given signature.
    pub fn bare(signature: CoreFuncType) -> Arc<Self> {
        Arc::new(Self {
            signature,
            is_final: true,
            supertype: None,
            rec_group: None,
            rec_index: 0,
        })
    }

    /// Whether this type is a subtype of `sup`: `sup` occurs on
    /// the declared supertype chain of `self`, counting `self`
    /// itself. The signature is part of the identity, so a chain
    /// hit also establishes that the signatures agree at the
    /// matched type; the rectype decoder only lets a subtype be
    /// declared with a signature matching its supertype's, so every
    /// type on a chain shares the chain head's signature.
    pub fn is_subtype_of(&self, sup: &Self) -> bool {
        let mut cache = IdentityEqCache::new();
        let mut current = self;
        loop {
            if current.eq_memoized(sup, &mut cache) {
                return true;
            }
            match &current.supertype {
                Some(supertype) => current = supertype,
                None => return false,
            }
        }
    }

    /// Structural equality with `other`, memoised over node pairs.
    ///
    /// The plain derived comparison re-traverses a shared subgraph
    /// once per path that reaches it: an identity carries its
    /// external ancestors both on its own `supertype` chain and in
    /// every `rec_group` member's `CoreRecSuper::External`, so a
    /// tower of such declarations compares in time exponential in
    /// its height even when the two sides are identical. The cache
    /// keys each decided pair by the two nodes' addresses (both
    /// graphs outlive the comparison, so an address names one node
    /// for the whole session) and stores only completed results --
    /// identities are immutable acyclic `Arc` graphs, so a pair's
    /// result cannot change once computed. The result is exactly
    /// the derived `PartialEq` result; only the cost changes.
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        if ptr::eq(self, other) {
            return true;
        }
        let key = (ptr::from_ref(self) as usize, ptr::from_ref(other) as usize);
        if let Some(&result) = cache.identities.get(&key) {
            return result;
        }
        let result = self.signature == other.signature
            && self.is_final == other.is_final
            && self.rec_index == other.rec_index
            && opt_identity_eq(&self.supertype, &other.supertype, cache)
            && opt_rec_group_eq(&self.rec_group, &other.rec_group, cache);
        cache.identities.insert(key, result);
        result
    }
}

/// The memoisation tables for one identity comparison session; see
/// `CoreFuncIdentity::eq_memoized`. Identity pairs and rec-group
/// pairs are keyed in separate maps because they are different
/// node kinds.
#[derive(Default)]
pub(crate) struct IdentityEqCache {
    identities: HashMap<(usize, usize), bool>,
    rec_groups: HashMap<(usize, usize), bool>,
}

impl IdentityEqCache {
    pub(crate) fn new() -> Self {
        Self::default()
    }
}

fn opt_identity_eq(
    lhs: &Option<Arc<CoreFuncIdentity>>,
    rhs: &Option<Arc<CoreFuncIdentity>>,
    cache: &mut IdentityEqCache,
) -> bool {
    match (lhs, rhs) {
        (None, None) => true,
        (Some(lhs), Some(rhs)) => lhs.eq_memoized(rhs, cache),
        _ => false,
    }
}

fn opt_rec_group_eq(
    lhs: &Option<Arc<CoreRecGroup>>,
    rhs: &Option<Arc<CoreRecGroup>>,
    cache: &mut IdentityEqCache,
) -> bool {
    match (lhs, rhs) {
        (None, None) => true,
        (Some(lhs), Some(rhs)) => rec_group_eq_memoized(lhs, rhs, cache),
        _ => false,
    }
}

fn rec_group_eq_memoized(
    lhs: &CoreRecGroup,
    rhs: &CoreRecGroup,
    cache: &mut IdentityEqCache,
) -> bool {
    if ptr::eq(lhs, rhs) {
        return true;
    }
    let key = (ptr::from_ref(lhs) as usize, ptr::from_ref(rhs) as usize);
    if let Some(&result) = cache.rec_groups.get(&key) {
        return result;
    }
    let result = lhs.members.len() == rhs.members.len()
        && lhs
            .members
            .iter()
            .zip(rhs.members.iter())
            .all(|(lhs, rhs)| rec_member_eq(lhs, rhs, cache));
    cache.rec_groups.insert(key, result);
    result
}

fn rec_member_eq(
    lhs: &CoreRecMember,
    rhs: &CoreRecMember,
    cache: &mut IdentityEqCache,
) -> bool {
    lhs.signature == rhs.signature
        && lhs.is_final == rhs.is_final
        && match (&lhs.supertype, &rhs.supertype) {
            (None, None) => true,
            (Some(CoreRecSuper::Member(lhs)), Some(CoreRecSuper::Member(rhs))) => lhs == rhs,
            (Some(CoreRecSuper::External(lhs)), Some(CoreRecSuper::External(rhs))) => {
                lhs.eq_memoized(rhs, cache)
            }
            _ => false,
        }
}

/// A func type as recorded in a core module import/export
/// descriptor: the signature, plus the canonical identity of the
/// declared type it came from, which matching needs in addition to
/// the signature (see `CoreFuncIdentity`).
#[derive(Debug, Clone, PartialEq)]
pub struct CoreModuleFuncType {
    pub signature: CoreFuncType,
    pub identity: Arc<CoreFuncIdentity>,
}

impl CoreModuleFuncType {
    /// A descriptor for a bare func type (see
    /// `CoreFuncIdentity::bare`).
    pub fn bare(signature: CoreFuncType) -> Self {
        Self {
            identity: CoreFuncIdentity::bare(signature.clone()),
            signature,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleExportType {
    Memory(CoreMemoryType),
    Table(CoreTableType),
    Func(CoreModuleFuncType),
    Global(CoreGlobalType),
    Type(CoreType),
    Module(CoreModuleType),
    Instance(CoreInstanceType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleImportType {
    Func(CoreModuleFuncType),
    Memory(CoreMemoryType),
    Table(CoreTableType),
    Global(CoreGlobalType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreInstanceType {
    pub(crate) exports: HashMap<String, CoreModuleExportType>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreType {
    Func(CoreFuncType),
    Module(CoreModuleType),
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoreModuleType {
    pub imports: HashMap<String, HashMap<String, CoreModuleImportType>>,
    pub exports: HashMap<String, CoreModuleExportType>,
}

impl From<CoreModuleType> for CoreInstanceType {
    fn from(value: CoreModuleType) -> Self {
        Self {
            exports: value.exports,
        }
    }
}

impl From<&Module> for CoreModuleType {
    fn from(value: &Module) -> Self {
        let function_types = module_function_types(value);
        let functions = module_functions(value);
        let tables = module_tables(value);
        let memories = module_memories(value);
        let globals = module_globals(value);
        let mut imports = HashMap::<String, HashMap<String, CoreModuleImportType>>::new();
        module_imports(value).iter().for_each(|x| {
            let value = match x.desc {
                ImportDesc::TypeIdx(ref idx) => {
                    let ty = function_types.get(idx.0 as usize).unwrap();
                    CoreModuleImportType::Func(CoreModuleFuncType::bare(ty.clone()))
                }
                ImportDesc::TableType(ref ty) => CoreModuleImportType::Table(*ty),
                ImportDesc::MemType(ref ty) => CoreModuleImportType::Memory(*ty),
                ImportDesc::GlobalType(ref ty) => CoreModuleImportType::Global(*ty),
            };
            if let Some(y) = imports.get_mut(&x.module) {
                y.insert(x.name.clone(), value);
            } else {
                let mut map = HashMap::new();
                map.insert(x.name.clone(), value);
                imports.insert(x.module.clone(), map);
            }
        });
        let exports = module_exports(value)
            .iter()
            .map(|export| {
                (export.0.clone(), {
                    match export.1 {
                        ExportDesc::Func(ref idx) => {
                            let tidx = functions.get(idx.0 as usize).unwrap();
                            let ty = function_types.get(tidx.0 as usize).unwrap();
                            CoreModuleExportType::Func(CoreModuleFuncType::bare(ty.clone()))
                        }
                        ExportDesc::Table(ref idx) => {
                            let ty = tables.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Table(*ty)
                        }
                        ExportDesc::Mem(ref idx) => {
                            let ty = memories.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Memory(*ty)
                        }
                        ExportDesc::Global(ref idx) => {
                            let ty = globals.get(idx.0 as usize).unwrap();
                            CoreModuleExportType::Global(*ty)
                        }
                    }
                })
            })
            .collect::<HashMap<_, _>>();
        Self { imports, exports }
    }
}

impl CoreModuleType {
    pub fn get_export(&self, name: &String) -> ParseResult<&CoreModuleExportType> {
        self.exports
            .get(name)
            .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))
    }

    pub fn assert_subtype_of(&self, parent: &Self) -> ParseResult<()> {
        for (module, actual_imports) in &self.imports {
            let Some(expected_imports) = parent.imports.get(module) else {
                return Err(ComponentParseError::TypeMismatch(
                    "core module import mismatch".to_owned(),
                ));
            };
            for (name, actual_import) in actual_imports {
                let Some(expected_import) = expected_imports.get(name) else {
                    return Err(ComponentParseError::TypeMismatch(
                        "core module import mismatch".to_owned(),
                    ));
                };
                expected_import.assert_satisfies_import(actual_import)?;
            }
        }
        for (name, expected_export) in &parent.exports {
            let Some(actual_export) = self.exports.get(name) else {
                return Err(ComponentParseError::TypeMismatch(
                    "core module import mismatch".to_owned(),
                ));
            };
            expected_export.assert_satisfied_by(actual_export)?;
        }
        Ok(())
    }

    pub fn assert_instantiation_args(
        &self,
        actual_args: &HashMap<String, CoreInstanceType>,
    ) -> ParseResult<()> {
        for (module_name, expected_imports) in &self.imports {
            let actual_instance = actual_args.get(module_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("core module import mismatch".to_owned())
            })?;
            for (export_name, expected_ty) in expected_imports {
                let actual_ty = actual_instance.exports.get(export_name).ok_or_else(|| {
                    ComponentParseError::TypeMismatch("core module import mismatch".to_owned())
                })?;
                expected_ty.assert_satisfied_by(actual_ty)?;
            }
        }
        Ok(())
    }
}

impl CoreModuleImportType {
    pub fn assert_satisfied_by(&self, actual: &CoreModuleExportType) -> ParseResult<()> {
        match (self, actual) {
            (CoreModuleImportType::Func(expected), CoreModuleExportType::Func(actual)) => {
                // The provided func's type must be a subtype of the
                // import's declared type: the signatures must agree
                // and the declared type must occur on the provided
                // type's declared supertype chain, so a plain
                // `(func)` does not satisfy an import declared with
                // a non-final `(sub (func))` type.
                if actual.signature == expected.signature
                    && actual.identity.is_subtype_of(&expected.identity)
                {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "core module import mismatch".to_owned(),
                    ))
                }
            }
            (CoreModuleImportType::Table(expected), CoreModuleExportType::Table(actual)) => {
                assert_table_matches(expected, actual)
            }
            (CoreModuleImportType::Memory(expected), CoreModuleExportType::Memory(actual)) => {
                assert_memory_matches(expected, actual)
            }
            (CoreModuleImportType::Global(expected), CoreModuleExportType::Global(actual)) => {
                assert_global_matches(expected, actual)
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "core module import mismatch".to_owned(),
            )),
        }
    }

    pub fn assert_satisfies_import(&self, actual: &CoreModuleImportType) -> ParseResult<()> {
        match (self, actual) {
            (CoreModuleImportType::Func(expected), CoreModuleImportType::Func(actual)) => {
                // Module-type subtyping is contravariant in the
                // imports: the parent's import type must be a
                // subtype of this module's import type, so the
                // chain walk runs from `expected` (the parent's)
                // towards `actual` — the table and memory arms
                // below swap their arguments the same way.
                if actual.signature == expected.signature
                    && expected.identity.is_subtype_of(&actual.identity)
                {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "core module import mismatch".to_owned(),
                    ))
                }
            }
            (CoreModuleImportType::Table(expected), CoreModuleImportType::Table(actual)) => {
                assert_table_matches(actual, expected)
            }
            (CoreModuleImportType::Memory(expected), CoreModuleImportType::Memory(actual)) => {
                assert_memory_matches(actual, expected)
            }
            (CoreModuleImportType::Global(expected), CoreModuleImportType::Global(actual)) => {
                assert_global_matches(actual, expected)
            }
            _ => Err(ComponentParseError::TypeMismatch(
                "core module import mismatch".to_owned(),
            )),
        }
    }
}

impl CoreModuleExportType {
    pub fn assert_satisfied_by(&self, actual: &CoreModuleExportType) -> ParseResult<()> {
        match self {
            CoreModuleExportType::Func(expected) => {
                CoreModuleImportType::Func(expected.clone()).assert_satisfied_by(actual)
            }
            CoreModuleExportType::Table(expected) => {
                CoreModuleImportType::Table(*expected).assert_satisfied_by(actual)
            }
            CoreModuleExportType::Memory(expected) => {
                CoreModuleImportType::Memory(*expected).assert_satisfied_by(actual)
            }
            CoreModuleExportType::Global(expected) => {
                CoreModuleImportType::Global(*expected).assert_satisfied_by(actual)
            }
            _ => {
                if self.eq_memoized(actual, &mut IdentityEqCache::new()) {
                    Ok(())
                } else {
                    Err(ComponentParseError::TypeMismatch(
                        "core module import mismatch".to_owned(),
                    ))
                }
            }
        }
    }
}

/// Memoised structural equality for the descriptor types, with
/// exactly the derived `PartialEq` semantics: the only difference
/// is that func identities are compared through
/// `CoreFuncIdentity::eq_memoized`, so a descriptor carrying a
/// shared-subgraph identity tower compares in time linear in the
/// number of distinct node pairs instead of exponential in the
/// number of paths reaching them. Used by the export-matching
/// fallback above, where module, instance and type exports are
/// compared whole.
impl CoreModuleFuncType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        self.signature == other.signature && self.identity.eq_memoized(&other.identity, cache)
    }
}

impl CoreModuleImportType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        match (self, other) {
            (Self::Func(lhs), Self::Func(rhs)) => lhs.eq_memoized(rhs, cache),
            (Self::Memory(lhs), Self::Memory(rhs)) => lhs == rhs,
            (Self::Table(lhs), Self::Table(rhs)) => lhs == rhs,
            (Self::Global(lhs), Self::Global(rhs)) => lhs == rhs,
            _ => false,
        }
    }
}

impl CoreModuleExportType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        match (self, other) {
            (Self::Memory(lhs), Self::Memory(rhs)) => lhs == rhs,
            (Self::Table(lhs), Self::Table(rhs)) => lhs == rhs,
            (Self::Func(lhs), Self::Func(rhs)) => lhs.eq_memoized(rhs, cache),
            (Self::Global(lhs), Self::Global(rhs)) => lhs == rhs,
            (Self::Type(lhs), Self::Type(rhs)) => lhs.eq_memoized(rhs, cache),
            (Self::Module(lhs), Self::Module(rhs)) => lhs.eq_memoized(rhs, cache),
            (Self::Instance(lhs), Self::Instance(rhs)) => lhs.eq_memoized(rhs, cache),
            _ => false,
        }
    }
}

impl CoreType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        match (self, other) {
            (Self::Func(lhs), Self::Func(rhs)) => lhs == rhs,
            (Self::Module(lhs), Self::Module(rhs)) => lhs.eq_memoized(rhs, cache),
            _ => false,
        }
    }
}

impl CoreModuleType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        self.imports.len() == other.imports.len()
            && self.exports.len() == other.exports.len()
            && self.imports.iter().all(|(module, lhs_imports)| {
                other.imports.get(module).is_some_and(|rhs_imports| {
                    lhs_imports.len() == rhs_imports.len()
                        && lhs_imports.iter().all(|(name, lhs)| {
                            rhs_imports
                                .get(name)
                                .is_some_and(|rhs| lhs.eq_memoized(rhs, cache))
                        })
                })
            })
            && self.exports.iter().all(|(name, lhs)| {
                other
                    .exports
                    .get(name)
                    .is_some_and(|rhs| lhs.eq_memoized(rhs, cache))
            })
    }
}

impl CoreInstanceType {
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        self.exports.len() == other.exports.len()
            && self.exports.iter().all(|(name, lhs)| {
                other
                    .exports
                    .get(name)
                    .is_some_and(|rhs| lhs.eq_memoized(rhs, cache))
            })
    }
}

fn assert_table_matches(expected: &CoreTableType, actual: &CoreTableType) -> ParseResult<()> {
    if expected.reftype != actual.reftype {
        return Err(ComponentParseError::TypeMismatch(
            "core module import mismatch".to_owned(),
        ));
    }
    assert_limits_match(expected.limits, actual.limits)
}

fn assert_memory_matches(expected: &CoreMemoryType, actual: &CoreMemoryType) -> ParseResult<()> {
    if expected.shared != actual.shared {
        return Err(ComponentParseError::TypeMismatch(
            "core module import mismatch".to_owned(),
        ));
    }
    assert_limits_match(expected.limits, actual.limits)
}

fn assert_global_matches(expected: &CoreGlobalType, actual: &CoreGlobalType) -> ParseResult<()> {
    if expected.0 != actual.0 || expected.1 != actual.1 {
        return Err(ComponentParseError::TypeMismatch(
            "core module import mismatch".to_owned(),
        ));
    }
    Ok(())
}

fn assert_limits_match(
    expected: crate::support::common::Limits,
    actual: crate::support::common::Limits,
) -> ParseResult<()> {
    if actual.min < expected.min {
        return Err(ComponentParseError::TypeMismatch(
            "core module import mismatch".to_owned(),
        ));
    }
    match (expected.max, actual.max) {
        (Some(expected_max), Some(actual_max)) if actual_max > expected_max => Err(
            ComponentParseError::TypeMismatch("core module import mismatch".to_owned()),
        ),
        (Some(_), None) => Err(ComponentParseError::TypeMismatch(
            "core module import mismatch".to_owned(),
        )),
        _ => Ok(()),
    }
}

impl From<CoreModuleExportType> for CoreSortType {
    fn from(val: CoreModuleExportType) -> Self {
        match val {
            CoreModuleExportType::Memory(_) => CoreSortType::Memory,
            CoreModuleExportType::Table(_) => CoreSortType::Table,
            CoreModuleExportType::Func(_) => CoreSortType::Func,
            CoreModuleExportType::Global(_) => CoreSortType::Global,
            CoreModuleExportType::Type(_) => CoreSortType::Type,
            CoreModuleExportType::Module(_) => CoreSortType::Module,
            CoreModuleExportType::Instance(_) => CoreSortType::Instance,
        }
    }
}

impl From<CoreFuncType> for CoreModuleExportType {
    fn from(value: CoreFuncType) -> Self {
        CoreModuleExportType::Func(CoreModuleFuncType::bare(value))
    }
}

impl From<CoreModuleFuncType> for CoreModuleExportType {
    fn from(value: CoreModuleFuncType) -> Self {
        CoreModuleExportType::Func(value)
    }
}

impl From<CoreMemoryType> for CoreModuleExportType {
    fn from(value: CoreMemoryType) -> Self {
        CoreModuleExportType::Memory(value)
    }
}

impl From<CoreTableType> for CoreModuleExportType {
    fn from(value: CoreTableType) -> Self {
        CoreModuleExportType::Table(value)
    }
}

impl From<CoreGlobalType> for CoreModuleExportType {
    fn from(value: CoreGlobalType) -> Self {
        CoreModuleExportType::Global(value)
    }
}

impl From<CoreType> for CoreModuleExportType {
    fn from(value: CoreType) -> Self {
        CoreModuleExportType::Type(value)
    }
}

impl From<CoreModuleType> for CoreModuleExportType {
    fn from(value: CoreModuleType) -> Self {
        CoreModuleExportType::Module(value)
    }
}

impl From<CoreInstanceType> for CoreModuleExportType {
    fn from(value: CoreInstanceType) -> Self {
        CoreModuleExportType::Instance(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::support::common::ValType;

    fn sig() -> CoreFuncType {
        CoreFuncType::new(vec![], vec![])
    }

    fn other_sig() -> CoreFuncType {
        CoreFuncType::new(vec![ValType::I32], vec![])
    }

    /// Builds the identity of the top member of a tower of
    /// two-member recursion groups, mirroring the identities the
    /// rectype decoder builds for the tower declarations in the
    /// `component_module_subtype_identity` integration tests: each
    /// level's identity reaches the level below along three paths
    /// (its supertype chain and both group members' recorded
    /// external supertypes). With `alter_deepest`, the first
    /// group's first member carries a different signature.
    fn tower_identity(depth: usize, alter_deepest: bool) -> Arc<CoreFuncIdentity> {
        let mut below = Arc::new(CoreFuncIdentity {
            signature: sig(),
            is_final: false,
            supertype: None,
            rec_group: None,
            rec_index: 0,
        });
        for level in 1..=depth {
            let first_sig = if alter_deepest && level == 1 {
                other_sig()
            } else {
                sig()
            };
            let group = Arc::new(CoreRecGroup {
                members: vec![
                    CoreRecMember {
                        signature: first_sig,
                        is_final: false,
                        supertype: Some(CoreRecSuper::External(below.clone())),
                    },
                    CoreRecMember {
                        signature: sig(),
                        is_final: false,
                        supertype: Some(CoreRecSuper::External(below.clone())),
                    },
                ],
            });
            below = Arc::new(CoreFuncIdentity {
                signature: sig(),
                is_final: false,
                supertype: Some(below.clone()),
                rec_group: Some(group),
                rec_index: 1,
            });
        }
        below
    }

    #[test]
    fn memoized_identity_eq_agrees_with_derived_eq() {
        let pairs: Vec<(Arc<CoreFuncIdentity>, Arc<CoreFuncIdentity>)> = vec![
            (tower_identity(5, false), tower_identity(5, false)),
            (tower_identity(5, false), tower_identity(5, true)),
            (tower_identity(5, false), tower_identity(4, false)),
            (tower_identity(5, false), CoreFuncIdentity::bare(sig())),
            (CoreFuncIdentity::bare(sig()), CoreFuncIdentity::bare(sig())),
            (
                CoreFuncIdentity::bare(sig()),
                CoreFuncIdentity::bare(other_sig()),
            ),
        ];
        for (lhs, rhs) in &pairs {
            let mut cache = IdentityEqCache::new();
            assert_eq!(
                lhs.eq_memoized(rhs, &mut cache),
                lhs.as_ref() == rhs.as_ref(),
                "memoised and derived equality must agree"
            );
        }
    }

    #[test]
    fn memoized_identity_eq_scales_to_deep_towers() {
        // The unmemoised comparison of two depth-200 towers would
        // need ~3^200 subgraph traversals; the memoised one decides
        // each distinct node pair once. Completion is the
        // assertion.
        let lhs = tower_identity(200, false);
        let rhs = tower_identity(200, false);
        let mut cache = IdentityEqCache::new();
        assert!(lhs.eq_memoized(&rhs, &mut cache));
        assert!(lhs.is_subtype_of(&rhs));
        let different = tower_identity(200, true);
        let mut cache = IdentityEqCache::new();
        assert!(!lhs.eq_memoized(&different, &mut cache));
        assert!(!lhs.is_subtype_of(&different));
    }

    #[test]
    fn descriptor_eq_memoized_covers_instance_exports() {
        // The export-matching fallback compares instance exports
        // whole; a func export carrying a tower identity must
        // compare through the memoised path too.
        let instance_with_tower = |depth: usize, alter: bool| {
            let mut exports = HashMap::new();
            exports.insert(
                "f".to_string(),
                CoreModuleExportType::Func(CoreModuleFuncType {
                    signature: sig(),
                    identity: tower_identity(depth, alter),
                }),
            );
            CoreModuleExportType::Instance(CoreInstanceType { exports })
        };
        let expected = instance_with_tower(64, false);
        let matching = instance_with_tower(64, false);
        expected
            .assert_satisfied_by(&matching)
            .expect("identical tower-carrying instance exports must match");
        let different = instance_with_tower(64, true);
        assert!(expected.assert_satisfied_by(&different).is_err());
    }
}
