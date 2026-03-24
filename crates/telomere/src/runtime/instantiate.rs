use std::sync::Arc;

use crate::{
    common::{
        execute_elem_init_const_expr, store::FunctionBody as RuntimeFunctionBody,
        AsyncHostFunction, AsyncHostFunctionDefinition, AsyncNativeModule, CallFrameCache,
        CodeSection, ConstExpr, ControlFlowMetadataKind, ControlFlowMetadataSite, DataMode,
        DataSection, ElemInit, ElemMode, ElementSection, ExecuteContext, Export, ExportDesc,
        ExportSection, FrameLayoutHeader, FuncIdx, FuncType, FunctionBody, FunctionInstanceData,
        GlobalIdx, HostFunction, HostFunctionDefinition, ImportDesc, ImportSection, InstanceData,
        InstanceHandle, Instr, Limits, LocalReference, MemIdx, ModuleInstance, NativeModule,
        ObjectRef, Op, Operand, StablePc, StackMapSafepointKind, StoreInner, TableIdx, TypeIdx,
        TypeSection, PAGE_SIZE_MAX,
    },
    runtime::{
        scheduler::{ReadyFlag, Scheduler, Task},
        vm,
    },
    Instance, Module, Registry, Stack, Store, VMResult,
};

use crate::common::store::{FunctionExecutionMetadata, FunctionKind, WasmExecutionMetadata};
use crate::common::store::{
    InstanceId, PrecomputedBlockReturnSite, PrecomputedCallFrame, PrecomputedDirectCallSite,
    PrecomputedFunctionReturnSite, PrecomputedImportCallSite, PrecomputedIndirectCallSite,
    PrecomputedLoopSite, PrecomputedWaitSite,
};

#[derive(Clone)]
struct PendingWasmDerivedData {
    func_addr: ObjectRef,
    canonical_code: Arc<[Instr]>,
    control_flow_metadata: Arc<[ControlFlowMetadataSite]>,
}

type PrecomputedSiteSets = (
    Arc<[PrecomputedDirectCallSite]>,
    Arc<[PrecomputedImportCallSite]>,
    Arc<[PrecomputedIndirectCallSite]>,
    Arc<[PrecomputedWaitSite]>,
    Arc<[PrecomputedLoopSite]>,
    Arc<[PrecomputedBlockReturnSite]>,
    Option<Arc<PrecomputedFunctionReturnSite>>,
);

struct RuntimeSiteRefs<'a> {
    direct_call_sites: &'a [PrecomputedDirectCallSite],
    import_call_sites: &'a [PrecomputedImportCallSite],
    indirect_call_sites: &'a [PrecomputedIndirectCallSite],
    wait_sites: &'a [PrecomputedWaitSite],
    loop_sites: &'a [PrecomputedLoopSite],
    block_return_sites: &'a [PrecomputedBlockReturnSite],
    function_return_site: Option<&'a PrecomputedFunctionReturnSite>,
}

fn shape_for_result_type(ty: &crate::common::ResultType) -> crate::common::ReturnShape {
    match (ty.0.first(), ty.0.get(1)) {
        (None, _) => crate::common::ReturnShape::Empty,
        (Some(value), None) => crate::common::ReturnShape::from_size(value.stack_size().u32()),
        _ => crate::common::ReturnShape::Generic,
    }
}

fn build_execution_metadata(
    typeidx: TypeIdx,
    ft: &FuncType,
    kind: FunctionKind,
) -> FunctionExecutionMetadata {
    FunctionExecutionMetadata {
        kind,
        typeidx,
        type_identity: ft.identity(),
        param_stack_bytes: ft.param_stack_byte_size(),
        param_shape: shape_for_result_type(&ft.0),
        result_stack_bytes: ft.result_stack_byte_size(),
        result_shape: shape_for_result_type(&ft.1),
    }
}

fn op_eq(op: Op, expected: Op) -> bool {
    std::ptr::fn_addr_eq(op, expected)
}

fn rewrite_jump_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_br as Op) {
        vm::op_br_ptr
    } else if op_eq(op, vm::op_br_if as Op) {
        vm::op_br_if_ptr
    } else if op_eq(op, vm::op_br_if_r0 as Op) {
        vm::op_br_if_ptr_r0
    } else if op_eq(op, vm::op_br_if_r1 as Op) {
        vm::op_br_if_ptr_r1
    } else if op_eq(op, vm::op_br_if_r2 as Op) {
        vm::op_br_if_ptr_r2
    } else if op_eq(op, vm::op_br_if_r3 as Op) {
        vm::op_br_if_ptr_r3
    } else if op_eq(op, vm::op_if as Op) {
        vm::op_if_ptr
    } else if op_eq(op, vm::op_else as Op) {
        vm::op_else_ptr
    } else if op_eq(op, vm::op_br_table as Op) {
        vm::op_br_table_ptr
    } else if op_eq(op, vm::op_i32_local_br_if as Op) {
        vm::op_i32_local_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_eqz_br_if as Op) {
        vm::op_i32_local_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_if as Op) {
        vm::op_i32_local_if_ptr
    } else if op_eq(op, vm::op_i32_local_eqz_if as Op) {
        vm::op_i32_local_eqz_if_ptr
    } else if op_eq(op, vm::op_i64_local_br_if as Op) {
        vm::op_i64_local_br_if_ptr
    } else if op_eq(op, vm::op_i64_local_eqz_br_if as Op) {
        vm::op_i64_local_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i64_local_if as Op) {
        vm::op_i64_local_if_ptr
    } else if op_eq(op, vm::op_i64_local_eqz_if as Op) {
        vm::op_i64_local_eqz_if_ptr
    } else if op_eq(op, vm::op_i32_local_and_imm_br_if as Op) {
        vm::op_i32_local_and_imm_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_and_imm_eqz_br_if as Op) {
        vm::op_i32_local_and_imm_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_and_imm_if as Op) {
        vm::op_i32_local_and_imm_if_ptr
    } else if op_eq(op, vm::op_i32_local_and_imm_eqz_if as Op) {
        vm::op_i32_local_and_imm_eqz_if_ptr
    } else if op_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if as Op) {
        vm::op_i32_local_addr_load8_u_and_imm_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_addr_load8_u_and_imm_eqz_if as Op) {
        vm::op_i32_local_addr_load8_u_and_imm_eqz_if_ptr
    } else if op_eq(op, vm::op_i32_seed_tee_eqz_br_if as Op) {
        vm::op_i32_seed_tee_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i32_seed_tee_eqz_if as Op) {
        vm::op_i32_seed_tee_eqz_if_ptr
    } else if op_eq(op, vm::op_i64_seed_tee_eqz_br_if as Op) {
        vm::op_i64_seed_tee_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i64_seed_tee_eqz_if as Op) {
        vm::op_i64_seed_tee_eqz_if_ptr
    } else if op_eq(op, vm::op_i32_seed_tee_imm_compare_br_if as Op) {
        vm::op_i32_seed_tee_imm_compare_br_if_ptr
    } else if op_eq(op, vm::op_i32_seed_tee_imm_compare_if as Op) {
        vm::op_i32_seed_tee_imm_compare_if_ptr
    } else if op_eq(op, vm::op_i64_seed_tee_imm_compare_br_if as Op) {
        vm::op_i64_seed_tee_imm_compare_br_if_ptr
    } else if op_eq(op, vm::op_i64_seed_tee_imm_compare_if as Op) {
        vm::op_i64_seed_tee_imm_compare_if_ptr
    } else if op_eq(op, vm::op_i32_seed_imm_and_br_if as Op) {
        vm::op_i32_seed_imm_and_br_if_ptr
    } else if op_eq(op, vm::op_i32_seed_imm_and_eqz_br_if as Op) {
        vm::op_i32_seed_imm_and_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i32_seed_imm_and_if as Op) {
        vm::op_i32_seed_imm_and_if_ptr
    } else if op_eq(op, vm::op_i32_seed_imm_and_eqz_if as Op) {
        vm::op_i32_seed_imm_and_eqz_if_ptr
    } else if op_eq(op, vm::op_i64_seed_imm_and_br_if as Op) {
        vm::op_i64_seed_imm_and_br_if_ptr
    } else if op_eq(op, vm::op_i64_seed_imm_and_eqz_br_if as Op) {
        vm::op_i64_seed_imm_and_eqz_br_if_ptr
    } else if op_eq(op, vm::op_i64_seed_imm_and_if as Op) {
        vm::op_i64_seed_imm_and_if_ptr
    } else if op_eq(op, vm::op_i64_seed_imm_and_eqz_if as Op) {
        vm::op_i64_seed_imm_and_eqz_if_ptr
    } else if op_eq(op, vm::op_i32_local_local_ge_u_br_if as Op) {
        vm::op_i32_local_local_ge_u_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_local_compare_br_if as Op) {
        vm::op_i32_local_local_compare_br_if_ptr
    } else if op_eq(op, vm::op_i32_local_const_compare_br_if as Op) {
        vm::op_i32_local_const_compare_br_if_ptr
    } else if op_eq(op, vm::op_i64_local_local_compare_br_if as Op) {
        vm::op_i64_local_local_compare_br_if_ptr
    } else if op_eq(op, vm::op_i64_local_const_compare_br_if as Op) {
        vm::op_i64_local_const_compare_br_if_ptr
    } else if op_eq(op, vm::op_f32_local_local_compare_br_if as Op) {
        vm::op_f32_local_local_compare_br_if_ptr
    } else if op_eq(op, vm::op_f32_local_const_compare_br_if as Op) {
        vm::op_f32_local_const_compare_br_if_ptr
    } else if op_eq(op, vm::op_f64_local_local_compare_br_if as Op) {
        vm::op_f64_local_local_compare_br_if_ptr
    } else if op_eq(op, vm::op_f64_local_const_compare_br_if as Op) {
        vm::op_f64_local_const_compare_br_if_ptr
    } else {
        return None;
    })
}

