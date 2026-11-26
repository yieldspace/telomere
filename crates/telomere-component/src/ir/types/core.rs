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
use std::sync::Arc;

/// The canonical identity of a declared core func type: its
/// signature, its finality, and the identity of its declared
/// supertype, if any.
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
#[derive(Debug, Clone, PartialEq)]
pub struct CoreFuncIdentity {
    pub signature: CoreFuncType,
    pub is_final: bool,
    pub supertype: Option<Arc<CoreFuncIdentity>>,
}

impl CoreFuncIdentity {
    /// The identity of a bare (final, supertype-less) func type
    /// with the given signature.
    pub fn bare(signature: CoreFuncType) -> Arc<Self> {
        Arc::new(Self {
            signature,
            is_final: true,
            supertype: None,
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
        let mut current = self;
        loop {
            if current == sup {
                return true;
            }
            match &current.supertype {
                Some(supertype) => current = supertype,
                None => return false,
            }
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
                if self == actual {
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
