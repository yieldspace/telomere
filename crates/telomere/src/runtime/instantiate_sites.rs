use std::sync::Arc;

use crate::{
    common::{
        structured_jump_rewrite, FrameLayoutHeader, FuncTypeIdentity, Instr, ObjectRef, Op,
        SafepointMetadataCache, StablePc, StackMapSafepointKind, StoreInner,
    },
    runtime::vm,
};

use crate::common::store::{
    InstanceId, PrecomputedBlockReturnSite, PrecomputedCallFrame, PrecomputedDirectCallSite,
    PrecomputedFunctionReturnSite, PrecomputedImportCallSite, PrecomputedIndirectCallSite,
    PrecomputedLoopSite, PrecomputedWaitSite,
};

pub(crate) type PrecomputedSiteSets = (
    Arc<[PrecomputedDirectCallSite]>,
    Arc<[PrecomputedImportCallSite]>,
    Arc<[PrecomputedIndirectCallSite]>,
    Arc<[PrecomputedWaitSite]>,
    Arc<[PrecomputedLoopSite]>,
    Arc<[PrecomputedBlockReturnSite]>,
    Option<Arc<PrecomputedFunctionReturnSite>>,
);

fn op_eq(op: Op, expected: Op) -> bool {
    std::ptr::fn_addr_eq(op, expected)
}

pub(crate) fn rewrite_jump_op(op: Op) -> Option<Op> {
    structured_jump_rewrite(op).map(|rewrite| rewrite.ptr_op)
}

pub(crate) fn rewrite_loop_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_loop_empty as Op) {
        vm::op_loop_empty_precomputed
    } else if op_eq(op, vm::op_loop4 as Op) {
        vm::op_loop4_precomputed
    } else if op_eq(op, vm::op_loop8 as Op) {
        vm::op_loop8_precomputed
    } else if op_eq(op, vm::op_loop_generic as Op) || op_eq(op, vm::op_loop as Op) {
        vm::op_loop_generic_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_block_return_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::special_block_return_empty as Op) {
        vm::special_block_return_empty_precomputed
    } else if op_eq(op, vm::special_block_return4 as Op) {
        vm::special_block_return4_precomputed
    } else if op_eq(op, vm::special_block_return8 as Op) {
        vm::special_block_return8_precomputed
    } else if op_eq(op, vm::special_block_return_generic as Op)
        || op_eq(op, vm::special_block_return as Op)
    {
        vm::special_block_return_generic_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_function_return_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::special_function_return_empty as Op) {
        vm::special_function_return_empty_precomputed
    } else if op_eq(op, vm::special_function_return4 as Op) {
        vm::special_function_return4_precomputed
    } else if op_eq(op, vm::special_function_return8 as Op) {
        vm::special_function_return8_precomputed
    } else if op_eq(op, vm::special_function_return_generic as Op)
        || op_eq(op, vm::special_function_return as Op)
    {
        vm::special_function_return_generic_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_direct_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call as Op) {
        vm::op_call_precomputed
    } else if op_eq(op, vm::op_return_call as Op) {
        vm::op_return_call_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_import_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call_import as Op) {
        vm::op_call_import_precomputed
    } else if op_eq(op, vm::op_return_call_import as Op) {
        vm::op_return_call_import_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_indirect_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call_indirect as Op) {
        vm::op_call_indirect_precomputed
    } else if op_eq(op, vm::op_return_call_indirect as Op) {
        vm::op_return_call_indirect_precomputed
    } else {
        return None;
    })
}

pub(crate) fn rewrite_wait_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_memory_atomic_wait32_shared as Op) {
        vm::op_memory_atomic_wait32_shared_precomputed
    } else if op_eq(op, vm::op_memory_atomic_wait32_indexed_shared as Op) {
        vm::op_memory_atomic_wait32_indexed_shared_precomputed
    } else if op_eq(op, vm::op_memory_atomic_wait64_shared as Op) {
        vm::op_memory_atomic_wait64_shared_precomputed
    } else if op_eq(op, vm::op_memory_atomic_wait64_indexed_shared as Op) {
        vm::op_memory_atomic_wait64_indexed_shared_precomputed
    } else {
        return None;
    })
}

enum ControlRuntimeSite {
    Loop(PrecomputedLoopSite),
    BlockReturn(PrecomputedBlockReturnSite),
}