fn rewrite_loop_op(op: Op) -> Option<Op> {
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

fn rewrite_block_return_op(op: Op) -> Option<Op> {
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

fn rewrite_function_return_op(op: Op) -> Option<Op> {
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

fn rewrite_direct_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call as Op) {
        vm::op_call_precomputed
    } else if op_eq(op, vm::op_return_call as Op) {
        vm::op_return_call_precomputed
    } else {
        return None;
    })
}

fn rewrite_import_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call_import as Op) {
        vm::op_call_import_precomputed
    } else if op_eq(op, vm::op_return_call_import as Op) {
        vm::op_return_call_import_precomputed
    } else {
        return None;
    })
}

fn rewrite_indirect_call_op(op: Op) -> Option<Op> {
    Some(if op_eq(op, vm::op_call_indirect as Op) {
        vm::op_call_indirect_precomputed
    } else if op_eq(op, vm::op_return_call_indirect as Op) {
        vm::op_return_call_indirect_precomputed
    } else {
        return None;
    })
}

fn rewrite_wait_op(op: Op) -> Option<Op> {
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

fn build_function_return_site(
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
                    Arc::new(PrecomputedFunctionReturnSite {
                        instruction_ordinal: stack_map_site.instruction_ordinal,
                        stack_map_site_addr: stack_map_site as *const _ as usize,
                        unwind_site_addr: unwind_site as *const _ as usize,
                    })
                })
        })
}

