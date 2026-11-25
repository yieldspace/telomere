use super::*;
use crate::ir::types::PrimValType;

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
        // The Canonical ABI bound (`elem_size` < 2^28) applies to value
        // types (defvaltypes). Other type kinds (func / component /
        // instance / resource / abstract generics) have no `elem_size`
        // and are not gated here: the defvaltypes they contain are gated
        // at their own definitions, and substituted defvaltypes are
        // gated when instantiation creates them.
        if !self.resolves_to_defval(type_id)? {
            return Ok(());
        }
        let mut visiting = VisitTracker::new(self.types.len());
        let size = self.compute_effective_type_size(type_id, &mut visiting)?;
        if size >= EFFECTIVE_TYPE_SIZE_LIMIT {
            return Err(ComponentParseError::TypeMismatch(
                "effective type size exceeds the maximum byte size".to_owned(),
            ));
        }
        Ok(())
    }

    fn resolves_to_defval(&self, type_id: TypeId) -> ParseResult<bool> {
        let mut seen = std::collections::HashSet::new();
        let mut current = type_id;
        loop {
            if !seen.insert(current) {
                return Ok(false);
            }
            match self.get_type(current)? {
                Type::DefVal(_) => return Ok(true),
                Type::Generic(Generic {
                    bound: GenericBound::Eq(inner),
                    ..
                }) => current = *inner,
                _ => return Ok(false),
            }
        }
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
                    match import {
                        ComponentImportType::Type { type_id, .. } => {
                            summary.merge(&self.resource_owner_summary(*type_id, visiting)?);
                        }
                        ComponentImportType::Value(ty) => {
                            summary.merge(&self.valtype_resource_owner_summary(ty, visiting)?);
                        }
                        ComponentImportType::CoreModule(_) => {}
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
                        ComponentExportType::Value(ty) => {
                            summary.merge(&self.valtype_resource_owner_summary(ty, visiting)?);
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
                        InstanceExportType::Value(ty) => {
                            summary.merge(&self.valtype_resource_owner_summary(ty, visiting)?);
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
            ComponentImportType::Value(ty) => self.compute_valtype_size(ty, visiting),
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
            ComponentExportType::Value(ty) => self.compute_valtype_size(ty, visiting),
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
            InstanceExportType::Value(ty) => self.compute_valtype_size(ty, visiting),
        }
    }

    /// Canonical ABI `elem_size` of a defined value type, computed for
    /// a 64-bit pointer (`ptr_size() == 8`): validation requires the
    /// 2^28 bound to hold for every `cx`, `elem_size` is monotonically
    /// non-decreasing in the pointer size, and the upstream
    /// `validation/max-value-size.wast` boundaries are calibrated to
    /// the 64-bit layout, so that layout is the one to check. Results
    /// saturate at `EFFECTIVE_TYPE_SIZE_CEILING` (= 2^28): a computed
    /// value equal to the ceiling means "at least 2^28".
    fn compute_defval_size(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match def {
            DefValType::Primitive(prim) => Ok(primitive_elem_size(prim)),
            DefValType::Record(fields) => {
                // C-like layout: each field starts at the next offset
                // aligned to the field's alignment, and the record's
                // size is rounded up to the record's alignment.
                let mut aligning = VisitTracker::new(self.types.len());
                let mut total = 0;
                let mut record_alignment = 1;
                for field in fields {
                    let field_alignment =
                        self.compute_valtype_alignment(&field.ty, &mut aligning)?;
                    record_alignment = record_alignment.max(field_alignment);
                    total = align_to(total, field_alignment);
                    total = saturating_add(total, self.compute_valtype_size(&field.ty, visiting)?);
                    if total >= EFFECTIVE_TYPE_SIZE_CEILING {
                        return Ok(EFFECTIVE_TYPE_SIZE_CEILING);
                    }
                }
                Ok(align_to(total, record_alignment))
            }
            DefValType::Variant(cases) => {
                // Discriminant first, then the largest payload at its
                // alignment; the total is rounded up to the variant's
                // alignment.
                let mut aligning = VisitTracker::new(self.types.len());
                let discriminant = discriminant_elem_size(cases.len());
                let mut max_case_alignment = 1;
                let mut max_payload = 0;
                for case in cases {
                    if let Some(ty) = &case.ty {
                        max_case_alignment = max_case_alignment
                            .max(self.compute_valtype_alignment(ty, &mut aligning)?);
                        max_payload = max_payload.max(self.compute_valtype_size(ty, visiting)?);
                    }
                }
                let total = saturating_add(align_to(discriminant, max_case_alignment), max_payload);
                Ok(align_to(total, discriminant.max(max_case_alignment)))
            }
            DefValType::Flags(labels) => Ok(flags_elem_size(labels.len())),
            DefValType::List(ty, maybe_len) => Ok(match maybe_len {
                Some(len) => {
                    let elem = self.compute_valtype_size(ty, visiting)?;
                    saturating_mul(elem, *len as u64)
                }
                // A dynamically-sized list is stored as a (pointer,
                // length) pair; the element type does not contribute.
                None => 2 * ELEM_POINTER_SIZE,
            }),
            // Handles are stored as `i32` table indices. For `borrow`,
            // the Canonical ABI may substitute the resource's
            // representation when lowering into the defining component,
            // but the representation is not part of the arena type, so
            // the handle size is used here.
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(4),
        }
    }

    fn compute_type_alignment(
        &self,
        type_id: TypeId,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        if !visiting.enter(type_id) {
            return Ok(1);
        }
        let ty = self.get_type(type_id)?.clone();
        let result = match &ty {
            Type::DefVal(def) => self.compute_defval_alignment(def, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Eq(inner),
                ..
            }) => self.compute_type_alignment(*inner, visiting),
            Type::Generic(Generic {
                bound: GenericBound::Sub,
                ..
            })
            | Type::Resource(_)
            | Type::Func(_)
            | Type::Component(_)
            | Type::Instance(_) => Ok(1),
        };
        visiting.leave(type_id);
        result
    }

    fn compute_defval_alignment(
        &self,
        def: &DefValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match def {
            DefValType::Primitive(prim) => Ok(primitive_alignment(prim)),
            DefValType::Record(fields) => {
                let mut alignment = 1;
                for field in fields {
                    alignment = alignment.max(self.compute_valtype_alignment(&field.ty, visiting)?);
                }
                Ok(alignment)
            }
            DefValType::Variant(cases) => {
                let mut alignment = discriminant_elem_size(cases.len());
                for case in cases {
                    if let Some(ty) = &case.ty {
                        alignment = alignment.max(self.compute_valtype_alignment(ty, visiting)?);
                    }
                }
                Ok(alignment)
            }
            DefValType::Flags(labels) => Ok(flags_elem_size(labels.len())),
            DefValType::List(ty, maybe_len) => match maybe_len {
                Some(_) => self.compute_valtype_alignment(ty, visiting),
                None => Ok(ELEM_POINTER_SIZE),
            },
            DefValType::Own(_) | DefValType::Borrow(_) => Ok(4),
        }
    }

    fn compute_valtype_alignment(
        &self,
        ty: &ValType,
        visiting: &mut VisitTracker,
    ) -> ParseResult<u64> {
        match ty {
            ValType::Primitive(prim) => Ok(primitive_alignment(prim)),
            ValType::Type(type_id) => self.compute_type_alignment(*type_id, visiting),
        }
    }

    fn compute_valtype_size(&self, ty: &ValType, visiting: &mut VisitTracker) -> ParseResult<u64> {
        match ty {
            ValType::Primitive(prim) => Ok(primitive_elem_size(prim)),
            ValType::Type(type_id) => self.compute_effective_type_size(*type_id, visiting),
        }
    }
}

