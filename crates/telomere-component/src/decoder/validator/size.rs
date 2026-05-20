use super::*;

pub(super) struct VisitTracker {
    epoch: u32,
    marks: Vec<u32>,
}

impl VisitTracker {
    pub(super) fn new(type_count: usize) -> Self {
        Self {
            epoch: 1,
            marks: vec![0; type_count],
        }
    }

    pub(super) fn enter(&mut self, type_id: TypeId) -> bool {
        let slot = &mut self.marks[type_id.index() as usize];
        if *slot == self.epoch {
            return false;
        }
        *slot = self.epoch;
        true
    }

    pub(super) fn leave(&mut self, type_id: TypeId) {
        self.marks[type_id.index() as usize] = 0;
    }
}

impl<'a> Validator<'a> {
    pub fn validate_effective_type_size(&self, type_id: TypeId) -> ParseResult<()> {
        const EFFECTIVE_TYPE_SIZE_LIMIT: u64 = 1_000_000;

        let mut visiting = VisitTracker::new(self.types.len());
        let size = self.compute_effective_type_size(type_id, &mut visiting)?;
        if size > EFFECTIVE_TYPE_SIZE_LIMIT {
            return Err(ComponentParseError::TypeMismatch(
                "effective type size exceeds the limit".to_owned(),
            ));
        }
        Ok(())
    }

    pub(super) fn contains_resource_handle(&self, type_id: TypeId) -> ParseResult<bool> {
        let mut visiting = VisitTracker::new(self.types.len());
        self.contains_resource_handle_with_tracker(type_id, &mut visiting)
    }