#[allow(clippy::too_many_arguments)]
fn build_precomputed_runtime_sites(
    canonical: &[Instr],
    frame_layout: &FrameLayoutHeader,
    current_funcidx: u32,
    caller_instance: InstanceId,
    funcs: &[ObjectRef],
    function_return_sites: &[Option<Arc<PrecomputedFunctionReturnSite>>],
    function_type_identities: &[crate::common::FuncTypeIdentity],
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
                import_sites.push(PrecomputedImportCallSite {
                    instruction_ordinal: instruction_ordinal as u32,
                    funcidx,
                    return_pc: StablePc::from_relative_index(instruction_ordinal + 2),
                    stack_map_site_addr,
                    unwind_site_addr,
                });
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
                    0
                } else {
                    funcinst
                        .code_pointer()
                        .expect("stable local wasm direct call must expose code")
                        as usize
                }
            };
            direct_sites.push(PrecomputedDirectCallSite {
                instruction_ordinal: instruction_ordinal as u32,
                return_pc: StablePc::from_relative_index(instruction_ordinal + 2),
                frame: PrecomputedCallFrame {
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
                param_bytes: funcinst.execution.param_stack_bytes,
                param_shape: funcinst.execution.param_shape,
                callee_layout_addr: funcinst
                    .frame_layout_header()
                    .map_or(0, |layout| layout as *const FrameLayoutHeader as usize),
                stack_map_site_addr,
                unwind_site_addr,
            });
        } else if rewrite_import_call_op(op).is_some() {
            let funcidx = unsafe { canonical[instruction_ordinal + 1].operand.u32 };
            import_sites.push(PrecomputedImportCallSite {
                instruction_ordinal: instruction_ordinal as u32,
                funcidx,
                return_pc: StablePc::from_relative_index(instruction_ordinal + 2),
                stack_map_site_addr,
                unwind_site_addr,
            });
        } else if rewrite_indirect_call_op(op).is_some() {
            let tableidx = unsafe { canonical[instruction_ordinal + 1].operand.u32 };
            let expected_typeidx = unsafe { canonical[instruction_ordinal + 2].operand.u32 };
            indirect_sites.push(PrecomputedIndirectCallSite {
                instruction_ordinal: instruction_ordinal as u32,
                return_pc: StablePc::from_relative_index(instruction_ordinal + 3),
                tableidx,
                expected_type_identity_addr: function_type_identities
                    .get(expected_typeidx as usize)
                    .expect("validated indirect call type")
                    as *const crate::common::FuncTypeIdentity
                    as usize,
                stack_map_site_addr,
                unwind_site_addr,
            });
        } else if rewrite_wait_op(op).is_some() {
            let memarg = unsafe { canonical[instruction_ordinal + 1].operand.memarg };
            let indexed = op_eq(op, vm::op_memory_atomic_wait32_indexed_shared as Op)
                || op_eq(op, vm::op_memory_atomic_wait64_indexed_shared as Op);
            wait_sites.push(PrecomputedWaitSite {
                instruction_ordinal: instruction_ordinal as u32,
                resume_pc: StablePc::from_relative_index(
                    instruction_ordinal + if indexed { 3 } else { 2 },
                ),
                memarg,
                memidx: if indexed {
                    unsafe { canonical[instruction_ordinal + 2].operand.u32 }
                } else {
                    0
                },
                stack_map_site_addr,
                unwind_site_addr,
            });
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

fn rebuild_wasm_derived_data_for_instance(instance_addr: ObjectRef, gc: &mut StoreInner) {
    let (funcs, function_type_identities) = {
        let instance = unsafe { &*gc.get_instance_unchecked(instance_addr) };
        let funcs = instance.funcs.clone();
        let function_type_identities = gc
            .get_module(instance.module_addr)
            .function_type_identities
            .clone();
        (funcs, function_type_identities)
    };
    let function_return_sites = funcs
        .iter()
        .map(|&funcaddr| {
            gc.get_func(funcaddr)
                .frame_layout_header()
                .and_then(build_function_return_site)
        })
        .collect::<Vec<_>>();

    let mut rebuilt = Vec::new();
    for &funcaddr in &funcs {
        let func = gc.get_func(funcaddr);
        let Some(metadata) = func.wasm_metadata() else {
            continue;
        };
        let canonical_code = func
            .canonical_code_arc()
            .expect("wasm function must retain canonical code");
        let (
            direct_call_sites,
            import_call_sites,
            indirect_call_sites,
            wait_sites,
            loop_sites,
            block_return_sites,
            function_return_site,
        ) = build_precomputed_runtime_sites(
            canonical_code.as_ref(),
            metadata.frame_layout_header(),
            func.funcidx,
            func.instance,
            funcs.as_slice(),
            function_return_sites.as_slice(),
            function_type_identities.as_slice(),
            gc,
            false,
        );
        let derived_code = build_derived_code(
            &canonical_code,
            metadata.control_flow_metadata.as_ref(),
            RuntimeSiteRefs {
                direct_call_sites: direct_call_sites.as_ref(),
                import_call_sites: import_call_sites.as_ref(),
                indirect_call_sites: indirect_call_sites.as_ref(),
                wait_sites: wait_sites.as_ref(),
                loop_sites: loop_sites.as_ref(),
                block_return_sites: block_return_sites.as_ref(),
                function_return_site: function_return_site.as_deref(),
            },
        );
        rebuilt.push((
            funcaddr,
            direct_call_sites,
            import_call_sites,
            indirect_call_sites,
            wait_sites,
            loop_sites,
            block_return_sites,
            function_return_site,
            derived_code,
        ));
    }

    for (
        funcaddr,
        direct_call_sites,
        import_call_sites,
        indirect_call_sites,
        wait_sites,
        loop_sites,
        block_return_sites,
        function_return_site,
        derived_code,
    ) in rebuilt
    {
        let func = gc.get_func_mut(funcaddr);
        func.set_precomputed_runtime_sites(
            direct_call_sites,
            import_call_sites,
            indirect_call_sites,
            wait_sites,
            loop_sites,
            block_return_sites,
            function_return_site,
        );
        if let Some(derived_code) = derived_code {
            func.set_derived_code(derived_code);
        } else {
            func.clear_derived_code();
        }
    }
}

fn build_derived_code(
    canonical: &Arc<[Instr]>,
    control_flow_metadata: &[ControlFlowMetadataSite],
    runtime_sites: RuntimeSiteRefs<'_>,
) -> Option<Arc<[Instr]>> {
    if control_flow_metadata.is_empty()
        && runtime_sites.direct_call_sites.is_empty()
        && runtime_sites.import_call_sites.is_empty()
        && runtime_sites.indirect_call_sites.is_empty()
        && runtime_sites.wait_sites.is_empty()
        && runtime_sites.loop_sites.is_empty()
        && runtime_sites.block_return_sites.is_empty()
        && runtime_sites.function_return_site.is_none()
    {
        return None;
    }

    let mut derived: Arc<[Instr]> = canonical.iter().copied().collect::<Vec<_>>().into();
    let base = derived.as_ptr();
    let code = Arc::get_mut(&mut derived).expect("derived code must be uniquely owned");

    for site in control_flow_metadata {
        let start = site.instruction_ordinal as usize;
        let op = unsafe { code[start].op };
        match &site.kind {
            ControlFlowMetadataKind::Jump {
                jump_operand_slots,
                target_ordinals,
            } => {
                let ptr_op =
                    rewrite_jump_op(op).expect("jump metadata must target a jump-capable op");
                code[start] = Instr { op: ptr_op };
                for (&slot, &target) in jump_operand_slots.iter().zip(target_ordinals.iter()) {
                    let target_ptr = unsafe { base.add(target as usize) } as usize;
                    code[start + usize::from(slot)] = Instr {
                        operand: Operand {
                            code_ptr: target_ptr,
                        },
                    };
                }
            }
            ControlFlowMetadataKind::Loop { .. } => {
                if let Some(site) = runtime_sites.loop_sites.iter().find(|runtime_site| {
                    runtime_site.instruction_ordinal == site.instruction_ordinal
                }) {
                    let ptr_op = rewrite_loop_op(op).expect("loop metadata must target a loop op");
                    code[start] = Instr { op: ptr_op };
                    code[start + 1] = Instr {
                        operand: Operand {
                            code_ptr: site as *const PrecomputedLoopSite as usize,
                        },
                    };
                }
            }
            ControlFlowMetadataKind::BlockReturn { .. } => {
                if let Some(site) = runtime_sites
                    .block_return_sites
                    .iter()
                    .find(|runtime_site| {
                        runtime_site.instruction_ordinal == site.instruction_ordinal
                    })
                {
                    let ptr_op = rewrite_block_return_op(op)
                        .expect("block-return metadata must target a block-return op");
                    code[start] = Instr { op: ptr_op };
                    code[start + 1] = Instr {
                        operand: Operand {
                            code_ptr: site as *const PrecomputedBlockReturnSite as usize,
                        },
                    };
                }
            }
        }
    }

    for site in runtime_sites.direct_call_sites {
        let start = site.instruction_ordinal as usize;
        let ptr_op =
            rewrite_direct_call_op(unsafe { code[start].op }).expect("direct-call rewrite target");
        code[start] = Instr { op: ptr_op };
        code[start + 1] = Instr {
            operand: Operand {
                code_ptr: site as *const PrecomputedDirectCallSite as usize,
            },
        };
    }

    for site in runtime_sites.import_call_sites {
        let start = site.instruction_ordinal as usize;
        let ptr_op =
            rewrite_import_call_op(unsafe { code[start].op }).expect("import-call rewrite target");
        code[start] = Instr { op: ptr_op };
        code[start + 1] = Instr {
            operand: Operand {
                code_ptr: site as *const PrecomputedImportCallSite as usize,
            },
        };
    }

    for site in runtime_sites.indirect_call_sites {
        let start = site.instruction_ordinal as usize;
        let ptr_op = rewrite_indirect_call_op(unsafe { code[start].op })
            .expect("indirect-call rewrite target");
        code[start] = Instr { op: ptr_op };
        code[start + 1] = Instr {
            operand: Operand {
                code_ptr: site as *const PrecomputedIndirectCallSite as usize,
            },
        };
    }

    for site in runtime_sites.wait_sites {
        let start = site.instruction_ordinal as usize;
        let ptr_op = rewrite_wait_op(unsafe { code[start].op }).expect("wait rewrite target");
        code[start] = Instr { op: ptr_op };
        code[start + 1] = Instr {
            operand: Operand {
                code_ptr: site as *const PrecomputedWaitSite as usize,
            },
        };
    }

    if let Some(site) = runtime_sites.function_return_site {
        let start = code
            .get(site.instruction_ordinal as usize)
            .and_then(|instr| {
                rewrite_function_return_op(unsafe { instr.op })
                    .map(|ptr_op| (site.instruction_ordinal as usize, ptr_op))
            })
            .or_else(|| {
                code.iter().enumerate().find_map(|(index, instr)| {
                    rewrite_function_return_op(unsafe { instr.op }).map(|ptr_op| (index, ptr_op))
                })
            })
            .expect("function-return rewrite target");
        code[start.0] = Instr { op: start.1 };
    }

    Some(derived)
}

pub(crate) fn init_global(
    gc: &mut StoreInner,
    init: &ConstExpr,
    globals: &[ObjectRef],
    funcs: &[ObjectRef],
) -> VMResult<ObjectRef> {
    tracing::trace!("global init: {init:?}");

    let res = match init {
        ConstExpr::I32(v) => gc.new_global_data4(*v as u32),
        ConstExpr::I64(v) => gc.new_global_data8(*v as u64),
        ConstExpr::F32(v) => gc.new_global_data4(v.to_bits()),
        ConstExpr::F64(v) => gc.new_global_data8(v.to_bits()),
        ConstExpr::V128(v) => gc.new_global_data16(*v),

        ConstExpr::RefNull(_t) => gc.new_global_ref(ObjectRef(0)),
        ConstExpr::FuncRef(v) => {
            let addr = funcs.get(*v as usize);
            if let Some(addr) = addr {
                gc.new_global_ref(*addr)
            } else {
                return VMResult::InvalidOperand;
            }
        }
        ConstExpr::GlobalGet(idx) => {
            let idx = *idx as usize;
            let addr = globals[idx];
            gc.copy_object(addr)
        }
    };
    VMResult::Success(res)
}
fn validate_limit(import_limit: Limits, real: u32, export_limit: Limits) -> VMResult<()> {
    if import_limit.min > real {
        tracing::trace!("invalid import_limit min");

        return VMResult::Unlinkable;
    }
    match export_limit.max {
        None => {
            if import_limit.max.is_some() {
                tracing::trace!("invalid import_limit max");

                return VMResult::Unlinkable;
            }
        }
        Some(export_max) => {
            if let Some(import_max) = import_limit.max {
                if export_max > import_max {
                    tracing::trace!("invalid import_limit max");

                    return VMResult::Unlinkable;
                }
            }
        }
    }
    VMResult::Success(())
}
fn execute_offset_const_expr(
    gc: &mut StoreInner,
    globals: &[ObjectRef],
    exprs: &[ConstExpr],
) -> VMResult<u32> {
    if exprs.len() != 1 {
        return VMResult::Unlinkable;
    }
    match &exprs[0] {
        ConstExpr::I32(v) => VMResult::Success(*v as u32),
        ConstExpr::GlobalGet(idx) => {
            let addr = *vm_try!(VMResult::from_option(globals.get(*idx as usize), || {
                VMResult::Unlinkable
            }));
            let Ok(buf): Result<[u8; 4], _> = gc.get_global(addr).try_into() else {
                return VMResult::Unlinkable;
            };
            VMResult::Success(u32::from_le_bytes(buf))
        }
        _ => VMResult::Unlinkable,
    }
}

fn convert_native_module_to_module(m: NativeModule) -> Module {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    for HostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(fp));
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    Module {
        codes: CodeSection(codes),
        functions,
        fts: TypeSection(fts),
        data: DataSection(vec![]),
        elems: ElementSection(vec![]),
        imports: ImportSection(vec![]),
        mems: vec![],
        globals: vec![],
        global_init: vec![],
        exs: ExportSection(exs),
        tables: vec![],
        start: None,
        name: None,
    }
}

