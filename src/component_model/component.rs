use crate::component_model::{
    ComponentDecl, ComponentFunction, ComponentIdx, ComponentType, CoreModule, CoreModuleIdx,
    CoreSort, CoreSortWithIdx, CoreType, ExternDesc, FuncIdx, Instance, InstanceDecl, InstanceIdx,
    Reference, Slot, Sort, SortLike, SortWithIdx, Type, TypeBound, TypeIdx,
};
use crate::parser::component_model::{ComponentParseError, Validator};
use crate::runtime::component_model::instantiate::InstantiateInstr;

/// コンポーネントからexportされた型を表します．
/// exportされた型に明示的にexterndescが設定されている場合，externdescの型として表されます．
pub enum ComponentExportSlot {
    CoreModule(Slot<CoreModule, CoreModuleIdx>),
    Func(Slot<ComponentFunction, FuncIdx>),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value,
    Type(Slot<Type, TypeIdx>),
    Component(Slot<Component, ComponentIdx>),
    Instance(Slot<Instance, InstanceIdx>),
}

impl SortLike for ComponentExportSlot {
    fn eq_sort(&self, sort: Sort) -> bool {
        match self {
            ComponentExportSlot::CoreModule(_) => sort == Sort::Core(CoreSort::Module),
            ComponentExportSlot::Func(_) => sort == Sort::Func,
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            ComponentExportSlot::Value => sort == Sort::Value,
            ComponentExportSlot::Type(_) => sort == Sort::Type,
            ComponentExportSlot::Component(_) => sort == Sort::Component,
            ComponentExportSlot::Instance(_) => sort == Sort::Instance,
        }
    }
}

pub enum Component {
    Defined {
        instrs: Vec<InstantiateInstr>,
        imports: Vec<ComponentImport>,
        exports: Vec<ComponentExport>,
    },
    Typed(ComponentType, Reference),
    SuperTyped(ComponentType, ComponentIdx, Reference),
}

impl Component {
    pub(crate) fn new(
        instrs: Vec<InstantiateInstr>,
        imports: Vec<ComponentImport>,
        exports: Vec<ComponentExport>,
    ) -> Self {
        Self::Defined {
            instrs,
            imports,
            exports,
        }
    }