fn control_flow_metadata_to_loop_and_block_sites(
    frame_layout: &FrameLayoutHeader,
    canonical: &[Instr],
) -> Vec<ControlRuntimeSite> {
    let mut sites = Vec::new();
    for stack_map_site in frame_layout.cold().stack_map_sites.iter() {
        let start = stack_map_site.instruction_ordinal as usize;
        let Some(op) = canonical.get(start).map(|instr| unsafe { instr.op }) else {
            continue;
        };
        let Some(unwind_site) = frame_layout.unwind_site(stack_map_site.instruction_ordinal) else {
            continue;
        };
        match stack_map_site.kind {
            StackMapSafepointKind::Loop => {
                if rewrite_loop_op(op).is_none() {
                    continue;
                }
                let loop_param = unsafe { canonical[start + 1].operand.loop_param };
                sites.push(ControlRuntimeSite::Loop(PrecomputedLoopSite::new(
                    stack_map_site.instruction_ordinal,
                    loop_param.param_size(),
                    loop_param.param_shape(),
                    stack_map_site as *const _ as usize,
                    unwind_site as *const _ as usize,
                )));
            }
            StackMapSafepointKind::BlockReturn => {
                if rewrite_block_return_op(op).is_none() {
                    continue;
                }
                let block_return = unsafe { canonical[start + 1].operand.block_return };
                sites.push(ControlRuntimeSite::BlockReturn(
                    PrecomputedBlockReturnSite::new(
                        stack_map_site.instruction_ordinal,
                        block_return.return_size(),
                        block_return.return_shape(),
                        stack_map_site as *const _ as usize,
                        unwind_site as *const _ as usize,
                    ),
                ));
            }
            _ => {}
        }
    }
    sites
}