fn async_host_placeholder(_ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    VMResult::Unreachable
}

fn convert_async_native_module_to_module(m: AsyncNativeModule) -> (Module, Vec<AsyncHostFunction>) {
    let mut codes = vec![];
    let mut functions = vec![];
    let mut fts = vec![];
    let mut exs = vec![];
    let mut async_functions = Vec::with_capacity(m.functions.len());
    for AsyncHostFunctionDefinition {
        fp,
        name,
        signature,
    } in m.functions.into_iter()
    {
        let funcidx = functions.len();
        functions.push(TypeIdx(fts.len() as u32));
        fts.push(signature);
        codes.push(FunctionBody::Host(async_host_placeholder));
        async_functions.push(fp);
        if let Some(name) = name {
            exs.push(Export(name, ExportDesc::Func(FuncIdx(funcidx as u32))));
        }
    }
    (
        Module {
            codes: CodeSection(codes),
            functions,
            fts: TypeSection(fts),
            data: DataSection(vec![]),
            elems: ElementSection(vec![]),
            imports: ImportSection(vec![]),
            mems: vec![],
            globals: vec![],
            global_init: vec![],
            exs: ExportSection(exs),
            tables: vec![],
            start: None,
            name: None,
        },
        async_functions,
    )
}
pub async fn instantiate_native_module(
    m: NativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    instantiate(convert_native_module_to_module(m), store, registry).await
}

pub async fn instantiate_native_async_module(
    m: AsyncNativeModule,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    let (module, async_functions) = convert_async_native_module_to_module(m);
    let instance = vm_try!(instantiate(module, store, registry).await);
    for (funcidx, fp) in async_functions.into_iter().enumerate() {
        link_async_host_function_with_function_idx(&instance, funcidx as u32, fp, store);
    }
    VMResult::Success(instance)
}

