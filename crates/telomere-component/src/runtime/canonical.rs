use super::*;

pub(super) fn direct_wasm_to_component_value(
    value: WasmValue,
) -> Result<ComponentValue, ComponentError> {
    Ok(match value {
        WasmValue::I32(v) => ComponentValue::I32(v),
        WasmValue::I64(v) => ComponentValue::I64(v),
        WasmValue::F32(v) => ComponentValue::F32(v),
        WasmValue::F64(v) => ComponentValue::F64(v),
        WasmValue::FuncRef(v) => ComponentValue::Own(v),
        WasmValue::ExternRef(v) => ComponentValue::Borrow(v),
        WasmValue::V128(_) => {
            return Err(ComponentError::Unsupported(
                "v128 values are not supported in component runtime".to_owned(),
            ))
        }
    })
}

pub(super) fn component_value_to_direct_wasm(
    value: &ComponentValue,
) -> Result<WasmValue, ComponentError> {
    Ok(match value {
        ComponentValue::Bool(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U32(v) => WasmValue::I32(*v as i32),
        ComponentValue::S32(v) | ComponentValue::I32(v) => WasmValue::I32(*v),
        ComponentValue::U64(v) => WasmValue::I64(*v as i64),
        ComponentValue::S64(v) | ComponentValue::I64(v) => WasmValue::I64(*v),
        ComponentValue::F32(v) => WasmValue::F32(*v),
        ComponentValue::F64(v) => WasmValue::F64(*v),
        ComponentValue::Own(v) | ComponentValue::Borrow(v) => WasmValue::I32(*v as i32),
        other => {
            return Err(ComponentError::Unsupported(format!(
                "direct core invocation does not support {other:?}"
            )))
        }
    })
}

pub(super) fn lower_component_args(
    func_type: &FuncType,
    args: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    if func_type.params.len() != args.len() {
        return Err(ComponentError::InvalidArgument(format!(
            "expected {} component arguments, got {}",
            func_type.params.len(),
            args.len()
        )));
    }
    let total_flat_len = function_params_flat_len(func_type, program)?;
    if total_flat_len > MAX_FLAT_PARAMS {
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let realloc = options.realloc.as_ref().ok_or_else(|| {
            ComponentError::Runtime("canonical option `realloc` is required".to_owned())
        })?;
        let layout = value_area_layout(&func_type.params, program)?;
        // Canonical ABI `lower_flat_values` allocates the indirect area by
        // calling realloc unconditionally, with no zero-size shortcut. The
        // returned pointer is validated (alignment and bounds) inside
        // `call_realloc`.
        let ptr = call_realloc(
            realloc,
            store,
            &memory,
            0,
            0,
            layout.align as i32,
            layout.size as i32,
            "indirect canonical parameter area",
        )? as u32;
        for ((value, ty), offset) in args
            .iter()
            .zip(func_type.params.iter())
            .zip(layout.offsets.iter().copied())
        {
            write_value_to_memory(value, ty, options, program, store, ptr + offset)?;
        }
        return Ok(vec![WasmValue::I32(ptr as i32)]);
    }
    let mut lowered = Vec::new();
    for (value, ty) in args.iter().zip(func_type.params.iter()) {
        lower_value(value, ty, options, program, store, &mut lowered)?;
    }
    Ok(lowered)
}

pub(super) fn lift_component_args(
    func_type: &FuncType,
    args: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    if function_params_flat_len(func_type, program)? > MAX_FLAT_PARAMS {
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let ptr = match args.first() {
            Some(WasmValue::I32(ptr)) => *ptr as u32,
            Some(other) => {
                return Err(ComponentError::Runtime(format!(
                    "indirect canonical parameter area must be an i32 pointer, got {other:?}"
                )))
            }
            None => {
                return Err(ComponentError::Runtime(
                    "indirect canonical parameter area is missing".to_owned(),
                ))
            }
        };
        let layout = value_area_layout(&func_type.params, program)?;
        return func_type
            .params
            .iter()
            .zip(layout.offsets.iter().copied())
            .map(|(ty, offset)| {
                read_value_from_memory(ty, options, program, store, &memory, ptr + offset)
            })
            .collect();
    }
    let mut cursor = CoreValueCursor::new(args);
    func_type
        .params
        .iter()
        .map(|ty| lift_value(ty, options, program, store, &mut cursor))
        .collect()
}

pub(super) fn lift_component_results(
    func_type: &FuncType,
    results: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        return Ok(Vec::new());
    };
    let mut cursor = CoreValueCursor::new(results);
    let result = if value_flat_len(result_ty, program)? > MAX_FLAT_RESULTS {
        let pointer = cursor.next_i32()? as u32;
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        read_value_from_memory(result_ty, options, program, store, &memory, pointer)?
    } else {
        lift_value(result_ty, options, program, store, &mut cursor)?
    };
    Ok(vec![result])
}

pub(super) fn lower_component_results(
    func_type: &FuncType,
    results: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    result_area: Option<u32>,
) -> Result<Vec<WasmValue>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        if results.is_empty() {
            return Ok(Vec::new());
        }
        return Err(ComponentError::InvalidArgument(
            "function does not return a value".to_owned(),
        ));
    };
    let value = results
        .first()
        .ok_or_else(|| ComponentError::InvalidArgument("function result is missing".to_owned()))?;
    let flat_len = value_flat_len(result_ty, program)?;
    if flat_len > MAX_FLAT_RESULTS {
        options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let return_ptr = result_area
            .ok_or_else(|| ComponentError::Runtime("indirect result area is missing".to_owned()))?;
        write_value_to_memory(value, result_ty, options, program, store, return_ptr)?;
        Ok(Vec::new())
    } else {
        lower_value_to_flat(value, result_ty, options, program, store)
    }
}

/// Records the handles of resources in a lowered function's result, so a
/// later `canon resource.drop` can release the host-provided ones.
///
/// Handles are recorded under the resource type the producing function
/// declared; see `SharedState::track_host_resource_handle` for why drop
/// cannot rely on that identity matching. Borrows are not owned by the
/// guest and are never recorded.
pub(super) fn track_host_owned_results(
    func_type: &FuncType,
    results: &[ComponentValue],
    program: &ComponentProgram,
    shared: &SharedState,
) {
    let (Some(result_ty), Some(value)) = (&func_type.result, results.first()) else {
        return;
    };
    track_host_owned_value(result_ty, value, program, shared);
}

fn track_host_owned_value(
    ty: &ValType,
    value: &ComponentValue,
    program: &ComponentProgram,
    shared: &SharedState,
) {
    let ValType::Type(type_id) = ty else {
        return;
    };
    track_host_owned_defined(*type_id, value, program, shared);
}

fn track_host_owned_defined(
    type_id: TypeId,
    value: &ComponentValue,
    program: &ComponentProgram,
    shared: &SharedState,
) {
    let Some(ty) = program.get_type(type_id) else {
        return;
    };
    match ty {
        Type::Resource(_) => {
            if let ComponentValue::Own(handle) = value {
                if let Ok(resource) = runtime_resource_id(program, shared, type_id) {
                    shared.track_host_resource_handle(resource, *handle);
                }
            }
        }
        Type::DefVal(def) => match def {
            DefValType::Own(_) => {
                if let ComponentValue::Own(handle) = value {
                    if let Ok(resource) = runtime_resource_id(program, shared, type_id) {
                        shared.track_host_resource_handle(resource, *handle);
                    }
                }
            }
            DefValType::Record(fields) => {
                let values: Vec<&ComponentValue> = match value {
                    ComponentValue::Tuple(values) if is_tuple_fields(fields) => {
                        values.iter().collect()
                    }
                    ComponentValue::Record(entries) => {
                        let mut values = Vec::with_capacity(fields.len());
                        for field in fields {
                            match entries.iter().find(|(name, _)| name == &field.label.0) {
                                Some((_, field_value)) => values.push(field_value),
                                None => return,
                            }
                        }
                        values
                    }
                    _ => return,
                };
                for (field, field_value) in fields.iter().zip(values) {
                    track_host_owned_value(&field.ty, field_value, program, shared);
                }
            }
            DefValType::List(elem, _) => {
                if let ComponentValue::List(items) = value {
                    for item in items {
                        track_host_owned_value(elem, item, program, shared);
                    }
                }
            }
            DefValType::Variant(cases) => {
                let (case, payload) = match value {
                    ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
                    ComponentValue::Enum(case) => (case.as_str(), None),
                    ComponentValue::Option(value) if is_option_cases(cases) => match value {
                        Some(inner) => ("some", Some(inner.as_ref())),
                        None => ("none", None),
                    },
                    ComponentValue::Result { ok, err } if is_result_cases(cases) => {
                        match (ok, err) {
                            (Some(inner), None) => ("ok", Some(inner.as_ref())),
                            (None, Some(inner)) => ("err", Some(inner.as_ref())),
                            _ => return,
                        }
                    }
                    _ => return,
                };
                if let Some(payload) = payload {
                    if let Some(case) = cases.iter().find(|c| c.label.0.as_str() == case) {
                        if let Some(case_ty) = &case.ty {
                            track_host_owned_value(case_ty, payload, program, shared);
                        }
                    }
                }
            }
            DefValType::Primitive(_) | DefValType::Flags(_) | DefValType::Borrow(_) => {}
        },
        Type::Generic(generic) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => {
                track_host_owned_defined(inner, value, program, shared);
            }
            crate::ir::types::GenericBound::Sub => {
                if let ComponentValue::Own(handle) = value {
                    if let Ok(resource) = runtime_resource_id(program, shared, type_id) {
                        shared.track_host_resource_handle(resource, *handle);
                    }
                }
            }
        },
        Type::Func(_) | Type::Component(_) | Type::Instance(_) => {}
    }
}

/// Collects the resource-handle leaves of a value that is crossing a
/// canonical boundary, as `(declared type, handle index, is_own)`
/// triples. The walk mirrors `track_host_owned_value`'s, except that
/// `borrow` leaves are collected as well as `own` leaves.
fn collect_handle_leaves(
    ty: &ValType,
    value: &ComponentValue,
    program: &ComponentProgram,
    leaves: &mut Vec<(TypeId, u32, bool)>,
) {
    let ValType::Type(type_id) = ty else {
        return;
    };
    collect_defined_handle_leaves(*type_id, value, program, leaves);
}

fn collect_defined_handle_leaves(
    type_id: TypeId,
    value: &ComponentValue,
    program: &ComponentProgram,
    leaves: &mut Vec<(TypeId, u32, bool)>,
) {
    let Some(ty) = program.get_type(type_id) else {
        return;
    };
    match ty {
        Type::Resource(_) => {
            if let ComponentValue::Own(handle) = value {
                leaves.push((type_id, *handle, true));
            }
        }
        Type::DefVal(def) => match def {
            DefValType::Own(_) => {
                if let ComponentValue::Own(handle) = value {
                    leaves.push((type_id, *handle, true));
                }
            }
            DefValType::Borrow(_) => {
                if let ComponentValue::Borrow(handle) = value {
                    leaves.push((type_id, *handle, false));
                }
            }
            DefValType::Record(fields) => {
                let values: Vec<&ComponentValue> = match value {
                    ComponentValue::Tuple(values) if is_tuple_fields(fields) => {
                        values.iter().collect()
                    }
                    ComponentValue::Record(entries) => {
                        let mut values = Vec::with_capacity(fields.len());
                        for field in fields {
                            match entries.iter().find(|(name, _)| name == &field.label.0) {
                                Some((_, field_value)) => values.push(field_value),
                                None => return,
                            }
                        }
                        values
                    }
                    _ => return,
                };
                for (field, field_value) in fields.iter().zip(values) {
                    collect_handle_leaves(&field.ty, field_value, program, leaves);
                }
            }
            DefValType::List(elem, _) => {
                if let ComponentValue::List(items) = value {
                    for item in items {
                        collect_handle_leaves(elem, item, program, leaves);
                    }
                }
            }
            DefValType::Variant(cases) => {
                let (case, payload) = match value {
                    ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
                    ComponentValue::Enum(case) => (case.as_str(), None),
                    ComponentValue::Option(value) if is_option_cases(cases) => match value {
                        Some(inner) => ("some", Some(inner.as_ref())),
                        None => ("none", None),
                    },
                    ComponentValue::Result { ok, err } if is_result_cases(cases) => {
                        match (ok, err) {
                            (Some(inner), None) => ("ok", Some(inner.as_ref())),
                            (None, Some(inner)) => ("err", Some(inner.as_ref())),
                            _ => return,
                        }
                    }
                    _ => return,
                };
                if let Some(payload) = payload {
                    if let Some(case) = cases.iter().find(|c| c.label.0.as_str() == case) {
                        if let Some(case_ty) = &case.ty {
                            collect_handle_leaves(case_ty, payload, program, leaves);
                        }
                    }
                }
            }
            DefValType::Primitive(_) | DefValType::Flags(_) => {}
        },
        Type::Generic(generic) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => {
                collect_defined_handle_leaves(inner, value, program, leaves)
            }
            crate::ir::types::GenericBound::Sub => {
                if let ComponentValue::Own(handle) = value {
                    leaves.push((type_id, *handle, true));
                }
            }
        },
        Type::Func(_) | Type::Component(_) | Type::Instance(_) => {}
    }
}

