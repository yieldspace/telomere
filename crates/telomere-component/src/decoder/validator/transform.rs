use super::*;

impl<'a> Validator<'a> {
    fn next_transform_generation(&self) -> u32 {
        let next = self.transform_generation.get().wrapping_add(1).max(1);
        self.transform_generation.set(next);
        next
    }

    pub(crate) fn new_transform_context(&self) -> TransformContext {
        TransformContext::new(self.next_transform_generation())
    }

    pub(crate) fn instantiate_type_id(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
            return Ok(mapped);
        }
        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::Instantiate,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.instantiate_type(&ty, context)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::Instantiate,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub(crate) fn instantiate_sub_resource_type(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?;
        if !matches!(
            ty,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) | Type::Resource(_)
        ) {
            return Err(ComponentParseError::TypeMismatch(
                "expected resource".to_owned(),
            ));
        }

        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::SubResource,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let new_type_id = self.new_type(Type::Resource(ResourceId::synthetic()));
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::SubResource,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    pub fn freshen_import_type_id(&mut self, type_id: TypeId) -> ParseResult<TypeId> {
        let mut context = self.new_transform_context();
        self.freshen_import_type_id_with_context(type_id, &mut context)
    }

    pub(crate) fn resolve_surface_type_id(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        self.freshen_import_type_id_with_context(type_id, context)
    }

    fn freshen_import_type_id_with_context(
        &mut self,
        type_id: TypeId,
        context: &mut TransformContext,
    ) -> ParseResult<TypeId> {
        if let Some(mapped) = context.get(type_id) {
            return Ok(mapped);
        }
        let env_id = self.types.subst_env_id(context);
        if let Some(mapped) = self.types.lookup_transform(
            type_id,
            TypeTransformKind::FreshenImport,
            context.generation(),
            env_id,
        ) {
            context.insert(type_id, mapped);
            return Ok(mapped);
        }

        let ty = self.get_type(type_id)?.clone();
        let cloned = self.freshen_import_type(&ty, context)?;
        let new_type_id = self.new_type(cloned);
        self.validate_effective_type_size(new_type_id)?;
        self.types.record_transform(
            type_id,
            TypeTransformKind::FreshenImport,
            context.generation(),
            env_id,
            new_type_id,
        );
        context.insert(type_id, new_type_id);
        Ok(new_type_id)
    }

