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

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleExportType {
    Memory(CoreMemoryType),
    Table(CoreTableType),
    Func(CoreFuncType),
    Global(CoreGlobalType),
    Type(CoreType),
    Module(CoreModuleType),
    Instance(CoreInstanceType),
}

#[derive(Debug, Clone, PartialEq)]
pub enum CoreModuleImportType {
    Func(CoreFuncType),
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
                    CoreModuleImportType::Func(ty.clone())
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
                            CoreModuleExportType::Func(ty.clone())
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
                if actual == expected {
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
                if actual == expected {
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