/// Checks one handle leaf crossing a canonical boundary (#335). An
/// index the instance's table holds for the declared resource type is
/// valid; a live host-provided handle, which is tracked in
/// `SharedState` rather than in any instance table, is valid
/// pass-through; any other index the table does not hold is the
/// canonical `unknown handle index N` trap, and an index held for a
/// different resource type is the canonical wrong-type trap.
/// Host-provided handles enter `SharedState` either as results of
/// lowered calls or as arguments of a public call, which the public
/// entry point registers before the call runs
/// (`register_host_argument_handles`).
///
/// Two shapes make an index held under a *different* identity than
/// the declared type legitimate, because a bare index does not say
/// which namespace produced it:
///
/// - the local table's allocator and the host allocator both start
///   at 1, so a live host-provided handle can collide with an
///   unrelated local entry; if `SharedState` tracks the index, the
///   value may be that host handle passing through, and trapping it
///   would reject a valid program;
/// - the decoder freshens every import declaration independently,
///   so one provider resource carries a distinct synthetic identity
///   along each import path of a consumer (the handle transfers pair
///   the two sides' types positionally for the same reason; see
///   `HandleTransfer::own`). An entry keyed under one import path
///   therefore cannot match another path's identity exactly.
///
/// The wrong-type trap is reserved for mismatches that involve a
/// resource *defined* in this program: those identities are the
/// definition sites themselves, so a mismatch there is unambiguous.
/// Two synthetic (imported) identities are not distinguished at this
/// boundary; the canon `resource.rep` / `resource.drop` operations
/// keep comparing their table entries exactly.
///
/// Borrow leaves are the one exception to the table discipline: for
/// a resource *defined* in this program, the value crossing the
/// boundary is the resource's representation, not an index (the
/// defining component's core code works with reps, exactly like the
/// handle transfers deliver reps to defining instances), so there is
/// no table entry to validate it against. `own` values are always
/// handle indices, locally defined or not, and keep the full check.
fn check_boundary_handle(
    program: &ComponentProgram,
    shared: &SharedState,
    handles: &RefCell<HandleTable>,
    type_id: TypeId,
    handle: u32,
    is_own: bool,
) -> Result<(), ComponentError> {
    let resource = runtime_resource_id(program, shared, type_id)?;
    if !is_own && resource_is_local_definition(resource) {
        return Ok(());
    }
    let table = handles.borrow();
    match table.resource_of(handle) {
        Some(entry_resource) if entry_resource == resource => Ok(()),
        Some(entry_resource) => {
            if shared.tracks_host_handle(handle)
                || (!resource_is_local_definition(entry_resource)
                    && !resource_is_local_definition(resource))
            {
                Ok(())
            } else {
                table.validate_handle(resource, handle)
            }
        }
        None => {
            if shared.tracks_host_handle(handle) {
                Ok(())
            } else {
                Err(ComponentError::Trap(format!(
                    "unknown handle index {handle}"
                )))
            }
        }
    }
}

/// Validates the handles in a canon-lifted function's results against
/// the lifted instance's own handle table (#335). The core code behind
/// a lift addresses that instance's table, so a result handle the
/// table does not hold — or holds for a different resource type — is a
/// canonical trap (`unknown handle index N` / the wrong-type text),
/// not a value the caller may use. Host-provided handles the guest is
/// passing through (a lowered host call's result returned upward) are
/// the one exception: they live in `SharedState`, not in the table.
pub(super) fn validate_lifted_handle_results(
    func_type: &FuncType,
    results: &[ComponentValue],
    program: &ComponentProgram,
    shared: &SharedState,
    handles: &RefCell<HandleTable>,
) -> Result<(), ComponentError> {
    let (Some(result_ty), Some(value)) = (&func_type.result, results.first()) else {
        return Ok(());
    };
    let mut leaves = Vec::new();
    collect_handle_leaves(result_ty, value, program, &mut leaves);
    for (type_id, handle, is_own) in leaves {
        check_boundary_handle(program, shared, handles, type_id, handle, is_own)?;
    }
    Ok(())
}

/// Validates the `borrow` handles among a canon-lowered function's
/// arguments against the caller's handle table (#335). A borrow names a
/// handle the caller owns: an index its table holds for a different
/// resource type traps the canonical wrong-type text, and an index no
/// table holds traps `unknown handle index N` instead of reaching the
/// callee. Host-provided handles are the one exception: they are
/// tracked in `SharedState`, not in the caller's table, and a borrow of
/// a live one is valid. A borrow of a resource the caller itself
/// defines is not an index either: the caller's core code passes the
/// representation, which the table cannot judge (see
/// `check_boundary_handle`). `own` arguments are deliberately not checked
/// here: moving ownership across the boundary is the handle-transfer
/// mechanism (#342), which this validation must not pre-empt.
pub(super) fn validate_lowered_borrow_args(
    func_type: &FuncType,
    args: &[ComponentValue],
    program: &ComponentProgram,
    shared: &SharedState,
    handles: &RefCell<HandleTable>,
) -> Result<(), ComponentError> {
    for (value, ty) in args.iter().zip(func_type.params.iter()) {
        let mut leaves = Vec::new();
        collect_handle_leaves(ty, value, program, &mut leaves);
        for (type_id, handle, is_own) in leaves {
            if is_own {
                continue;
            }
            check_boundary_handle(program, shared, handles, type_id, handle, false)?;
        }
    }
    Ok(())
}

/// Registers the handle leaves of a public call's arguments in
/// `SharedState` before the call runs. Handles an embedder passes
/// directly to a public call live in the host namespace, like handles
/// returned from lowered host calls, but no lowered call ever sees
/// them, so the boundary validation would judge them unknown. `own`
/// leaves are recorded as host-provided handles the guest now owns,
/// exactly like lowered-call results (`track_host_owned_results`);
/// `borrow` leaves are registered for the duration of this call only,
/// and the caller releases them when the call returns (the returned
/// indices). Indices a guest invents never pass through this boundary
/// and stay unregistered, so the validation keeps trapping them.
pub(super) fn register_host_argument_handles(
    func_type: &FuncType,
    args: &[ComponentValue],
    program: &ComponentProgram,
    shared: &SharedState,
) -> Vec<u32> {
    let mut borrows = Vec::new();
    for (value, ty) in args.iter().zip(func_type.params.iter()) {
        let mut leaves = Vec::new();
        collect_handle_leaves(ty, value, program, &mut leaves);
        for (type_id, handle, is_own) in leaves {
            if is_own {
                if let Ok(resource) = runtime_resource_id(program, shared, type_id) {
                    shared.track_host_resource_handle(resource, handle);
                }
            } else {
                shared.track_host_borrow(handle);
                borrows.push(handle);
            }
        }
    }
    borrows
}

/// Transfers resource handles in `values` between two component
/// instances' handle tables at a call boundary (#342).
///
/// `values` are component values whose handle indices are expressed in
/// the source instance's table; on return, every handle the source
/// table actually holds has been rewritten to the destination table's
/// index for the same resource:
///
/// - an `own` handle moves: its entry leaves the source table (freeing
///   the index for LIFO reuse) and is allocated in the destination
///   table under the destination side's resource identity, keeping its
///   rep and its defining instance's destructor;
/// - a `borrow` handle is registered in the destination table as a
///   borrowed entry for the same resource, except that the component
///   defining a resource works with reps in its core code: a borrow
///   crossing into the defining instance is resolved to the rep, and a
///   borrowed value the defining instance produced by lifting (a call
///   result, marked by `src_reps`) already is a rep and is registered
///   in the destination table keyed by it.
///
/// Handles the source table does not hold (host-provided resources,
/// which are tracked in `SharedState` instead) pass through unchanged.
/// Borrow entries created in the destination table are recorded in
/// `lent` when it is `Some` (the argument direction) so the caller can
/// retire them when the call returns; result registrations belong to
/// the receiving instance and are not recorded.
///
/// The walk is fallible for one reason: moving an `own` removes its
/// entry from the source table, so an own whose borrows are still lent
/// to a call in flight cannot be transferred (#334) — the destination
/// would receive it with a zero borrow count and could destroy the
/// resource while the original borrow is outstanding. Such a transfer
/// is rejected with the canonical borrowed-removal trap, before the
/// entry is taken.
pub(super) fn transfer_handles(
    values: &mut [ComponentValue],
    src_tys: &[ValType],
    dst_tys: &[ValType],
    transfer: &mut HandleTransfer<'_>,
) -> Result<(), ComponentError> {
    // First pass: resolve every borrowed resource's record before
    // any own moves its entry out of the source table. A call that
    // passes the same resource as `(own R, borrow R)` would otherwise
    // lose the borrow when the own is processed first: the source
    // entry is already gone when the borrow is looked up, so its
    // identity is never reported and the destination can destroy the
    // resource while it is borrowed. The protection must not depend
    // on parameter or field order (#334).
    transfer.prewalk = true;
    for (value, (src_ty, dst_ty)) in values.iter_mut().zip(src_tys.iter().zip(dst_tys.iter())) {
        transfer.value(value, src_ty, dst_ty)?;
    }
    transfer.prewalk = false;
    for (value, (src_ty, dst_ty)) in values.iter_mut().zip(src_tys.iter().zip(dst_tys.iter())) {
        transfer.value(value, src_ty, dst_ty)?;
    }
    Ok(())
}

/// Removes borrow entries that [`transfer_handles`] registered for one
/// call's arguments; entries the callee already dropped are absent and
/// skipped.
pub(super) fn retire_lent_handles(handles: &RefCell<HandleTable>, lent: &[u32]) {
    if lent.is_empty() {
        return;
    }
    let mut table = handles.borrow_mut();
    for handle in lent {
        table.remove_entry(*handle);
    }
}

pub(super) struct HandleTransfer<'a> {
    pub(super) program: &'a ComponentProgram,
    pub(super) shared: &'a SharedState,
    pub(super) src: &'a RefCell<HandleTable>,
    pub(super) dst: &'a RefCell<HandleTable>,
    pub(super) lent: Option<&'a mut Vec<u32>>,
    /// When `Some` (the argument direction), receives the destination
    /// resource identity and the resource identity (`ResourceRecord::id`)
    /// of every borrow the walk carries across, so the caller can
    /// register each borrow against the very owned entry it belongs
    /// to in the destination table (#334). The rewritten borrow value
    /// alone — a rep or a registration index — cannot identify that
    /// entry: reps are guest-chosen and may collide across resources.
    pub(super) borrowed: Option<&'a mut Vec<(ResourceId, u64)>>,
    /// Whether borrowed values coming from the source were produced by
    /// lifting in the source component, in which case a borrow of a
    /// resource the source defines already holds the resource's rep
    /// rather than a source-table handle.
    pub(super) src_reps: bool,
    /// Whether the walk is in its first pass (see
    /// [`transfer_handles`]): owns do not move and borrows only
    /// resolve their records in this pass.
    pub(super) prewalk: bool,
    /// The record each borrow position resolved to during the first
    /// pass, in walk order (`None` where the borrow passes through
    /// untransferred); the second pass consumes them in the same
    /// order, so a borrow whose source entry an earlier own in the
    /// same call already moved still resolves to its record.
    pub(super) resolved_borrows: Vec<Option<ResourceRecord>>,
    /// How many entries of `resolved_borrows` the second pass has
    /// consumed.
    pub(super) resolved_cursor: usize,
}