/// The spec bound: `elem_size` of a defined value type must be less
/// than 2^28 (Canonical ABI, "Element Size").
pub(super) const EFFECTIVE_TYPE_SIZE_LIMIT: u64 = 1 << 28;

/// Saturation cap for size computations. Any value that reaches the
/// cap is at least 2^28 and therefore over the limit; capping keeps
/// products of nested fixed-size lists from overflowing `u64`.
pub(super) const EFFECTIVE_TYPE_SIZE_CEILING: u64 = 1 << 28;

/// Pointer size used for `elem_size` / alignment computations (the
/// 64-bit layout; see `compute_defval_size`).
pub(super) const ELEM_POINTER_SIZE: u64 = 8;

pub(super) fn align_to(value: u64, alignment: u64) -> u64 {
    value.div_ceil(alignment) * alignment
}

pub(super) fn primitive_elem_size(prim: &PrimValType) -> u64 {
    match prim {
        PrimValType::Bool | PrimValType::S8 | PrimValType::U8 => 1,
        PrimValType::S16 | PrimValType::U16 => 2,
        PrimValType::S32 | PrimValType::U32 | PrimValType::F32 | PrimValType::Char => 4,
        PrimValType::S64 | PrimValType::U64 | PrimValType::F64 => 8,
        PrimValType::String => 2 * ELEM_POINTER_SIZE,
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => 4,
    }
}

pub(super) fn primitive_alignment(prim: &PrimValType) -> u64 {
    match prim {
        // A string is a (pointer, length) pair: pointer-aligned, two
        // pointers wide.
        PrimValType::String => ELEM_POINTER_SIZE,
        _ => primitive_elem_size(prim),
    }
}

pub(super) fn flags_elem_size(label_count: usize) -> u64 {
    // Flags use the smallest integer that fits all the bits. The
    // decoder rejects more than 32 labels, so three cases suffice.
    match label_count {
        0..=8 => 1,
        9..=16 => 2,
        _ => 4,
    }
}

pub(super) fn discriminant_elem_size(case_count: usize) -> u64 {
    // The variant discriminant is the smallest integer covering the
    // number of cases.
    match case_count {
        0..=256 => 1,
        257..=65_536 => 2,
        _ => 4,
    }
}

pub(super) fn saturating_add(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_add(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}

pub(super) fn saturating_mul(lhs: u64, rhs: u64) -> u64 {
    lhs.saturating_mul(rhs).min(EFFECTIVE_TYPE_SIZE_CEILING)
}