    fn contains_resource_handle_with_tracker(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        if let Some(found) = self.types.contains_resource_handle(type_id) {
            return Ok(found);
        }
        if !visiting.enter(type_id) {
            return Ok(false);
        }

        let result = match self.get_type(type_id)? {
            Type::DefVal(def) => self.defval_contains_resource_handle(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.contains_resource_handle_with_tracker(*inner, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => Ok(false),
        };
        visiting.leave(type_id);
        let found = result?;
        self.types.set_contains_resource_handle(type_id, found);
        Ok(found)
    }

    fn defval_contains_resource_handle(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match def {
            DefValType::Primitive(_) => Ok(false),
            DefValType::Record(fields) => fields.iter().try_fold(false, |found, field| {
                if found {
                    Ok(true)
                } else {
                    self.valtype_contains_resource_handle(&field.ty, visiting)
                }
            }),
            DefValType::Variant(cases) => cases.iter().try_fold(false, |found, case| {
                if found {
                    Ok(true)
                } else if let Some(ty) = &case.ty {
                    self.valtype_contains_resource_handle(ty, visiting)
                } else {
                    Ok(false)
                }
            }),
            DefValType::Flags(_) => Ok(false),
            DefValType::List(ty, _) => self.valtype_contains_resource_handle(ty, visiting),
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(ty) | DefValType::Future(ty) => ty
                .as_ref()
                .map(|ty| self.valtype_contains_resource_handle(ty, visiting))
                .transpose()
                .map(|found| found.unwrap_or(false)),
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(true),
        }
    }

    fn valtype_contains_resource_handle(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<bool> {
        match ty {
            ValType::Primitive(_) => Ok(false),
            ValType::Type(type_id) => {
                self.contains_resource_handle_with_tracker(*type_id, visiting)
            }
        }
    }

    pub(super) fn resource_owner_summary(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        if let Some(summary) = self.types.resource_owner_summary(type_id) {
            return Ok(summary);
        }

        if !visiting.enter(type_id) {
            return Ok(ResourceOwnerSummary::default());
        }

        let summary = match self.get_type(type_id)? {
            Type::DefVal(def) => self.defval_resource_owner_summary(def, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.resource_owner_summary(*inner, visiting)?,
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            }) => ResourceOwnerSummary::default(),
            Type::Resource(resource) => ResourceOwnerSummary::from_owner(resource.owner()),
            Type::Func(func_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for param in &func_ty.params {
                    summary.merge(&self.valtype_resource_owner_summary(param, visiting)?);
                }
                if let Some(result) = &func_ty.result {
                    summary.merge(&self.valtype_resource_owner_summary(result, visiting)?);
                }
                summary
            }
            Type::Component(component_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for import in component_ty.imports.values() {
                    if let ComponentImportType::Type { type_id, .. } = import {
                        summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                    }
                }
                for export in component_ty.exports.values() {
                    match export {
                        ComponentExportType::CoreModule(_) => {}
                        ComponentExportType::Component(type_id)
                        | ComponentExportType::Instance(type_id)
                        | ComponentExportType::Type(type_id)
                        | ComponentExportType::Func(type_id) => {
                            summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                        }
                    }
                }
                summary
            }
            Type::Instance(instance_ty) => {
                let mut summary = ResourceOwnerSummary::default();
                for export in instance_ty.exports.values() {
                    match export {
                        InstanceExportType::CoreModule(_) => {}
                        InstanceExportType::Component(type_id)
                        | InstanceExportType::Instance(type_id)
                        | InstanceExportType::Type(type_id)
                        | InstanceExportType::Func(type_id) => {
                            summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                        }
                    }
                }
                summary
            }
        };
        visiting.leave(type_id);
        self.types
            .set_resource_owner_summary(type_id, summary.clone());
        Ok(summary)
    }

    fn defval_resource_owner_summary(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        match def {
            DefValType::Primitive(_) => Ok(ResourceOwnerSummary::default()),
            DefValType::Record(fields) => {
                let mut summary = ResourceOwnerSummary::default();
                for field in fields {
                    summary.merge(&self.valtype_resource_owner_summary(&field.ty, visiting)?);
                }
                Ok(summary)
            }
            DefValType::Variant(cases) => {
                let mut summary = ResourceOwnerSummary::default();
                for case in cases {
                    if let Some(ty) = &case.ty {
                        summary.merge(&self.valtype_resource_owner_summary(ty, visiting)?);
                    }
                }
                Ok(summary)
            }
            DefValType::Flags(_) => Ok(ResourceOwnerSummary::default()),
            DefValType::List(ty, _) => self.valtype_resource_owner_summary(ty, visiting),
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(ty) | DefValType::Future(ty) => ty
                .as_ref()
                .map(|ty| self.valtype_resource_owner_summary(ty, visiting))
                .transpose()
                .map(|summary| summary.unwrap_or_default()),
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.resource_owner_summary(*type_id, visiting)
            }
        }
    }

    fn valtype_resource_owner_summary(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<ResourceOwnerSummary> {
        match ty {
            ValType::Primitive(_) => Ok(ResourceOwnerSummary::default()),
            ValType::Type(type_id) => self.resource_owner_summary(*type_id, visiting),
        }
    }

    pub(super) fn variant_is_inline(&self, cases: &[crate::ir::types::Case]) -> bool {
        matches!(
            cases,
            [
                crate::ir::types::Case { label, ty: None },
                crate::ir::types::Case { label: some, ty: Some(_) }
            ] if label.0 == "none" && some.0 == "some"
        ) || matches!(
            cases,
            [
                crate::ir::types::Case { label, .. },
                crate::ir::types::Case { label: err, .. }
            ] if label.0 == "ok" && err.0 == "err"
        )
    }

    pub(super) fn compute_effective_type_size(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        if let Some(size) = self.types.effective_size(type_id) {
            return Ok(size);
        }
        if !visiting.enter(type_id) {
            return Ok(1);
        }
        let ty = self.get_type(type_id)?.clone();
        let size = self.compute_type_size(&ty, visiting)?;
        visiting.leave(type_id);
        self.types.set_effective_size(type_id, size);
        Ok(size)
    }

    fn compute_type_size(&self, ty: &Type, visiting: &mut VisitTracker) -> ParseResult<u64> {
        match ty {
            Type::DefVal(def) => self.compute_defval_size(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.compute_effective_type_size(*inner, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_) => Ok(1),
            Type::Func(func_ty) => {
                let mut total = 1;
                for param in &func_ty.params {
                    total = saturating_add(total, self.compute_valtype_size(param, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                if let Some(result) = &func_ty.result {
                    total = saturating_add(total, self.compute_valtype_size(result, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            Type::Component(component_ty) => {
                let mut total = 1;
                for import in component_ty.imports.values() {
                    total = saturating_add(
                        total,
                        self.compute_component_import_size(import, visiting)?,
                    );
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                for export in component_ty.exports.values() {
                    total = saturating_add(
                        total,
                        self.compute_component_export_size(export, visiting)?,
                    );
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            Type::Instance(instance_ty) => {
                let mut total = 1;
                for export in instance_ty.exports.values() {
                    total =
                        saturating_add(total, self.compute_instance_export_size(export, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
        }
    }

    fn compute_component_import_size(
        &self,
        import: &ComponentImportType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match import {
            ComponentImportType::CoreModule(_) => Ok(1),
            ComponentImportType::Type { type_id, .. } => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_component_export_size(
        &self,
        export: &ComponentExportType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match export {
            ComponentExportType::CoreModule(_) => Ok(1),
            ComponentExportType::Component(type_id)
            | ComponentExportType::Instance(type_id)
            | ComponentExportType::Type(type_id)
            | ComponentExportType::Func(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_instance_export_size(
        &self,
        export: &InstanceExportType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match export {
            InstanceExportType::CoreModule(_) => Ok(1),
            InstanceExportType::Component(type_id)
            | InstanceExportType::Instance(type_id)
            | InstanceExportType::Type(type_id)
            | InstanceExportType::Func(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_defval_size(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match def {
            DefValType::Primitive(_) => Ok(1),
            DefValType::Record(fields) => {
                let mut total = 1;
                for field in fields {
                    total = saturating_add(total, self.compute_valtype_size(&field.ty, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            DefValType::Variant(cases) => {
                let mut total = 1;
                for case in cases {
                    if let Some(ty) = &case.ty {
                        total = saturating_add(total, self.compute_valtype_size(ty, visiting)?);
                    } else {
                        total = saturating_add(total, 1);
                    }
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(total)
            }
            DefValType::Flags(labels) => Ok((labels.len() as u64).div_ceil(32).max(1)),
            DefValType::List(ty, maybe_len) => {
                let elem = self.compute_valtype_size(ty, visiting)?;
                Ok(match maybe_len {
                    Some(len) => saturating_mul(elem, *len as u64),
                    None => saturating_add(elem, 1),
                })
            }
            #[cfg(feature = "component-gated-feature-async")]
            DefValType::Stream(ty) | DefValType::Future(ty) => {
                if let Some(ty) = ty {
                    let _ = self.compute_valtype_size(ty, visiting)?;
                }
                Ok(1)
            }
            DefValType::Own(type_id) | DefValType::Borrow(type_id) => {
                self.compute_effective_type_size(*type_id, visiting)
            }
        }
    }

    fn compute_valtype_size(&self, ty: &ValType, visiting: &mut VisitTracker) -> ParseResult<u64> {
        match ty {
            ValType::Primitive(_) => Ok(1),
            ValType::Type(type_id) => self.compute_effective_type_size(*type_id, visiting),
        }
    }
}

pub(super) const EFFECTIVE_TYPE_SIZE_CEILING: u64 = 1_000_001;

pub(super) fn saturating_add(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_add(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}

pub(super) fn saturating_mul(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_mul(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}