impl HandleTransfer<'_> {
    fn value(
        &mut self,
        value: &mut ComponentValue,
        src_ty: &ValType,
        dst_ty: &ValType,
    ) -> Result<(), ComponentError> {
        let (ValType::Type(src_id), ValType::Type(dst_id)) = (src_ty, dst_ty) else {
            return Ok(());
        };
        self.defined(value, *src_id, *dst_id)
    }

    fn defined(
        &mut self,
        value: &mut ComponentValue,
        src_id: TypeId,
        dst_id: TypeId,
    ) -> Result<(), ComponentError> {
        let (Some(src_ty), Some(dst_ty)) =
            (self.program.get_type(src_id), self.program.get_type(dst_id))
        else {
            return Ok(());
        };
        match (src_ty, dst_ty) {
            (Type::Resource(_), Type::Resource(_)) => self.own(value, src_id, dst_id),
            (Type::DefVal(src_def), Type::DefVal(dst_def)) => {
                self.defined_value(value, src_def, dst_def, src_id, dst_id)
            }
            (Type::Generic(src_generic), Type::Generic(dst_generic)) => {
                match (&src_generic.bound, &dst_generic.bound) {
                    (
                        crate::ir::types::GenericBound::Eq(src_inner),
                        crate::ir::types::GenericBound::Eq(dst_inner),
                    ) => self.defined(value, *src_inner, *dst_inner),
                    (crate::ir::types::GenericBound::Sub, crate::ir::types::GenericBound::Sub) => {
                        self.own(value, src_id, dst_id)
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn defined_value(
        &mut self,
        value: &mut ComponentValue,
        src_def: &DefValType,
        dst_def: &DefValType,
        src_id: TypeId,
        dst_id: TypeId,
    ) -> Result<(), ComponentError> {
        match (src_def, dst_def) {
            (DefValType::Own(_), DefValType::Own(_)) => self.own(value, src_id, dst_id),
            (DefValType::Borrow(_), DefValType::Borrow(_)) => self.borrow(value, src_id, dst_id),
            (DefValType::Record(src_fields), DefValType::Record(dst_fields)) => {
                match value {
                    ComponentValue::Tuple(values) if is_tuple_fields(src_fields) => {
                        for ((value, src_field), dst_field) in values
                            .iter_mut()
                            .zip(src_fields.iter())
                            .zip(dst_fields.iter())
                        {
                            self.value(value, &src_field.ty, &dst_field.ty)?;
                        }
                    }
                    ComponentValue::Record(entries) => {
                        for (src_field, dst_field) in src_fields.iter().zip(dst_fields.iter()) {
                            if let Some((_, field_value)) = entries
                                .iter_mut()
                                .find(|(name, _)| name == &src_field.label.0)
                            {
                                self.value(field_value, &src_field.ty, &dst_field.ty)?;
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            (DefValType::List(src_elem, _), DefValType::List(dst_elem, _)) => {
                if let ComponentValue::List(items) = value {
                    for item in items.iter_mut() {
                        self.value(item, src_elem, dst_elem)?;
                    }
                }
                Ok(())
            }
            (DefValType::Variant(src_cases), DefValType::Variant(dst_cases)) => {
                let (case, payload) = match value {
                    ComponentValue::Variant { case, value } => (case.clone(), value.as_deref_mut()),
                    ComponentValue::Enum(case) => (case.clone(), None),
                    ComponentValue::Option(inner) if is_option_cases(src_cases) => match inner {
                        Some(inner) => ("some".to_owned(), Some(inner.as_mut())),
                        None => ("none".to_owned(), None),
                    },
                    ComponentValue::Result { ok, err } if is_result_cases(src_cases) => {
                        match (ok.as_deref_mut(), err.as_deref_mut()) {
                            (Some(inner), None) => ("ok".to_owned(), Some(inner)),
                            (None, Some(inner)) => ("err".to_owned(), Some(inner)),
                            _ => return Ok(()),
                        }
                    }
                    _ => return Ok(()),
                };
                if let Some(payload) = payload {
                    let src_case = src_cases.iter().find(|c| c.label.0.as_str() == case);
                    let dst_case = dst_cases.iter().find(|c| c.label.0.as_str() == case);
                    if let (Some(src_case), Some(dst_case)) = (src_case, dst_case) {
                        if let (Some(src_ty), Some(dst_ty)) = (&src_case.ty, &dst_case.ty) {
                            self.value(payload, src_ty, dst_ty)?;
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Moves an owned handle's entry from the source table to the
    /// destination table, re-keying it to the destination side's
    /// resource identity. The pairing is positional (the two function
    /// types were checked compatible), so the entry's stored identity
    /// is not consulted: one resource legitimately carries different
    /// synthetic identities along different import paths of a single
    /// consumer.
    fn own(
        &mut self,
        value: &mut ComponentValue,
        _src_id: TypeId,
        dst_id: TypeId,
    ) -> Result<(), ComponentError> {
        if self.prewalk {
            return Ok(());
        }
        let ComponentValue::Own(handle) = value else {
            return Ok(());
        };
        let Ok(dst_resource) = runtime_resource_id(self.program, self.shared, dst_id) else {
            return Ok(());
        };
        // Moving an own removes its entry from the source table, so an
        // own whose borrows are still lent to a call in flight cannot
        // be transferred either: the destination would receive the
        // entry with its borrow count reset to zero and could destroy
        // the resource while the original borrow is outstanding
        // (#334). Reject before the entry is taken.
        if self.src.borrow().has_outstanding_borrows(*handle) {
            return Err(ComponentError::Trap(
                "cannot remove owned resource while borrowed".to_owned(),
            ));
        }
        let Some(record) = self.src.borrow_mut().take_entry(*handle) else {
            return Ok(());
        };
        let new_handle = self
            .dst
            .borrow_mut()
            .insert_entry(dst_resource, record, false);
        *value = ComponentValue::Own(new_handle);
        Ok(())
    }

    fn borrow(
        &mut self,
        value: &mut ComponentValue,
        src_id: TypeId,
        dst_id: TypeId,
    ) -> Result<(), ComponentError> {
        if self.prewalk {
            // First pass: resolve the borrowed resource's record and
            // report its identity, without moving or rewriting
            // anything. The record is kept for the second pass, which
            // can no longer look it up once an own earlier in the
            // same call has moved the source entry (#334).
            let record = self.lookup_borrow_record(value, src_id, dst_id);
            if let Some(record) = &record {
                if let Some(borrowed) = &mut self.borrowed {
                    if let Ok(dst_resource) = runtime_resource_id(self.program, self.shared, dst_id)
                    {
                        borrowed.push((dst_resource, record.id));
                    }
                }
            }
            self.resolved_borrows.push(record);
            return Ok(());
        }
        // Second pass: consume the record the first pass resolved
        // for this borrow position. Every borrow position consumes
        // exactly one entry, whatever value it holds, so the two
        // passes stay aligned; a position that resolved to nothing
        // (including a non-`Borrow` representation, which the
        // transfer leaves alone) passes through unchanged.
        let record = self
            .resolved_borrows
            .get(self.resolved_cursor)
            .cloned()
            .flatten();
        self.resolved_cursor += 1;
        if !matches!(value, ComponentValue::Borrow(_)) {
            return Ok(());
        }
        let Some(record) = record else {
            return Ok(());
        };
        let Ok(dst_resource) = runtime_resource_id(self.program, self.shared, dst_id) else {
            return Ok(());
        };
        // The component defining the resource receives the rep itself;
        // any other destination gets a borrowed entry in its own table.
        if resource_is_local_definition(dst_resource) {
            *value = ComponentValue::Borrow(record.rep as u32);
            return Ok(());
        }
        let new_handle = self
            .dst
            .borrow_mut()
            .insert_entry(dst_resource, record, true);
        if let Some(lent) = &mut self.lent {
            lent.push(new_handle);
        }
        *value = ComponentValue::Borrow(new_handle);
        Ok(())
    }

    /// Resolves the record a borrowed value names, exactly as the
    /// transfer's second pass needs it. A value the source produced
    /// by lifting, for a resource the source defines, already holds
    /// the rep; the record (and with it the resource identity) is
    /// recovered from the source table by that rep, falling back to a
    /// bare record when the source table holds no such entry. Every
    /// other value is a handle in the source table. `None` covers
    /// everything the transfer leaves alone: a non-handle value, an
    /// unresolvable resource identity, and a handle the source table
    /// does not hold (a host-provided resource, which passes through
    /// unchanged).
    fn lookup_borrow_record(
        &self,
        value: &ComponentValue,
        src_id: TypeId,
        dst_id: TypeId,
    ) -> Option<ResourceRecord> {
        let ComponentValue::Borrow(handle) = value else {
            return None;
        };
        let (Ok(src_resource), Ok(_)) = (
            runtime_resource_id(self.program, self.shared, src_id),
            runtime_resource_id(self.program, self.shared, dst_id),
        ) else {
            return None;
        };
        if self.src_reps && resource_is_local_definition(src_resource) {
            match self
                .src
                .borrow()
                .record_by_rep(src_resource, *handle as i32)
            {
                Some(record) => Some(record),
                None => Some(ResourceRecord {
                    id: next_resource_id(),
                    rep: *handle as i32,
                    destructor: None,
                }),
            }
        } else {
            self.src.borrow().record_of(*handle)
        }
    }
}

/// Whether `resource` identifies a resource type *defined* by a
/// component in this program. Defined resources carry the defining
/// scope's id, whose depth is the component's nesting depth; types
/// that merely name another component's resource (imported
/// `(sub resource)` types, freshened import types, generic resources)
/// carry a synthetic identity minted by the decoder, whose scope
/// depth is `u32::MAX` (see `ScopeId::synthetic`).
pub(super) fn resource_is_local_definition(resource: ResourceId) -> bool {
    resource.owner().depth() != u32::MAX
}

/// Collects every `(resource, handle)` pair lent as a `borrow`
/// anywhere in a call's arguments, so the callee can register the
/// borrows against its handle table for the duration of the call
/// (#334). Owns transfer ownership and are not collected. The traversal
/// mirrors `track_host_owned_value`, with the opposite polarity: only
/// `borrow` positions are visited.
pub(super) fn collect_borrowed_handles(
    func_type: &FuncType,
    args: &[ComponentValue],
    program: &ComponentProgram,
    shared: &SharedState,
) -> Vec<(ResourceId, u32)> {
    let mut borrows = Vec::new();
    for (ty, value) in func_type.params.iter().zip(args.iter()) {
        collect_borrowed_value(ty, value, program, shared, &mut borrows);
    }
    borrows
}

fn collect_borrowed_value(
    ty: &ValType,
    value: &ComponentValue,
    program: &ComponentProgram,
    shared: &SharedState,
    borrows: &mut Vec<(ResourceId, u32)>,
) {
    let ValType::Type(type_id) = ty else {
        return;
    };
    collect_borrowed_defined(*type_id, value, program, shared, borrows);
}

fn collect_borrowed_defined(
    type_id: TypeId,
    value: &ComponentValue,
    program: &ComponentProgram,
    shared: &SharedState,
    borrows: &mut Vec<(ResourceId, u32)>,
) {
    let Some(ty) = program.get_type(type_id) else {
        return;
    };
    match ty {
        Type::DefVal(def) => match def {
            DefValType::Borrow(_) => {
                // The handle extraction mirrors lowering's
                // `expect_handle`: `Own` and `U32` values in a borrow
                // position are accepted there and reach the callee as
                // the same handle integer, so their borrows must be
                // collected too (otherwise the borrowed resource is
                // left unregistered and its drop goes unchecked).
                if let Ok(handle) = expect_handle(value) {
                    if let Ok(resource) = runtime_resource_id(program, shared, type_id) {
                        borrows.push((resource, handle));
                    }
                }
            }
            DefValType::Record(fields) => {
                let values: Vec<&ComponentValue> = match value {
                    ComponentValue::Tuple(values) if is_tuple_fields(fields) => {
                        values.iter().collect()
                    }
                    ComponentValue::Record(entries) => {
                        let mut values = Vec::with_capacity(fields.len());
                        for field in fields {
                            match entries.iter().find(|(name, _)| name == &field.label.0) {
                                Some((_, field_value)) => values.push(field_value),
                                None => return,
                            }
                        }
                        values
                    }
                    _ => return,
                };
                for (field, field_value) in fields.iter().zip(values) {
                    collect_borrowed_value(&field.ty, field_value, program, shared, borrows);
                }
            }
            DefValType::List(elem, _) => {
                if let ComponentValue::List(items) = value {
                    for item in items {
                        collect_borrowed_value(elem, item, program, shared, borrows);
                    }
                }
            }
            DefValType::Variant(cases) => {
                let (case, payload) = match value {
                    ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
                    ComponentValue::Enum(case) => (case.as_str(), None),
                    ComponentValue::Option(value) if is_option_cases(cases) => match value {
                        Some(inner) => ("some", Some(inner.as_ref())),
                        None => ("none", None),
                    },
                    ComponentValue::Result { ok, err } if is_result_cases(cases) => {
                        match (ok, err) {
                            (Some(inner), None) => ("ok", Some(inner.as_ref())),
                            (None, Some(inner)) => ("err", Some(inner.as_ref())),
                            _ => return,
                        }
                    }
                    _ => return,
                };
                if let Some(payload) = payload {
                    if let Some(case) = cases.iter().find(|c| c.label.0.as_str() == case) {
                        if let Some(case_ty) = &case.ty {
                            collect_borrowed_value(case_ty, payload, program, shared, borrows);
                        }
                    }
                }
            }
            DefValType::Primitive(_) | DefValType::Flags(_) | DefValType::Own(_) => {}
        },
        Type::Generic(generic) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => {
                collect_borrowed_defined(inner, value, program, shared, borrows);
            }
            crate::ir::types::GenericBound::Sub => {}
        },
        Type::Resource(_) | Type::Func(_) | Type::Component(_) | Type::Instance(_) => {}
    }
}

pub(super) fn lowered_indirect_result_area(
    func_type: &FuncType,
    args: &[WasmValue],
    program: &ComponentProgram,
) -> Result<Option<u32>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        return Ok(None);
    };
    if value_flat_len(result_ty, program)? <= MAX_FLAT_RESULTS {
        return Ok(None);
    }
    match args.last() {
        Some(WasmValue::I32(ptr)) => Ok(Some(*ptr as u32)),
        Some(other) => Err(ComponentError::Runtime(format!(
            "indirect result area must be an i32 pointer, got {other:?}"
        ))),
        None => Err(ComponentError::Runtime(
            "indirect result area is missing".to_owned(),
        )),
    }
}

fn lower_value(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    out: &mut Vec<WasmValue>,
) -> Result<(), ComponentError> {
    out.extend(lower_value_to_flat(value, ty, options, program, store)?);
    Ok(())
}

fn lower_value_to_flat(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lower_primitive(value, prim, options, store, program),
        ValType::Type(type_id) => lower_defined_value(value, *type_id, options, program, store),
    }
}

#[cfg(feature = "component-gated-feature-async")]
fn unsupported_error_context() -> ComponentError {
    ComponentError::Unsupported("canonical ABI for `error-context` is not supported".to_owned())
}

fn lower_primitive(
    value: &ComponentValue,
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &Store,
    program: &ComponentProgram,
) -> Result<Vec<WasmValue>, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => vec![WasmValue::I32(i32::from(expect_bool(value)?))],
        PrimValType::S8 => vec![WasmValue::I32(expect_i32(value)? as i8 as i32)],
        PrimValType::U8 => vec![WasmValue::I32(expect_u32(value)? as u8 as i32)],
        PrimValType::S16 => vec![WasmValue::I32(expect_i32(value)? as i16 as i32)],
        PrimValType::U16 => vec![WasmValue::I32(expect_u32(value)? as u16 as i32)],
        PrimValType::S32 => vec![WasmValue::I32(expect_i32(value)?)],
        PrimValType::U32 => vec![WasmValue::I32(expect_u32(value)? as i32)],
        PrimValType::S64 => vec![WasmValue::I64(expect_i64(value)?)],
        PrimValType::U64 => vec![WasmValue::I64(expect_u64(value)? as i64)],
        PrimValType::F32 => vec![WasmValue::F32(expect_f32(value)?)],
        PrimValType::F64 => vec![WasmValue::F64(expect_f64(value)?)],
        PrimValType::Char => vec![WasmValue::I32(expect_char(value)? as u32 as i32)],
        PrimValType::String => lower_string(value, options, program, store)?,
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => return Err(unsupported_error_context()),
    })
}

fn lower_defined_value(
    value: &ComponentValue,
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            lower_primitive(value, prim, options, store, program)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            lower_record_value(value, fields, options, program, store)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            lower_variant_value(value, cases, options, program, store)
        }
        Type::DefVal(DefValType::Flags(labels)) => lower_flags_value(value, labels),
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            lower_list_value(value, elem, *maybe_len, options, program, store)
        }
        Type::DefVal(DefValType::Own(resource)) | Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(vec![WasmValue::I32(expect_handle(value)? as i32)])
        }
        Type::Resource(_resource) => Ok(vec![WasmValue::I32(expect_handle(value)? as i32)]),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_value(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lift_primitive(prim, options, store, cursor),
        ValType::Type(type_id) => lift_defined_value(*type_id, options, program, store, cursor),
    }
}

fn lift_defined_value(
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => lift_primitive(prim, options, store, cursor),
        Type::DefVal(DefValType::Record(fields)) => {
            lift_record_value(fields, options, program, store, cursor)
        }
        Type::DefVal(DefValType::Variant(variant)) => {
            lift_variant_value(variant, options, program, store, cursor)
        }
        Type::DefVal(DefValType::Flags(labels)) => lift_flags_value(labels, cursor),
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            lift_list_value(elem, *maybe_len, options, program, store, cursor)
        }
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Own(cursor.next_i32()? as u32))
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Borrow(cursor.next_i32()? as u32))
        }
        Type::Resource(_resource) => Ok(ComponentValue::Own(cursor.next_i32()? as u32)),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_primitive(
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => ComponentValue::Bool(cursor.next_i32()? != 0),
        PrimValType::S8 => ComponentValue::S8(cursor.next_i32()? as i8),
        PrimValType::U8 => ComponentValue::U8(cursor.next_i32()? as u8),
        PrimValType::S16 => ComponentValue::S16(cursor.next_i32()? as i16),
        PrimValType::U16 => ComponentValue::U16(cursor.next_i32()? as u16),
        PrimValType::S32 => ComponentValue::S32(cursor.next_i32()?),
        PrimValType::U32 => ComponentValue::U32(cursor.next_i32()? as u32),
        PrimValType::S64 => ComponentValue::S64(cursor.next_i64()?),
        PrimValType::U64 => ComponentValue::U64(cursor.next_i64()? as u64),
        PrimValType::F32 => ComponentValue::F32(cursor.next_f32()?),
        PrimValType::F64 => ComponentValue::F64(cursor.next_f64()?),
        PrimValType::Char => {
            ComponentValue::Char(char_from_canonical_bits(cursor.next_i32()? as u32)?)
        }
        PrimValType::String => {
            let memory = options.memory.as_ref().ok_or_else(|| {
                ComponentError::Runtime("canonical option `memory` is required".to_owned())
            })?;
            let ptr = cursor.next_i32()? as u32;
            let len = cursor.next_i32()? as u32;
            ComponentValue::String(read_string_from_memory(
                store,
                memory,
                ptr,
                len,
                options.string_encoding,
            )?)
        }
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => return Err(unsupported_error_context()),
    })
}

