use super::size::VisitTracker;
use super::*;

impl<'a> Validator<'a> {
    pub fn validate_current_component_resources(&self, type_id: TypeId) -> ParseResult<()> {
        let mut visiting = VisitTracker::new(self.types.len());
        if self
            .resource_owner_summary(type_id, &mut visiting)?
            .refs_foreign_resource(self.current_scope_id())
        {
            return Err(ComponentParseError::TypeMismatch(
                "refers to resources not defined in the current component".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_component_surface(&self, ty: &ComponentType) -> ParseResult<()> {
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_component_surface_inner(ty, &[], &mut seen)
    }

    pub fn validate_instance_surface(&self, ty: &InstanceType) -> ParseResult<()> {
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_instance_surface_inner(ty, &[], &mut seen)
    }

    pub fn validate_component_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if matches!(
            self.types.validation_state(type_id),
            Some(ValidationState::Validated)
        ) {
            return Ok(());
        }
        let Type::Component(component_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any component".to_owned(),
            ));
        };
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_component_surface_inner(&component_ty, &[], &mut seen)?;
        self.types
            .set_validation_state(type_id, ValidationState::Validated);
        Ok(())
    }

    pub fn validate_instance_type_definition(&self, type_id: TypeId) -> ParseResult<()> {
        if matches!(
            self.types.validation_state(type_id),
            Some(ValidationState::Validated)
        ) {
            return Ok(());
        }
        let Type::Instance(instance_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(
                "Type ID does not refer to any instance".to_owned(),
            ));
        };
        let mut seen = VisitTracker::new(self.types.len());
        self.validate_instance_surface_inner(&instance_ty, &[], &mut seen)?;
        self.types
            .set_validation_state(type_id, ValidationState::Validated);
        Ok(())
    }

    fn validate_component_surface_inner(
        &self,
        ty: &ComponentType,
        inherited_visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let import_type_ids = self.collect_component_visible_types(ty, SurfaceRole::Import)?;
        let mut export_type_ids = import_type_ids.clone();
        let mut visiting = VisitTracker::new(self.types.len());
        for export in ty.exports.values() {
            self.extend_export_visible_types(export, &mut export_type_ids, &mut visiting)?;
        }
        let mut import_visible = inherited_visible.to_vec();
        merge_type_ids(&mut import_visible, &import_type_ids);
        let mut export_visible = import_visible.clone();
        merge_type_ids(&mut export_visible, &export_type_ids);

        for import in ty.imports.values() {
            self.validate_component_import_surface(import, &import_visible, seen)?;
        }
        for export in ty.exports.values() {
            self.validate_component_export_surface(export, &export_visible, seen)?;
        }
        Ok(())
    }

    fn validate_instance_surface_inner(
        &self,
        ty: &InstanceType,
        inherited_visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let mut visible = inherited_visible.to_vec();
        let instance_visible = self.collect_instance_visible_types(ty)?;
        merge_type_ids(&mut visible, &instance_visible);
        for export in ty.exports.values() {
            self.validate_instance_export_surface(export, &visible, seen)?;
        }
        Ok(())
    }

    fn validate_component_import_surface(
        &self,
        import: &ComponentImportType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match import {
            ComponentImportType::CoreModule(_) => Ok(()),
            ComponentImportType::Type { type_id, .. } => {
                self.validate_type_root(*type_id, visible, SurfaceRole::Import, seen)
            }
        }
    }

