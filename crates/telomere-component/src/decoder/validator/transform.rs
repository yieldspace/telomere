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
        // A `(sub resource)` export face tied to this type (seeded by
        // the generics-replace evaluation) makes a reference to the
        // underlying resource denote the face's instantiation, so an
        // exported function stays tied to the exported face instead
        // of materialising a second, unrelated resource. A resource
        // the program also exports plainly is exempt: references to
        // it denote the plain export's instantiation, which a
        // consumer pairs the signature with.
        if let Some(face) = context.tie_target(type_id) {
            if !context.is_plain_exported(type_id) {
                return self.instantiate_sub_resource_type(face, context);
            }
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
        // A face over a resource the context has already substituted
        // -- an imported resource instantiated by an actual argument
        // -- denotes that same resource: the face is an abstract
        // view of the provider's resource, and the component cannot
        // mint an identity distinct from it. That correspondence is
        // recorded under the face itself by the generics-replace
        // evaluation before the program runs (see
        // `GenericsReplaceDSL::evaluate`), so it is covered by the
        // lookup above; a mapping for the underlying resource that
        // the evaluation itself produced must not collapse the face
        // onto it -- the face is a distinct identity there.

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
        // A `(sub resource)` generic is freshened only when a type
        // on the current walk declares it. A generic that belongs to
        // an enclosing component and is merely referenced through
        // the imported type (e.g. an instance type declared in the
        // enclosing component whose methods mention the enclosing
        // component's imported abstract resource) keeps its
        // identity, so surfaces mentioning it still resolve to the
        // enclosing component's name for it.
        if let Type::Generic(Generic {
            bound: GenericBound::Sub,
            ..
        }) = &ty
        {
            if context.freshen_declares(type_id) {
                let new_type_id = self.new_type(Type::Generic(Generic::new(GenericBound::Sub)));
                self.validate_effective_type_size(new_type_id)?;
                self.types.record_transform(
                    type_id,
                    TypeTransformKind::FreshenImport,
                    context.generation(),
                    env_id,
                    new_type_id,
                );
                context.insert(type_id, new_type_id);
                return Ok(new_type_id);
            }
            context.insert(type_id, type_id);
            return Ok(type_id);
        }
        let declared = self.declared_freshen_generics(&ty);
        context.push_freshen_declared(declared);
        let result = self.freshen_import_type(&ty, context);
        context.pop_freshen_declared();
        let cloned = result?;
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

    /// The `(sub resource)` generics a component or instance type
    /// declares through its type import/export entries, together
    /// with the generics declared by the component/instance types
    /// nested inside it. The nested declarations are determined up
    /// front, before any reference is traversed: an entry that
    /// references a nested declaration (e.g. a func whose parameter
    /// mentions a resource declared by a nested instance) may be
    /// visited before the nested type itself — the entry maps do
    /// not iterate in a fixed order — and a reference visited first
    /// must not pin the declaration to its template identity before
    /// the declaration itself has been freshened.
    fn declared_freshen_generics(&self, ty: &Type) -> std::collections::HashSet<TypeId> {
        let mut declared = std::collections::HashSet::new();
        let mut visited = std::collections::HashSet::new();
        self.collect_declared_freshen_generics(ty, &mut declared, &mut visited);
        declared
    }

    fn collect_declared_freshen_generics(
        &self,
        ty: &Type,
        declared: &mut std::collections::HashSet<TypeId>,
        visited: &mut std::collections::HashSet<TypeId>,
    ) {
        match ty {
            Type::Component(component) => {
                for import in component.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.note_declared_freshen_generic(*type_id, declared, visited);
                    }
                }
                for export in component.exports.values() {
                    match export {
                        ComponentExportType::Type(type_id)
                        | ComponentExportType::Component(type_id)
                        | ComponentExportType::Instance(type_id) => {
                            self.note_declared_freshen_generic(*type_id, declared, visited);
                        }
                        _ => {}
                    }
                }
            }
            Type::Instance(instance) => {
                for export in instance.exports.values() {
                    match export {
                        InstanceExportType::Type(type_id)
                        | InstanceExportType::Component(type_id)
                        | InstanceExportType::Instance(type_id) => {
                            self.note_declared_freshen_generic(*type_id, declared, visited);
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    /// Note `type_id` in `declared` when it is a `(sub resource)`
    /// generic, and collect the declarations of the component or
    /// instance type the entry ultimately denotes (following any
    /// `(eq ...)` faces) when that type has not been collected yet.
    fn note_declared_freshen_generic(
        &self,
        type_id: TypeId,
        declared: &mut std::collections::HashSet<TypeId>,
        visited: &mut std::collections::HashSet<TypeId>,
    ) {
        if let Ok(Type::Generic(Generic {
            bound: GenericBound::Sub,
            ..
        })) = self.get_type(type_id)
        {
            declared.insert(type_id);
        }
        let mut target = type_id;
        let target_ty = loop {
            match self.get_type(target) {
                Ok(Type::Generic(Generic {
                    bound: GenericBound::Eq(inner),
                    ..
                })) => {
                    if !visited.insert(target) {
                        return;
                    }
                    target = *inner;
                }
                Ok(ty) => break ty,
                Err(_) => return,
            }
        };
        if matches!(target_ty, Type::Component(_) | Type::Instance(_)) && visited.insert(target) {
            self.collect_declared_freshen_generics(target_ty, declared, visited);
        }
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
                ComponentImportType::Value(ty) => {
                    ComponentImportType::Value(self.instantiate_valtype(ty, context)?)
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
                ComponentExportType::Value(ty) => {
                    ComponentExportType::Value(self.instantiate_valtype(ty, context)?)
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
                ComponentImportType::Value(ty) => {
                    ComponentImportType::Value(self.freshen_import_valtype(ty, context)?)
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
                ComponentExportType::Value(ty) => {
                    ComponentExportType::Value(self.freshen_import_valtype(ty, context)?)
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
                InstanceExportType::Value(ty) => {
                    InstanceExportType::Value(self.instantiate_valtype(ty, context)?)
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
                InstanceExportType::Value(ty) => {
                    InstanceExportType::Value(self.freshen_import_valtype(ty, context)?)
                }
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

    pub(crate) fn instantiate_valtype(
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
            DefValType::Variant(variant) => {
                DefValType::Variant(crate::ir::types::VariantType::new(
                    variant
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
                    variant.form,
                ))
            }
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
            DefValType::Variant(variant) => {
                DefValType::Variant(crate::ir::types::VariantType::new(
                    variant
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
                    variant.form,
                ))
            }
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