fn lower_string(
    value: &ComponentValue,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let string = value
        .as_str()
        .ok_or_else(|| ComponentError::InvalidArgument("expected string value".to_owned()))?;
    let encoding = options
        .string_encoding
        .unwrap_or(CanonicalStringEncoding::Utf8);
    match encoding {
        CanonicalStringEncoding::Utf8 => {
            let bytes = string.as_bytes();
            let ptr = call_realloc(
                realloc,
                store,
                &memory,
                0,
                0,
                1,
                bytes.len() as i32,
                "UTF-8 string",
            )? as u32;
            write_memory(store, &memory, ptr, bytes)?;
            Ok(vec![
                WasmValue::I32(ptr as i32),
                WasmValue::I32(bytes.len() as i32),
            ])
        }
        CanonicalStringEncoding::Utf16 => {
            let bytes = encode_string_utf16(string);
            let ptr = call_realloc(
                realloc,
                store,
                &memory,
                0,
                0,
                2,
                bytes.len() as i32,
                "UTF-16 string",
            )? as u32;
            write_memory(store, &memory, ptr, &bytes)?;
            Ok(vec![
                WasmValue::I32(ptr as i32),
                WasmValue::I32((bytes.len() / 2) as i32),
            ])
        }
        CanonicalStringEncoding::CompactUtf16 => {
            lower_compact_utf16_string(string, &memory, realloc, store)
        }
    }
}

const COMPACT_UTF16_TAG: u32 = 0x8000_0000;

// Canonical ABI `store_string_to_latin1_or_utf16`:
// https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
fn lower_compact_utf16_string(
    string: &str,
    memory: &CoreExportRef,
    realloc: &RuntimeCoreFunc,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let src_code_units = string.len();
    let mut ptr = call_realloc(
        realloc,
        store,
        memory,
        0,
        0,
        2,
        src_code_units as i32,
        "latin1+utf16 string",
    )? as u32;

    let mut dst_len = 0usize;
    for character in string.chars() {
        if (character as u32) >= 0x100 {
            let worst_case_len = 2 * src_code_units;
            ptr = call_realloc(
                realloc,
                store,
                memory,
                ptr as i32,
                src_code_units as i32,
                2,
                worst_case_len as i32,
                "latin1+utf16 string",
            )? as u32;

            let encoded = encode_string_utf16(string);
            write_memory(store, memory, ptr, &encoded)?;
            if worst_case_len > encoded.len() {
                ptr = call_realloc(
                    realloc,
                    store,
                    memory,
                    ptr as i32,
                    worst_case_len as i32,
                    2,
                    encoded.len() as i32,
                    "latin1+utf16 string",
                )? as u32;
            }
            let tagged_len = ((encoded.len() / 2) as u32) | COMPACT_UTF16_TAG;
            return Ok(vec![
                WasmValue::I32(ptr as i32),
                WasmValue::I32(tagged_len as i32),
            ]);
        }

        write_memory(store, memory, ptr + dst_len as u32, &[character as u8])?;
        dst_len += 1;
    }

    if dst_len < src_code_units {
        ptr = call_realloc(
            realloc,
            store,
            memory,
            ptr as i32,
            src_code_units as i32,
            2,
            dst_len as i32,
            "latin1+utf16 string",
        )? as u32;
    }
    Ok(vec![
        WasmValue::I32(ptr as i32),
        WasmValue::I32(dst_len as i32),
    ])
}

fn encode_string_utf16(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

#[derive(Clone, Copy)]
struct MemoryAbiInfo {
    size: u32,
    align: u32,
}

struct ValueAreaLayout {
    offsets: Vec<u32>,
    size: u32,
    align: u32,
}

fn variant_discriminant_size(case_count: usize) -> Result<u32, ComponentError> {
    if case_count <= (1 << 8) {
        Ok(1)
    } else if case_count <= (1 << 16) {
        Ok(2)
    } else if (case_count as u64) <= (1 << 32) {
        Ok(4)
    } else {
        Err(ComponentError::Unsupported(
            "variants with more than 2^32 cases are not supported".to_owned(),
        ))
    }
}

fn flags_memory_abi(count: usize) -> MemoryAbiInfo {
    if count == 0 {
        MemoryAbiInfo { size: 0, align: 1 }
    } else if count <= 8 {
        MemoryAbiInfo { size: 1, align: 1 }
    } else if count <= 16 {
        MemoryAbiInfo { size: 2, align: 2 }
    } else {
        MemoryAbiInfo {
            size: 4 * count.div_ceil(32) as u32,
            align: 4,
        }
    }
}

fn memory_abi_for_primitive(prim: &PrimValType) -> MemoryAbiInfo {
    match prim {
        PrimValType::Bool | PrimValType::S8 | PrimValType::U8 => {
            MemoryAbiInfo { size: 1, align: 1 }
        }
        PrimValType::S16 | PrimValType::U16 => MemoryAbiInfo { size: 2, align: 2 },
        PrimValType::S32 | PrimValType::U32 | PrimValType::Char | PrimValType::F32 => {
            MemoryAbiInfo { size: 4, align: 4 }
        }
        PrimValType::S64 | PrimValType::U64 | PrimValType::F64 => {
            MemoryAbiInfo { size: 8, align: 8 }
        }
        PrimValType::String => MemoryAbiInfo { size: 8, align: 4 },
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => MemoryAbiInfo { size: 4, align: 4 },
    }
}

fn memory_abi_for_type(
    type_id: TypeId,
    program: &ComponentProgram,
) -> Result<MemoryAbiInfo, ComponentError> {
    program
        .get_type_info(type_id)
        .map(|info| MemoryAbiInfo {
            size: info.indirect_size,
            align: info.indirect_align.max(1),
        })
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))
}

fn memory_abi_for_valtype(
    ty: &ValType,
    program: &ComponentProgram,
) -> Result<MemoryAbiInfo, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(memory_abi_for_primitive(prim)),
        ValType::Type(type_id) => memory_abi_for_type(*type_id, program),
    }
}

fn value_area_layout(
    values: &[ValType],
    program: &ComponentProgram,
) -> Result<ValueAreaLayout, ComponentError> {
    let mut offsets = Vec::with_capacity(values.len());
    let mut cursor = 0u32;
    let mut max_align = 1u32;
    for ty in values {
        let abi = memory_abi_for_valtype(ty, program)?;
        max_align = max_align.max(abi.align.max(1));
        cursor = align_to(cursor, abi.align.max(1));
        offsets.push(cursor);
        cursor = cursor.saturating_add(abi.size);
    }
    Ok(ValueAreaLayout {
        offsets,
        size: align_to(cursor, max_align),
        align: max_align,
    })
}

fn element_stride(elem: &ValType, program: &ComponentProgram) -> Result<u32, ComponentError> {
    let abi = memory_abi_for_valtype(elem, program)?;
    Ok(align_to(abi.size, abi.align.max(1)))
}

fn function_params_flat_len(
    func_type: &FuncType,
    program: &ComponentProgram,
) -> Result<usize, ComponentError> {
    func_type
        .params
        .iter()
        .try_fold(0usize, |len, ty| Ok(len + value_flat_len(ty, program)?))
}

