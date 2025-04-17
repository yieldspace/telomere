use crate::binary::BinaryReader;
use crate::common::ExportDesc;
use crate::component_model::{
    Binding, CoreBinding, CoreFuncType, CoreFunction, CoreGlobalRef, CoreInstanceImport,
    CoreInstanceInlineExport, CoreMemoryRef, CoreModuleIdx, CoreTableRef, Idx,
};
use crate::parser::component_model::{ParseContext, Validator};
use std::collections::HashMap;

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

    pub fn get_func(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        name: String,
    ) -> CoreBinding<CoreFunction, (usize, CoreFuncType)> {
        match self {
            CoreInstance::Real { module_idx, .. } => {
                let module = ctx.validator.get_core_module(module_idx);
                let export = module
                    .exs
                    .0
                    .iter()
                    .find(|ex| {
                        if ex.0 == name {
                            match ex.1 {
                                ExportDesc::Func(_) => true,
                                _ => false,
                            }
                        } else {
                            false
                        }
                    })
                    .unwrap();
                match export.1 {
                    ExportDesc::Func(f) => {
                        let ty = module.fts.0.get(f.0 as usize).unwrap().clone();
                        CoreBinding::Real((f.0 as usize, ty))
                    }
                    _ => unreachable!(),
                }
            }
            CoreInstance::Alias { exports } => match exports.get(&name).unwrap() {
                CoreInstanceInlineExport::Func(idx) => {
                    CoreBinding::Binding(Binding::Alias(idx.global()))
                }
                _ => unreachable!(),
            },
        }
    }

    pub fn get_table(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        name: String,
    ) -> CoreBinding<CoreTableRef, usize> {
        match self {
            CoreInstance::Real { module_idx, .. } => {
                let module = ctx.validator.get_core_module(module_idx);
                let idx = module
                    .exs
                    .0
                    .iter()
                    .find_map(|ex| {
                        if ex.0 == name {
                            if let ExportDesc::Table(idx) = ex.1 {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap();
                CoreBinding::Real(idx.0 as usize)
            }
            CoreInstance::Alias { exports } => {
                if let CoreInstanceInlineExport::Table(idx) = exports.get(&name).unwrap() {
                    CoreBinding::Binding(Binding::Alias(idx.global()))
                } else {
                    unreachable!()
                }
            }
        }
    }

    pub fn get_memory(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        name: String,
    ) -> CoreBinding<CoreMemoryRef, usize> {
        match self {
            CoreInstance::Real { module_idx, .. } => {
                let module = ctx.validator.get_core_module(module_idx);
                let idx = module
                    .exs
                    .0
                    .iter()
                    .find_map(|ex| {
                        if ex.0 == name {
                            if let ExportDesc::Mem(idx) = ex.1 {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap();
                CoreBinding::Real(idx.0 as usize)
            }
            CoreInstance::Alias { exports } => {
                if let CoreInstanceInlineExport::Memory(idx) = exports.get(&name).unwrap() {
                    CoreBinding::Binding(Binding::Alias(idx.global()))
                } else {
                    unreachable!()
                }
            }
        }
    }

    pub fn get_global(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        name: String,
    ) -> CoreBinding<CoreGlobalRef, usize> {
        match self {
            CoreInstance::Real { module_idx, .. } => {
                let module = ctx.validator.get_core_module(module_idx);
                let idx = module
                    .exs
                    .0
                    .iter()
                    .find_map(|ex| {
                        if ex.0 == name {
                            if let ExportDesc::Global(idx) = ex.1 {
                                Some(idx)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    })
                    .unwrap();
                CoreBinding::Real(idx.0 as usize)
            }
            CoreInstance::Alias { exports } => {
                if let CoreInstanceInlineExport::Global(idx) = exports.get(&name).unwrap() {
                    CoreBinding::Binding(Binding::Alias(idx.global()))
                } else {
                    unreachable!()
                }
            }
        }
    }

    pub fn get_type(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        name: String,
    ) -> CoreBinding<CoreFuncType, usize> {
        unreachable!("export type proposal")
    }
}