pub(crate) fn build_function_return_site(
    frame_layout: &FrameLayoutHeader,
) -> Option<Arc<PrecomputedFunctionReturnSite>> {
    frame_layout
        .cold()
        .stack_map_sites
        .iter()
        .find(|site| site.kind == StackMapSafepointKind::FunctionReturn)
        .and_then(|stack_map_site| {
            frame_layout
                .unwind_site(stack_map_site.instruction_ordinal)
                .map(|unwind_site| {
                    Arc::new(PrecomputedFunctionReturnSite::new(
                        stack_map_site.instruction_ordinal,
                        SafepointMetadataCache::new(
                            stack_map_site as *const _ as usize,
                            unwind_site as *const _ as usize,
                        ),
                    ))
                })
        })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn build_precomputed_runtime_sites(
    canonical: &[Instr],
    frame_layout: &FrameLayoutHeader,
    current_funcidx: u32,
    caller_instance: InstanceId,
    funcs: &[ObjectRef],
    function_return_sites: &[Option<Arc<PrecomputedFunctionReturnSite>>],
    function_type_identities: &[FuncTypeIdentity],
    gc: &StoreInner,
    allow_local_direct_precompute: bool,
) -> PrecomputedSiteSets {
    let mut direct_sites = Vec::new();
    let mut import_sites = Vec::new();
    let mut indirect_sites = Vec::new();
    let mut wait_sites = Vec::new();
    let mut loop_sites = Vec::new();
    let mut block_return_sites = Vec::new();

    for (instruction_ordinal, instr) in canonical.iter().enumerate() {
        let op = unsafe { instr.op };
        let safepoint_instruction_ordinal =
            frame_layout.instruction_ordinal_for_raw_start(instruction_ordinal);
        let stack_map_site_addr = safepoint_instruction_ordinal
            .and_then(|ordinal| frame_layout.stack_map_site(ordinal))
            .map_or(0, |site| site as *const _ as usize);
        let unwind_site_addr = safepoint_instruction_ordinal
            .and_then(|ordinal| frame_layout.unwind_site(ordinal))
            .map_or(0, |site| site as *const _ as usize);
        let safepoint_kind = safepoint_instruction_ordinal
            .and_then(|ordinal| frame_layout.stack_map_site(ordinal))
            .map(|site| site.kind);
        if rewrite_direct_call_op(op).is_some() {
            let funcidx = unsafe { canonical[instruction_ordinal + 1].operand.u32 };
            let funcaddr = funcs[funcidx as usize];
            let funcinst = gc.get_func(funcaddr);
            let import_like = matches!(
                safepoint_kind,
                Some(StackMapSafepointKind::CallImport | StackMapSafepointKind::ReturnCallImport)
            );
            if import_like
                || !allow_local_direct_precompute
                || funcinst.instance != caller_instance
                || funcinst.wasm_metadata().is_none()
            {
                import_sites.push(PrecomputedImportCallSite::new(
                    instruction_ordinal as u32,
                    funcidx,
                    StablePc::from_relative_index(instruction_ordinal + 2),
                    SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
                ));
                continue;
            }
            let memory0 = gc
                .instance(funcinst.instance)
                .memory_slots
                .first()
                .copied()
                .and_then(|slot| slot.handle());
            let (memory0_kind, memory0_raw) =
                crate::common::stack::CachedMemoryKind::from_memory_handle(memory0);
            let code_base_addr = {
                let metadata = funcinst
                    .wasm_metadata()
                    .expect("local wasm direct call must expose metadata");
                let canonical = funcinst
                    .canonical_code()
                    .expect("local wasm direct call must expose canonical code");
                let has_dynamic_rewrite = !metadata.control_flow_metadata.is_empty()
                    || canonical.iter().any(|instr| {
                        let op = unsafe { instr.op };
                        rewrite_direct_call_op(op).is_some()
                            || rewrite_import_call_op(op).is_some()
                            || rewrite_indirect_call_op(op).is_some()
                    });
                if has_dynamic_rewrite {
                    None
                } else {
                    Some(
                        funcinst
                            .code_pointer()
                            .expect("stable local wasm direct call must expose code")
                            as usize,
                    )
                }
            };
            direct_sites.push(PrecomputedDirectCallSite::new(
                instruction_ordinal as u32,
                StablePc::from_relative_index(instruction_ordinal + 2),
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
                PrecomputedCallFrame {
                    code_addr: funcaddr,
                    code_base_addr,
                    code_len: funcinst.code().map_or(0, |code| {
                        u32::try_from(code.len()).expect("code length overflow")
                    }),
                    function_return_site_addr: function_return_sites
                        .get(funcidx as usize)
                        .and_then(|site| site.as_ref())
                        .map_or(0, |site| site.as_ref() as *const _ as usize),
                    instance: funcinst.instance,
                    memory0_kind,
                    memory0_raw,
                },
                funcinst.execution.param_stack_bytes,
                funcinst.execution.param_shape,
                funcinst
                    .frame_layout_header()
                    .map_or(0, |layout| layout as *const FrameLayoutHeader as usize),
            ));
        } else if rewrite_import_call_op(op).is_some() {
            let funcidx = unsafe { canonical[instruction_ordinal + 1].operand.u32 };
            import_sites.push(PrecomputedImportCallSite::new(
                instruction_ordinal as u32,
                funcidx,
                StablePc::from_relative_index(instruction_ordinal + 2),
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
            ));
        } else if rewrite_indirect_call_op(op).is_some() {
            let tableidx = unsafe { canonical[instruction_ordinal + 1].operand.u32 };
            let expected_typeidx = unsafe { canonical[instruction_ordinal + 2].operand.u32 };
            indirect_sites.push(PrecomputedIndirectCallSite::new(
                instruction_ordinal as u32,
                StablePc::from_relative_index(instruction_ordinal + 3),
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
                tableidx,
                function_type_identities
                    .get(expected_typeidx as usize)
                    .expect("validated indirect call type")
                    as *const FuncTypeIdentity as usize,
            ));
        } else if rewrite_wait_op(op).is_some() {
            let memarg = unsafe { canonical[instruction_ordinal + 1].operand.memarg };
            let indexed = op_eq(op, vm::op_memory_atomic_wait32_indexed_shared as Op)
                || op_eq(op, vm::op_memory_atomic_wait64_indexed_shared as Op);
            wait_sites.push(PrecomputedWaitSite::new(
                instruction_ordinal as u32,
                StablePc::from_relative_index(instruction_ordinal + if indexed { 3 } else { 2 }),
                SafepointMetadataCache::new(stack_map_site_addr, unwind_site_addr),
                memarg,
                if indexed {
                    unsafe { canonical[instruction_ordinal + 2].operand.u32 }
                } else {
                    0
                },
            ));
        }
    }

    for site in control_flow_metadata_to_loop_and_block_sites(frame_layout, canonical) {
        match site {
            ControlRuntimeSite::Loop(loop_site) => loop_sites.push(loop_site),
            ControlRuntimeSite::BlockReturn(block_site) => block_return_sites.push(block_site),
        }
    }

    let function_return_site = function_return_sites
        .get(current_funcidx as usize)
        .cloned()
        .unwrap_or(None);

    (
        direct_sites.into(),
        import_sites.into(),
        indirect_sites.into(),
        wait_sites.into(),
        loop_sites.into(),
        block_return_sites.into(),
        function_return_site,
    )
}