fn flat_types_for_valtype(
    ty: &ValType,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(flat_types_for_primitive(prim)),
        ValType::Type(type_id) => flat_types_for_type(*type_id, program),
    }
}

fn flat_types_for_type(
    type_id: TypeId,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(def) => flat_types_for_defval(def, program),
        Type::Resource(_) | Type::Generic(_) => Ok(vec![CoreValType::I32]),
        _ => Err(ComponentError::Unsupported(
            "flat types for this component type are not implemented yet".to_owned(),
        )),
    }
}

fn flat_types_for_defval(
    def: &DefValType,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    Ok(match def {
        DefValType::Primitive(prim) => flat_types_for_primitive(prim),
        DefValType::Record(fields) => {
            let mut types = Vec::new();
            for field in fields {
                types.extend(flat_types_for_valtype(&field.ty, program)?);
            }
            types
        }
        DefValType::Variant(cases) => {
            let mut payload = Vec::new();
            for case in cases.iter() {
                if let Some(ty) = &case.ty {
                    for (index, flat_ty) in
                        flat_types_for_valtype(ty, program)?.into_iter().enumerate()
                    {
                        if let Some(current) = payload.get_mut(index) {
                            *current = join_component_flat_types(*current, flat_ty);
                        } else {
                            payload.push(flat_ty);
                        }
                    }
                }
            }
            let mut types = Vec::with_capacity(1 + payload.len());
            types.push(CoreValType::I32);
            types.extend(payload);
            types
        }
        DefValType::Flags(labels) => {
            let flat_len = if labels.is_empty() {
                0
            } else if labels.len() <= 16 {
                1
            } else {
                labels.len().div_ceil(32)
            };
            vec![CoreValType::I32; flat_len]
        }
        DefValType::List(_, _) => vec![CoreValType::I32, CoreValType::I32],
        DefValType::Own(_) | DefValType::Borrow(_) => vec![CoreValType::I32],
    })
}

fn flat_types_for_primitive(prim: &PrimValType) -> Vec<CoreValType> {
    match prim {
        PrimValType::Bool
        | PrimValType::S8
        | PrimValType::U8
        | PrimValType::S16
        | PrimValType::U16
        | PrimValType::S32
        | PrimValType::U32
        | PrimValType::Char => vec![CoreValType::I32],
        PrimValType::S64 | PrimValType::U64 => vec![CoreValType::I64],
        PrimValType::F32 => vec![CoreValType::F32],
        PrimValType::F64 => vec![CoreValType::F64],
        PrimValType::String => vec![CoreValType::I32, CoreValType::I32],
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => vec![CoreValType::I32],
    }
}

fn join_component_flat_types(lhs: CoreValType, rhs: CoreValType) -> CoreValType {
    if lhs == rhs {
        lhs
    } else if matches!(
        (lhs, rhs),
        (CoreValType::I32, CoreValType::F32) | (CoreValType::F32, CoreValType::I32)
    ) {
        CoreValType::I32
    } else {
        CoreValType::I64
    }
}

fn write_value_to_memory(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    validate_pointer_alignment(ptr, memory_abi_for_valtype(ty, program)?.align)?;
    match ty {
        ValType::Primitive(prim) => {
            write_primitive_to_memory(value, prim, options, program, store, ptr)
        }
        ValType::Type(type_id) => {
            write_defined_value_to_memory(value, *type_id, options, program, store, ptr)
        }
    }
}

fn read_value_from_memory(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    validate_pointer_alignment(ptr, memory_abi_for_valtype(ty, program)?.align)?;
    match ty {
        ValType::Primitive(prim) => {
            read_primitive_from_memory(prim, options, program, store, memory, ptr)
        }
        ValType::Type(type_id) => {
            read_defined_value_from_memory(*type_id, options, program, store, memory, ptr)
        }
    }
}

fn write_defined_value_to_memory(
    value: &ComponentValue,
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            write_primitive_to_memory(value, prim, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            write_record_value_to_memory(value, fields, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            write_variant_value_to_memory(value, cases, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Flags(labels)) => {
            write_flags_value_to_memory(value, labels, options, program, store, ptr)
        }
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            let flat = lower_list_value(value, elem, *maybe_len, options, program, store)?;
            write_flat_values(store, memory, ptr, &flat)
        }
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            write_memory(
                store,
                memory,
                ptr,
                &(expect_handle(value)? as i32).to_le_bytes(),
            )
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            write_memory(
                store,
                memory,
                ptr,
                &(expect_handle(value)? as i32).to_le_bytes(),
            )
        }
        Type::Resource(_) | Type::Generic(_) => write_memory(
            store,
            memory,
            ptr,
            &(expect_handle(value)? as i32).to_le_bytes(),
        ),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn read_defined_value_from_memory(
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            read_primitive_from_memory(prim, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            read_record_value_from_memory(fields, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Variant(variant)) => {
            read_variant_value_from_memory(variant, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Flags(labels)) => {
            read_flags_value_from_memory(labels, memory, store, ptr)
        }
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            read_list_value_from_memory(elem, *maybe_len, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Own(
                read_i32_from_memory(store, memory, ptr)? as u32,
            ))
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Borrow(
                read_i32_from_memory(store, memory, ptr)? as u32,
            ))
        }
        Type::Resource(_) | Type::Generic(_) => Ok(ComponentValue::Own(read_i32_from_memory(
            store, memory, ptr,
        )? as u32)),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn write_primitive_to_memory(
    value: &ComponentValue,
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    #[cfg(feature = "component-gated-feature-async")]
    if matches!(prim, PrimValType::ErrorContext) {
        return Err(unsupported_error_context());
    }
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match prim {
        PrimValType::Bool => write_memory(store, memory, ptr, &[u8::from(expect_bool(value)?)]),
        PrimValType::S8 => write_memory(store, memory, ptr, &[(expect_i32(value)? as i8) as u8]),
        PrimValType::U8 => write_memory(store, memory, ptr, &[expect_u32(value)? as u8]),
        PrimValType::S16 => write_memory(
            store,
            memory,
            ptr,
            &(expect_i32(value)? as i16).to_le_bytes(),
        ),
        PrimValType::U16 => write_memory(
            store,
            memory,
            ptr,
            &(expect_u32(value)? as u16).to_le_bytes(),
        ),
        PrimValType::S32 => write_memory(store, memory, ptr, &expect_i32(value)?.to_le_bytes()),
        PrimValType::U32 => write_memory(store, memory, ptr, &expect_u32(value)?.to_le_bytes()),
        PrimValType::S64 => write_memory(store, memory, ptr, &expect_i64(value)?.to_le_bytes()),
        PrimValType::U64 => write_memory(store, memory, ptr, &expect_u64(value)?.to_le_bytes()),
        PrimValType::F32 => write_memory(store, memory, ptr, &expect_f32(value)?.to_le_bytes()),
        PrimValType::F64 => write_memory(store, memory, ptr, &expect_f64(value)?.to_le_bytes()),
        PrimValType::Char => write_memory(
            store,
            memory,
            ptr,
            &(expect_char(value)? as u32).to_le_bytes(),
        ),
        PrimValType::String => {
            let flat = lower_string(value, options, program, store)?;
            write_flat_values(store, memory, ptr, &flat)
        }
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => Err(unsupported_error_context()),
    }
}

fn read_primitive_from_memory(
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => ComponentValue::Bool(read_u8_from_memory(store, memory, ptr)? != 0),
        PrimValType::S8 => ComponentValue::S8(read_u8_from_memory(store, memory, ptr)? as i8),
        PrimValType::U8 => ComponentValue::U8(read_u8_from_memory(store, memory, ptr)?),
        PrimValType::S16 => ComponentValue::S16(read_u16_from_memory(store, memory, ptr)? as i16),
        PrimValType::U16 => ComponentValue::U16(read_u16_from_memory(store, memory, ptr)?),
        PrimValType::S32 => ComponentValue::S32(read_i32_from_memory(store, memory, ptr)?),
        PrimValType::U32 => ComponentValue::U32(read_i32_from_memory(store, memory, ptr)? as u32),
        PrimValType::S64 => ComponentValue::S64(read_i64_from_memory(store, memory, ptr)?),
        PrimValType::U64 => ComponentValue::U64(read_i64_from_memory(store, memory, ptr)? as u64),
        PrimValType::F32 => ComponentValue::F32(f32::from_le_bytes(read_memory_array::<4>(
            store, memory, ptr,
        )?)),
        PrimValType::F64 => ComponentValue::F64(f64::from_le_bytes(read_memory_array::<8>(
            store, memory, ptr,
        )?)),
        PrimValType::Char => ComponentValue::Char(char_from_canonical_bits(read_i32_from_memory(
            store, memory, ptr,
        )? as u32)?),
        PrimValType::String => {
            let string_ptr = read_i32_from_memory(store, memory, ptr)? as u32;
            let len = read_i32_from_memory(store, memory, ptr + 4)? as u32;
            ComponentValue::String(read_string_from_memory(
                store,
                memory,
                string_ptr,
                len,
                options.string_encoding,
            )?)
        }
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => return Err(unsupported_error_context()),
    })
}

fn write_record_value_to_memory(
    value: &ComponentValue,
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let values = if is_tuple_fields(fields) {
        match value {
            ComponentValue::Tuple(values) => values.clone(),
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected tuple value, got {other:?}"
                )))
            }
        }
    } else {
        match value {
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected record value, got {other:?}"
                )))
            }
        }
    };

    let mut cursor = 0u32;
    for (field_value, field) in values.iter().zip(fields.iter()) {
        let abi = memory_abi_for_valtype(&field.ty, program)?;
        cursor = align_to(cursor, abi.align.max(1));
        write_value_to_memory(
            field_value,
            &field.ty,
            options,
            program,
            store,
            ptr + cursor,
        )?;
        cursor = cursor.saturating_add(abi.size);
    }
    Ok(())
}

fn read_record_value_from_memory(
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let mut values = Vec::with_capacity(fields.len());
    let mut cursor = 0u32;
    for field in fields {
        let abi = memory_abi_for_valtype(&field.ty, program)?;
        cursor = align_to(cursor, abi.align.max(1));
        values.push((
            field.label.0.clone(),
            read_value_from_memory(&field.ty, options, program, store, memory, ptr + cursor)?,
        ));
        cursor = cursor.saturating_add(abi.size);
    }
    if is_tuple_fields(fields) {
        Ok(ComponentValue::Tuple(
            values.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Ok(ComponentValue::Record(values))
    }
}

fn write_variant_value_to_memory(
    value: &ComponentValue,
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let (case_name, payload) = match value {
        ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
        ComponentValue::Enum(case) => (case.as_str(), None),
        ComponentValue::Option(value) if is_option_cases(cases) => match value {
            Some(value) => ("some", Some(value.as_ref())),
            None => ("none", None),
        },
        ComponentValue::Result { ok, err } if is_result_cases(cases) => match (ok, err) {
            (Some(value), None) => ("ok", Some(value.as_ref())),
            (None, Some(value)) => ("err", Some(value.as_ref())),
            _ => {
                return Err(ComponentError::InvalidArgument(
                    "result value must set exactly one branch".to_owned(),
                ))
            }
        },
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected variant-compatible value, got {other:?}"
            )))
        }
    };
    let case_index = cases
        .iter()
        .position(|case| case.label.0 == case_name)
        .ok_or_else(|| {
            ComponentError::InvalidArgument(format!("unknown variant case '{case_name}'"))
        })?;
    write_variant_discriminant_to_memory(store, options, ptr, cases.len(), case_index)?;

    let (payload_offset, payload_size) = variant_payload_offset_and_size(cases, program)?;
    if payload_size != 0 {
        write_memory(
            store,
            options.memory.as_ref().expect("memory already checked"),
            ptr + payload_offset,
            &vec![0; payload_size as usize],
        )?;
    }

    let case = &cases[case_index];
    if let Some(ty) = &case.ty {
        let payload = payload.ok_or_else(|| {
            ComponentError::InvalidArgument(format!(
                "variant case '{}' expects a payload",
                case.label
            ))
        })?;
        write_value_to_memory(payload, ty, options, program, store, ptr + payload_offset)?;
    } else {
        let payloadless_result_case = is_result_cases(cases)
            && matches!(
                payload,
                Some(ComponentValue::Tuple(values)) if values.is_empty()
            );
        if payload.is_some() && !payloadless_result_case {
            return Err(ComponentError::InvalidArgument(format!(
                "variant case '{}' does not accept a payload",
                case.label
            )));
        }
    }

    Ok(())
}

