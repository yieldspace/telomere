use crate::common::ExportDesc;
use crate::component_model::{
    Binding, CoreBinding, CoreExportSlot, CoreFuncRef, CoreFunction, CoreGlobalRef,
    CoreInstanceIdx, CoreInstanceImport, CoreInstanceInlineExport, CoreMemoryRef, CoreModule,
    CoreModuleIdx, CoreReference, CoreSort, CoreTableRef, Idx, Slot,
};
use crate::parser::component_model::{ComponentParseError, Validator};
use std::collections::HashMap;
use crate::Module;

pub enum CoreInstance {
    Real {
        module_idx: CoreModuleIdx,
        imports: HashMap<String, CoreInstanceImport>,
    },
    Alias {
        exports: HashMap<String, CoreInstanceInlineExport>,
    },
}

impl CoreInstance {
    pub fn get_instance(&self, name: String) -> CoreBinding<CoreInstance, usize> {
        match self {
            CoreInstance::Real { .. } => {
                unreachable!()
            }
            CoreInstance::Alias { exports } => match exports.get(&name).unwrap() {
                CoreInstanceInlineExport::Instance(idx) => {
                    CoreBinding::Binding(Binding::Alias(idx.global()))
                }
                _ => unreachable!(),
            },
        }
    }

    pub fn get_export(
        &self,
        validator: &dyn Validator,
        self_idx: CoreInstanceIdx,
        sort: CoreSort,
        name: String,
    ) -> Result<CoreExportSlot, ComponentParseError> {
        match self {
            CoreInstance::Real { module_idx, .. } => {
                let module = validator.get_core_module(module_idx);
                match &module.value {
                    None => {
                        module.ty.get_export(self_idx, sort, name)
                    }
                    Some(module) => {
                        let export = module
                            .exs
                            .0
                            .iter()
                            .find(|ex| ex.0 == name)
                            .ok_or(ComponentParseError::ExportNotFound(name.clone()))?;
                        match (sort, export.1) {
                            (CoreSort::Func, ExportDesc::Func(idx)) => Ok(CoreExportSlot::Func(
                                Slot::Value(CoreFunction::Export(CoreFuncRef(
                                    self_idx,
                                    idx,
                                    name.clone(),
                                ))),
                                CoreReference::Instance(self_idx, name),
                            )),
                            (CoreSort::Global, ExportDesc::Global(idx)) => {
                                Ok(CoreExportSlot::Global(
                                    Slot::Value(CoreGlobalRef(self_idx, idx, name.clone())),
                                    CoreReference::Instance(self_idx, name),
                                ))
                            }
                            (CoreSort::Table, ExportDesc::Table(idx)) => Ok(CoreExportSlot::Table(
                                Slot::Value(CoreTableRef(self_idx, idx, name.clone())),
                                CoreReference::Instance(self_idx, name),
                            )),
                            (CoreSort::Memory, ExportDesc::Mem(idx)) => Ok(CoreExportSlot::Memory(
                                Slot::Value(CoreMemoryRef(self_idx, idx, name.clone())),
                                CoreReference::Instance(self_idx, name),
                            )),
                            _ => {
                                panic!("Invalid export")
                            }
                        }
                    }
                }
            }
            CoreInstance::Alias { exports } => match exports.get(&name) {
                None => Err(ComponentParseError::ExportNotFound(name)),
                Some(export) => match (export, sort) {
                    (CoreInstanceInlineExport::Func(idx), CoreSort::Func) => {
                        Ok(CoreExportSlot::Func(
                            Slot::Idx(*idx),
                            CoreReference::Instance(self_idx, name),
                        ))
                    }
                    (CoreInstanceInlineExport::Table(idx), CoreSort::Table) => {
                        Ok(CoreExportSlot::Table(
                            Slot::Idx(*idx),
                            CoreReference::Instance(self_idx, name),
                        ))
                    }
                    (CoreInstanceInlineExport::Memory(idx), CoreSort::Memory) => {
                        Ok(CoreExportSlot::Memory(
                            Slot::Idx(*idx),
                            CoreReference::Instance(self_idx, name),
                        ))
                    }
                    (CoreInstanceInlineExport::Global(idx), CoreSort::Global) => {
                        Ok(CoreExportSlot::Global(
                            Slot::Idx(*idx),
                            CoreReference::Instance(self_idx, name),
                        ))
                    }
                    (CoreInstanceInlineExport::Type(_), _) => {
                        unimplemented!("because of export type proposal")
                    }
                    (CoreInstanceInlineExport::Module(_), _) => {
                        unimplemented!("because of module link proposal")
                    }
                    (CoreInstanceInlineExport::Instance(_), _) => {
                        unimplemented!("because of instance link proposal")
                    }
                    _ => panic!("Mismatch between export and sort"),
                },
            },
        }
    }
}