    fn instantiate_type(&mut self, ty: &Type, context: &mut TransformContext) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.instantiate_defval(def, context)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                let inner = self.instantiate_type_id(*inner, context)?;
                Type::Generic(Generic::new(GenericBound::Eq(inner)))
            }
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.instantiate_func(func_ty, context)?),
            Type::Resource(_) => Type::Resource(ResourceId::synthetic()),
            Type::Component(component_ty) => {
                Type::Component(self.instantiate_component_type(component_ty, context)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.instantiate_instance_type(instance_ty, context)?)
            }
        })
    }

    fn freshen_import_type(
        &mut self,
        ty: &Type,
        context: &mut TransformContext,
    ) -> ParseResult<Type> {
        Ok(match ty {
            Type::DefVal(def) => Type::DefVal(self.freshen_import_defval(def, context)?),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => Type::Generic(Generic::new(GenericBound::Eq(
                self.freshen_import_type_id_with_context(*inner, context)?,
            ))),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => Type::Generic(Generic::new(GenericBound::Sub)),
            Type::Func(func_ty) => Type::Func(self.freshen_import_func(func_ty, context)?),
            Type::Resource(resource) => Type::Resource(*resource),
            Type::Component(component_ty) => {
                Type::Component(self.freshen_import_component_type(component_ty, context)?)
            }
            Type::Instance(instance_ty) => {
                Type::Instance(self.freshen_import_instance_type(instance_ty, context)?)
            }
        })
    }

    fn instantiate_component_type(
        &mut self,
        ty: &ComponentType,
        context: &mut TransformContext,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.instantiate_type_id(*type_id, context)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.instantiate_type_id(*inner, context)?,
                        )),
                        GenericBound::Sub => Generic::new(GenericBound::Sub),
                    };
                    ComponentImportType::Type { type_id, generic }
                }
            };
            imports.insert(name.clone(), import);
        }

        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                ComponentExportType::CoreModule(module_ty) => {
                    ComponentExportType::CoreModule(module_ty.clone())
                }
                ComponentExportType::Component(type_id) => {
                    ComponentExportType::Component(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Instance(type_id) => {
                    ComponentExportType::Instance(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Type(type_id) => {
                    ComponentExportType::Type(self.instantiate_type_id(*type_id, context)?)
                }
                ComponentExportType::Func(type_id) => {
                    ComponentExportType::Func(self.instantiate_type_id(*type_id, context)?)
                }
            };
            exports.insert(name.clone(), export);
        }

        Ok(ComponentType {
            import_order: ty.import_order.clone(),
            imports,
            exports,
            generics_replacing_program: ty.generics_replacing_program.clone(),
        })
    }

    fn freshen_import_component_type(
        &mut self,
        ty: &ComponentType,
        context: &mut TransformContext,
    ) -> ParseResult<ComponentType> {
        let mut imports = HashMap::new();
        for (name, import) in &ty.imports {
            let import = match import {
                ComponentImportType::CoreModule(module_ty) => {
                    ComponentImportType::CoreModule(module_ty.clone())
                }
                ComponentImportType::Type { type_id, generic } => {
                    let type_id = self.freshen_import_type_id_with_context(*type_id, context)?;
                    let generic = match &generic.bound {
                        GenericBound::Eq(inner) => Generic::new(GenericBound::Eq(
                            self.freshen_import_type_id_with_context(*inner, context)?,
                        )),
                        GenericBound::Sub => Generic::new(GenericBound::Sub),
                    };
                    ComponentImportType::Type { type_id, generic }
                }
            };
            imports.insert(name.clone(), import);
        }

        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                ComponentExportType::CoreModule(module_ty) => {
                    ComponentExportType::CoreModule(module_ty.clone())
                }
                ComponentExportType::Component(type_id) => ComponentExportType::Component(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Instance(type_id) => ComponentExportType::Instance(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Type(type_id) => ComponentExportType::Type(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                ComponentExportType::Func(type_id) => ComponentExportType::Func(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
            };
            exports.insert(name.clone(), export);
        }

        Ok(ComponentType {
            import_order: ty.import_order.clone(),
            imports,
            exports,
            generics_replacing_program: ty.generics_replacing_program.clone(),
        })
    }

    fn instantiate_instance_type(
        &mut self,
        ty: &InstanceType,
        context: &mut TransformContext,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => {
                    InstanceExportType::Func(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Component(type_id) => {
                    InstanceExportType::Component(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Instance(type_id) => {
                    InstanceExportType::Instance(self.instantiate_type_id(*type_id, context)?)
                }
                InstanceExportType::Type(type_id) => {
                    InstanceExportType::Type(self.instantiate_type_id(*type_id, context)?)
                }
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn freshen_import_instance_type(
        &mut self,
        ty: &InstanceType,
        context: &mut TransformContext,
    ) -> ParseResult<InstanceType> {
        let mut exports = HashMap::new();
        for (name, export) in &ty.exports {
            let export = match export {
                InstanceExportType::CoreModule(module_ty) => {
                    InstanceExportType::CoreModule(module_ty.clone())
                }
                InstanceExportType::Func(type_id) => InstanceExportType::Func(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Component(type_id) => InstanceExportType::Component(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Instance(type_id) => InstanceExportType::Instance(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
                InstanceExportType::Type(type_id) => InstanceExportType::Type(
                    self.freshen_import_type_id_with_context(*type_id, context)?,
                ),
            };
            exports.insert(name.clone(), export);
        }
        Ok(InstanceType { exports })
    }

    fn instantiate_func(
        &mut self,
        ty: &FuncType,
        context: &mut TransformContext,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.instantiate_valtype(param, context))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.instantiate_valtype(result, context))
            .transpose()?;
        Ok(FuncType {
            params,
            param_names: ty.param_names.clone(),
            result,
        })
    }

    fn freshen_import_func(
        &mut self,
        ty: &FuncType,
        context: &mut TransformContext,
    ) -> ParseResult<FuncType> {
        let params = ty
            .params
            .iter()
            .map(|param| self.freshen_import_valtype(param, context))
            .collect::<ParseResult<Vec<_>>>()?;
        let result = ty
            .result
            .as_ref()
            .map(|result| self.freshen_import_valtype(result, context))
            .transpose()?;
        Ok(FuncType {
            params,
            param_names: ty.param_names.clone(),
            result,
        })
    }

    fn instantiate_valtype(
        &mut self,
        ty: &ValType,
        context: &mut TransformContext,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => ValType::Type(self.instantiate_type_id(*type_id, context)?),
        })
    }

    fn freshen_import_valtype(
        &mut self,
        ty: &ValType,
        context: &mut TransformContext,
    ) -> ParseResult<ValType> {
        Ok(match ty {
            ValType::Primitive(prim) => ValType::Primitive(prim.clone()),
            ValType::Type(type_id) => {
                ValType::Type(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
        })
    }

    fn instantiate_defval(
        &mut self,
        ty: &DefValType,
        context: &mut TransformContext,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.instantiate_valtype(&field.ty, context)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.instantiate_valtype(ty, context))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Flags(labels) => DefValType::Flags(labels.clone()),
            DefValType::List(ty, len) => {
                DefValType::List(self.instantiate_valtype(ty, context)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.instantiate_type_id(*type_id, context)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.instantiate_type_id(*type_id, context)?)
            }
        })
    }

    fn freshen_import_defval(
        &mut self,
        ty: &DefValType,
        context: &mut TransformContext,
    ) -> ParseResult<DefValType> {
        Ok(match ty {
            DefValType::Primitive(prim) => DefValType::Primitive(prim.clone()),
            DefValType::Record(fields) => DefValType::Record(
                fields
                    .iter()
                    .map(|field| {
                        Ok(crate::ir::types::LabelValType::new(
                            field.label.clone(),
                            self.freshen_import_valtype(&field.ty, context)?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Variant(cases) => DefValType::Variant(
                cases
                    .iter()
                    .map(|case| {
                        Ok(crate::ir::types::Case::new(
                            case.label.clone(),
                            case.ty
                                .as_ref()
                                .map(|ty| self.freshen_import_valtype(ty, context))
                                .transpose()?,
                        ))
                    })
                    .collect::<ParseResult<Vec<_>>>()?,
            ),
            DefValType::Flags(labels) => DefValType::Flags(labels.clone()),
            DefValType::List(ty, len) => {
                DefValType::List(self.freshen_import_valtype(ty, context)?, *len)
            }
            DefValType::Own(type_id) => {
                DefValType::Own(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
            DefValType::Borrow(type_id) => {
                DefValType::Borrow(self.freshen_import_type_id_with_context(*type_id, context)?)
            }
        })
    }
}