fn read_variant_value_from_memory(
    variant: &VariantType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let discriminant = read_variant_discriminant_from_memory(store, memory, ptr, variant.len())?;
    let case = variant
        .get(discriminant)
        .ok_or_else(|| ComponentError::Trap("invalid variant discriminant".to_owned()))?;
    let (payload_offset, _) = variant_payload_offset_and_size(variant, program)?;
    let payload = if let Some(ty) = &case.ty {
        Some(Box::new(read_value_from_memory(
            ty,
            options,
            program,
            store,
            memory,
            ptr + payload_offset,
        )?))
    } else {
        None
    };

    Ok(variant_representation(variant, case, payload))
}

fn write_flags_value_to_memory(
    value: &ComponentValue,
    labels: &[crate::ir::Label],
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let words = lower_flags_value(value, labels)?
        .into_iter()
        .map(|word| match word {
            WasmValue::I32(bits) => Ok(bits as u32),
            other => Err(ComponentError::Runtime(format!(
                "flags lowered to unexpected flat value {other:?}"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let abi = flags_memory_abi(labels.len());
    match abi.size {
        0 => Ok(()),
        1 => write_memory(
            store,
            memory,
            ptr,
            &[words.first().copied().unwrap_or(0) as u8],
        ),
        2 => write_memory(
            store,
            memory,
            ptr,
            &(words.first().copied().unwrap_or(0) as u16).to_le_bytes(),
        ),
        _ => {
            for (index, word) in words.iter().enumerate() {
                write_memory(store, memory, ptr + (index as u32) * 4, &word.to_le_bytes())?;
            }
            Ok(())
        }
    }
}

fn read_flags_value_from_memory(
    labels: &[crate::ir::Label],
    memory: &CoreExportRef,
    store: &Store,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let abi = flags_memory_abi(labels.len());
    let words = match abi.size {
        0 => Vec::new(),
        1 => vec![read_u8_from_memory(store, memory, ptr)? as u32],
        2 => vec![read_u16_from_memory(store, memory, ptr)? as u32],
        _ => {
            let mut words = Vec::with_capacity(labels.len().div_ceil(32));
            for index in 0..labels.len().div_ceil(32) {
                words.push(read_i32_from_memory(store, memory, ptr + (index as u32) * 4)? as u32);
            }
            words
        }
    };
    let mut selected = Vec::new();
    for chunk_start in (0..labels.len()).step_by(32) {
        let bits = words.get(chunk_start / 32).copied().unwrap_or(0);
        for bit in 0..32 {
            let index = chunk_start + bit;
            if index >= labels.len() {
                break;
            }
            if bits & (1 << bit) != 0 {
                selected.push(labels[index].0.clone());
            }
        }
    }
    Ok(ComponentValue::Flags(selected))
}

fn read_list_value_from_memory(
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let list_ptr = read_i32_from_memory(store, memory, ptr)? as u32;
    let len_ptr = ptr.checked_add(4).ok_or_else(|| {
        ComponentError::Trap(format!("list length at {ptr} is outside the address space"))
    })?;
    let len = read_i32_from_memory(store, memory, len_ptr)? as u32;
    if let Some(expected) = fixed_len {
        if len as usize != expected {
            return Err(ComponentError::Trap(format!(
                "fixed-length list expected {expected} elements, got {len}"
            )));
        }
    }
    let values = read_list_elements(elem, list_ptr, len, options, program, store, memory)?;
    Ok(ComponentValue::List(values))
}

fn variant_payload_offset_and_size(
    cases: &[Case],
    program: &ComponentProgram,
) -> Result<(u32, u32), ComponentError> {
    let discriminant_size = variant_discriminant_size(cases.len())?;
    let mut payload_size = 0u32;
    let mut payload_align = discriminant_size;
    for case in cases {
        if let Some(ty) = &case.ty {
            let abi = memory_abi_for_valtype(ty, program)?;
            payload_size = payload_size.max(abi.size);
            payload_align = payload_align.max(abi.align.max(1));
        }
    }
    Ok((
        align_to(discriminant_size, payload_align.max(1)),
        payload_size,
    ))
}

fn write_variant_discriminant_to_memory(
    store: &Store,
    options: &RuntimeCanonicalOptions,
    ptr: u32,
    case_count: usize,
    discriminant: usize,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match variant_discriminant_size(case_count)? {
        1 => write_memory(store, memory, ptr, &[discriminant as u8]),
        2 => write_memory(store, memory, ptr, &(discriminant as u16).to_le_bytes()),
        4 => write_memory(store, memory, ptr, &(discriminant as u32).to_le_bytes()),
        _ => unreachable!(),
    }
}

fn read_variant_discriminant_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    case_count: usize,
) -> Result<usize, ComponentError> {
    Ok(match variant_discriminant_size(case_count)? {
        1 => read_u8_from_memory(store, memory, ptr)? as usize,
        2 => read_u16_from_memory(store, memory, ptr)? as usize,
        4 => read_i32_from_memory(store, memory, ptr)? as usize,
        _ => unreachable!(),
    })
}

fn read_u8_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<u8, ComponentError> {
    Ok(read_memory_array::<1>(store, memory, ptr)?[0])
}

fn read_u16_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<u16, ComponentError> {
    Ok(u16::from_le_bytes(read_memory_array::<2>(
        store, memory, ptr,
    )?))
}

fn read_i32_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<i32, ComponentError> {
    Ok(i32::from_le_bytes(read_memory_array::<4>(
        store, memory, ptr,
    )?))
}

fn read_i64_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<i64, ComponentError> {
    Ok(i64::from_le_bytes(read_memory_array::<8>(
        store, memory, ptr,
    )?))
}

fn align_to(value: u32, align: u32) -> u32 {
    if align <= 1 {
        value
    } else {
        let rem = value % align;
        if rem == 0 {
            value
        } else {
            value + (align - rem)
        }
    }
}

fn lower_record_value(
    value: &ComponentValue,
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let values = if is_tuple_fields(fields) {
        match value {
            ComponentValue::Tuple(values) => values.clone(),
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected tuple value, got {other:?}"
                )))
            }
        }
    } else {
        match value {
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected record value, got {other:?}"
                )))
            }
        }
    };
    let mut lowered = Vec::new();
    for (value, field) in values.iter().zip(fields.iter()) {
        lowered.extend(lower_value_to_flat(
            value, &field.ty, options, program, store,
        )?);
    }
    Ok(lowered)
}

fn lift_record_value(
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let mut values = Vec::with_capacity(fields.len());
    for field in fields {
        values.push((
            field.label.0.clone(),
            lift_value(&field.ty, options, program, store, cursor)?,
        ));
    }
    if is_tuple_fields(fields) {
        Ok(ComponentValue::Tuple(
            values.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Ok(ComponentValue::Record(values))
    }
}

fn lower_variant_value(
    value: &ComponentValue,
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let (case_name, payload) = match value {
        ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
        ComponentValue::Enum(case) => (case.as_str(), None),
        ComponentValue::Option(value) if is_option_cases(cases) => match value {
            Some(value) => ("some", Some(value.as_ref())),
            None => ("none", None),
        },
        ComponentValue::Result { ok, err } if is_result_cases(cases) => match (ok, err) {
            (Some(value), None) => ("ok", Some(value.as_ref())),
            (None, Some(value)) => ("err", Some(value.as_ref())),
            _ => {
                return Err(ComponentError::InvalidArgument(
                    "result value must set exactly one branch".to_owned(),
                ))
            }
        },
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected variant-compatible value, got {other:?}"
            )))
        }
    };
    let case_index = cases
        .iter()
        .position(|case| case.label.0 == case_name)
        .ok_or_else(|| {
            ComponentError::InvalidArgument(format!("unknown variant case '{case_name}'"))
        })?;
    let payload_types = variant_payload_flat_types(cases, program)?;
    let mut lowered = Vec::with_capacity(1 + payload_types.len());
    lowered.push(WasmValue::I32(case_index as i32));
    let case = &cases[case_index];
    let (case_flat, case_flat_types) = if let Some(ty) = &case.ty {
        let payload = payload.ok_or_else(|| {
            ComponentError::InvalidArgument(format!(
                "variant case '{}' expects a payload",
                case.label
            ))
        })?;
        (
            lower_value_to_flat(payload, ty, options, program, store)?,
            flat_types_for_valtype(ty, program)?,
        )
    } else {
        let payloadless_result_case = is_result_cases(cases)
            && matches!(
                payload,
                Some(ComponentValue::Tuple(values)) if values.is_empty()
            );
        if payload.is_some() && !payloadless_result_case {
            return Err(ComponentError::InvalidArgument(format!(
                "variant case '{}' does not accept a payload",
                case.label
            )));
        }
        (Vec::new(), Vec::new())
    };
    for (index, payload_ty) in payload_types.iter().enumerate() {
        if let Some(value) = case_flat.get(index) {
            lowered.push(coerce_flat_value(
                *value,
                case_flat_types[index],
                *payload_ty,
            )?);
        } else {
            lowered.push(zero_wasm_value(*payload_ty));
        }
    }
    Ok(lowered)
}

fn lift_variant_value(
    variant: &VariantType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let case_index = cursor.next_i32()? as usize;
    let case = variant
        .get(case_index)
        .ok_or_else(|| ComponentError::Trap("invalid variant discriminant".to_owned()))?;
    let payload_types = variant_payload_flat_types(variant, program)?;
    let raw_payload = payload_types
        .iter()
        .map(|ty| cursor.next_for_type(*ty))
        .collect::<Result<Vec<_>, _>>()?;
    let payload = if let Some(ty) = &case.ty {
        let expected_types = flat_types_for_valtype(ty, program)?;
        let values = expected_types
            .iter()
            .enumerate()
            .map(|(index, expected)| {
                coerce_flat_value(raw_payload[index], payload_types[index], *expected)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Some(Box::new(lift_value_from_flat_values(
            ty, options, program, store, &values,
        )?))
    } else {
        None
    };
    Ok(variant_representation(variant, case, payload))
}

/// Choose the dynamic representation for a lifted variant-shaped value.
///
/// The representation follows the constructor the type was declared with
/// (its `VariantForm`), not the shape of its cases: a type declared as
/// `variant { none, some(T) }` lifts to `ComponentValue::Variant`, while a
/// type declared as `option T` — whose cases are identical — lifts to
/// `ComponentValue::Option`. The shape checks remain as a sanity guard:
/// a form whose cases do not have the expected shape (which the decoder
/// cannot produce) falls back to the plain `Variant` representation.
fn variant_representation(
    variant: &VariantType,
    case: &Case,
    payload: Option<Box<ComponentValue>>,
) -> ComponentValue {
    match variant.form {
        VariantForm::Option if is_option_cases(variant) => ComponentValue::Option(payload),
        VariantForm::Result if is_result_cases(variant) => match case.label.0.as_str() {
            "ok" | "err" => {
                let payload = if case.ty.is_none() {
                    Some(Box::new(ComponentValue::Tuple(Vec::new())))
                } else {
                    payload
                };
                if case.label.0 == "ok" {
                    ComponentValue::Result {
                        ok: payload,
                        err: None,
                    }
                } else {
                    ComponentValue::Result {
                        ok: None,
                        err: payload,
                    }
                }
            }
            _ => ComponentValue::Variant {
                case: case.label.0.clone(),
                value: payload,
            },
        },
        VariantForm::Enum if is_enum_cases(variant) => ComponentValue::Enum(case.label.0.clone()),
        _ => ComponentValue::Variant {
            case: case.label.0.clone(),
            value: payload,
        },
    }
}

fn lower_flags_value(
    value: &ComponentValue,
    labels: &[crate::ir::Label],
) -> Result<Vec<WasmValue>, ComponentError> {
    let selected = match value {
        ComponentValue::Flags(flags) => flags.as_slice(),
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected flags value, got {other:?}"
            )))
        }
    };
    let mut words = vec![
        0u32;
        if labels.is_empty() {
            0
        } else if labels.len() <= 16 {
            1
        } else {
            labels.len().div_ceil(32)
        }
    ];
    for flag in selected {
        let index = labels
            .iter()
            .position(|label| &label.0 == flag)
            .ok_or_else(|| ComponentError::InvalidArgument(format!("unknown flag '{flag}'")))?;
        words[index / 32] |= 1 << (index % 32);
    }
    Ok(words
        .into_iter()
        .map(|word| WasmValue::I32(word as i32))
        .collect())
}

fn lift_flags_value(
    labels: &[crate::ir::Label],
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let mut selected = Vec::new();
    for chunk_start in (0..labels.len()).step_by(32) {
        let bits = cursor.next_i32()? as u32;
        for bit in 0..32 {
            let index = chunk_start + bit;
            if index >= labels.len() {
                break;
            }
            if bits & (1 << bit) != 0 {
                selected.push(labels[index].0.clone());
            }
        }
    }
    Ok(ComponentValue::Flags(selected))
}