pub async fn instantiate(
    m: Module,
    store: &Store,
    registry: &Registry,
) -> VMResult<InstanceHandle> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("instantiate is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let Module {
        fts,
        functions,
        imports,
        mems,
        globals: m_globals,
        global_init,
        exs,
        tables: m_tables,
        elems: m_elems,
        codes,
        data,
        start,
        ..
    } = m;

    let mut scheduler = Scheduler::new(store);
    let (addr, has_start) = {
        let mut gc = store.lock_gc();
        let instance_id = store.new_instance_id();

        // -> addr
        let mut memories: Vec<ObjectRef> = Vec::new();
        let mut globals = vec![];
        let mut funcs: Vec<ObjectRef> = vec![];
        let mut tables = vec![];

        for import in &imports.0 {
            tracing::trace!("processing import: {import:?}");
            let ext_inst_addr =
                vm_try!(VMResult::from_option(registry.get(&import.module), || {
                    tracing::error!("unknown instance");
                    VMResult::Unlinkable
                }));
            let instance_object_ref = vm_try!(VMResult::from_option(
                ext_inst_addr.object_ref_for_store(store),
                || {
                    tracing::error!("instance handle belongs to another store");
                    VMResult::Unlinkable
                }
            ));
            let ext_inst = unsafe { &*gc.get_instance_unchecked(instance_object_ref) };
            let ext_module = gc.get_module(ext_inst.module_addr);
            let export = vm_try!(VMResult::from_option(
                ext_module.exports.find(&import.name),
                || {
                    tracing::error!("unknown export");
                    VMResult::Unlinkable
                }
            ));
            match (&import.desc, export) {
                (ImportDesc::TypeIdx(tidx), ExportDesc::Func(funcidx)) => {
                    let import_ft = fts.get(*tidx).unwrap();
                    let export_ft_idx = ext_module.functions[funcidx.0 as usize];
                    let export_ft = ext_module
                        .function_types
                        .get(export_ft_idx.0 as usize)
                        .unwrap();
                    if import_ft != export_ft {
                        tracing::trace!("import function type");
                        return VMResult::Unlinkable;
                    }
                    let funcaddr = ext_inst.funcs.as_slice()[funcidx.0 as usize];
                    let funcidx = funcs.len();
                    funcs.push(funcaddr);
                    tracing::trace!("linking: {funcidx} => {funcaddr:?}")
                }
                (ImportDesc::GlobalType(import_gt), ExportDesc::Global(global_idx)) => {
                    let export_gt = ext_module.globals.get(global_idx.0 as usize).unwrap();
                    if import_gt != export_gt {
                        tracing::trace!("import global type");
                        return VMResult::Unlinkable;
                    }
                    globals.push(ext_inst.globals.as_slice()[global_idx.0 as usize]);
                }
                (ImportDesc::TableType(import_tt), ExportDesc::Table(idx)) => {
                    let export_tt = ext_module.tables[idx.0 as usize];
                    tracing::trace!("{export_tt:?}");

                    if import_tt.reftype != export_tt.reftype {
                        tracing::trace!("import table type");
                        return VMResult::Unlinkable;
                    }
                    let addr = ext_inst.tables.as_slice()[idx.0 as usize];
                    vm_try!(validate_limit(
                        import_tt.limits,
                        gc.get_table(addr).1.len() as u32,
                        export_tt.limits
                    ));
                    tables.push(ext_inst.tables.as_slice()[idx.0 as usize]);
                }
                (ImportDesc::MemType(mt), ExportDesc::Mem(_idx)) => {
                    let memory_addr = *vm_try!(VMResult::from_option(
                        ext_inst.mems.as_slice().get(_idx.0 as usize),
                        || {
                            tracing::trace!("invalid instance memory");
                            VMResult::Unlinkable
                        }
                    ));
                    let limits = ext_module.mems[_idx.0 as usize];

                    if mt.shared != limits.shared {
                        tracing::trace!("import shared memory flag mismatch");
                        return VMResult::Unlinkable;
                    }
                    let handle = gc.memory_handle(memory_addr);
                    vm_try!(validate_limit(
                        mt.limits,
                        gc.memory_page_size(handle),
                        limits.limits
                    ));
                    memories.push(memory_addr);
                }
                _ => {
                    tracing::trace!("import other type objects");
                    return VMResult::Unlinkable;
                }
            }
        }

        let function_type_identities = fts.0.iter().map(FuncType::identity).collect::<Vec<_>>();
        let mod_addr = gc.new_module(ModuleInstance {
            function_types: fts.0.clone(),
            function_type_identities,
            functions: functions.clone(),
            exports: exs.clone(),
            tables: m_tables.clone(),
            globals: m_globals.clone(),
            mems: mems.clone(),
        });
        let inst_id = gc.alloc_instance(InstanceData {
            instance_id,
            module_addr: mod_addr,
            globals: Vec::new(),
            funcs: Vec::new(),
            tables: Vec::new(),
            mems: Vec::new(),
            memory_slots: Vec::new(),
        });
        let inst_addr = gc.object_ref_for_instance(inst_id);

        for mem in mems.iter().skip(memories.len()) {
            let limits = mem.limits;
            memories.push(if mem.shared {
                gc.new_shared_memory(limits.min, limits.max.unwrap_or(PAGE_SIZE_MAX as u32))
            } else {
                gc.new_memory(limits.min, limits.max.unwrap_or(PAGE_SIZE_MAX as u32))
            });
        }

        for (idx, d) in (0..).zip(data.0.into_iter()) {
            match &d.mode {
                DataMode::Active(mem, offset) => {
                    let offset =
                        vm_try!(execute_offset_const_expr(&mut gc, &globals, offset)) as usize;
                    let memory =
                        *vm_try!(VMResult::from_option(memories.get(mem.0 as usize), || {
                            VMResult::MemoryIndexOutOfRange
                        }));
                    vm_try!(gc.with_memory_by_addr(memory, |memory| {
                        if let Some(slice) = memory.get_mut(offset..offset + d.init.len()) {
                            slice.copy_from_slice(&d.init);
                            VMResult::Success(())
                        } else {
                            VMResult::MemoryIndexOutOfRange
                        }
                    }));
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
                DataMode::Passive => {
                    store.lock_segments().data.insert((instance_id, idx), d);
                }
            }
        }

        let mut pending_wasm_functions = Vec::new();
        for func in codes.0.into_iter() {
            let funcidx = funcs.len() as u32;
            let typeidx = functions[funcidx as usize];
            let ft = &fts.0[typeidx.0 as usize];

            let func_addr = match func {
                FunctionBody::Wasm(code) => {
                    let code_expr: Arc<[Instr]> = code.expr.into();
                    let func_addr = gc.new_func(&FunctionInstanceData {
                        instance: inst_id,
                        execution: build_execution_metadata(typeidx, ft, FunctionKind::Wasm),
                        body: RuntimeFunctionBody::Wasm {
                            locals: code.locals.clone(),
                            code: code_expr.clone(),
                            derived_code: None,
                            metadata: WasmExecutionMetadata {
                                code_base_addr: code_expr.as_ptr() as usize,
                                frame_layout: code.frame_layout.clone(),
                                frame_layout_addr: code.frame_layout.header() as *const _ as usize,
                                control_flow_metadata: code.control_flow_metadata.clone(),
                                derived_runtime_metadata: None,
                                function_return_site_addr: 0,
                            },
                        },
                        funcidx,
                    });
                    pending_wasm_functions.push(PendingWasmDerivedData {
                        func_addr,
                        canonical_code: code_expr,
                        control_flow_metadata: code.control_flow_metadata.clone(),
                    });
                    func_addr
                }
                FunctionBody::Host(fp) => gc.new_func(&FunctionInstanceData {
                    instance: inst_id,
                    execution: build_execution_metadata(typeidx, ft, FunctionKind::Host),
                    body: RuntimeFunctionBody::Host(fp),
                    funcidx,
                }),
            };

            funcs.push(func_addr);
            tracing::trace!("linking: {funcidx} => {func_addr:?}");
        }

        for init in &global_init {
            globals.push(vm_try!(init_global(&mut gc, init, &globals, &funcs)));
        }
        let mut table_instances: Vec<ObjectRef> =
            m_tables.iter().map(|v| gc.new_table(*v)).collect();
        tables.append(&mut table_instances);

        let res = (|| {
            for (idx, elem) in (0u32..).zip(m_elems.0.into_iter()) {
                match &elem.mode {
                    ElemMode::Active(idx, offset) => match &elem.init {
                        ElemInit::FuncIdx(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut gc, &globals, offset))
                                    as usize;
                            let table_addr = tables[idx.0 as usize];
                            let instance = gc.get_table(table_addr);

                            if instance.0.reftype != elem.kind {
                                panic!("reftype mismatch")
                            }
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            for (idx, funcidx) in idxs.iter().enumerate() {
                                instance.1[offset + idx] = funcs[*funcidx as usize].get();
                            }
                        }
                        ElemInit::ConstExpr(idxs) => {
                            let offset =
                                vm_try!(execute_offset_const_expr(&mut gc, &globals, offset))
                                    as usize;
                            let table_addr = tables[idx.0 as usize];
                            let instance = gc.get_table(table_addr);
                            if offset + idxs.len() > instance.1.len() {
                                return VMResult::TableIndexOutOfRange;
                            }
                            let rt = instance.0.reftype;

                            for (idx, idx_expr) in idxs.iter().enumerate() {
                                let elem_addr = vm_try!(execute_elem_init_const_expr(
                                    &mut gc, &globals, &funcs, idx_expr, rt
                                ));
                                let instance = gc.get_table(table_addr);
                                instance.1[offset + idx] = elem_addr.get();
                            }
                        }
                    },
                    ElemMode::Passive => {
                        store.lock_segments().elems.insert((instance_id, idx), elem);
                    }
                    ElemMode::Declarative => {}
                }
            }
            VMResult::Success(())
        })();

        let instance = Instance {
            module_addr: mod_addr,
            instance_id,
            memory: memories,
            tables,
            globals,
            funcs,
        };

        unsafe {
            gc.place_instance_unchecked(inst_addr, &instance);
        }
        let function_return_sites = instance
            .funcs
            .iter()
            .map(|&funcaddr| {
                gc.get_func(funcaddr)
                    .frame_layout_header()
                    .and_then(build_function_return_site)
            })
            .collect::<Vec<_>>();
        for pending in pending_wasm_functions {
            let pending_func = gc.get_func(pending.func_addr);
            let caller_instance = pending_func.instance;
            let frame_layout = pending_func
                .frame_layout_header()
                .expect("wasm function must expose frame layout");
            let function_type_identities = gc
                .get_module(instance.module_addr)
                .function_type_identities
                .as_slice();
            let (
                direct_call_sites,
                import_call_sites,
                indirect_call_sites,
                wait_sites,
                loop_sites,
                block_return_sites,
                function_return_site,
            ) = build_precomputed_runtime_sites(
                pending.canonical_code.as_ref(),
                frame_layout,
                pending_func.funcidx,
                caller_instance,
                instance.funcs.as_slice(),
                function_return_sites.as_slice(),
                function_type_identities,
                &gc,
                true,
            );
            let derived_code = build_derived_code(
                &pending.canonical_code,
                pending.control_flow_metadata.as_ref(),
                RuntimeSiteRefs {
                    direct_call_sites: direct_call_sites.as_ref(),
                    import_call_sites: import_call_sites.as_ref(),
                    indirect_call_sites: indirect_call_sites.as_ref(),
                    wait_sites: wait_sites.as_ref(),
                    loop_sites: loop_sites.as_ref(),
                    block_return_sites: block_return_sites.as_ref(),
                    function_return_site: function_return_site.as_deref(),
                },
            );
            let func = gc.get_func_mut(pending.func_addr);
            func.set_precomputed_runtime_sites(
                direct_call_sites,
                import_call_sites,
                indirect_call_sites,
                wait_sites,
                loop_sites,
                block_return_sites,
                function_return_site,
            );
            if let Some(derived_code) = derived_code {
                func.set_derived_code(derived_code);
            }
        }
        vm_try!(res);
        let addr = InstanceHandle::new(store, inst_id, instance_id);

        let has_start = if let Some(start) = start {
            let mut stack = Stack::new(128 * 1024);
            let funcaddr = instance.funcs[start.0 as usize];
            let funcinst = gc.get_func(funcaddr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(
                funcaddr,
                funcinst,
                func_instance
                    .memory_slots
                    .first()
                    .copied()
                    .and_then(|slot| slot.handle()),
            );
            if funcinst.is_host_func() {
                let local_reference = vm_try!(stack.function_call_raw(
                    0,
                    0,
                    frame,
                    LocalReference::empty(),
                    &vm::VM_END,
                    &gc,
                ));

                scheduler.push(Task {
                    task_id: 0,
                    stack,
                    local_reference,
                    current_frame: frame,
                    safepoint: crate::common::SafepointMetadataCache::EMPTY,
                    ready_flag: ReadyFlag::Ready,
                    fp: StablePc::from_stable_ptr(vm::START_HOST_FUNCTION_PROGRAM.as_ptr()),
                    pending_effects: 0,
                    terminal_result: None,
                });
            } else {
                let wasm_metadata = funcinst
                    .wasm_metadata()
                    .expect("wasm start function must expose metadata");
                let local_reference = vm_try!(stack.function_call_layout(
                    wasm_metadata.frame_layout.as_ref(),
                    frame,
                    LocalReference::empty(),
                    &vm::VM_END,
                    &gc,
                ));

                scheduler.push(Task {
                    fp: StablePc::from_relative_index(0),
                    task_id: 0,
                    stack,
                    local_reference,
                    current_frame: frame,
                    safepoint: crate::common::SafepointMetadataCache::EMPTY,
                    ready_flag: ReadyFlag::Ready,
                    pending_effects: 0,
                    terminal_result: None,
                });
            }
            true
        } else {
            false
        };

        (addr, has_start)
    };

    if has_start {
        scheduler.run().await;
        vm_try!(scheduler.completed_tasks.pop().unwrap().result);
    }

    VMResult::Success(addr)
}
#[allow(dead_code)]
pub fn aliasing(
    registry: &Registry,
    triplets: &[(String, String, String)],
    store: &Store,
) -> VMResult<InstanceHandle> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("aliasing is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let mut gc = store.lock_gc();
    let inst_id = store.new_instance_id();
    let mut functions = vec![];
    let mut function_types = vec![];
    let mut function_type_identities = vec![];
    let mut globals = vec![];
    let mut memories = vec![];
    let mut tables = vec![];
    let mut function_addrs = vec![];
    let mut global_addrs = vec![];
    let mut memory_addrs = vec![];
    let mut table_addrs = vec![];
    let mut exports = vec![];
    for (modname, importname, exportname) in triplets {
        let instance_addr = vm_try!(VMResult::from_option(registry.get(modname), || {
            VMResult::Unlinkable
        }));

        let object_ref = vm_try!(VMResult::from_option(
            instance_addr.object_ref_for_store(store),
            || { VMResult::Unlinkable }
        ));
        let ext_instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
        let ext_module = gc.get_module(ext_instance.module_addr);
        let export_desc = vm_try!(VMResult::from_option(
            ext_module.exports.find(importname),
            || { VMResult::Unlinkable }
        ));
        let exportname = (*exportname).to_owned();
        match export_desc {
            ExportDesc::Func(idx) => {
                let tidx = ext_module.functions[idx.0 as usize];
                let ft = &ext_module.function_types[tidx.0 as usize];
                let new_tidx = function_types.len();
                let new_funcidx = functions.len();
                function_types.push(ft.clone());
                function_type_identities
                    .push(ext_module.function_type_identities[tidx.0 as usize].clone());
                functions.push(TypeIdx(new_tidx as u32));
                let addr = ext_instance.funcs.as_slice()[idx.0 as usize];
                function_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Func(FuncIdx(new_funcidx as u32)),
                ));
            }
            ExportDesc::Global(idx) => {
                let gt = ext_module.globals[idx.0 as usize];
                let new_gidx = globals.len();
                globals.push(gt);
                let addr = ext_instance.globals.as_slice()[idx.0 as usize];
                global_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Global(GlobalIdx(new_gidx as u32)),
                ));
            }
            ExportDesc::Mem(idx) => {
                let mt = ext_module.mems[idx.0 as usize];
                let new_memidx = memories.len();
                memories.push(mt);
                let addr = ext_instance.mems.as_slice()[idx.0 as usize];
                memory_addrs.push(addr);
                exports.push(Export(
                    exportname,
                    ExportDesc::Mem(MemIdx(new_memidx as u32)),
                ));
            }
            ExportDesc::Table(idx) => {
                let tt = ext_module.tables[idx.0 as usize];
                let new_tableidx = tables.len();
                tables.push(tt);
                table_addrs.push(ext_instance.tables.as_slice()[idx.0 as usize]);
                exports.push(Export(
                    exportname,
                    ExportDesc::Table(TableIdx(new_tableidx as u32)),
                ));
            }
        }
    }
    let mod_addr = gc.new_module(ModuleInstance {
        exports: ExportSection(exports),
        tables,
        globals,
        functions,
        function_types,
        function_type_identities,
        mems: memories,
    });
    let inst_id_handle = gc.alloc_instance(InstanceData {
        module_addr: mod_addr,
        mems: memory_addrs,
        globals: global_addrs,
        funcs: function_addrs,
        tables: table_addrs,
        instance_id: inst_id,
        memory_slots: Vec::new(),
    });
    VMResult::Success(InstanceHandle::new(store, inst_id_handle, inst_id))
}
pub fn link_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: HostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_host_function_with_function_idx is unsupported while the same store GC is already active"
        );
        return;
    }
    let mut gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let funcaddr = {
        let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
        instance.funcs.as_slice()[funcidx as usize]
    };
    let func = gc.get_func_mut(funcaddr);
    func.replace_host_code_pointer(f);
    rebuild_wasm_derived_data_for_instance(object_ref, &mut gc);
}
pub fn link_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: HostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_host_function_with_export_name is unsupported while the same store GC is already active"
        );
        return;
    }
    let gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let module = gc.get_module(instance.module_addr);
    let export = &module.exports.find(name).unwrap();
    let func_idx = if let ExportDesc::Func(v) = export {
        v.0
    } else {
        unreachable!()
    };
    link_host_function_with_function_idx(addr, func_idx, f, store);
}

