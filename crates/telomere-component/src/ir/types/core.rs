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
        self.is_subtype_of_with_cache(sup, &mut IdentityEqCache::new())
    }

    /// `is_subtype_of` with the comparison cache supplied by the
    /// caller, so a matching operation that checks many func types
    /// against one another (a whole module-type match or module
    /// instantiation) reuses the pairs decided by its earlier
    /// checks instead of re-deriving them per function import --
    /// with a fresh cache per import, matching two module types
    /// whose imports reference levels of one shared identity tower
    /// costs time quadratic in the number of imports. Sharing the
    /// cache across the checks of one matching operation is sound:
    /// a cached entry is the structural equality of two identity
    /// nodes, a pure function of the two immutable graphs that no
    /// matching context can change, and every graph taking part in
    /// one matching operation outlives the operation, so no node
    /// address can be recycled while the cache is in use. The
    /// subtype relation itself is never cached -- each call still
    /// runs its own chain walk -- so the walk's direction cannot
    /// leak between checks either.
    pub(crate) fn is_subtype_of_with_cache(&self, sup: &Self, cache: &mut IdentityEqCache) -> bool {
        let mut current = self;
        loop {
            if current.eq_memoized(sup, cache) {
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
    ///
    /// The comparison runs on an explicit work stack (`EqMachine`),
    /// not by recursion: memoisation bounds the repeated work but
    /// not the call-stack depth, and the comparison reaches a
    /// recursion group's recorded external supertypes one group per
    /// level, so a recursive formulation descends one native frame
    /// chain per group and overflows the stack on a deep enough
    /// tower (measured: matching two towers of 100,000 two-member
    /// groups aborted the process in release mode). Every step of
    /// the machine descends to nodes defined strictly earlier than
    /// the pair it started from -- a declared supertype, or a group
    /// member's external supertype -- so the pair graph is acyclic
    /// and a pair is never requested again while its own evaluation
    /// is still pending.
    pub(crate) fn eq_memoized(&self, other: &Self, cache: &mut IdentityEqCache) -> bool {
        EqMachine::new(cache).compare(self, other)
    }
}

impl Drop for CoreFuncIdentity {
    fn drop(&mut self) {
        // A tower of identities links every level to the level
        // below -- along the declared supertype chain and through
        // the recursion-group members' external supertypes -- so
        // the default field-by-field drop cascades one native
        // frame chain per level and overflows the stack on a deep
        // enough tower (measured: dropping the same 100,000-group
        // tower whose comparison is iterative above still aborted
        // the process). Dismantle uniquely owned descendants on
        // an explicit worklist instead. A child whose `Arc` turns
        // out to be shared is left to its remaining owners; the
        // last owner's drop continues the dismantling from there,
        // so every path into the cascade reaches this loop within
        // a constant number of native frames.
        let mut work: Vec<Arc<CoreFuncIdentity>> = Vec::new();
        if let Some(supertype) = self.supertype.take() {
            work.push(supertype);
        }
        if let Some(rec_group) = self.rec_group.take() {
            take_group_externals(rec_group, &mut work);
        }
        while let Some(arc) = work.pop() {
            if let Ok(mut node) = Arc::try_unwrap(arc) {
                if let Some(supertype) = node.supertype.take() {
                    work.push(supertype);
                }
                if let Some(rec_group) = node.rec_group.take() {
                    take_group_externals(rec_group, &mut work);
                }
            }
        }
    }
}

/// Moves a recursion group's members' external supertype `Arc`s
/// into the drop worklist when this is the group's last owner;
/// a shared group is left for its remaining owners (see
/// `CoreFuncIdentity`'s `Drop`).
fn take_group_externals(rec_group: Arc<CoreRecGroup>, work: &mut Vec<Arc<CoreFuncIdentity>>) {
    if let Ok(rec_group) = Arc::try_unwrap(rec_group) {
        for mut member in rec_group.members {
            if let Some(CoreRecSuper::External(supertype)) = member.supertype.take() {
                work.push(supertype);
            }
        }
    }
}

/// The memoisation tables for one identity comparison session; see
/// `CoreFuncIdentity::eq_memoized`. Identity pairs and rec-group
/// pairs are keyed in separate maps because they are different
/// node kinds. A session spans a single comparison or a single
/// whole matching operation (`CoreModuleType::assert_subtype_of` /
/// `assert_instantiation_args`), never longer: every graph taking
/// part must outlive the session, because pairs are keyed by node
/// address.
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

/// One step of the iterative identity comparison (`EqMachine`).
enum EqStep<'a> {
    /// Compare an identity pair, leaving exactly one result.
    Identity(&'a CoreFuncIdentity, &'a CoreFuncIdentity),
    /// Compare a recursion-group pair, leaving exactly one result.
    RecGroup(&'a CoreRecGroup, &'a CoreRecGroup),
    /// Combine the top results with `&&`, record the identity pair
    /// under the key, and leave the combined result.
    FinishIdentity((usize, usize), usize),
    /// Combine and record a recursion-group pair, as above.
    FinishRecGroup((usize, usize), usize),
}

/// The identity comparison as an explicit work-stack machine: a
/// stack of `EqStep`s plus a result stack, evaluating the same
/// conjunction as the derived `PartialEq` in postfix order. A step
/// either decides its pair on the spot -- pointer equality, a
/// cached result, or a local mismatch -- and leaves one result, or
/// leaves a `Finish` step under one child step per sub-comparison
/// that still needs evaluating; the `Finish` step runs once every
/// child has left its result, records the pair in the cache (only
/// completed results are stored, exactly as the recursive form
/// did), and leaves the combined result. The native call-stack
/// depth is therefore constant in the depth of the type graphs
/// being compared, however deep their recursion-group towers run.
struct EqMachine<'a, 'c> {
    cache: &'c mut IdentityEqCache,
    work: Vec<EqStep<'a>>,
    results: Vec<bool>,
}

impl<'a, 'c> EqMachine<'a, 'c> {
    fn new(cache: &'c mut IdentityEqCache) -> Self {
        Self {
            cache,
            work: Vec::new(),
            results: Vec::new(),
        }
    }

    fn compare(mut self, lhs: &'a CoreFuncIdentity, rhs: &'a CoreFuncIdentity) -> bool {
        self.work.push(EqStep::Identity(lhs, rhs));
        while let Some(step) = self.work.pop() {
            match step {
                EqStep::Identity(lhs, rhs) => self.step_identity(lhs, rhs),
                EqStep::RecGroup(lhs, rhs) => self.step_rec_group(lhs, rhs),
                EqStep::FinishIdentity(key, count) => {
                    let result = self.combine(count);
                    self.cache.identities.insert(key, result);
                    self.results.push(result);
                }
                EqStep::FinishRecGroup(key, count) => {
                    let result = self.combine(count);
                    self.cache.rec_groups.insert(key, result);
                    self.results.push(result);
                }
            }
        }
        debug_assert_eq!(self.results.len(), 1);
        // Every step leaves exactly one result, so the seed pair's
        // result is the one element left; an empty stack would be
        // a machine bug, and `false` is the fail-closed direction
        // for a validator.
        self.results.pop().unwrap_or(false)
    }

    /// Pops the top `count` results and returns their conjunction.
    fn combine(&mut self, count: usize) -> bool {
        self.results
            .drain(self.results.len() - count..)
            .all(|result| result)
    }

    fn step_identity(&mut self, lhs: &'a CoreFuncIdentity, rhs: &'a CoreFuncIdentity) {
        if ptr::eq(lhs, rhs) {
            self.results.push(true);
            return;
        }
        let key = (ptr::from_ref(lhs) as usize, ptr::from_ref(rhs) as usize);
        if let Some(&result) = self.cache.identities.get(&key) {
            self.results.push(result);
            return;
        }
        if lhs.signature != rhs.signature
            || lhs.is_final != rhs.is_final
            || lhs.rec_index != rhs.rec_index
        {
            self.cache.identities.insert(key, false);
            self.results.push(false);
            return;
        }
        // A pair present on one side only decides the comparison
        // at once, as the derived equality's `Option` arms do.
        let supertype_pair = match (&lhs.supertype, &rhs.supertype) {
            (None, None) => None,
            (Some(lhs), Some(rhs)) => Some((&**lhs, &**rhs)),
            _ => {
                self.cache.identities.insert(key, false);
                self.results.push(false);
                return;
            }
        };
        let rec_group_pair = match (&lhs.rec_group, &rhs.rec_group) {
            (None, None) => None,
            (Some(lhs), Some(rhs)) => Some((&**lhs, &**rhs)),
            _ => {
                self.cache.identities.insert(key, false);
                self.results.push(false);
                return;
            }
        };
        let count = supertype_pair.is_some() as usize + rec_group_pair.is_some() as usize;
        if count == 0 {
            self.cache.identities.insert(key, true);
            self.results.push(true);
            return;
        }
        self.work.push(EqStep::FinishIdentity(key, count));
        if let Some((lhs, rhs)) = supertype_pair {
            self.work.push(EqStep::Identity(lhs, rhs));
        }
        if let Some((lhs, rhs)) = rec_group_pair {
            self.work.push(EqStep::RecGroup(lhs, rhs));
        }
    }

    fn step_rec_group(&mut self, lhs: &'a CoreRecGroup, rhs: &'a CoreRecGroup) {
        if ptr::eq(lhs, rhs) {
            self.results.push(true);
            return;
        }
        let key = (ptr::from_ref(lhs) as usize, ptr::from_ref(rhs) as usize);
        if let Some(&result) = self.cache.rec_groups.get(&key) {
            self.results.push(result);
            return;
        }
        if lhs.members.len() != rhs.members.len() {
            self.cache.rec_groups.insert(key, false);
            self.results.push(false);
            return;
        }
        // The members' local fields decide on the spot; a member's
        // external supertype is a sub-comparison of its own and
        // becomes a child step, exactly as the recursive form
        // recursed into it.
        let mut externals: Vec<(&'a CoreFuncIdentity, &'a CoreFuncIdentity)> = Vec::new();
        for (lhs, rhs) in lhs.members.iter().zip(rhs.members.iter()) {
            let local = lhs.signature == rhs.signature
                && lhs.is_final == rhs.is_final
                && match (&lhs.supertype, &rhs.supertype) {
                    (None, None) => true,
                    (Some(CoreRecSuper::Member(lhs)), Some(CoreRecSuper::Member(rhs))) => {
                        lhs == rhs
                    }
                    (Some(CoreRecSuper::External(_)), Some(CoreRecSuper::External(_))) => true,
                    _ => false,
                };
            if !local {
                self.cache.rec_groups.insert(key, false);
                self.results.push(false);
                return;
            }
            if let (Some(CoreRecSuper::External(lhs)), Some(CoreRecSuper::External(rhs))) =
                (&lhs.supertype, &rhs.supertype)
            {
                externals.push((lhs, rhs));
            }
        }
        if externals.is_empty() {
            self.cache.rec_groups.insert(key, true);
            self.results.push(true);
            return;
        }
        let count = externals.len();
        self.work.push(EqStep::FinishRecGroup(key, count));
        for (lhs, rhs) in externals {
            self.work.push(EqStep::Identity(lhs, rhs));
        }
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
        // One comparison cache for the whole match: the per-import
        // and per-export checks below share the identity pairs they
        // decide (see `CoreFuncIdentity::is_subtype_of_with_cache`).
        let mut cache = IdentityEqCache::new();
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
                expected_import.assert_satisfies_import_with_cache(actual_import, &mut cache)?;
            }
        }
        for (name, expected_export) in &parent.exports {
            let Some(actual_export) = self.exports.get(name) else {
                return Err(ComponentParseError::TypeMismatch(
                    "core module import mismatch".to_owned(),
                ));
            };
            expected_export.assert_satisfied_by_with_cache(actual_export, &mut cache)?;
        }
        Ok(())
    }

    pub fn assert_instantiation_args(
        &self,
        actual_args: &HashMap<String, CoreInstanceType>,
    ) -> ParseResult<()> {
        // One comparison cache for the whole instantiation match,
        // as in `assert_subtype_of` above.
        let mut cache = IdentityEqCache::new();
        for (module_name, expected_imports) in &self.imports {
            let actual_instance = actual_args.get(module_name).ok_or_else(|| {
                ComponentParseError::TypeMismatch("core module import mismatch".to_owned())
            })?;
            for (export_name, expected_ty) in expected_imports {
                let actual_ty = actual_instance.exports.get(export_name).ok_or_else(|| {
                    ComponentParseError::TypeMismatch("core module import mismatch".to_owned())
                })?;
                expected_ty.assert_satisfied_by_with_cache(actual_ty, &mut cache)?;
            }
        }
        Ok(())
    }
}