fn lower_list_value(
    value: &ComponentValue,
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let values = match value {
        ComponentValue::List(values) => values,
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected list value, got {other:?}"
            )))
        }
    };
    if let Some(expected) = fixed_len {
        if values.len() != expected {
            return Err(ComponentError::InvalidArgument(format!(
                "expected list length {expected}, got {}",
                values.len()
            )));
        }
    }
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let elem_abi = memory_abi_for_valtype(elem, program)?;
    let elem_align = elem_abi.align.max(1);
    let stride = align_to(elem_abi.size, elem_align);
    let total_len = stride.saturating_mul(values.len() as u32);
    // Canonical ABI `lower_list` calls realloc unconditionally: even an
    // empty list allocates with (0, 0, align, 0) and uses the returned
    // pointer, so there is no zero-size shortcut here. The returned pointer
    // is validated (alignment and bounds) inside `call_realloc`.
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let ptr = call_realloc(
        realloc,
        store,
        &memory,
        0,
        0,
        elem_align as i32,
        total_len as i32,
        "canonical list",
    )? as u32;
    for (index, value) in values.iter().enumerate() {
        write_value_to_memory(
            value,
            elem,
            options,
            program,
            store,
            ptr + stride * index as u32,
        )?;
    }
    Ok(vec![
        WasmValue::I32(ptr as i32),
        WasmValue::I32(values.len() as i32),
    ])
}

fn lift_list_value(
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let ptr = cursor.next_i32()? as u32;
    let len = cursor.next_i32()? as u32;
    if let Some(expected) = fixed_len {
        if len as usize != expected {
            return Err(ComponentError::Trap(format!(
                "fixed-length list expected {expected} elements, got {len}"
            )));
        }
    }
    let values = read_list_elements(elem, ptr, len, options, program, store, &memory)?;
    Ok(ComponentValue::List(values))
}

/// Reads `len` list elements of type `elem`, starting at `base`.
///
/// `base` and `len` come from the guest, so neither is trusted: the length is
/// never used as an allocation hint, and every element address is computed with
/// checked arithmetic. With a non-zero stride the loop is bounded by linear
/// memory, because the first out-of-range element read fails. A zero-stride
/// element type would leave the loop bounded only by the guest's length, so it
/// is rejected instead.
fn read_list_elements(
    elem: &ValType,
    base: u32,
    len: u32,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let stride = element_stride(elem, program)?;
    if stride == 0 && len != 0 {
        return Err(ComponentError::Trap(format!(
            "list element type occupies no memory, so a guest-supplied length of {len} cannot be bounded"
        )));
    }
    let mut values = Vec::new();
    for index in 0..len {
        let offset = stride
            .checked_mul(index)
            .and_then(|offset| base.checked_add(offset))
            .ok_or_else(|| {
                ComponentError::Trap(format!(
                    "list element {index} at base {base} with stride {stride} is outside the address space"
                ))
            })?;
        values.push(read_value_from_memory(
            elem, options, program, store, memory, offset,
        )?);
    }
    Ok(values)
}

fn lift_value_from_flat_values(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    values: &[WasmValue],
) -> Result<ComponentValue, ComponentError> {
    let mut cursor = CoreValueCursor::new(values);
    lift_value(ty, options, program, store, &mut cursor)
}

fn variant_payload_flat_types(
    cases: &[Case],
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    let mut payload = Vec::new();
    for case in cases {
        if let Some(ty) = &case.ty {
            for (index, flat_ty) in flat_types_for_valtype(ty, program)?.into_iter().enumerate() {
                if let Some(current) = payload.get_mut(index) {
                    *current = join_component_flat_types(*current, flat_ty);
                } else {
                    payload.push(flat_ty);
                }
            }
        }
    }
    Ok(payload)
}

fn zero_wasm_value(ty: CoreValType) -> WasmValue {
    match ty {
        CoreValType::I32 => WasmValue::I32(0),
        CoreValType::I64 => WasmValue::I64(0),
        CoreValType::F32 => WasmValue::F32(0.0),
        CoreValType::F64 => WasmValue::F64(0.0),
        CoreValType::FuncRef => WasmValue::FuncRef(0),
        CoreValType::ExternRef => WasmValue::ExternRef(0),
        CoreValType::V128 => WasmValue::V128(0),
    }
}

fn coerce_flat_value(
    value: WasmValue,
    from: CoreValType,
    to: CoreValType,
) -> Result<WasmValue, ComponentError> {
    if from == to {
        return Ok(value);
    }
    Ok(match (value, from, to) {
        (WasmValue::I32(value), CoreValType::I32, CoreValType::I64) => {
            WasmValue::I64((value as u32 as u64) as i64)
        }
        (WasmValue::F32(value), CoreValType::F32, CoreValType::I32) => {
            WasmValue::I32(value.to_bits() as i32)
        }
        (WasmValue::I32(value), CoreValType::I32, CoreValType::F32) => {
            WasmValue::F32(f32::from_bits(value as u32))
        }
        (WasmValue::F32(value), CoreValType::F32, CoreValType::I64) => {
            WasmValue::I64(value.to_bits() as u64 as i64)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::I32) => {
            WasmValue::I32(value as u32 as i32)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::F32) => {
            WasmValue::F32(f32::from_bits(value as u32))
        }
        (WasmValue::F64(value), CoreValType::F64, CoreValType::I64) => {
            WasmValue::I64(value.to_bits() as i64)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::F64) => {
            WasmValue::F64(f64::from_bits(value as u64))
        }
        (other, _, _) => {
            return Err(ComponentError::Trap(format!(
                "cannot coerce canonical value {other:?} from {from:?} to {to:?}"
            )))
        }
    })
}

fn is_tuple_fields(fields: &[LabelValType]) -> bool {
    !fields.is_empty()
        && fields
            .iter()
            .enumerate()
            .all(|(index, field)| field.label.0 == index.to_string())
}

fn is_option_cases(cases: &[Case]) -> bool {
    matches!(
        cases,
        [Case { label, ty: None }, Case { label: some, ty: Some(_) }]
            if label.0 == "none" && some.0 == "some"
    )
}

fn is_result_cases(cases: &[Case]) -> bool {
    matches!(
        cases,
        [Case { label: ok, .. }, Case { label: err, .. }] if ok.0 == "ok" && err.0 == "err"
    )
}

fn is_enum_cases(cases: &[Case]) -> bool {
    !cases.is_empty() && cases.iter().all(|case| case.ty.is_none())
}

fn read_string_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: u32,
    encoding: Option<CanonicalStringEncoding>,
) -> Result<String, ComponentError> {
    let encoding = encoding.unwrap_or(CanonicalStringEncoding::Utf8);
    match encoding {
        CanonicalStringEncoding::Utf8 => {
            let bytes = read_memory(store, memory, ptr, len as usize)?;
            String::from_utf8(bytes).map_err(|error| ComponentError::Trap(error.to_string()))
        }
        CanonicalStringEncoding::Utf16 => {
            validate_pointer_alignment(ptr, 2)?;
            let bytes = read_memory(store, memory, ptr, len as usize * 2)?;
            decode_utf16_le(bytes)
        }
        CanonicalStringEncoding::CompactUtf16 => {
            validate_pointer_alignment(ptr, 2)?;
            if len & COMPACT_UTF16_TAG != 0 {
                let utf16_len = len ^ COMPACT_UTF16_TAG;
                let bytes = read_memory(store, memory, ptr, utf16_len as usize * 2)?;
                decode_utf16_le(bytes)
            } else {
                let bytes = read_memory(store, memory, ptr, len as usize)?;
                Ok(bytes.into_iter().map(|byte| byte as char).collect())
            }
        }
    }
}

fn decode_utf16_le(bytes: Vec<u8>) -> Result<String, ComponentError> {
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    String::from_utf16(&units).map_err(|error| ComponentError::Trap(error.to_string()))
}

fn read_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: usize,
) -> Result<Vec<u8>, ComponentError> {
    let memory = memory_addr(store, memory)?;
    crate::support::common::read_memory(store, &memory, ptr, len)
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))
}

fn read_memory_array<const N: usize>(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<[u8; N], ComponentError> {
    let memory = memory_addr(store, memory)?;
    crate::support::common::read_memory_array::<N>(store, &memory, ptr)
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))
}

fn write_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    bytes: &[u8],
) -> Result<(), ComponentError> {
    let memory = memory_addr(store, memory)?;
    if crate::support::common::write_memory(store, &memory, ptr, bytes) {
        Ok(())
    } else {
        Err(ComponentError::Trap(
            "memory access out of bounds".to_owned(),
        ))
    }
}

fn write_flat_values(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    values: &[WasmValue],
) -> Result<(), ComponentError> {
    let mut cursor = ptr;
    for value in values {
        match value {
            WasmValue::I32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::I64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::F32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::F64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::FuncRef(v) | WasmValue::ExternRef(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::V128(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 16;
            }
        }
    }
    Ok(())
}

fn memory_addr(
    store: &Store,
    memory: &CoreExportRef,
) -> Result<crate::support::common::CoreMemoryHandle, ComponentError> {
    crate::support::common::memory_export(&memory.instance, store, &memory.export_name)
        .map_err(ComponentError::Link)
}

#[allow(clippy::too_many_arguments)]
fn call_realloc(
    realloc: &RuntimeCoreFunc,
    store: &Store,
    memory: &CoreExportRef,
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
    allocation: &str,
) -> Result<i32, ComponentError> {
    let result = realloc.call_sync(
        store,
        &[
            WasmValue::I32(old_ptr),
            WasmValue::I32(old_len),
            WasmValue::I32(align),
            WasmValue::I32(new_len),
        ],
    )?;
    match result.as_slice() {
        [WasmValue::I32(ptr)] => {
            validate_realloc_result(
                store,
                memory,
                *ptr as u32,
                align as u32,
                new_len as u32,
                allocation,
            )?;
            Ok(*ptr)
        }
        _ => Err(ComponentError::Runtime(
            "realloc returned an unexpected result".to_owned(),
        )),
    }
}

/// The canonical ABI traps when a pointer used to load or store a value
/// is not aligned to the value type's alignment (`unaligned pointer` in
/// the spec's trap wording). This applies to every guest-supplied pointer
/// the runtime dereferences on the guest's behalf: retptrs, spilled
/// argptrs, record/variant field bases (aligned by construction once the
/// base is checked), list element bases, and string content pointers.
fn validate_pointer_alignment(ptr: u32, align: u32) -> Result<(), ComponentError> {
    if align > 1 && ptr % align != 0 {
        return Err(ComponentError::Trap(format!(
            "unaligned pointer: address {ptr} is not aligned to {align} bytes"
        )));
    }
    Ok(())
}

/// Validates a pointer returned by a canonical `realloc` against the
/// canonical ABI contract: the pointer must be aligned to the requested
/// alignment, and the whole allocation `[ptr, ptr + size)` must lie inside
/// the memory. The result is validated here, at the `realloc` site, rather
/// than relying on the writes that follow it: some results are never
/// written through afterwards (a shrunk latin1+utf16 string is returned to
/// the guest as-is), and others are only partially written (the indirect
/// parameter area's alignment padding), so a write-side check alone would
/// let an out-of-bounds allocation through.
fn validate_realloc_result(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    align: u32,
    size: u32,
    allocation: &str,
) -> Result<(), ComponentError> {
    if ptr % align != 0 {
        return Err(ComponentError::Trap(format!(
            "realloc return: result not aligned: canonical realloc returned pointer {ptr:#x} for {allocation}, which is not aligned to {align} bytes"
        )));
    }
    let handle = memory_addr(store, memory)?;
    let memory_len = crate::support::common::memory_len(store, &handle);
    if u64::from(ptr) + u64::from(size) > memory_len as u64 {
        return Err(ComponentError::Trap(format!(
            "realloc return: beyond end of memory: canonical realloc returned pointer {ptr:#x} for {allocation}, and the {size}-byte allocation at {ptr:#x} is out of bounds of the {memory_len}-byte memory"
        )));
    }
    Ok(())
}

pub(super) fn program_func_type(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<FuncType, ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Func(func_type)) => Ok(func_type.clone()),
        _ => Err(ComponentError::Runtime(
            "function type is missing".to_owned(),
        )),
    }
}

/// Whether two function types denote the same component type up to the
/// declared constructors of variant-shaped types.
///
/// The component model treats `option`, `result` and `enum` as sugar for
/// particular variants: subtyping, size and flattening only ever look at
/// the cases, so a type declared with one constructor and the variant it
/// desugars to are the *same* type with the same canonical layout. The
/// one place the constructor is observable is the dynamic value
/// representation chosen when lifting (see `variant_representation`).
/// This comparison is therefore structural equality that resolves type
/// indices through `program` and ignores only `VariantForm`; everything
/// that affects layout or identity — record and case labels, list
/// fixed lengths, resource identities — must match exactly.
pub(super) fn func_types_equal_modulo_variant_form(
    program: &ComponentProgram,
    a: &FuncType,
    b: &FuncType,
) -> bool {
    let mut seen = Vec::new();
    a.params.len() == b.params.len()
        && a.params
            .iter()
            .zip(&b.params)
            .all(|(x, y)| val_types_equal_modulo_variant_form(program, x, y, &mut seen))
        && match (&a.result, &b.result) {
            (None, None) => true,
            (Some(x), Some(y)) => val_types_equal_modulo_variant_form(program, x, y, &mut seen),
            _ => false,
        }
}