    pub(crate) fn get_export(
        &self,
        validator: &dyn Validator,
        self_idx: ComponentIdx,
        name: String,
    ) -> Result<ComponentExportSlot, ComponentParseError> {
        match self {
            Component::Defined { exports, .. } => {
                let export = exports
                    .iter()
                    .find(|export| export.name == name)
                    .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))?;
                if let Some(desc) = &export.desc {
                    // externdescの型として返します．
                    // externdescの型がsortの型のsuper typeかどうかはparse時に検証されることを想定しています．
                    // core module, component, instanceは型情報だけに落とし込む事ができないため，SuperTypedを用意する．
                    match desc {
                        ExternDesc::Core(idx) => {
                            let ty = validator.get_core_type(&idx);
                            if let CoreType::ModuleType(ty) = ty {
                                Ok(ComponentExportSlot::CoreModule(Slot::Value(
                                    CoreModule::SuperTyped(
                                        ty.clone(),
                                        export.sort.clone().try_into()?,
                                        Reference::Component(self_idx, name),
                                    ),
                                )))
                            } else {
                                panic!("Expected a module type");
                            }
                        }
                        ExternDesc::Func(idx) => {
                            let ty = validator.get_type(&idx);
                            if let Type::Func(ty) = ty {
                                Ok(ComponentExportSlot::Func(Slot::Value(
                                    ComponentFunction::SuperTyped(
                                        ty.clone(),
                                        export.sort.clone().try_into()?,
                                        Reference::Component(self_idx, name),
                                    ),
                                )))
                            } else {
                                panic!("Expected a function type");
                            }
                        }
                        #[cfg(feature = "component-gated-feature-value-imports-exports")]
                        ExternDesc::Value(bound) => todo!(),
                        ExternDesc::Type(ty) => match ty {
                            TypeBound::Eq(idx) => {
                                Ok(ComponentExportSlot::Type(Slot::Idx(idx.clone())))
                            }
                            TypeBound::Sub => Ok(ComponentExportSlot::Type(Slot::Value(
                                Type::SuperTypedUniqueResource(export.sort.clone().try_into()?),
                            ))),
                        },
                        ExternDesc::Component(idx) => {
                            let ty = validator.get_type(&idx);
                            if let Type::Component(ty) = ty {
                                Ok(ComponentExportSlot::Component(Slot::Value(
                                    Component::SuperTyped(
                                        ty.clone(),
                                        export.sort.clone().try_into()?,
                                        Reference::Component(self_idx, name),
                                    ),
                                )))
                            } else {
                                panic!("Expected a component type");
                            }
                        }
                        ExternDesc::Instance(idx) => {
                            let ty = validator.get_type(&idx);
                            if let Type::Instance(ty) = ty {
                                Ok(ComponentExportSlot::Instance(Slot::Value(
                                    Instance::SuperTyped(
                                        ty.clone(),
                                        export.sort.clone().try_into()?,
                                        Reference::Component(self_idx, name),
                                    ),
                                )))
                            } else {
                                panic!("Expected an instance type");
                            }
                        }
                    }
                } else {
                    match export.sort {
                        SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
                            Ok(ComponentExportSlot::CoreModule(Slot::Idx(idx)))
                        }
                        SortWithIdx::Func(idx) => Ok(ComponentExportSlot::Func(Slot::Idx(idx))),
                        #[cfg(feature = "component-gated-feature-value-imports-exports")]
                        SortWithIdx::Value(_) => todo!(),
                        SortWithIdx::Type(idx) => Ok(ComponentExportSlot::Type(Slot::Idx(idx))),
                        SortWithIdx::Component(idx) => {
                            Ok(ComponentExportSlot::Component(Slot::Idx(idx)))
                        }
                        SortWithIdx::Instance(idx) => {
                            Ok(ComponentExportSlot::Instance(Slot::Idx(idx)))
                        }
                        _ => Err(ComponentParseError::InvalidSort(
                            export.sort.clone(),
                            "ComponentExport".to_string(),
                        )),
                    }
                }
            }
            Component::Typed(ty, _) | Component::SuperTyped(ty, _, _) => {
                let desc =
                    ty.0.iter()
                        .find_map(|x| match x {
                            ComponentDecl::Import(_) => None,
                            ComponentDecl::Instance(decl) => match decl {
                                InstanceDecl::ExportDecl(decl) if decl.name == name => {
                                    Some(decl.ed.clone())
                                }
                                _ => None,
                            },
                        })
                        .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone()))?;
                match desc {
                    ExternDesc::Core(idx) => {
                        let ty = validator.get_core_type(&idx);
                        if let CoreType::ModuleType(ty) = ty {
                            Ok(ComponentExportSlot::CoreModule(Slot::Value(
                                CoreModule::Typed(ty.clone(), Reference::Component(self_idx, name)),
                            )))
                        } else {
                            panic!("Expected a module type");
                        }
                    }
                    ExternDesc::Func(idx) => {
                        let ty = validator.get_type(&idx);
                        if let Type::Func(ty) = ty {
                            Ok(ComponentExportSlot::Func(Slot::Value(
                                ComponentFunction::Typed(ty.clone(), Reference::Component(self_idx, name)),
                            )))
                        } else {
                            panic!("Expected a function type");
                        }
                    },
                    #[cfg(feature = "component-gated-feature-value-imports-exports")]
                    ExternDesc::Value(_) => todo!(),
                    ExternDesc::Type(bound) => match bound {
                        TypeBound::Eq(idx) => Ok(ComponentExportSlot::Type(Slot::Idx(idx))),
                        TypeBound::Sub => {
                            Ok(ComponentExportSlot::Type(Slot::Value(Type::UniqueResource)))
                        }
                    },
                    ExternDesc::Component(idx) => {
                        let ty = validator.get_type(&idx);
                        if let Type::Component(ty) = ty {
                            Ok(ComponentExportSlot::Component(Slot::Value(
                                Component::Typed(ty.clone(), Reference::Component(self_idx, name)),
                            )))
                        } else {
                            panic!("Expected a component type");
                        }
                    }
                    ExternDesc::Instance(idx) => {
                        let ty = validator.get_type(&idx);
                        if let Type::Instance(ty) = ty {
                            Ok(ComponentExportSlot::Instance(Slot::Value(Instance::Typed(
                                ty.clone(),
                                Reference::Component(self_idx, name)
                            ))))
                        } else {
                            panic!("Expected an instance type");
                        }
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum ComponentImport {
    CoreModule(String, CoreModuleIdx),
    Func(String, FuncIdx),
    #[cfg(feature = "component-gated-feature-value-imports-exports")]
    Value(String),
    Type(String, TypeIdx),
    Component(String, ComponentIdx),
    Instance(String, InstanceIdx),
}

#[derive(Debug, Clone)]
pub struct ComponentExport {
    pub name: String,
    pub sort: SortWithIdx,
    pub desc: Option<ExternDesc>,
}