impl CoreModuleImportType {
    pub fn assert_satisfied_by(&self, actual: &CoreModuleExportType) -> ParseResult<()> {
        self.assert_satisfied_by_with_cache(actual, &mut IdentityEqCache::new())
    }

    /// `assert_satisfied_by` with the identity comparison cache
    /// supplied by the caller, so a whole module match shares the
    /// pairs it decides across its function imports (see
    /// `CoreFuncIdentity::is_subtype_of_with_cache`).
    pub(crate) fn assert_satisfied_by_with_cache(
        &self,
        actual: &CoreModuleExportType,
        cache: &mut IdentityEqCache,
    ) -> ParseResult<()> {
        match (self, actual) {
            (CoreModuleImportType::Func(expected), CoreModuleExportType::Func(actual)) => {
                // The provided func's type must be a subtype of the
                // import's declared type: the signatures must agree
                // and the declared type must occur on the provided
                // type's declared supertype chain, so a plain
                // `(func)` does not satisfy an import declared with
                // a non-final `(sub (func))` type.
                if actual.signature == expected.signature
                    && actual
                        .identity
                        .is_subtype_of_with_cache(&expected.identity, cache)
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
        self.assert_satisfies_import_with_cache(actual, &mut IdentityEqCache::new())
    }

    /// `assert_satisfies_import` with the identity comparison
    /// cache supplied by the caller, as in
    /// `assert_satisfied_by_with_cache`.
    pub(crate) fn assert_satisfies_import_with_cache(
        &self,
        actual: &CoreModuleImportType,
        cache: &mut IdentityEqCache,
    ) -> ParseResult<()> {
        match (self, actual) {
            (CoreModuleImportType::Func(expected), CoreModuleImportType::Func(actual)) => {
                // Module-type subtyping is contravariant in the
                // imports: the parent's import type must be a
                // subtype of this module's import type, so the
                // chain walk runs from `expected` (the parent's)
                // towards `actual` — the table and memory arms
                // below swap their arguments the same way.
                if actual.signature == expected.signature
                    && expected
                        .identity
                        .is_subtype_of_with_cache(&actual.identity, cache)
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
        self.assert_satisfied_by_with_cache(actual, &mut IdentityEqCache::new())
    }

    /// `assert_satisfied_by` with the identity comparison cache
    /// supplied by the caller, as in
    /// `CoreModuleImportType::assert_satisfied_by_with_cache`.
    pub(crate) fn assert_satisfied_by_with_cache(
        &self,
        actual: &CoreModuleExportType,
        cache: &mut IdentityEqCache,
    ) -> ParseResult<()> {
        match self {
            CoreModuleExportType::Func(expected) => CoreModuleImportType::Func(expected.clone())
                .assert_satisfied_by_with_cache(actual, cache),
            CoreModuleExportType::Table(expected) => {
                CoreModuleImportType::Table(*expected).assert_satisfied_by_with_cache(actual, cache)
            }
            CoreModuleExportType::Memory(expected) => CoreModuleImportType::Memory(*expected)
                .assert_satisfied_by_with_cache(actual, cache),
            CoreModuleExportType::Global(expected) => CoreModuleImportType::Global(*expected)
                .assert_satisfied_by_with_cache(actual, cache),
            _ => {
                if self.eq_memoized(actual, cache) {
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

    /// Builds the identity of the exported member of a tower of
    /// two-member recursion groups in the round-5 shape: the
    /// exported member declares no supertype, and the group's
    /// other member subtypes the previous group's exported
    /// member, so the identity comparison descends one group per
    /// level through that member's recorded external supertype
    /// while the declared subtype chains stay one edge deep.
    /// With `alter_deepest`, the tower's base identity carries a
    /// different signature -- a difference only visible at the
    /// very bottom of the tower.
    fn group_tower_identity(depth: usize, alter_deepest: bool) -> Arc<CoreFuncIdentity> {
        let mut below = Arc::new(CoreFuncIdentity {
            signature: if alter_deepest { other_sig() } else { sig() },
            is_final: false,
            supertype: None,
            rec_group: None,
            rec_index: 0,
        });
        for _ in 1..=depth {
            let group = Arc::new(CoreRecGroup {
                members: vec![
                    CoreRecMember {
                        signature: sig(),
                        is_final: false,
                        supertype: Some(CoreRecSuper::External(below.clone())),
                    },
                    CoreRecMember {
                        signature: sig(),
                        is_final: false,
                        supertype: None,
                    },
                ],
            });
            below = Arc::new(CoreFuncIdentity {
                signature: sig(),
                is_final: false,
                supertype: None,
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
            (
                group_tower_identity(5, false),
                group_tower_identity(5, false),
            ),
            (
                group_tower_identity(5, false),
                group_tower_identity(5, true),
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
    fn iterative_identity_eq_handles_deep_group_towers() {
        // Round 5: memoisation bounded the comparison's repeated
        // work but not its native recursion depth -- in the
        // recursive formulation, matching two towers of 100,000
        // two-member groups aborted the process with a stack
        // overflow (the abort threshold on the integration path
        // measured ~20,000 groups). The work-stack machine's
        // depth is constant, so both the identical pair and the
        // pair differing only at its base are decided; completion
        // is the assertion.
        let lhs = group_tower_identity(100_000, false);
        let rhs = group_tower_identity(100_000, false);
        let mut cache = IdentityEqCache::new();
        assert!(lhs.eq_memoized(&rhs, &mut cache));
        let different = group_tower_identity(100_000, true);
        let mut cache = IdentityEqCache::new();
        assert!(!lhs.eq_memoized(&different, &mut cache));
        // The towers are dropped normally at the end of the
        // test: `CoreFuncIdentity`'s iterative `Drop` dismantles
        // the 100,000-deep `Arc` chains without cascading, which
        // the end-to-end form of this shape relies on as well.
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

    #[test]
    fn shared_cache_across_subtype_walks_agrees_with_fresh_caches() {
        // Round 7: whole matching operations share one comparison
        // cache across their per-import subtype walks. A cached
        // entry is the structural equality of two immutable
        // identity graphs -- a pure function of the pair -- so
        // sharing must not change any walk's outcome: the same
        // pairs, run through one shared cache in both orders and
        // through fresh caches (`is_subtype_of`), must agree.
        let pairs: Vec<(Arc<CoreFuncIdentity>, Arc<CoreFuncIdentity>, bool)> = vec![
            (tower_identity(6, false), tower_identity(6, false), true),
            (tower_identity(6, false), tower_identity(6, true), false),
            (
                group_tower_identity(6, false),
                group_tower_identity(6, false),
                true,
            ),
            (
                group_tower_identity(6, false),
                group_tower_identity(6, true),
                false,
            ),
        ];
        let mut shared = IdentityEqCache::new();
        for (lhs, rhs, expected) in &pairs {
            assert_eq!(
                lhs.is_subtype_of_with_cache(rhs, &mut shared),
                *expected,
                "shared-cache walk must decide the pair as a fresh cache does"
            );
        }
        for (lhs, rhs, expected) in pairs.iter().rev() {
            assert_eq!(
                lhs.is_subtype_of_with_cache(rhs, &mut shared),
                *expected,
                "decisions recorded earlier in the shared cache must not leak \
                 into later walks"
            );
            assert_eq!(lhs.is_subtype_of(rhs), *expected);
        }
    }
}