pub fn link_async_host_function_with_function_idx(
    addr: &InstanceHandle,
    funcidx: u32,
    f: AsyncHostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_async_host_function_with_function_idx is unsupported while the same store GC is already active"
        );
        return;
    }
    let mut gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let funcaddr = {
        let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
        instance.funcs.as_slice()[funcidx as usize]
    };
    let func = gc.get_func_mut(funcaddr);
    func.replace_async_host_code_pointer(f);
    rebuild_wasm_derived_data_for_instance(object_ref, &mut gc);
}

pub fn link_async_host_function_with_export_name(
    addr: &InstanceHandle,
    name: &str,
    f: AsyncHostFunction,
    store: &Store,
) {
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "link_async_host_function_with_export_name is unsupported while the same store GC is already active"
        );
        return;
    }
    let gc = store.lock_gc();
    let Some(object_ref) = addr.object_ref_for_store(store) else {
        tracing::error!("instance handle belongs to another store");
        return;
    };
    let instance = unsafe { &*gc.get_instance_unchecked(object_ref) };
    let module = gc.get_module(instance.module_addr);
    let export = &module.exports.find(name).unwrap();
    let func_idx = if let ExportDesc::Func(v) = export {
        v.0
    } else {
        unreachable!()
    };
    drop(gc);
    link_async_host_function_with_function_idx(addr, func_idx, f, store);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{common::FunctionBody as ParsedFunctionBody, IoReadBinaryReader, WasmParser};

    fn parse_wat_module(wat: &str) -> crate::common::Module {
        let source = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(source.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        parser.parse_module().expect("module must parse")
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_const() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let result = execute_offset_const_expr(&mut gc, &[], &[ConstExpr::F64(1.0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[test]
    fn execute_offset_const_expr_fail_closes_non_i32_global_get() {
        let store = Store::new();
        let mut gc = store.lock_gc();
        let global = gc.new_global_data8(42);
        let result = execute_offset_const_expr(&mut gc, &[global], &[ConstExpr::GlobalGet(0)]);
        assert!(matches!(result, VMResult::Unlinkable));
    }

    #[tokio::test]
    async fn instantiate_builds_pointer_bearing_derived_code_for_control_flow_sites() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_wat_module(
            r#"
            (module
              (func (export "branchy") (param i32) (result i32)
                block (result i32)
                  local.get 0
                  if (result i32)
                    i32.const 1
                  else
                    i32.const 2
                  end
                end)
              (func (export "looped") (param i32) (result i32)
                (block
                  loop
                    local.get 0
                    br_if 1
                  end)
                i32.const 9))
            "#,
        );

        let parsed_branch = match &module.codes.0[0] {
            ParsedFunctionBody::Wasm(func) => func.clone(),
            ParsedFunctionBody::Host(_) => panic!("expected wasm function"),
        };
        assert!(!parsed_branch.control_flow_metadata.is_empty());

        let instance = instantiate(module, &store, &registry).await.unwrap();
        let gc = store.lock_gc();
        let inst = gc.get_instance(
            instance
                .object_ref_for_store(&store)
                .expect("instance must stay live in store"),
        );
        let branch_func = gc.get_func(inst.funcs[0]);
        let loop_func = gc.get_func(inst.funcs[1]);

        let branch_canonical = branch_func.canonical_code().expect("canonical wasm code");
        let branch_active = branch_func.code().expect("active wasm code");
        assert_eq!(branch_canonical.len(), branch_active.len());
        assert_ne!(
            branch_func.code_pointer().unwrap(),
            branch_func.canonical_code_pointer().unwrap()
        );
        assert!(branch_active.iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_if_ptr as Op)
                || std::ptr::fn_addr_eq(instr.op, vm::op_else_ptr as Op)
                || std::ptr::fn_addr_eq(instr.op, vm::special_block_return4_precomputed as Op)
                || std::ptr::fn_addr_eq(instr.op, vm::special_function_return4_precomputed as Op)
        }));
        assert!(branch_canonical.iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_if as Op)
                || std::ptr::fn_addr_eq(instr.op, vm::op_else as Op)
        }));
        assert!(branch_func.function_return_site().is_some());

        let loop_active = loop_func.code().expect("active wasm code");
        assert!(loop_active.iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_loop_empty_precomputed as Op)
                || std::ptr::fn_addr_eq(instr.op, vm::special_block_return_empty_precomputed as Op)
        }));
        assert!(!loop_func.loop_sites().is_empty());
        assert!(loop_func.function_return_site().is_some());
    }

    #[tokio::test]
    async fn instantiate_rewrites_seed_tee_branch_to_pointer_bearing_handler() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_wat_module(
            r#"
            (module
              (memory 1)
              (func (export "branchy") (param i32) (result i32)
                (local i32)
                block $exit
                  local.get 0
                  i32.load8_u
                  local.tee 1
                  i32.const 32
                  i32.gt_u
                  br_if $exit
                  i32.const 0
                  return
                end
                local.get 1))
            "#,
        );

        let instance = instantiate(module, &store, &registry).await.unwrap();
        let gc = store.lock_gc();
        let inst = gc.get_instance(
            instance
                .object_ref_for_store(&store)
                .expect("instance must stay live in store"),
        );
        let func = gc.get_func(inst.funcs[0]);
        let canonical = func.canonical_code().expect("canonical wasm code");
        let active = func.code().expect("active wasm code");

        assert!(canonical.iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_i32_seed_tee_imm_compare_br_if as Op)
        }));
        assert!(active.iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_i32_seed_tee_imm_compare_br_if_ptr as Op)
        }));
    }

    #[tokio::test]
    async fn instantiate_rewrites_seed_imm_and_branch_op_mapping() {
        assert!(
            rewrite_jump_op(vm::op_i32_seed_imm_and_br_if as Op).is_some_and(|op| {
                std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_br_if_ptr as Op)
            })
        );
        assert!(
            rewrite_jump_op(vm::op_i32_seed_imm_and_eqz_br_if as Op).is_some_and(|op| {
                std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_eqz_br_if_ptr as Op)
            })
        );
        assert!(rewrite_jump_op(vm::op_i32_seed_imm_and_if as Op)
            .is_some_and(|op| { std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_if_ptr as Op) }));
        assert!(
            rewrite_jump_op(vm::op_i32_seed_imm_and_eqz_if as Op).is_some_and(|op| {
                std::ptr::fn_addr_eq(op, vm::op_i32_seed_imm_and_eqz_if_ptr as Op)
            })
        );
    }

    #[tokio::test]
    async fn instantiate_builds_pointer_bearing_call_site_metadata_for_wasm_calls() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_wat_module(
            r#"
            (module
              (type $sig (func (param externref) (result externref)))
              (table 1 funcref)
              (elem (i32.const 0) $callee)

              (func $callee (param externref) (result externref)
                local.get 0)

              (func (export "caller") (param externref) (result externref)
                local.get 0
                call $callee)

              (func (export "tailcaller") (param externref) (result externref)
                local.get 0
                return_call $callee)

              (func (export "indirect") (param externref i32) (result externref)
                local.get 0
                local.get 1
                call_indirect (type $sig))

              (func (export "tailindirect") (param externref i32) (result externref)
                local.get 0
                local.get 1
                return_call_indirect (type $sig)))
            "#,
        );

        let instance = instantiate(module, &store, &registry).await.unwrap();
        let gc = store.lock_gc();
        let inst = gc.get_instance(
            instance
                .object_ref_for_store(&store)
                .expect("instance must stay live in store"),
        );

        let caller = gc.get_func(inst.funcs[1]);
        let tailcaller = gc.get_func(inst.funcs[2]);
        let indirect = gc.get_func(inst.funcs[3]);
        let tailindirect = gc.get_func(inst.funcs[4]);

        assert!(caller
            .code()
            .unwrap()
            .iter()
            .any(|instr| unsafe { std::ptr::fn_addr_eq(instr.op, vm::op_call_precomputed as Op) }));
        assert!(tailcaller.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_return_call_precomputed as Op)
        }));
        assert!(indirect.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_call_indirect_precomputed as Op)
        }));
        assert!(tailindirect.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_return_call_indirect_precomputed as Op)
        }));

        let direct_site = caller
            .direct_call_sites()
            .first()
            .expect("direct call site");
        assert!(direct_site.callee_layout_ptr().is_some());
        assert_eq!(direct_site.return_pc, StablePc::from_relative_index(4),);

        let return_call_site = tailcaller
            .direct_call_sites()
            .first()
            .expect("return_call site");
        assert!(return_call_site.callee_layout_ptr().is_some());
        assert_eq!(return_call_site.return_pc, StablePc::from_relative_index(4),);

        let indirect_site = indirect
            .indirect_call_sites()
            .first()
            .expect("indirect call site");
        assert_eq!(indirect_site.tableidx, 0);
        assert!(!indirect_site.expected_type_identity_ptr().is_null());
        assert_eq!(indirect_site.return_pc, StablePc::from_relative_index(7),);

        let return_indirect_site = tailindirect
            .indirect_call_sites()
            .first()
            .expect("return_call_indirect site");
        assert_eq!(return_indirect_site.tableidx, 0);
        assert!(!return_indirect_site.expected_type_identity_ptr().is_null());
        assert_eq!(
            return_indirect_site.return_pc,
            StablePc::from_relative_index(7),
        );
    }

    fn passthrough_host(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
        let value = ctx.stack.pop_u32();
        crate::vm_try!(ctx.stack.push_u32(value));
        let (prev_local_ref, return_addr) =
            ctx.stack
                .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
        ctx.set_local_reference(prev_local_ref);
        VMResult::Success(return_addr)
    }

    #[tokio::test]
    async fn instantiate_builds_pointer_bearing_call_site_metadata_for_import_calls() {
        let store = Store::new();
        let mut registry = Registry::new();
        let host = instantiate_native_module(
            NativeModule {
                functions: vec![HostFunctionDefinition {
                    fp: passthrough_host,
                    name: Some("passthrough".to_string()),
                    signature: FuncType::new(
                        vec![crate::common::ValType::I32],
                        vec![crate::common::ValType::I32],
                    ),
                }],
            },
            &store,
            &registry,
        )
        .await
        .unwrap();
        registry.register("host", host);
        let module = parse_wat_module(
            r#"
            (module
              (import "host" "passthrough" (func $passthrough (param i32) (result i32)))
              (func (export "caller") (param i32) (result i32)
                local.get 0
                call $passthrough)
              (func (export "tailcaller") (param i32) (result i32)
                local.get 0
                return_call $passthrough))
            "#,
        );

        let instance = instantiate(module, &store, &registry).await.unwrap();
        let gc = store.lock_gc();
        let inst = gc.get_instance(
            instance
                .object_ref_for_store(&store)
                .expect("instance must stay live in store"),
        );

        let caller = gc.get_func(inst.funcs[1]);
        let tailcaller = gc.get_func(inst.funcs[2]);

        assert_eq!(caller.import_call_sites().len(), 1);
        assert_eq!(tailcaller.import_call_sites().len(), 1);
        assert!(caller.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_call_import_precomputed as Op)
        }));
        assert!(tailcaller.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(instr.op, vm::op_return_call_import_precomputed as Op)
        }));

        let direct_site = caller
            .import_call_sites()
            .first()
            .expect("import call site");
        assert_eq!(direct_site.funcidx, 0);
        assert_eq!(direct_site.return_pc, StablePc::from_relative_index(4));
        let stack_map_sites = &caller
            .wasm_metadata()
            .unwrap()
            .frame_layout
            .cold()
            .stack_map_sites;
        eprintln!("caller stack_map_sites={stack_map_sites:?}");
        assert!(direct_site.stack_map_site_ptr().is_some());

        let tail_site = tailcaller
            .import_call_sites()
            .first()
            .expect("tail import call site");
        assert_eq!(tail_site.funcidx, 0);
        assert_eq!(tail_site.return_pc, StablePc::from_relative_index(4));
        assert!(tail_site.stack_map_site_ptr().is_some());
    }

    #[cfg(feature = "threads")]
    #[tokio::test]
    async fn instantiate_rewrites_shared_wait_handlers_to_precomputed_variants() {
        let store = Store::new();
        let registry = Registry::new();
        let module = parse_wat_module(
            r#"
            (module
              (memory 1 1 shared)
              (func (export "wait32") (param i32 i32 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait32)
              (func (export "wait64") (param i32 i64 i64) (result i32)
                local.get 0
                local.get 1
                local.get 2
                memory.atomic.wait64))
            "#,
        );

        let instance = instantiate(module, &store, &registry).await.unwrap();
        let gc = store.lock_gc();
        let inst = gc.get_instance(
            instance
                .object_ref_for_store(&store)
                .expect("instance must stay live in store"),
        );

        let wait32 = gc.get_func(inst.funcs[0]);
        let wait64 = gc.get_func(inst.funcs[1]);

        assert!(wait32.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(
                instr.op,
                vm::op_memory_atomic_wait32_shared_precomputed as Op,
            )
        }));
        assert!(wait64.code().unwrap().iter().any(|instr| unsafe {
            std::ptr::fn_addr_eq(
                instr.op,
                vm::op_memory_atomic_wait64_shared_precomputed as Op,
            )
        }));

        let wait32_site = wait32
            .wait_sites()
            .first()
            .expect("wait32 site metadata must exist");
        assert!(wait32_site.stack_map_site_ptr().is_some());
        assert!(wait32_site.unwind_site_ptr().is_some());

        let wait64_site = wait64
            .wait_sites()
            .first()
            .expect("wait64 site metadata must exist");
        assert!(wait64_site.stack_map_site_ptr().is_some());
        assert!(wait64_site.unwind_site_ptr().is_some());
    }
}