fn val_types_equal_modulo_variant_form(
    program: &ComponentProgram,
    a: &ValType,
    b: &ValType,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    match (a, b) {
        (ValType::Primitive(x), ValType::Primitive(y)) => x == y,
        (ValType::Type(x), ValType::Type(y)) => {
            if x == y {
                return true;
            }
            // Defined types form a DAG (a type only references earlier
            // indices), but guard against re-entering a pair anyway: a
            // pair already under comparison is assumed equal, which is
            // the coinductive reading and cannot manufacture a layout
            // difference.
            if seen.contains(&(*x, *y)) {
                return true;
            }
            seen.push((*x, *y));
            match (program.get_type(*x), program.get_type(*y)) {
                (Some(Type::DefVal(dx)), Some(Type::DefVal(dy))) => {
                    defval_types_equal_modulo_variant_form(program, dx, dy, seen)
                }
                (Some(Type::Resource(rx)), Some(Type::Resource(ry))) => rx == ry,
                _ => false,
            }
        }
        (ValType::Type(x), ValType::Primitive(y)) | (ValType::Primitive(y), ValType::Type(x)) => {
            matches!(
                program.get_type(*x),
                Some(Type::DefVal(DefValType::Primitive(found))) if found == y
            )
        }
    }
}

fn defval_types_equal_modulo_variant_form(
    program: &ComponentProgram,
    a: &DefValType,
    b: &DefValType,
    seen: &mut Vec<(TypeId, TypeId)>,
) -> bool {
    match (a, b) {
        (DefValType::Primitive(x), DefValType::Primitive(y)) => x == y,
        (DefValType::Record(x), DefValType::Record(y)) => {
            x.len() == y.len()
                && x.iter().zip(y.iter()).all(|(f, g)| {
                    f.label == g.label
                        && val_types_equal_modulo_variant_form(program, &f.ty, &g.ty, seen)
                })
        }
        (DefValType::Variant(x), DefValType::Variant(y)) => {
            // `form` is deliberately not compared: it is the declared
            // constructor, the single difference this comparison exists
            // to abstract over. Labels and payload types must match.
            x.cases.len() == y.cases.len()
                && x.cases.iter().zip(&y.cases).all(|(c, d)| {
                    c.label == d.label
                        && match (&c.ty, &d.ty) {
                            (None, None) => true,
                            (Some(t), Some(u)) => {
                                val_types_equal_modulo_variant_form(program, t, u, seen)
                            }
                            _ => false,
                        }
                })
        }
        (DefValType::Flags(x), DefValType::Flags(y)) => x == y,
        (DefValType::List(x, fixed_x), DefValType::List(y, fixed_y)) => {
            fixed_x == fixed_y && val_types_equal_modulo_variant_form(program, x, y, seen)
        }
        (DefValType::Own(x), DefValType::Own(y))
        | (DefValType::Borrow(x), DefValType::Borrow(y)) => {
            // Handles denote resources nominally: the two indices must
            // be the same entry or entries for the same resource. A
            // generic face is only equal to itself (handled by the
            // index comparison); faces of one resource under different
            // indices stay distinct, as they do for subtyping.
            x == y
                || matches!(
                    (program.get_type(*x), program.get_type(*y)),
                    (Some(Type::Resource(rx)), Some(Type::Resource(ry))) if rx == ry
                )
        }
        _ => false,
    }
}

fn validate_resource_type(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<(), ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Resource(_)) => Ok(()),
        Some(Type::DefVal(DefValType::Own(inner)))
        | Some(Type::DefVal(DefValType::Borrow(inner))) => validate_resource_type(program, *inner),
        Some(Type::Generic(generic)) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => validate_resource_type(program, inner),
            crate::ir::types::GenericBound::Sub => Ok(()),
        },
        _ => Err(ComponentError::Runtime(
            "resource type is missing".to_owned(),
        )),
    }
}

pub(super) fn runtime_resource_id(
    program: &ComponentProgram,
    shared: &SharedState,
    type_id: TypeId,
) -> Result<ResourceId, ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Resource(resource)) => Ok(*resource),
        Some(Type::DefVal(DefValType::Own(inner)))
        | Some(Type::DefVal(DefValType::Borrow(inner))) => {
            runtime_resource_id(program, shared, *inner)
        }
        Some(Type::Generic(generic)) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => {
                runtime_resource_id(program, shared, inner)
            }
            crate::ir::types::GenericBound::Sub => {
                let mut resources = shared.generic_resources.borrow_mut();
                Ok(*resources
                    .entry(type_id)
                    .or_insert_with(ResourceId::synthetic))
            }
        },
        _ => Err(ComponentError::Runtime(
            "resource type is missing".to_owned(),
        )),
    }
}

fn value_flat_len(ty: &ValType, program: &ComponentProgram) -> Result<usize, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(flat_types_for_primitive(prim).len()),
        ValType::Type(type_id) => program
            .get_type_info(*type_id)
            .map(|info| info.flat_len)
            .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned())),
    }
}

struct CoreValueCursor<'a> {
    values: &'a [WasmValue],
    offset: usize,
}

impl<'a> CoreValueCursor<'a> {
    fn new(values: &'a [WasmValue]) -> Self {
        Self { values, offset: 0 }
    }

    fn next(&mut self) -> Result<WasmValue, ComponentError> {
        let value = *self
            .values
            .get(self.offset)
            .ok_or_else(|| ComponentError::Trap("canonical ABI value underflow".to_owned()))?;
        self.offset += 1;
        Ok(value)
    }

    fn next_i32(&mut self) -> Result<i32, ComponentError> {
        match self.next()? {
            WasmValue::I32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i32, got {other:?}"))),
        }
    }

    fn next_i64(&mut self) -> Result<i64, ComponentError> {
        match self.next()? {
            WasmValue::I64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i64, got {other:?}"))),
        }
    }

    fn next_f32(&mut self) -> Result<f32, ComponentError> {
        match self.next()? {
            WasmValue::F32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f32, got {other:?}"))),
        }
    }

    fn next_f64(&mut self) -> Result<f64, ComponentError> {
        match self.next()? {
            WasmValue::F64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f64, got {other:?}"))),
        }
    }

    fn next_for_type(&mut self, ty: CoreValType) -> Result<WasmValue, ComponentError> {
        match ty {
            CoreValType::I32 => self.next().and_then(|value| match value {
                WasmValue::I32(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected i32, got {other:?}"))),
            }),
            CoreValType::I64 => self.next().and_then(|value| match value {
                WasmValue::I64(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected i64, got {other:?}"))),
            }),
            CoreValType::F32 => self.next().and_then(|value| match value {
                WasmValue::F32(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected f32, got {other:?}"))),
            }),
            CoreValType::F64 => self.next().and_then(|value| match value {
                WasmValue::F64(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected f64, got {other:?}"))),
            }),
            other => Err(ComponentError::Unsupported(format!(
                "core cursor does not support {other:?}"
            ))),
        }
    }
}

fn expect_bool(value: &ComponentValue) -> Result<bool, ComponentError> {
    value
        .as_bool()
        .ok_or_else(|| ComponentError::InvalidArgument("expected bool".to_owned()))
}

fn expect_u32(value: &ComponentValue) -> Result<u32, ComponentError> {
    value
        .as_u32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected u32".to_owned()))
}

fn expect_i32(value: &ComponentValue) -> Result<i32, ComponentError> {
    value
        .as_i32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected i32".to_owned()))
}

fn expect_i64(value: &ComponentValue) -> Result<i64, ComponentError> {
    match value {
        ComponentValue::I64(v) | ComponentValue::S64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected i64".to_owned())),
    }
}

fn expect_u64(value: &ComponentValue) -> Result<u64, ComponentError> {
    match value {
        ComponentValue::U64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected u64".to_owned())),
    }
}

fn expect_f32(value: &ComponentValue) -> Result<f32, ComponentError> {
    match value {
        ComponentValue::F32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f32".to_owned())),
    }
}

fn expect_f64(value: &ComponentValue) -> Result<f64, ComponentError> {
    match value {
        ComponentValue::F64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f64".to_owned())),
    }
}

/// Converts a raw canonical-ABI bit pattern to a `char`, trapping with
/// the canonical message when the pattern is not a Unicode scalar value
/// (a surrogate, or above U+10FFFF). Every core-to-component `char`
/// conversion funnels through here: the flat lift path and the memory
/// read path.
fn char_from_canonical_bits(bits: u32) -> Result<char, ComponentError> {
    char::from_u32(bits)
        .ok_or_else(|| ComponentError::Trap("invalid `char` bit pattern".to_owned()))
}

fn expect_char(value: &ComponentValue) -> Result<char, ComponentError> {
    match value {
        ComponentValue::Char(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected char".to_owned())),
    }
}

fn expect_handle(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Own(v) | ComponentValue::Borrow(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected resource handle".to_owned(),
        )),
    }
}

#[cfg(all(test, feature = "component-gated-feature-async"))]
mod tests {
    use super::*;
    use crate::support::binary::IoReadBinaryReader;
    use telomere::WasmParser;

    const MEMORY_MODULE: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x07,
        0x01, 0x03, b'm', b'e', b'm', 0x02, 0x00,
    ];

    fn empty_program() -> ComponentProgram {
        ComponentProgram {
            type_infos: Vec::new(),
            imports: Vec::new(),
            callable_imports: Vec::new(),
            exports: Vec::new(),
            callable_exports: Vec::new(),
            ops: Vec::new(),
            bytes: Vec::new(),
            root: Component {
                imports: HashMap::new(),
                exports: HashMap::new(),
                defined_instances: Vec::new(),
            },
            types: Vec::new().into_boxed_slice(),
            component_store: HashMap::new(),
            instance_store: HashMap::new(),
            func_store: HashMap::new(),
            core_module_store: HashMap::new(),
            core_type_store: HashMap::new(),
            core_instance_store: HashMap::new(),
            core_func_store: HashMap::new(),
            core_memory_store: HashMap::new(),
            core_global_store: HashMap::new(),
            core_table_store: HashMap::new(),
            value_store: HashMap::new(),
        }
    }

    fn assert_error_context_unsupported(error: ComponentError) {
        match error {
            ComponentError::Unsupported(message) => assert_eq!(
                message,
                "canonical ABI for `error-context` is not supported"
            ),
            other => panic!("expected error-context Unsupported error, got {other:?}"),
        }
    }

    fn memory_export(store: &Store) -> CoreExportRef {
        let mut reader = IoReadBinaryReader::from(MEMORY_MODULE);
        let module = WasmParser::new(&mut reader)
            .parse_module()
            .expect("memory fixture should parse");
        let instance = match block_on(crate::support::instantiate(module, store, &Registry::new()))
        {
            VMResult::Success(instance) => instance,
            other => panic!("memory fixture should instantiate: {other:?}"),
        };

        CoreExportRef {
            instance,
            export_name: "mem".to_owned(),
        }
    }

    #[test]
    fn error_context_layout_and_value_moves_are_unsupported() {
        let prim = PrimValType::ErrorContext;
        let memory_abi = memory_abi_for_primitive(&prim);
        assert_eq!(memory_abi.size, 4);
        assert_eq!(memory_abi.align, 4);
        assert_eq!(flat_types_for_primitive(&prim), vec![CoreValType::I32]);

        let program = empty_program();
        let store = Store::new();
        let options = RuntimeCanonicalOptions::default();
        assert_error_context_unsupported(
            lower_primitive(&ComponentValue::S32(0), &prim, &options, &store, &program)
                .expect_err("error-context lowering must be unsupported"),
        );

        let mut cursor = CoreValueCursor::new(&[]);
        assert_error_context_unsupported(
            lift_primitive(&prim, &options, &store, &mut cursor)
                .expect_err("error-context lifting must be unsupported"),
        );

        assert_error_context_unsupported(
            write_primitive_to_memory(
                &ComponentValue::S32(0),
                &prim,
                &options,
                &program,
                &store,
                0,
            )
            .expect_err("error-context memory lowering must be unsupported"),
        );

        let memory = memory_export(&store);
        assert_error_context_unsupported(
            read_primitive_from_memory(&prim, &options, &program, &store, &memory, 0)
                .expect_err("error-context memory lifting must be unsupported"),
        );
    }
}