    fn validate_component_export_surface(
        &self,
        export: &ComponentExportType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(()),
            ComponentExportType::Component(type_id) => {
                self.validate_component_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Instance(type_id) => {
                self.validate_instance_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Type(type_id) => {
                self.validate_type_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            ComponentExportType::Func(type_id) => {
                self.validate_func_root(*type_id, visible, SurfaceRole::Export, seen)
            }
        }
    }

    fn validate_instance_export_surface(
        &self,
        export: &InstanceExportType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(()),
            InstanceExportType::Component(type_id) => {
                self.validate_component_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            InstanceExportType::Instance(type_id) => {
                self.validate_instance_root(*type_id, visible, SurfaceRole::Export, seen)
            }
            InstanceExportType::Type(type_id) => {
                self.validate_instance_export_type_root(*type_id, visible, seen)
            }
            InstanceExportType::Func(type_id) => {
                self.validate_func_root(*type_id, visible, SurfaceRole::Export, seen)
            }
        }
    }

    fn validate_instance_export_type_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        if self.type_requires_name(&ty)
            && !contains_type_id(visible, type_id)
            && !self.resource_visible_by_identity(type_id, visible)?
            && !self.defval_visible_by_structure(type_id, visible)?
        {
            return Err(ComponentParseError::TypeMismatch(
                "type not valid to be used as export".to_owned(),
            ));
        }
        self.validate_type_definition(type_id, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "type not valid to be used as export: {}",
                    error
                ))
            })
    }

    fn validate_type_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if self.contains_resource_handle(type_id)?
            && !self.type_root_allows_visible_resource_alias(type_id, visible)?
        {
            return Err(ComponentParseError::TypeMismatch(format!(
                "type not valid to be used as {}",
                role.noun()
            )));
        }
        self.validate_type_definition(type_id, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "type not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            })
    }

    fn validate_func_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let Type::Func(func_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "func not valid to be used as {}",
                role.noun()
            )));
        };
        self.validate_func_definition(&func_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "func not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            })
    }

    fn validate_component_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
            return Ok(());
        }
        let Type::Component(component_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "component not valid to be used as {}",
                role.noun()
            )));
        };
        let result = self
            .validate_component_surface_inner(&component_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "component not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            });
        seen.leave(type_id);
        result
    }

    fn validate_instance_root(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        role: SurfaceRole,
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
            return Ok(());
        }
        let Type::Instance(instance_ty) = self.get_type(type_id)?.clone() else {
            return Err(ComponentParseError::TypeMismatch(format!(
                "instance not valid to be used as {}",
                role.noun()
            )));
        };
        let result = self
            .validate_instance_surface_inner(&instance_ty, visible, seen)
            .map_err(|error| {
                ComponentParseError::TypeMismatch(format!(
                    "instance not valid to be used as {}: {}",
                    role.noun(),
                    error
                ))
            });
        seen.leave(type_id);
        result
    }

    fn validate_type_definition(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        if !seen.enter(type_id) {
            return Ok(());
        }
        let ty = self.get_type(type_id)?.clone();
        let result = match ty {
            Type::DefVal(def) => self.validate_defval_definition(&def, visible, seen),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.validate_type_ref(inner, visible, seen),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_) => Ok(()),
            Type::Func(func_ty) => self.validate_func_definition(&func_ty, visible, seen),
            Type::Component(component_ty) => {
                match self.types.validation_state(type_id).unwrap_or_default() {
                    ValidationState::Validated => Ok(()),
                    ValidationState::InProgress => Ok(()),
                    ValidationState::Unknown => {
                        self.types
                            .set_validation_state(type_id, ValidationState::InProgress);
                        let result =
                            self.validate_component_surface_inner(&component_ty, visible, seen);
                        if result.is_ok() {
                            self.types
                                .set_validation_state(type_id, ValidationState::Validated);
                        } else {
                            self.types
                                .set_validation_state(type_id, ValidationState::Unknown);
                        }
                        result
                    }
                }
            }
            Type::Instance(instance_ty) => {
                match self.types.validation_state(type_id).unwrap_or_default() {
                    ValidationState::Validated => Ok(()),
                    ValidationState::InProgress => Ok(()),
                    ValidationState::Unknown => {
                        self.types
                            .set_validation_state(type_id, ValidationState::InProgress);
                        let result =
                            self.validate_instance_surface_inner(&instance_ty, visible, seen);
                        if result.is_ok() {
                            self.types
                                .set_validation_state(type_id, ValidationState::Validated);
                        } else {
                            self.types
                                .set_validation_state(type_id, ValidationState::Unknown);
                        }
                        result
                    }
                }
            }
        };
        seen.leave(type_id);
        result
    }

    fn validate_type_ref(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        if self.type_requires_name(&ty) {
            if contains_type_id(visible, type_id)
                || self.resource_visible_by_identity(type_id, visible)?
                || self.defval_visible_by_structure(type_id, visible)?
            {
                Ok(())
            } else {
                let ty = self.get_type(type_id)?.clone();
                Err(ComponentParseError::TypeMismatch(format!(
                    "surface type requires exported/imported name: {type_id:?} => {ty:?}"
                )))
            }
        } else {
            self.validate_type_definition(type_id, visible, seen)
        }
    }

    fn resource_visible_by_identity(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
    ) -> ParseResult<bool> {
        let ty = self.get_type(type_id)?;

        for candidate in visible {
            match (ty, self.get_type(*candidate)?) {
                (Type::Resource(resource), Type::Resource(candidate_resource))
                    if candidate_resource == resource =>
                {
                    return Ok(true);
                }
                (Type::Generic(generic), Type::Generic(candidate_generic))
                    if generic.id == candidate_generic.id =>
                {
                    return Ok(true);
                }
                (
                    Type::Generic(Generic {
                        bound: GenericBound::Sub,
                        ..
                    }),
                    Type::Generic(Generic {
                        bound: GenericBound::Sub,
                        ..
                    }),
                ) => {
                    return Ok(true);
                }
                _ => {}
            }
        }

        Ok(false)
    }

    fn type_root_allows_visible_resource_alias(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
    ) -> ParseResult<bool> {
        let mut visiting = VisitTracker::new(self.types.len());
        self.type_root_allows_visible_resource_alias_inner(type_id, visible, &mut visiting)
    }

    fn type_root_allows_visible_resource_alias_inner(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        if !visiting.enter(type_id) {
            return Ok(false);
        }
        let result = match self.get_type(type_id)? {
            Type::Resource(_) => {
                contains_type_id(visible, type_id)
                    || self.resource_visible_by_identity(type_id, visible)?
            }
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => {
                contains_type_id(visible, type_id)
                    || self.resource_visible_by_identity(type_id, visible)?
            }
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                contains_type_id(visible, *inner)
                    || self.resource_visible_by_identity(*inner, visible)?
                    || self
                        .type_root_allows_visible_resource_alias_inner(*inner, visible, visiting)?
            }
            _ => false,
        };
        visiting.leave(type_id);
        Ok(result)
    }

    fn defval_visible_by_structure(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
    ) -> ParseResult<bool> {
        let Type::DefVal(def) = self.get_type(type_id)? else {
            return Ok(false);
        };

        for candidate in visible {
            let Type::DefVal(candidate_def) = self.get_type(*candidate)? else {
                continue;
            };
            if def.assert_subtype_of(candidate_def, self).is_ok()
                && candidate_def.assert_subtype_of(def, self).is_ok()
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn validate_func_definition(
        &self,
        func_ty: &FuncType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        for param in &func_ty.params {
            self.validate_valtype_ref(param, visible, seen)?;
        }
        if let Some(result) = &func_ty.result {
            self.validate_valtype_ref(result, visible, seen)?;
        }
        Ok(())
    }

    fn validate_valtype_ref(
        &self,
        val_ty: &ValType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match val_ty {
            ValType::Primitive(_) => Ok(()),
            ValType::Type(type_id) => self.validate_type_ref(*type_id, visible, seen),
        }
    }

    fn validate_nested_valtype_ref(
        &self,
        val_ty: &ValType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match val_ty {
            ValType::Primitive(_) => Ok(()),
            ValType::Type(type_id) => self.validate_nested_type_ref(*type_id, visible, seen),
        }
    }

    fn validate_nested_type_ref(
        &self,
        type_id: TypeId,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        let ty = self.get_type(type_id)?.clone();
        let mut visiting = VisitTracker::new(self.types.len());
        if self.type_requires_nested_name(&ty, &mut visiting)? {
            if contains_type_id(visible, type_id)
                || self.resource_visible_by_identity(type_id, visible)?
                || self.defval_visible_by_structure(type_id, visible)?
            {
                Ok(())
            } else {
                Err(ComponentParseError::TypeMismatch(format!(
                    "surface type requires exported/imported name: {type_id:?} => {ty:?}"
                )))
            }
        } else {
            self.validate_type_definition(type_id, visible, seen)
        }
    }

    fn validate_defval_definition(
        &self,
        def: &DefValType,
        visible: &[TypeId],
        seen: &mut VisitTracker,
    ) -> ParseResult<()> {
        match def {
            DefValType::Primitive(_) => Ok(()),
            DefValType::Record(fields) => {
                for field in fields {
                    self.validate_nested_valtype_ref(&field.ty, visible, seen)?;
                }
                Ok(())
            }
            DefValType::Variant(cases) => {
                for case in cases {
                    if let Some(ty) = &case.ty {
                        self.validate_nested_valtype_ref(ty, visible, seen)?;
                    }
                }
                Ok(())
            }
            DefValType::Flags(_) => Ok(()),
            DefValType::List(ty, _) => self.validate_nested_valtype_ref(ty, visible, seen),
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(ty) | DefValType::Future(ty) => {
                if let Some(ty) = ty {
                    self.validate_nested_valtype_ref(ty, visible, seen)
                } else {
                    Ok(())
                }
            }
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.validate_type_ref(*type_id, visible, seen)
            }
        }
    }

    fn type_requires_name(&self, ty: &Type) -> bool {
        match ty {
            Type::DefVal(def) => self.defval_requires_name(def),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => true,
            Type::Generic(Generic {
                bound: GenericBound::Eq(_),
                ..
            }) => false,
        }
    }

    fn defval_requires_name(&self, def: &DefValType) -> bool {
        match def {
            DefValType::Primitive(_) => false,
            DefValType::Record(fields) => !fields
                .iter()
                .enumerate()
                .all(|(index, field)| field.label.0 == index.to_string()),
            DefValType::Variant(cases) => !self.variant_is_inline(cases),
            DefValType::Flags(_) => false,
            DefValType::List(_, _) => false,
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(_) | DefValType::Future(_) => false,
            DefValType::Own(_) | DefValType::Borrow(_) => false,
        }
    }

    fn type_requires_nested_name(
        &self,
        ty: &Type,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            Type::DefVal(def) => self.defval_requires_nested_name(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => Ok(true),
            Type::Generic(Generic {
                bound: GenericBound::Eq(_),
                ..
            }) => Ok(false),
        }
    }

    fn defval_requires_nested_name(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match def {
            DefValType::Primitive(_) => Ok(false),
            DefValType::Record(fields) => {
                if !fields
                    .iter()
                    .enumerate()
                    .all(|(index, field)| field.label.0 == index.to_string())
                {
                    return Ok(true);
                }
                for field in fields {
                    if self.nested_valtype_requires_name(&field.ty, visiting)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            DefValType::Variant(cases) => {
                if !self.variant_is_inline(cases) {
                    return Ok(true);
                }
                for case in cases {
                    if let Some(ty) = &case.ty {
                        if self.nested_valtype_requires_name(ty, visiting)? {
                            return Ok(true);
                        }
                    }
                }
                Ok(false)
            }
            DefValType::Flags(_) => Ok(true),
            DefValType::List(ty, _) => self.nested_valtype_requires_name(ty, visiting),
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(ty) | DefValType::Future(ty) => ty
                .as_ref()
                .map(|ty| self.nested_valtype_requires_name(ty, visiting))
                .transpose()
                .map(|requires| requires.unwrap_or(false)),
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(false),
        }
    }

    fn nested_valtype_requires_name(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => self.type_id_requires_nested_name(*type_id, visiting),
        }
    }

    fn type_id_requires_nested_name(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        if !visiting.enter(type_id) {
            return Ok(false);
        }
        let ty = self.get_type(type_id)?.clone();
        let result = self.type_requires_nested_name(&ty, visiting)?;
        visiting.leave(type_id);
        Ok(result)
    }

    fn collect_component_visible_types(
        &self,
        ty: &ComponentType,
        role: SurfaceRole,
    ) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        match role {
            SurfaceRole::Import => {
                let mut visiting = VisitTracker::new(self.types.len());
                for import in ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_visible_closure_into(*type_id, &mut visible, &mut visiting)?;
                    }
                }
            }
            SurfaceRole::Export => {
                let mut visiting = VisitTracker::new(self.types.len());
                for export in ty.exports.values() {
                    self.extend_export_visible_types(export, &mut visible, &mut visiting)?;
                }
            }
        }
        Ok(visible)
    }

    fn collect_instance_visible_types(&self, ty: &InstanceType) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        for export in ty.exports.values() {
            self.extend_instance_named_types(export, &mut visible);
        }
        Ok(visible)
    }

    fn extend_instance_named_types(&self, export: &InstanceExportType, visible: &mut Vec<TypeId>) {
        match export {
            InstanceExportType::CoreModule(_) => {}
            InstanceExportType::Func(type_id)
            | InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id) => merge_type_ids(visible, &[*type_id]),
        }
    }

    fn extend_export_visible_types(
        &self,
        export: &ComponentExportType,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(()),
            ComponentExportType::Component(type_id)
            | ComponentExportType::Instance(type_id)
            | ComponentExportType::Type(type_id)
            | ComponentExportType::Func(type_id) => {
                self.extend_visible_closure_into(*type_id, visible, visiting)
            }
        }
    }

    fn extend_instance_export_visible_types(
        &self,
        export: &InstanceExportType,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(()),
            InstanceExportType::Func(type_id)
            | InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id) => {
                self.extend_visible_closure_into(*type_id, visible, visiting)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn visible_closure(&self, type_id: TypeId) -> ParseResult<Vec<TypeId>> {
        let mut visible = Vec::new();
        let mut visiting = VisitTracker::new(self.types.len());
        self.extend_visible_closure_into(type_id, &mut visible, &mut visiting)?;
        Ok(visible)
    }

    fn extend_visible_closure_into(
        &self,
        type_id: TypeId,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        if let Some(closure) = self.types.visible_closure(type_id) {
            merge_type_ids(visible, &closure);
            return Ok(());
        }

        if !visiting.enter(type_id) {
            merge_type_ids(visible, &[type_id]);
            return Ok(());
        }

        if matches!(
            self.get_type(type_id)?,
            Type::DefVal(_)
                | Type::Func(_)
                | Type::Resource(_)
                | Type::Generic(Generic {
                    bound: GenericBound::Sub,
                    ..
                })
        ) {
            visiting.leave(type_id);
            merge_type_ids(visible, &[type_id]);
            return Ok(());
        }

        let mut closure = vec![type_id];
        self.compute_visible_closure(type_id, &mut closure, visiting)?;
        visiting.leave(type_id);
        merge_type_ids(visible, &closure);
        if closure.len() > 1 {
            self.types.set_visible_closure(type_id, closure);
        }
        Ok(())
    }

    fn compute_visible_closure(
        &self,
        type_id: TypeId,
        visible: &mut Vec<TypeId>,
        visiting: &mut VisitTracker,
    ) -> ParseResult<()> {
        match self.get_type(type_id)? {
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => {
                self.extend_visible_closure_into(*inner, visible, visiting)?;
            }
            Type::Component(component_ty) => {
                for import in component_ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        self.extend_visible_closure_into(*type_id, visible, visiting)?;
                    }
                }
                for export in component_ty.exports.values() {
                    self.extend_export_visible_types(export, visible, visiting)?;
                }
            }
            Type::Instance(instance_ty) => {
                for export in instance_ty.exports.values() {
                    self.extend_instance_export_visible_types(export, visible, visiting)?;
                }
            }
            Type::DefVal(_) | Type::Func(_) | Type::Resource(_) | Type::Generic(_) => {}
        }
        Ok(())
    }
}
