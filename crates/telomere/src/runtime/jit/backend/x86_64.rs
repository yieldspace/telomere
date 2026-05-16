use crate::common::{
    decode_local_binop32_kind, decode_local_binop64_kind, decode_local_cmp32_kind,
    decode_local_cmp64_kind, decode_local_unary32_kind, decode_local_unary64_kind,
    memory::MemoryJitLayout,
    store::{CallDispatchCache, CallDispatchTarget, GlobalValueJitLayout, StoreInner},
    ExecuteContext, Instr, LocalBinop32Op, LocalBinop64Op, LocalCmp32Op, LocalCmp64Op,
    LocalFastRhsShape, LocalUnary32Op, LocalUnary64Op, MemArg, ObjectRef, VMResult, PAGE_SIZE,
};
use crate::runtime::jit::abi::{vm_result_code, JitNativeExit};
use crate::runtime::jit::profile::{self, Counter};
use crate::runtime::jit::stubs::{
    atomic_fence as jit_atomic_fence, block_return as jit_block_return,
    call_i32_crc16_update16 as jit_call_i32_crc16_update16,
    call_i32_list_crc_summary as jit_call_i32_list_crc_summary, direct_call as jit_direct_call,
    f32_max_bits as jit_f32_max_bits, f32_min_bits as jit_f32_min_bits,
    f64_max_bits as jit_f64_max_bits, f64_min_bits as jit_f64_min_bits,
    function_return as jit_function_return,
    i32_core_state_benchmark as jit_i32_core_state_benchmark,
    i32_crc16_update16 as jit_i32_crc16_update16,
    i32_list_crc_pair_loop as jit_i32_list_crc_pair_loop,
    i32_list_crc_summary as jit_i32_list_crc_summary,
    i32_numeric_token_state_transition as jit_i32_numeric_token_state_transition,
    i32_popcnt_value as jit_i32_popcnt_value, i32_select_bit_step4 as jit_i32_select_bit_step4,
    i32_store_local_base_from_vm_stack as jit_i32_store_local_base_from_vm_stack,
    i32_trunc_f32 as jit_i32_trunc_f32, i32_trunc_f64 as jit_i32_trunc_f64,
    i64_popcnt_value as jit_i64_popcnt_value, i64_trunc_f32 as jit_i64_trunc_f32,
    i64_trunc_f64 as jit_i64_trunc_f64, indirect_call as jit_indirect_call,
    memory_copy as jit_memory_copy, memory_fill as jit_memory_fill, memory_grow as jit_memory_grow,
    memory_size as jit_memory_size, ref_func as jit_ref_func,
    runtime_continuation_op as jit_runtime_continuation_op,
    runtime_stack_op as jit_runtime_stack_op, wasm_direct_call_fast as jit_wasm_direct_call_fast,
};

use super::ops::{
    decode_baseline_op, BaselineOp, FloatBinaryOp, FloatCompareOp, FloatUnaryOp, FloatWidth,
    I32BinaryOp, I32CompareOp, I32UnaryOp, I64BinaryOp, I64CompareOp, I64UnaryOp, SearchCompare,
    SelectBitStep4, RUNTIME_CONT_CURRENT_VM_HANDLER,
};
use telomere_jit_codegen::arch::x86_64::{
    patch_branch as patch_a64_branch, BranchKind, Cond, X64BaselineMasm as A64BaselineMasm,
};

pub(crate) fn emit_baseline_function(
    funcaddr: ObjectRef,
    code: &[Instr],
    op_lens: &[u16],
    gc: &StoreInner,
) -> Result<Vec<u8>, ()> {
    let mut emitter = Emitter::new(funcaddr, code, op_lens, gc);
    emitter.emit()?;
    emitter.finish()
}

struct Fixup {
    at: usize,
    target_index: usize,
    kind: FixupKind,
}

#[derive(Clone, Copy)]
enum FixupKind {
    B,
    BCond(Cond),
    CbnzX(u8),
    CbnzW(u8),
}

fn cond_for_i32_compare(op: I32CompareOp) -> Cond {
    match op {
        I32CompareOp::Eq => Cond::Eq,
        I32CompareOp::Ne => Cond::Ne,
        I32CompareOp::LtS => Cond::Lt,
        I32CompareOp::LtU => Cond::Lo,
        I32CompareOp::GtS => Cond::Gt,
        I32CompareOp::GtU => Cond::Hi,
        I32CompareOp::LeS => Cond::Le,
        I32CompareOp::LeU => Cond::Ls,
        I32CompareOp::GeS => Cond::Ge,
        I32CompareOp::GeU => Cond::Hs,
    }
}

fn cond_for_i64_compare(op: I64CompareOp) -> Cond {
    match op {
        I64CompareOp::Eq => Cond::Eq,
        I64CompareOp::Ne => Cond::Ne,
        I64CompareOp::LtS => Cond::Lt,
        I64CompareOp::LtU => Cond::Lo,
        I64CompareOp::GtS => Cond::Gt,
        I64CompareOp::GtU => Cond::Hi,
        I64CompareOp::LeS => Cond::Le,
        I64CompareOp::LeU => Cond::Ls,
        I64CompareOp::GeS => Cond::Ge,
        I64CompareOp::GeU => Cond::Hs,
    }
}

const STACK_REGS: [u8; 7] = [22, 23, 24, 25, 26, 27, 28];

enum EmitControl {
    Continue,
    SkipNextOp,
    SkipOps(usize),
    SkipInstrSlots(usize),
    Stop,
}

struct SearchLoopPlan {
    node_local: u32,
    data_delta: u32,
    data_memarg: MemArg,
    data_local: u32,
    field_delta: u32,
    field_memarg: MemArg,
    field_width: u32,
    rhs_local: u32,
    rhs_mask: u32,
    compare: SearchCompare,
    next_delta: u32,
    next_memarg: MemArg,
    match_target: usize,
    miss_target: Option<usize>,
}

struct UpdateStore16LoopPlan {
    subtract: bool,
    ptr_local: u32,
    scalar_local: u32,
    counter_local: u32,
    load_delta: u32,
    store_delta: u32,
    load_memarg: MemArg,
    store_memarg: MemArg,
}

struct Emitter<'a> {
    funcaddr: ObjectRef,
    wasm: &'a [Instr],
    op_lens: &'a [u16],
    gc: &'a StoreInner,
    masm: A64BaselineMasm,
    labels: Vec<Option<usize>>,
    fixups: Vec<Fixup>,
    stack_depth: usize,
}

impl std::ops::Deref for Emitter<'_> {
    type Target = A64BaselineMasm;

    fn deref(&self) -> &Self::Target {
        &self.masm
    }
}

impl std::ops::DerefMut for Emitter<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.masm
    }
}

impl<'a> Emitter<'a> {
    fn new(funcaddr: ObjectRef, wasm: &'a [Instr], op_lens: &'a [u16], gc: &'a StoreInner) -> Self {
        Self {
            funcaddr,
            wasm,
            op_lens,
            gc,
            masm: A64BaselineMasm::with_capacity(wasm.len() * 16),
            labels: vec![None; wasm.len().saturating_add(1)],
            fixups: Vec::new(),
            stack_depth: 0,
        }
    }

    fn emit(&mut self) -> Result<(), ()> {
        self.prologue();
        let mut cursor = 0usize;
        let mut op_index = 0usize;
        while let Some(&len) = self.op_lens.get(op_index) {
            if cursor >= self.wasm.len() {
                return Err(());
            }
            self.labels[cursor] = Some(self.offset());
            let op = match decode_baseline_op(self.wasm, cursor) {
                Ok(op) => op,
                Err(()) => {
                    if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
                        let op = unsafe { self.wasm[cursor].op };
                        #[cfg(feature = "vm-diagnostics")]
                        let op_label = crate::runtime::vm::diagnostic_op_label(op);
                        #[cfg(not(feature = "vm-diagnostics"))]
                        let op_label = "unknown";
                        eprintln!(
                            "[telomere-jit] decode_stop pc={cursor} op={op_label} op_addr=0x{:x}",
                            op as usize
                        );
                    }
                    return Err(());
                }
            };
            match self.emit_op_or_runtime(cursor, op)? {
                EmitControl::Continue => {
                    cursor += usize::from(len);
                    op_index += 1;
                }
                EmitControl::SkipNextOp => {
                    cursor += usize::from(len);
                    op_index += 1;
                    if let Some(&next_len) = self.op_lens.get(op_index) {
                        cursor += usize::from(next_len);
                        op_index += 1;
                    }
                }
                EmitControl::SkipOps(count) => {
                    cursor += usize::from(len);
                    op_index += 1;
                    for _ in 0..count {
                        if let Some(&next_len) = self.op_lens.get(op_index) {
                            cursor += usize::from(next_len);
                            op_index += 1;
                        }
                    }
                }
                EmitControl::SkipInstrSlots(slots) => {
                    let target = match cursor
                        .checked_add(slots)
                        .and_then(|target| target.checked_add(1))
                    {
                        Some(target) => target,
                        None => {
                            trace_compile_message(format_args!(
                                "skip_slots_overflow pc={cursor} slots={slots}"
                            ));
                            return Err(());
                        }
                    };
                    cursor += usize::from(len);
                    op_index += 1;
                    while cursor < target {
                        let Some(&next_len) = self.op_lens.get(op_index) else {
                            trace_compile_message(format_args!(
                                "skip_slots_missing_len pc={cursor} target={target} slots={slots}"
                            ));
                            return Err(());
                        };
                        cursor += usize::from(next_len);
                        op_index += 1;
                    }
                    if cursor != target {
                        trace_compile_message(format_args!(
                            "skip_slots_misaligned cursor={cursor} target={target} slots={slots}"
                        ));
                        return Err(());
                    }
                }
                EmitControl::Stop => break,
            }
        }
        self.return_trap(VMResult::<()>::InvalidOperand);
        Ok(())
    }

    fn emit_op_or_runtime(&mut self, cursor: usize, op: BaselineOp) -> Result<EmitControl, ()> {
        match self.emit_op(cursor, op) {
            Ok(control) => Ok(control),
            Err(()) => {
                if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
                    let op = unsafe { self.wasm[cursor].op };
                    #[cfg(feature = "vm-diagnostics")]
                    let op_label = crate::runtime::vm::diagnostic_op_label(op);
                    #[cfg(not(feature = "vm-diagnostics"))]
                    let op_label = "unknown";
                    eprintln!(
                        "[telomere-jit] compile_reject_emit pc={cursor} op={op_label} stack_depth={} op_addr=0x{:x}",
                        self.stack_depth,
                        op as usize
                    );
                }
                Err(())
            }
        }
    }

    fn emit_op(&mut self, cursor: usize, op: BaselineOp) -> Result<EmitControl, ()> {
        BaselineOpEmitter {
            emitter: self,
            cursor,
        }
        .emit(op)
    }
}

struct BaselineOpEmitter<'e, 'a> {
    emitter: &'e mut Emitter<'a>,
    cursor: usize,
}

impl<'e, 'a> std::ops::Deref for BaselineOpEmitter<'e, 'a> {
    type Target = Emitter<'a>;

    fn deref(&self) -> &Self::Target {
        self.emitter
    }
}

impl std::ops::DerefMut for BaselineOpEmitter<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.emitter
    }
}

impl BaselineOpEmitter<'_, '_> {
    fn emit(&mut self, op: BaselineOp) -> Result<EmitControl, ()> {
        let cursor = self.cursor;
        match op {
            BaselineOp::I32Const { value } => {
                let dst = self.push_reg()?;
                self.mov_imm_u32(dst, value);
            }
            BaselineOp::I64Const { value } => {
                let low = self.push_reg()?;
                self.mov_imm_u32(low, value as u32);
                let high = self.push_reg()?;
                self.mov_imm_u32(high, (value >> 32) as u32);
            }
            BaselineOp::F32Const { bits } => {
                let dst = self.push_reg()?;
                self.mov_imm_u32(dst, bits);
            }
            BaselineOp::F64Const { bits } => {
                let low = self.push_reg()?;
                self.mov_imm_u32(low, bits as u32);
                let high = self.push_reg()?;
                self.mov_imm_u32(high, (bits >> 32) as u32);
            }
            BaselineOp::I64ExtendI32 { signed } => {
                self.ensure_stack_slots(1)?;
                let low = self.peek_reg()?;
                let high = self.push_reg()?;
                if signed {
                    self.asr_w_imm(high, low, 31);
                } else {
                    self.mov_imm_u32(high, 0);
                }
            }
            BaselineOp::I64ExtendS { bits } => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.pack_i64_slots_to_x(16, low, high, 9)?;
                self.sbfm_x(16, 16, 0, bits.checked_sub(1).ok_or(())?);
                self.push_x_as_i64_slots(16)?;
            }
            BaselineOp::I64Eqz => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.peek_reg()?;
                self.orr_w(low, low, high);
                self.cmp_w_imm(low, 0)?;
                self.cset_w(low, Cond::Eq);
            }
            BaselineOp::I64Unary { op } => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.pack_i64_slots_to_x(16, low, high, 9)?;
                match op {
                    I64UnaryOp::Clz => self.clz_x(16, 16),
                    I64UnaryOp::Ctz => {
                        self.rbit_x(16, 16);
                        self.clz_x(16, 16);
                    }
                    I64UnaryOp::Popcnt => {
                        self.mov_x(0, 16);
                        self.call_ptr(jit_i64_popcnt_value as *const () as usize);
                        self.mov_x(16, 0);
                    }
                }
                self.push_x_as_i64_slots(16)?;
            }
            BaselineOp::I64Binary { op } => {
                self.ensure_stack_slots(4)?;
                let rhs_high = self.pop_reg()?;
                let rhs_low = self.pop_reg()?;
                let lhs_high = self.pop_reg()?;
                let lhs_low = self.peek_reg()?;
                self.mov_w(16, lhs_high);
                self.lsl_x_imm(16, 16, 32)?;
                self.mov_w(17, lhs_low);
                self.orr_x(16, 17, 16);
                self.mov_w(17, rhs_high);
                self.lsl_x_imm(17, 17, 32)?;
                self.mov_w(9, rhs_low);
                self.orr_x(17, 9, 17);
                match op {
                    I64BinaryOp::Add => self.add_x(16, 16, 17),
                    I64BinaryOp::Sub => self.sub_x(16, 16, 17),
                    I64BinaryOp::Mul => self.mul_x(16, 16, 17),
                    I64BinaryOp::DivS => {
                        self.trap_if_i64_divisor_zero(17)?;
                        self.trap_if_i64_div_s_overflow(16, 17)?;
                        self.sdiv_x(16, 16, 17);
                    }
                    I64BinaryOp::DivU => {
                        self.trap_if_i64_divisor_zero(17)?;
                        self.udiv_x(16, 16, 17);
                    }
                    I64BinaryOp::RemS => {
                        self.trap_if_i64_divisor_zero(17)?;
                        self.sdiv_x(9, 16, 17);
                        self.msub_x(16, 9, 17, 16);
                    }
                    I64BinaryOp::RemU => {
                        self.trap_if_i64_divisor_zero(17)?;
                        self.udiv_x(9, 16, 17);
                        self.msub_x(16, 9, 17, 16);
                    }
                    I64BinaryOp::And => self.and_x(16, 16, 17),
                    I64BinaryOp::Or => self.orr_x(16, 16, 17),
                    I64BinaryOp::Xor => self.eor_x(16, 16, 17),
                    I64BinaryOp::Shl => self.lslv_x(16, 16, 17),
                    I64BinaryOp::ShrS => self.asrv_x(16, 16, 17),
                    I64BinaryOp::ShrU => self.lsrv_x(16, 16, 17),
                    I64BinaryOp::Rotl => {
                        self.neg_x(9, 17);
                        self.rorv_x(16, 16, 9);
                    }
                    I64BinaryOp::Rotr => self.rorv_x(16, 16, 17),
                }
                self.mov_w(lhs_low, 16);
                let lhs_high = self.push_reg()?;
                self.lsr_x_imm(17, 16, 32)?;
                self.mov_w(lhs_high, 17);
            }
            BaselineOp::I64Compare { op } => {
                self.ensure_stack_slots(4)?;
                let rhs_high = self.pop_reg()?;
                let rhs_low = self.pop_reg()?;
                let lhs_high = self.pop_reg()?;
                let lhs_low = self.peek_reg()?;
                self.mov_w(16, lhs_high);
                self.lsl_x_imm(16, 16, 32)?;
                self.mov_w(17, lhs_low);
                self.orr_x(16, 17, 16);
                self.mov_w(17, rhs_high);
                self.lsl_x_imm(17, 17, 32)?;
                self.mov_w(9, rhs_low);
                self.orr_x(17, 9, 17);
                self.cmp_x(16, 17);
                self.cset_w(lhs_low, cond_for_i64_compare(op));
            }
            BaselineOp::I32WrapI64 => {
                self.ensure_stack_slots(2)?;
                self.pop_reg()?;
            }
            BaselineOp::I32ConstWrite4 {
                value,
                local,
                keep_result,
            } => {
                self.mov_imm_u32(16, value);
                self.store_local4_from_reg(local, 16)?;
                if keep_result {
                    let dst = self.push_reg()?;
                    self.mov_w(dst, 16);
                }
            }
            BaselineOp::I32ConstBinop { kind, rhs } => {
                self.ensure_stack_slots(1)?;
                let lhs = self.peek_reg()?;
                self.emit_i32_const_binop(kind, lhs, rhs)?;
            }
            BaselineOp::I32ConstBinopBrIf { kind, rhs, target } => {
                if self.stack_depth > 1 {
                    let reload_slots = self.stack_depth.checked_sub(1).ok_or(())?;
                    self.call_runtime_continuation_op_reloading_stack(
                        cursor,
                        RUNTIME_CONT_CURRENT_VM_HANDLER,
                        reload_slots,
                    )?;
                    return Ok(EmitControl::Continue);
                }
                self.ensure_stack_slots(1)?;
                let value = self.pop_reg()?;
                self.emit_i32_const_binop(kind, value, rhs)?;
                self.flush_stack()?;
                self.branch_to(target, FixupKind::CbnzW(value));
            }
            BaselineOp::I32ConstBinopWrite4 {
                kind,
                rhs,
                dst,
                keep_result,
            } => {
                self.ensure_stack_slots(1)?;
                let lhs = self.peek_reg()?;
                self.emit_i32_const_binop(kind, lhs, rhs)?;
                self.store_local4_from_reg(dst, lhs)?;
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::I32ConstCmpWrite4 {
                kind,
                rhs,
                dst,
                keep_result,
            } => {
                self.ensure_stack_slots(1)?;
                let lhs = self.peek_reg()?;
                self.emit_i32_const_cmp(kind, lhs, rhs)?;
                if dst != u32::MAX {
                    self.store_local4_from_reg(dst, lhs)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::I32ConstCmpBrIf { kind, rhs, target } => {
                if self.stack_depth > 1 {
                    let reload_slots = self.stack_depth.checked_sub(1).ok_or(())?;
                    self.call_runtime_continuation_op_reloading_stack(
                        cursor,
                        RUNTIME_CONT_CURRENT_VM_HANDLER,
                        reload_slots,
                    )?;
                    return Ok(EmitControl::Continue);
                }
                self.ensure_stack_slots(1)?;
                let value = self.pop_reg()?;
                self.emit_i32_const_cmp(kind, value, rhs)?;
                self.flush_stack()?;
                self.branch_to(target, FixupKind::CbnzW(value));
            }
            BaselineOp::LocalBinop32Write4 {
                kind,
                lhs,
                rhs,
                dst,
                keep_result,
            } => {
                let result = self.push_reg()?;
                self.emit_local_binop32(kind, lhs, rhs, result)?;
                if dst != u32::MAX {
                    self.store_local4_from_reg(dst, result)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalBinop32BrIf {
                kind,
                lhs,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                let result = self.push_reg()?;
                self.emit_local_binop32(kind, lhs, rhs, result)?;
                let result = self.pop_reg()?;
                self.branch_to(target, FixupKind::CbnzW(result));
            }
            BaselineOp::LocalBinop64 { kind, lhs, rhs } => {
                let (op, rhs_shape) = decode_local_binop64_kind(kind).ok_or(())?;
                self.load_local_i64_to_x(16, lhs, 9)?;
                self.load_local_binop64_rhs_to_x(17, rhs_shape, rhs, 9)?;
                self.emit_i64_binop(op, 16, 17)?;
                let low = self.push_reg()?;
                self.mov_w(low, 16);
                let high = self.push_reg()?;
                self.lsr_x_imm(17, 16, 32)?;
                self.mov_w(high, 17);
            }
            BaselineOp::LocalBinop64Write8 {
                kind,
                lhs,
                rhs,
                dst,
                keep_result,
            } => {
                let (op, rhs_shape) = decode_local_binop64_kind(kind).ok_or(())?;
                self.load_local_i64_to_x(16, lhs, 9)?;
                self.load_local_binop64_rhs_to_x(17, rhs_shape, rhs, 9)?;
                self.emit_i64_binop(op, 16, 17)?;
                self.store_local_i64_from_x(dst, 16, 9)?;
                if keep_result {
                    self.push_x_as_i64_slots(16)?;
                }
            }
            BaselineOp::LocalCmp32Write4 {
                kind,
                lhs,
                rhs,
                dst,
                keep_result,
            } => {
                let result = self.push_reg()?;
                self.emit_local_cmp32(kind, lhs, rhs, result)?;
                if dst != u32::MAX {
                    self.store_local4_from_reg(dst, result)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalCmp32BrIf {
                kind,
                lhs,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                let result = self.push_reg()?;
                self.emit_local_cmp32(kind, lhs, rhs, result)?;
                let result = self.pop_reg()?;
                self.branch_to(target, FixupKind::CbnzW(result));
            }
            BaselineOp::LocalCmp64Write4 {
                kind,
                lhs,
                rhs,
                dst,
                keep_result,
            } => {
                let result = self.push_reg()?;
                self.emit_local_cmp64(kind, lhs, rhs, result)?;
                if dst != u32::MAX {
                    self.store_local4_from_reg(dst, result)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalCmp64BrIf {
                kind,
                lhs,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                let result = self.push_reg()?;
                self.emit_local_cmp64(kind, lhs, rhs, result)?;
                let result = self.pop_reg()?;
                self.branch_to(target, FixupKind::CbnzW(result));
            }
            BaselineOp::LocalUnary32Write4 {
                kind,
                src,
                dst,
                keep_result,
            } => {
                let result = self.push_reg()?;
                self.emit_local_unary32(kind, src, result)?;
                if dst != u32::MAX {
                    self.store_local4_from_reg(dst, result)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalUnary64Write8 {
                kind,
                src,
                dst,
                keep_result,
            } => {
                self.emit_local_unary64(kind, src, 16)?;
                if dst != u32::MAX {
                    self.store_local_i64_from_x(dst, 16, 9)?;
                }
                if keep_result {
                    self.push_x_as_i64_slots(16)?;
                }
            }
            BaselineOp::LocalGet4I32ConstAdd { local, value } => {
                let dst = self.push_reg()?;
                self.load_local4_to_reg(dst, local)?;
                self.add_imm_u32(dst, dst, value)?;
            }
            BaselineOp::LocalGet4I32ConstAddWrite4 {
                src,
                value,
                dst,
                keep_result,
            } => {
                let value_reg = self.push_reg()?;
                self.load_local4_to_reg(value_reg, src)?;
                self.add_imm_u32(value_reg, value_reg, value)?;
                self.store_local4_from_reg(dst, value_reg)?;
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalGet4LocalGet4I32Add { lhs, rhs } => {
                let lhs_reg = self.push_reg()?;
                self.load_local4_to_reg(lhs_reg, lhs)?;
                let rhs_reg = self.push_reg()?;
                self.load_local4_to_reg(rhs_reg, rhs)?;
                let rhs_reg = self.pop_reg()?;
                let lhs_reg = self.peek_reg()?;
                self.add_w(lhs_reg, lhs_reg, rhs_reg);
            }
            BaselineOp::LocalGet4LocalGet4I32AddWrite4 {
                lhs,
                rhs,
                dst,
                keep_result,
            } => {
                let lhs_reg = self.push_reg()?;
                self.load_local4_to_reg(lhs_reg, lhs)?;
                let rhs_reg = self.push_reg()?;
                self.load_local4_to_reg(rhs_reg, rhs)?;
                let rhs_reg = self.pop_reg()?;
                let lhs_reg = self.peek_reg()?;
                self.add_w(lhs_reg, lhs_reg, rhs_reg);
                self.store_local4_from_reg(dst, lhs_reg)?;
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalGet4 { local } => {
                let dst = self.push_reg()?;
                self.load_local4_to_reg(dst, local)?;
            }
            BaselineOp::LocalGet4Run { locals, count } => {
                for local in locals.iter().take(count) {
                    let dst = self.push_reg()?;
                    self.load_local4_to_reg(dst, *local)?;
                }
            }
            BaselineOp::LocalGet4RunSkip {
                locals,
                count,
                skip_slots,
            } => {
                for local in locals.iter().take(count) {
                    let dst = self.push_reg()?;
                    self.load_local4_to_reg(dst, *local)?;
                }
                return Ok(EmitControl::SkipInstrSlots(skip_slots));
            }
            BaselineOp::LocalGet8 { local } => {
                let low = self.push_reg()?;
                self.load_local4_to_reg(low, local)?;
                let high = self.push_reg()?;
                self.load_local4_to_reg(high, local.wrapping_add(4))?;
            }
            BaselineOp::LocalGet16 { local } => {
                for lane in 0..4 {
                    let dst = self.push_reg()?;
                    self.load_local4_to_reg(dst, local.wrapping_add(lane * 4))?;
                }
            }
            BaselineOp::GlobalGet4 { index } => {
                let dst = self.push_reg()?;
                self.inline_global_get4(dst, index, 0)?;
            }
            BaselineOp::GlobalGetSlots { index, slots } => {
                for lane in 0..slots {
                    let dst = self.push_reg()?;
                    self.inline_global_get4(dst, index, u32::try_from(lane).map_err(|_| ())?)?;
                }
            }
            BaselineOp::GlobalSet4 { index } => {
                self.ensure_stack_slots(1)?;
                let value = self.pop_reg()?;
                self.inline_global_set4(index, 0, value)?;
            }
            BaselineOp::GlobalSetSlots { index, slots } => {
                self.ensure_stack_slots(slots)?;
                for lane in (0..slots).rev() {
                    let value = self.pop_reg()?;
                    self.inline_global_set4(index, u32::try_from(lane).map_err(|_| ())?, value)?;
                }
            }
            BaselineOp::Drop { size } => {
                let slots = usize::try_from(size / 4).map_err(|_| ())?;
                if size % 4 != 0 {
                    return Err(());
                }
                self.ensure_stack_slots(slots)?;
                self.stack_depth -= slots;
            }
            BaselineOp::LocalGet4LocalGet4 { first, second } => {
                let first_reg = self.push_reg()?;
                self.load_local4_to_reg(first_reg, first)?;
                let second_reg = self.push_reg()?;
                self.load_local4_to_reg(second_reg, second)?;
            }
            BaselineOp::LocalGet4LocalGet4Compare { first, second, op } => {
                let result = self.push_reg()?;
                self.load_local4_to_reg(result, first)?;
                self.load_local4_to_reg(17, second)?;
                self.cmp_w(result, 17);
                self.cset_w(result, cond_for_i32_compare(op));
                return Ok(EmitControl::SkipOps(2));
            }
            BaselineOp::LocalGet4LocalGet4ConstShrUTee4Eq {
                first,
                second,
                shift,
                dst,
            } => {
                let result = self.push_reg()?;
                self.load_local4_to_reg(result, first)?;
                self.load_local4_to_reg(17, second)?;
                self.lsr_w_imm(17, 17, shift);
                self.store_local4_from_reg(dst, 17)?;
                self.cmp_w(result, 17);
                self.cset_w(result, Cond::Eq);
                return Ok(EmitControl::SkipOps(5));
            }
            BaselineOp::LocalGet4LocalGet4LocalGet4 {
                first,
                second,
                third,
            } => {
                let first_reg = self.push_reg()?;
                self.load_local4_to_reg(first_reg, first)?;
                let second_reg = self.push_reg()?;
                self.load_local4_to_reg(second_reg, second)?;
                let third_reg = self.push_reg()?;
                self.load_local4_to_reg(third_reg, third)?;
            }
            BaselineOp::LocalGet4LocalGet4XorTee4Load16U {
                lhs,
                rhs,
                dst,
                memarg,
            } => {
                let value = self.push_reg()?;
                self.load_local4_to_reg(value, lhs)?;
                self.load_local4_to_reg(17, rhs)?;
                self.eor_w(value, value, 17);
                self.store_local4_from_reg(dst, value)?;
                self.mov_imm_u32(17, 0xff);
                self.and_w(value, value, 17);
                self.lsl_w_imm(value, value, 1);
                self.inline_i32_load(value, memarg.offset, 2, false)?;
            }
            BaselineOp::LocalGet4Write4 {
                src,
                dst,
                keep_result,
            } => {
                let value = self.push_reg()?;
                self.load_local4_to_reg(value, src)?;
                self.store_local4_from_reg(dst, value)?;
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::LocalSet4 { local } => {
                self.ensure_stack_slots(1)?;
                let src = self.pop_reg()?;
                self.store_local4_from_reg(local, src)?;
            }
            BaselineOp::LocalSet8 { local } => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.store_local4_from_reg(local, low)?;
                self.store_local4_from_reg(local.wrapping_add(4), high)?;
            }
            BaselineOp::LocalSet16 { local } => {
                self.ensure_stack_slots(4)?;
                for lane in (0..4).rev() {
                    let src = self.pop_reg()?;
                    self.store_local4_from_reg(local.wrapping_add(lane * 4), src)?;
                }
            }
            BaselineOp::LocalTee4 { local } => {
                self.ensure_stack_slots(1)?;
                let src = self.peek_reg()?;
                self.store_local4_from_reg(local, src)?;
            }
            BaselineOp::LocalTee8 { local } => {
                self.ensure_stack_slots(2)?;
                let high = self.peek_reg()?;
                let low = STACK_REGS[self.stack_depth - 2];
                self.store_local4_from_reg(local, low)?;
                self.store_local4_from_reg(local.wrapping_add(4), high)?;
            }
            BaselineOp::LocalTee16 { local } => {
                self.ensure_stack_slots(4)?;
                let base = self.stack_depth - 4;
                for lane in 0..4 {
                    self.store_local4_from_reg(
                        local.wrapping_add(u32::try_from(lane).map_err(|_| ())? * 4),
                        STACK_REGS[base + lane],
                    )?;
                }
            }
            BaselineOp::Select4 { dst, keep_result } => {
                self.ensure_stack_slots(3)?;
                let cond = self.pop_reg()?;
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.cmp_w_imm(cond, 0)?;
                self.csel_w(lhs, lhs, rhs, Cond::Ne);
                if let Some(dst) = dst {
                    self.store_local4_from_reg(dst, lhs)?;
                }
                if !keep_result {
                    self.pop_reg()?;
                }
            }
            BaselineOp::SelectSlots { slots } => {
                self.ensure_stack_slots(slots * 2 + 1)?;
                self.emit_select_slots(slots)?;
            }
            BaselineOp::I32Binary { op } => {
                self.ensure_stack_slots(2)?;
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                match op {
                    I32BinaryOp::Add => self.add_w(lhs, lhs, rhs),
                    I32BinaryOp::Sub => self.sub_w(lhs, lhs, rhs),
                    I32BinaryOp::Mul => self.mul_w(lhs, lhs, rhs),
                    I32BinaryOp::DivS => {
                        self.trap_if_i32_divisor_zero(rhs)?;
                        self.trap_if_i32_div_s_overflow(lhs, rhs)?;
                        self.sdiv_w(lhs, lhs, rhs);
                    }
                    I32BinaryOp::DivU => {
                        self.trap_if_i32_divisor_zero(rhs)?;
                        self.udiv_w(lhs, lhs, rhs);
                    }
                    I32BinaryOp::RemS => {
                        self.trap_if_i32_divisor_zero(rhs)?;
                        self.sdiv_w(17, lhs, rhs);
                        self.msub_w(lhs, 17, rhs, lhs);
                    }
                    I32BinaryOp::RemU => {
                        self.trap_if_i32_divisor_zero(rhs)?;
                        self.udiv_w(17, lhs, rhs);
                        self.msub_w(lhs, 17, rhs, lhs);
                    }
                    I32BinaryOp::And => self.and_w(lhs, lhs, rhs),
                    I32BinaryOp::Or => self.orr_w(lhs, lhs, rhs),
                    I32BinaryOp::Xor => self.eor_w(lhs, lhs, rhs),
                    I32BinaryOp::Shl => self.lslv_w(lhs, lhs, rhs),
                    I32BinaryOp::ShrS => self.asrv_w(lhs, lhs, rhs),
                    I32BinaryOp::ShrU => self.lsrv_w(lhs, lhs, rhs),
                    I32BinaryOp::Rotl => {
                        self.neg_w(17, rhs);
                        self.rorv_w(lhs, lhs, 17);
                    }
                    I32BinaryOp::Rotr => self.rorv_w(lhs, lhs, rhs),
                }
            }
            BaselineOp::I32Unary { op } => {
                self.ensure_stack_slots(1)?;
                let value = self.peek_reg()?;
                match op {
                    I32UnaryOp::Clz => self.clz_w(value, value),
                    I32UnaryOp::Ctz => {
                        self.rbit_w(value, value);
                        self.clz_w(value, value);
                    }
                    I32UnaryOp::Popcnt => {
                        self.mov_w(0, value);
                        self.call_ptr(jit_i32_popcnt_value as *const () as usize);
                        self.mov_w(value, 0);
                    }
                    I32UnaryOp::Extend8S => self.sxtb_w(value, value),
                    I32UnaryOp::Extend16S => self.sxth_w(value, value),
                }
            }
            BaselineOp::I32Eqz => {
                self.ensure_stack_slots(1)?;
                let value = self.peek_reg()?;
                self.cmp_w_imm(value, 0)?;
                self.cset_w(value, Cond::Eq);
            }
            BaselineOp::I32Compare { op } => {
                self.ensure_stack_slots(2)?;
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.cmp_w(lhs, rhs);
                self.cset_w(lhs, cond_for_i32_compare(op));
            }
            BaselineOp::F32Compare { op } => {
                self.emit_f32_compare(op)?;
            }
            BaselineOp::F32Binary { op } => {
                self.emit_f32_binary(op)?;
            }
            BaselineOp::F32Unary { op } => {
                self.emit_f32_unary(op)?;
            }
            BaselineOp::F64Compare { op } => {
                self.emit_f64_compare(op)?;
            }
            BaselineOp::F64Binary { op } => {
                self.emit_f64_binary(op)?;
            }
            BaselineOp::F64Unary { op } => {
                self.emit_f64_unary(op)?;
            }
            BaselineOp::F32ConvertI32 { signed } => {
                self.emit_f32_convert_i32(signed)?;
            }
            BaselineOp::F32ConvertI64 { signed } => {
                self.emit_f32_convert_i64(signed)?;
            }
            BaselineOp::F32DemoteF64 => {
                self.emit_f32_demote_f64()?;
            }
            BaselineOp::F64ConvertI32 { signed } => {
                self.emit_f64_convert_i32(signed)?;
            }
            BaselineOp::F64ConvertI64 { signed } => {
                self.emit_f64_convert_i64(signed)?;
            }
            BaselineOp::F64PromoteF32 => {
                self.emit_f64_promote_f32()?;
            }
            BaselineOp::I32TruncSatFloat { source, signed } => {
                self.emit_i32_trunc_sat_float(source, signed)?;
            }
            BaselineOp::I32TruncFloat { source, signed } => {
                self.emit_i32_trunc_float(source, signed)?;
            }
            BaselineOp::I64TruncFloat {
                source,
                signed,
                saturating,
            } => {
                self.emit_i64_trunc_float(source, signed, saturating)?;
            }
            BaselineOp::I32Load {
                memarg,
                width,
                signed,
            } => {
                self.ensure_stack_slots(1)?;
                let addr = self.peek_reg()?;
                self.inline_i32_load(addr, memarg.offset, width, signed)?;
            }
            BaselineOp::I32LoadLocalGet4 {
                memarg,
                width,
                signed,
                local,
            } => {
                self.ensure_stack_slots(1)?;
                let addr = self.pop_reg()?;
                self.inline_i32_load(addr, memarg.offset, width, signed)?;
                let loaded = self.push_reg()?;
                if loaded != addr {
                    self.mov_w(loaded, addr);
                }
                let preserved = self.push_reg()?;
                self.load_local4_to_reg(preserved, local)?;
            }
            BaselineOp::I64Load {
                memarg,
                width,
                signed,
            } => {
                self.ensure_stack_slots(1)?;
                let addr = self.pop_reg()?;
                self.push_i64_load_from_addr(addr, memarg.offset, width, signed)?;
            }
            BaselineOp::F64LoadConstBase { memarg } => {
                self.mov_imm_u32(16, 0);
                self.push_i64_load_from_addr(16, memarg.offset, 8, false)?;
            }
            BaselineOp::F64LoadLocalBase {
                local,
                delta,
                memarg,
            } => {
                self.load_local_base_addr_to_reg(16, local, delta)?;
                self.push_i64_load_from_addr(16, memarg.offset, 8, false)?;
            }
            BaselineOp::I64LoadLocalBase {
                local,
                delta,
                memarg,
                width,
                signed,
            } => {
                self.load_local_base_addr_to_reg(16, local, delta)?;
                self.push_i64_load_from_addr(16, memarg.offset, width, signed)?;
            }
            BaselineOp::I32LoadConstBase { memarg } => {
                let value = self.push_reg()?;
                self.mov_imm_u32(value, 0);
                self.inline_i32_load(value, memarg.offset, 4, false)?;
            }
            BaselineOp::I32LoadConstBaseLocalGet4I32AddSet4 { memarg, rhs, dst } => {
                self.mov_imm_u32(16, 0);
                self.inline_i32_load(16, memarg.offset, 4, false)?;
                self.load_local4_to_reg(17, rhs)?;
                self.add_w(16, 16, 17);
                self.store_local4_from_reg(dst, 16)?;
            }
            BaselineOp::I32LoadStoreLocalBaseLocalGet4 {
                load_kind,
                store_kind,
                load_memarg,
                store_addr_local,
                store_delta,
                value_local,
                store_memarg,
                skip_slots,
            } => {
                let Some((load_width, load_signed)) = scalar_load_kind(load_kind) else {
                    return Err(());
                };
                let Some(store_width) = scalar_store_kind(store_kind) else {
                    return Err(());
                };
                let addr = self.pop_reg()?;
                self.inline_i32_load(addr, load_memarg.offset, load_width, load_signed)?;
                let loaded = self.push_reg()?;
                if loaded != addr {
                    self.mov_w(loaded, addr);
                }
                self.load_local_base_addr_to_reg(16, store_addr_local, store_delta)?;
                self.load_local4_to_reg(17, value_local)?;
                self.inline_i32_store(16, store_memarg.offset, store_width, 17)?;
                return Ok(EmitControl::SkipInstrSlots(skip_slots));
            }
            BaselineOp::I32LoadLocalBase {
                local,
                delta,
                memarg,
                width,
                signed,
            } => {
                let addr = self.push_reg()?;
                self.load_local_base_addr_to_reg(addr, local, delta)?;
                self.inline_i32_load(addr, memarg.offset, width, signed)?;
            }
            BaselineOp::I32LoadLocalBaseSet4 {
                local,
                delta,
                memarg,
                width,
                signed,
                dst,
                keep_result,
            } => {
                self.load_local_base_addr_to_reg(16, local, delta)?;
                self.inline_i32_load(16, memarg.offset, width, signed)?;
                self.store_local4_from_reg(dst, 16)?;
                if keep_result {
                    let result = self.push_reg()?;
                    self.mov_w(result, 16);
                }
            }
            BaselineOp::I32LoadLocalBaseLocalGet4 {
                local,
                delta,
                memarg,
                width,
                signed,
                dst,
                preserved,
            } => {
                let value = self.push_reg()?;
                self.load_local_base_addr_to_reg(value, local, delta)?;
                self.inline_i32_load(value, memarg.offset, width, signed)?;
                if let Some(dst) = dst {
                    self.store_local4_from_reg(dst, value)?;
                }
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
            }
            BaselineOp::I32LoadLocalBaseSet4LocalGet4 {
                local,
                delta,
                memarg,
                width,
                signed,
                dst,
                preserved,
            } => {
                self.load_local_base_addr_to_reg(16, local, delta)?;
                self.inline_i32_load(16, memarg.offset, width, signed)?;
                self.store_local4_from_reg(dst, 16)?;
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
            }
            BaselineOp::LocalGet4I32LoadLocalBase {
                preserved,
                base_local,
                delta,
                memarg,
                width,
                signed,
            } => {
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
                let value = self.push_reg()?;
                self.load_local_base_addr_to_reg(value, base_local, delta)?;
                self.inline_i32_load(value, memarg.offset, width, signed)?;
            }
            BaselineOp::LocalGet4I32IncLocalBase {
                preserved,
                base_local,
                store_delta,
                load_delta,
                load_memarg,
                store_memarg,
            } => {
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
                self.emit_i32_inc_local_base(
                    base_local,
                    store_delta,
                    load_delta,
                    load_memarg,
                    store_memarg,
                )?;
            }
            BaselineOp::LocalGet4I32Load8ULocalBaseSet4 {
                preserved,
                load_base_local,
                load_delta,
                load_memarg,
                dst,
            } => {
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
                self.load_local_base_addr_to_reg(16, load_base_local, load_delta)?;
                self.inline_i32_load(16, load_memarg.offset, 1, false)?;
                self.store_local4_from_reg(dst, 16)?;
            }
            BaselineOp::LocalGet4I32IncLocalBaseI32Load8ULocalBaseSet4 {
                preserved,
                inc_base_local,
                inc_store_delta,
                inc_load_delta,
                inc_load_memarg,
                inc_store_memarg,
                load_base_local,
                load_delta,
                load_memarg,
                dst,
            } => {
                let preserved_reg = self.push_reg()?;
                self.load_local4_to_reg(preserved_reg, preserved)?;
                self.emit_i32_inc_local_base(
                    inc_base_local,
                    inc_store_delta,
                    inc_load_delta,
                    inc_load_memarg,
                    inc_store_memarg,
                )?;
                self.load_local_base_addr_to_reg(16, load_base_local, load_delta)?;
                self.inline_i32_load(16, load_memarg.offset, 1, false)?;
                self.store_local4_from_reg(dst, 16)?;
            }
            BaselineOp::LocalGet4I32LoadLocalBaseI32AddWrite4 {
                rhs,
                base_local,
                delta,
                memarg,
                dst,
                keep_result,
            } => {
                self.load_local_base_addr_to_reg(16, base_local, delta)?;
                self.inline_i32_load(16, memarg.offset, 4, false)?;
                self.load_local4_to_reg(17, rhs)?;
                self.add_w(16, 16, 17);
                self.store_local4_from_reg(dst, 16)?;
                if keep_result {
                    let value = self.push_reg()?;
                    self.mov_w(value, 16);
                }
            }
            BaselineOp::LocalGet4x3I32AddConstBinopI32AddWrite4 {
                first,
                second,
                third,
                kind,
                rhs,
                dst,
                skip_slots,
                keep_result,
            } => {
                self.emit_local_get4x3_add_const_binop_add(first, second, third, kind, rhs, 16)?;
                self.store_local4_from_reg(dst, 16)?;
                if keep_result {
                    let value = self.push_reg()?;
                    self.mov_w(value, 16);
                }
                return Ok(EmitControl::SkipInstrSlots(skip_slots));
            }
            BaselineOp::LocalGet4x3I32AddConstBinopI32AddTee4I32ConstStore {
                first,
                second,
                third,
                kind,
                rhs,
                dst,
                value,
                memarg,
                skip_slots,
            } => {
                self.emit_local_get4x3_add_const_binop_add(first, second, third, kind, rhs, 16)?;
                self.store_local4_from_reg(dst, 16)?;
                self.mov_imm_u32(17, value);
                self.inline_i32_store(16, memarg.offset, 4, 17)?;
                return Ok(EmitControl::SkipInstrSlots(skip_slots));
            }
            BaselineOp::I32LoadLocalBaseSet4I32LoadLocalBase {
                first_base_local,
                first_delta,
                first_memarg,
                dst,
                second_delta,
                second_memarg,
                second_width,
                second_signed,
                preserved,
            } => {
                self.load_local_base_addr_to_reg(16, first_base_local, first_delta)?;
                self.inline_i32_load(16, first_memarg.offset, 4, false)?;
                self.store_local4_from_reg(dst, 16)?;
                self.add_imm_u32(16, 16, second_delta)?;
                self.inline_i32_load(16, second_memarg.offset, second_width, second_signed)?;
                let value = self.push_reg()?;
                self.mov_w(value, 16);
                if let Some(preserved) = preserved {
                    let preserved_reg = self.push_reg()?;
                    self.load_local4_to_reg(preserved_reg, preserved)?;
                }
            }
            BaselineOp::I32LoadLocalBaseSet4I32LoadLocalBaseEqBrIf {
                first_base_local,
                first_delta,
                first_memarg,
                dst,
                second_delta,
                second_memarg,
                second_width,
                second_signed,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local_base_addr_to_reg(16, first_base_local, first_delta)?;
                self.inline_i32_load(16, first_memarg.offset, 4, false)?;
                self.store_local4_from_reg(dst, 16)?;
                self.add_imm_u32(16, 16, second_delta)?;
                self.inline_i32_load(16, second_memarg.offset, second_width, second_signed)?;
                self.load_local4_to_reg(17, rhs)?;
                self.cmp_w(16, 17);
                self.branch_to(target, FixupKind::BCond(Cond::Eq));
            }
            BaselineOp::I32LoadLocalBaseSet4SearchLoop {
                node_local,
                data_delta,
                data_memarg,
                data_local,
                field_delta,
                field_memarg,
                field_width,
                rhs_local,
                rhs_mask,
                compare,
                next_delta,
                next_memarg,
                match_target,
                miss_target,
            } => {
                self.emit_search_loop(SearchLoopPlan {
                    node_local,
                    data_delta,
                    data_memarg,
                    data_local,
                    field_delta,
                    field_memarg,
                    field_width,
                    rhs_local,
                    rhs_mask,
                    compare,
                    next_delta,
                    next_memarg,
                    match_target,
                    miss_target,
                })?;
            }
            BaselineOp::I32LoadStoreLocalBaseReverseLoop {
                prev_local,
                saved_local,
                cursor_local,
                load_memarg,
                store_memarg,
            } => {
                self.emit_reverse_loop(
                    prev_local,
                    saved_local,
                    cursor_local,
                    load_memarg,
                    store_memarg,
                )?;
            }
            BaselineOp::I32LoadStoreLocalBaseRelinkLoop {
                cursor_local,
                current_local,
                prev_local,
                load_memarg,
                store_memarg,
            } => {
                self.emit_relink_loop(
                    cursor_local,
                    current_local,
                    prev_local,
                    load_memarg,
                    store_memarg,
                )?;
            }
            BaselineOp::I32Load16UpdateStore16LocalBaseLoop {
                subtract,
                ptr_local,
                scalar_local,
                counter_local,
                load_delta,
                store_delta,
                load_memarg,
                store_memarg,
            } => {
                self.emit_update_store16_loop(UpdateStore16LoopPlan {
                    subtract,
                    ptr_local,
                    scalar_local,
                    counter_local,
                    load_delta,
                    store_delta,
                    load_memarg,
                    store_memarg,
                })?;
            }
            BaselineOp::I32LoadLocalBaseLocalGet4I32Load {
                first_base_local,
                first_delta,
                first_memarg,
                first_width,
                first_signed,
                second_addr_local,
                second_memarg,
                second_width,
                second_signed,
            } => {
                let first = self.push_reg()?;
                self.load_local_base_addr_to_reg(first, first_base_local, first_delta)?;
                self.inline_i32_load(first, first_memarg.offset, first_width, first_signed)?;
                let second = self.push_reg()?;
                self.load_local4_to_reg(second, second_addr_local)?;
                self.inline_i32_load(second, second_memarg.offset, second_width, second_signed)?;
            }
            BaselineOp::I32LoadLocalBaseLocalGet4I32LoadCmpBrIf {
                first_base_local,
                first_delta,
                first_memarg,
                first_width,
                first_signed,
                first_dst,
                second_addr_local,
                second_memarg,
                second_width,
                second_signed,
                second_dst,
                compare,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local_base_addr_to_reg(16, first_base_local, first_delta)?;
                self.inline_i32_load(16, first_memarg.offset, first_width, first_signed)?;
                self.store_local4_from_reg(first_dst, 16)?;
                self.load_local4_to_reg(17, second_addr_local)?;
                self.inline_i32_load(17, second_memarg.offset, second_width, second_signed)?;
                self.store_local4_from_reg(second_dst, 17)?;
                self.cmp_w(16, 17);
                self.branch_to(target, FixupKind::BCond(cond_for_i32_compare(compare)));
            }
            BaselineOp::I32LoadLocalScaledIndex {
                base_local,
                index_local,
                scale_log2,
                delta,
                memarg,
                width,
                signed,
            } => {
                let addr = self.push_reg()?;
                self.load_local_scaled_index_addr_to_reg(
                    addr,
                    base_local,
                    index_local,
                    scale_log2,
                    delta,
                )?;
                self.inline_i32_load(addr, memarg.offset, width, signed)?;
            }
            BaselineOp::I32Store { memarg, width } => {
                self.ensure_stack_slots(2)?;
                let value = self.pop_reg()?;
                let addr = self.pop_reg()?;
                self.inline_i32_store(addr, memarg.offset, width, value)?;
            }
            BaselineOp::I64Store { memarg, width } => {
                self.ensure_stack_slots(3)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                let addr = self.pop_reg()?;
                if width == 8 {
                    self.checked_memory_start(9, 10, addr, memarg.offset, 8)?;
                    self.inline_i32_store(addr, memarg.offset, 4, low)?;
                    self.add_imm_u32(addr, addr, 4)?;
                    self.inline_i32_store(addr, memarg.offset, 4, high)?;
                } else {
                    self.inline_i32_store(addr, memarg.offset, width, low)?;
                }
            }
            BaselineOp::I64StoreLocalBase {
                base_local,
                delta,
                memarg,
                width,
            } => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.load_local_base_addr_to_reg(16, base_local, delta)?;
                if width == 8 {
                    self.checked_memory_start(9, 10, 16, memarg.offset, 8)?;
                    self.inline_i32_store(16, memarg.offset, 4, low)?;
                    self.add_imm_u32(16, 16, 4)?;
                    self.inline_i32_store(16, memarg.offset, 4, high)?;
                } else {
                    self.inline_i32_store(16, memarg.offset, width, low)?;
                }
            }
            BaselineOp::F64StoreLocalBase {
                base_local,
                delta,
                memarg,
            } => {
                self.ensure_stack_slots(2)?;
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.load_local_base_addr_to_reg(16, base_local, delta)?;
                self.checked_memory_start(9, 10, 16, memarg.offset, 8)?;
                self.inline_i32_store(16, memarg.offset, 4, low)?;
                self.add_imm_u32(16, 16, 4)?;
                self.inline_i32_store(16, memarg.offset, 4, high)?;
            }
            BaselineOp::StoreConstBaseLocal4 { memarg, local } => {
                self.mov_imm_u32(16, 0);
                self.load_local4_to_reg(17, local)?;
                self.inline_i32_store(16, memarg.offset, 4, 17)?;
            }
            BaselineOp::StoreConstBaseLocal8 { memarg, local } => {
                self.mov_imm_u32(16, 0);
                self.load_local_i64_to_x(17, local, 9)?;
                self.mov_w(12, 17);
                self.inline_i32_store(16, memarg.offset, 4, 12)?;
                self.add_imm_u32(16, 16, 4)?;
                self.lsr_x_imm(12, 17, 32)?;
                self.inline_i32_store(16, memarg.offset, 4, 12)?;
            }
            BaselineOp::I32StoreLocalBaseLocalGet4 {
                addr_local,
                delta,
                value_local,
                memarg,
                width,
            } => {
                self.load_local4_to_reg(16, addr_local)?;
                self.add_imm_u32(16, 16, delta)?;
                self.load_local4_to_reg(17, value_local)?;
                self.inline_i32_store(16, memarg.offset, width, 17)?;
            }
            BaselineOp::I32StoreLocalBase {
                base_local,
                delta,
                memarg,
                width,
            } => {
                if self.stack_depth == 0 {
                    self.call_i32_store_local_base_from_vm_stack(cursor, width)?;
                    return Ok(EmitControl::Continue);
                }
                let value = self.pop_reg()?;
                self.load_local_base_addr_to_reg(16, base_local, delta)?;
                self.inline_i32_store(16, memarg.offset, width, value)?;
            }
            BaselineOp::I32StoreLocalScaledIndex {
                base_local,
                index_local,
                scale_log2,
                delta,
                memarg,
                width,
            } => {
                let value = self.pop_reg()?;
                self.load_local_scaled_index_addr_to_reg(
                    16,
                    base_local,
                    index_local,
                    scale_log2,
                    delta,
                )?;
                self.inline_i32_store(16, memarg.offset, width, value)?;
            }
            BaselineOp::I32IncLocalBase {
                base_local,
                store_delta,
                load_delta,
                load_memarg,
                store_memarg,
            } => {
                self.load_local4_to_reg(16, base_local)?;
                self.add_imm_u32(16, 16, load_delta)?;
                self.inline_i32_load(16, load_memarg.offset, 4, false)?;
                self.add_imm_u32(16, 16, 1)?;
                self.load_local4_to_reg(17, base_local)?;
                self.add_imm_u32(17, 17, store_delta)?;
                self.inline_i32_store(17, store_memarg.offset, 4, 16)?;
            }
            BaselineOp::ScalarCopyLocalBaseRun {
                width,
                dst_base_local,
                src_base_local,
                lanes,
            } => {
                self.load_local4_to_reg(12, dst_base_local)?;
                self.load_local4_to_reg(13, src_base_local)?;
                for lane in lanes {
                    self.mov_w(16, 13);
                    self.add_imm_u32(16, 16, lane.src_delta)?;
                    self.inline_i32_load(16, lane.load_memarg.offset, width, false)?;
                    self.mov_w(17, 12);
                    self.add_imm_u32(17, 17, lane.dst_delta)?;
                    self.inline_i32_store(17, lane.store_memarg.offset, width, 16)?;
                }
            }
            BaselineOp::I32LoadLocalBaseTeeLoad8UTeeBrIf {
                first_base_local,
                first_delta,
                first_memarg,
                first_dst,
                byte_memarg,
                byte_dst,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local_base_addr_to_reg(16, first_base_local, first_delta)?;
                self.inline_i32_load(16, first_memarg.offset, 4, false)?;
                self.store_local4_from_reg(first_dst, 16)?;
                self.inline_i32_load(16, byte_memarg.offset, 1, false)?;
                self.store_local4_from_reg(byte_dst, 16)?;
                self.branch_to(target, FixupKind::CbnzW(16));
                return Ok(EmitControl::SkipNextOp);
            }
            BaselineOp::I32LoadTee4BrIf {
                memarg,
                width,
                signed,
                dst,
                eqz,
                target,
            } => {
                if self.stack_depth != 1 {
                    return Err(());
                }
                let addr = self.pop_reg()?;
                self.inline_i32_load(addr, memarg.offset, width, signed)?;
                self.store_local4_from_reg(dst, addr)?;
                if eqz {
                    self.cmp_w_imm(addr, 0)?;
                    self.branch_to(target, FixupKind::BCond(Cond::Eq));
                } else {
                    self.branch_to(target, FixupKind::CbnzW(addr));
                }
            }
            BaselineOp::I32LoadLocalBaseTee4BrIf {
                base_local,
                delta,
                memarg,
                width,
                signed,
                dst,
                eqz,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local_base_addr_to_reg(16, base_local, delta)?;
                self.inline_i32_load(16, memarg.offset, width, signed)?;
                self.store_local4_from_reg(dst, 16)?;
                if eqz {
                    self.cmp_w_imm(16, 0)?;
                    self.branch_to(target, FixupKind::BCond(Cond::Eq));
                } else {
                    self.branch_to(target, FixupKind::CbnzW(16));
                }
            }
            BaselineOp::I32GuardedLoad8UpdateBrIf {
                next_src,
                next_delta,
                next_dst,
                guard_kind,
                guard_lhs,
                guard_rhs,
                false_target,
                ptr_local,
                load_delta,
                memarg,
                byte_dst,
                update_src,
                ptr_dst,
                branch_local,
                true_target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }

                self.load_local4_to_reg(16, next_src)?;
                self.add_imm_u32(16, 16, next_delta)?;
                self.store_local4_from_reg(next_dst, 16)?;

                let Some((guard_op, rhs_shape)) = decode_local_cmp32_kind(guard_kind) else {
                    return Err(());
                };
                self.load_local4_to_reg(16, guard_lhs)?;
                match rhs_shape {
                    LocalFastRhsShape::Local => {
                        self.load_local4_to_reg(17, guard_rhs)?;
                        self.cmp_w(16, 17);
                    }
                    LocalFastRhsShape::Const => self.cmp_w_u32(16, guard_rhs),
                }
                self.branch_to(
                    false_target,
                    FixupKind::BCond(i32_cmp_cond(guard_op)?.inverted()),
                );

                self.load_local_base_addr_to_reg(16, ptr_local, load_delta)?;
                self.inline_i32_load(16, memarg.offset, 1, false)?;
                self.store_local4_from_reg(byte_dst, 16)?;
                self.load_local4_to_reg(17, update_src)?;
                self.store_local4_from_reg(ptr_dst, 17)?;
                self.load_local4_to_reg(16, branch_local)?;
                self.branch_to(true_target, FixupKind::CbnzW(16));
                self.branch_to(false_target, FixupKind::B);
            }
            BaselineOp::I32Load8UpdateBrIf {
                ptr_local,
                load_delta,
                memarg,
                byte_dst,
                next_src,
                ptr_dst,
                branch_local,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local_base_addr_to_reg(16, ptr_local, load_delta)?;
                self.inline_i32_load(16, memarg.offset, 1, false)?;
                self.store_local4_from_reg(byte_dst, 16)?;
                self.load_local4_to_reg(17, next_src)?;
                self.store_local4_from_reg(ptr_dst, 17)?;
                self.load_local4_to_reg(16, branch_local)?;
                self.branch_to(target, FixupKind::CbnzW(16));
            }
            BaselineOp::LocalAddSetLoad8EqzBrIf {
                add_src,
                imm,
                add_dst,
                load_base,
                load_delta,
                memarg,
                tee_dst,
                target,
            } => {
                if self.stack_depth > 0 {
                    return Err(());
                }
                self.load_local4_to_reg(16, add_src)?;
                self.add_imm_u32(16, 16, imm)?;
                self.store_local4_from_reg(add_dst, 16)?;
                self.load_local4_to_reg(17, load_base)?;
                self.add_imm_u32(17, 17, load_delta)?;
                self.inline_i32_load(17, memarg.offset, 1, false)?;
                self.store_local4_from_reg(tee_dst, 17)?;
                self.cmp_w_imm(17, 0)?;
                self.branch_to(target, FixupKind::BCond(Cond::Eq));
                return Ok(EmitControl::SkipNextOp);
            }
            BaselineOp::MemoryFill => {
                profile::count(Counter::EmitMemoryFill);
                let len = self.pop_reg()?;
                let data = self.pop_reg()?;
                let ptr = self.pop_reg()?;
                self.call_memory_fill(ptr, data, len);
            }
            BaselineOp::MemoryCopy => {
                profile::count(Counter::EmitMemoryCopy);
                let len = self.pop_reg()?;
                let src = self.pop_reg()?;
                let dst = self.pop_reg()?;
                self.call_memory_copy(dst, src, len);
            }
            BaselineOp::MemorySize { shared } => {
                self.mov_x(0, 19);
                self.mov_imm_u32(1, u32::from(shared));
                self.call_ptr(jit_memory_size as *const () as usize);
                let result = self.push_reg()?;
                self.mov_w(result, 0);
            }
            BaselineOp::MemoryGrow { shared } => {
                self.ensure_stack_slots(1)?;
                let delta = self.pop_reg()?;
                self.mov_x(0, 19);
                self.mov_w(1, delta);
                self.mov_imm_u32(2, u32::from(shared));
                self.call_ptr(jit_memory_grow as *const () as usize);
                let result = self.push_reg()?;
                self.mov_w(result, 0);
            }
            BaselineOp::Branch { target } => {
                self.flush_stack()?;
                self.branch_to(target, FixupKind::B);
            }
            BaselineOp::BrIf { target } => {
                if self.stack_depth == 0 {
                    self.inline_pop_i32(16)?;
                    self.branch_to(target, FixupKind::CbnzW(16));
                } else if self.stack_depth > 1 {
                    let reload_slots = self.stack_depth.checked_sub(1).ok_or(())?;
                    self.call_runtime_continuation_op_reloading_stack(
                        cursor,
                        RUNTIME_CONT_CURRENT_VM_HANDLER,
                        reload_slots,
                    )?;
                } else {
                    let cond = self.pop_reg()?;
                    if self.stack_depth > 0 {
                        self.flush_stack()?;
                    }
                    self.branch_to(target, FixupKind::CbnzW(cond));
                }
            }
            BaselineOp::LocalGet4BrIf { local, target } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.cmp_w_imm(16, 0)?;
                self.branch_to(target, FixupKind::BCond(Cond::Ne));
            }
            BaselineOp::LocalGet4I32ConstAddBrIf { local, imm, target } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.add_imm_u32(16, 16, imm)?;
                self.branch_to(target, FixupKind::CbnzW(16));
            }
            BaselineOp::LocalGet4LocalGet4I32AddBrIf { lhs, rhs, target } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, lhs)?;
                self.load_local4_to_reg(17, rhs)?;
                self.add_w(16, 16, 17);
                self.branch_to(target, FixupKind::CbnzW(16));
            }
            BaselineOp::LocalGet4I32EqzBrIf { local, target } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.cmp_w_imm(16, 0)?;
                self.branch_to(target, FixupKind::BCond(Cond::Eq));
            }
            BaselineOp::LocalGet4I32ConstCompareBrIf {
                local,
                kind,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.cmp_w_u32(16, rhs);
                self.branch_to(target, FixupKind::BCond(raw_i32_cmp_cond(kind)?));
            }
            BaselineOp::LocalGet4LocalGet4CompareBrIf {
                lhs,
                rhs,
                kind,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, lhs)?;
                self.load_local4_to_reg(17, rhs)?;
                self.cmp_w(16, 17);
                self.branch_to(target, FixupKind::BCond(raw_i32_cmp_cond(kind)?));
            }
            BaselineOp::LocalGet4I32ConstAndBrIf {
                local,
                mask,
                eqz,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.mov_imm_u32(17, mask);
                self.and_w(16, 16, 17);
                if eqz {
                    self.cmp_w_imm(16, 0)?;
                    self.branch_to(target, FixupKind::BCond(Cond::Eq));
                } else {
                    self.branch_to(target, FixupKind::CbnzW(16));
                }
            }
            BaselineOp::LocalGet4I32ConstAndI32ConstCompareBrIf {
                local,
                mask,
                kind,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.mov_imm_u32(17, mask);
                self.and_w(16, 16, 17);
                self.cmp_w_u32(16, rhs);
                self.branch_to(target, FixupKind::BCond(raw_i32_cmp_cond(kind)?));
            }
            BaselineOp::LocalGet4I32ConstAndTee4I32ConstEqBrIf {
                local,
                mask,
                dst,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.mov_imm_u32(17, mask);
                self.and_w(16, 16, 17);
                self.store_local4_from_reg(dst, 16)?;
                self.cmp_w_u32(16, rhs);
                self.branch_to(target, FixupKind::BCond(Cond::Eq));
            }
            BaselineOp::LocalGet4Set4LocalGet4I32ConstCompareBrIf {
                copy_src,
                copy_dst,
                lhs,
                kind,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, copy_src)?;
                self.store_local4_from_reg(copy_dst, 16)?;
                self.load_local4_to_reg(16, lhs)?;
                self.cmp_w_u32(16, rhs);
                self.branch_to(target, FixupKind::BCond(raw_i32_cmp_cond(kind)?));
            }
            BaselineOp::LocalGet4I32ConstAddI32ConstAndI32ConstCompareBrIf {
                local,
                imm,
                mask,
                kind,
                rhs,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                self.add_imm_u32(16, 16, imm)?;
                self.mov_imm_u32(17, mask);
                self.and_w(16, 16, 17);
                self.cmp_w_u32(16, rhs);
                self.branch_to(target, FixupKind::BCond(raw_i32_cmp_cond(kind)?));
            }
            BaselineOp::LocalGet4I32ConstAddTee4BrIf {
                src,
                imm,
                dst,
                target,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, src)?;
                self.add_imm_u32(16, 16, imm)?;
                self.store_local4_from_reg(dst, 16)?;
                self.branch_to(target, FixupKind::CbnzW(16));
            }
            BaselineOp::LocalGet4BrTable {
                local,
                addend,
                targets,
            } => {
                if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.load_local4_to_reg(16, local)?;
                if addend != 0 {
                    self.add_imm_u32(16, 16, addend)?;
                }
                self.branch_table(16, &targets)?;
                return Ok(EmitControl::SkipNextOp);
            }
            BaselineOp::BrTable { targets } => {
                if self.stack_depth != 1 {
                    self.call_runtime_continuation_op(cursor, RUNTIME_CONT_CURRENT_VM_HANDLER)?;
                } else {
                    let index = self.pop_reg()?;
                    self.branch_table(index, &targets)?;
                    return Ok(EmitControl::SkipNextOp);
                }
            }
            BaselineOp::If { else_target } => {
                self.ensure_stack_slots(1)?;
                let cond = self.pop_reg()?;
                self.flush_stack()?;
                self.cmp_w_imm(cond, 0)?;
                self.branch_to(else_target, FixupKind::BCond(Cond::Eq));
            }
            BaselineOp::Else { target } => {
                self.flush_stack()?;
                self.branch_to(target, FixupKind::B);
            }
            BaselineOp::Loop { param } => {
                if param.param_size != 0 {
                    self.flush_stack()?;
                    self.call_block_return_helper(param.stack_top, param.param_size);
                } else if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
            }
            BaselineOp::End {
                next_is_function_return,
                next_resets_stack,
            } => {
                if self.stack_depth > 0 && !next_is_function_return {
                    if next_resets_stack {
                        self.stack_depth = 0;
                        return Ok(EmitControl::Continue);
                    }
                    self.flush_stack()?;
                }
            }
            BaselineOp::BlockReturn { block_return } => {
                if block_return.return_size != 0 {
                    self.flush_stack()?;
                    self.call_block_return_helper(block_return.stack_top, block_return.return_size);
                } else if self.stack_depth > 0 {
                    self.flush_stack()?;
                }
                self.stack_depth = 0;
            }
            BaselineOp::FunctionReturn { return_size } => {
                self.flush_stack()?;
                self.call_return_helper(return_size);
            }
            BaselineOp::FunctionVmEnd => {
                self.mov_imm_u64(0, JitNativeExit::DONE);
                self.mov_imm_u64(1, 0);
                self.branch_to_epilogue();
            }
            BaselineOp::I32Crc16Update16 {
                data_local,
                crc_local,
                return_target,
                masked,
            } => {
                self.call_i32_crc16_update16(data_local, crc_local, masked);
                self.branch_to(return_target, FixupKind::B);
            }
            BaselineOp::I32CoreStateBenchmark {
                locals,
                return_target,
            } => {
                self.call_i32_core_state_benchmark(locals);
                self.branch_to(return_target, FixupKind::B);
            }
            BaselineOp::I32NumericTokenStateTransition {
                instr_ref_local,
                counts_local,
                return_target,
            } => {
                self.call_i32_numeric_token_state_transition(instr_ref_local, counts_local);
                self.branch_to(return_target, FixupKind::B);
            }
            BaselineOp::I32ListCrcPairLoop {
                frame_base_local,
                res_delta,
                iterations_delta,
                crc_delta,
                target,
            } => {
                self.call_i32_list_crc_pair_loop(
                    frame_base_local,
                    res_delta,
                    iterations_delta,
                    crc_delta,
                );
                self.branch_to(target, FixupKind::B);
            }
            BaselineOp::I32ListCrcSummary {
                res_local,
                finder_idx_local,
                return_target,
            } => {
                self.call_i32_list_crc_summary(res_local, finder_idx_local);
                self.branch_to(return_target, FixupKind::B);
            }
            BaselineOp::I32SelectBitStep4 { step } => {
                let slots = self.stack_depth;
                self.flush_stack()?;
                self.call_i32_select_bit_step4(&step);
                self.reload_stack_slots(slots)?;
            }
            BaselineOp::I32SelectBitStep4Run { steps } => {
                let slots = self.stack_depth;
                self.flush_stack()?;
                for step in steps {
                    self.call_i32_select_bit_step4(&step);
                }
                self.reload_stack_slots(slots)?;
            }
            BaselineOp::CallI32Crc16Update16 { masked } => {
                if self.stack_depth < 2 {
                    return Err(());
                }
                let reload_slots = self.stack_depth - 1;
                self.flush_stack()?;
                self.call_i32_crc16_update16_call(masked);
                self.reload_stack_slots(reload_slots)?;
            }
            BaselineOp::CallI32ListCrcSummary => {
                if self.stack_depth < 2 {
                    return Err(());
                }
                let reload_slots = self.stack_depth - 1;
                self.flush_stack()?;
                self.call_i32_list_crc_summary_call();
                self.reload_stack_slots(reload_slots)?;
            }
            BaselineOp::DirectCall {
                operand_index,
                continuation_index,
                is_return_call,
            } => {
                if is_return_call {
                    self.call_runtime_continuation_op(cursor, RUNTIME_CONT_CURRENT_VM_HANDLER)?;
                    self.branch_to_epilogue();
                    return Ok(EmitControl::Continue);
                }
                let recipe = self.call_recipe_for_operand(operand_index)?;
                let flushed_size =
                    u32::try_from(self.stack_depth.checked_mul(4).ok_or(())?).map_err(|_| ())?;
                let reload_size = if flushed_size >= recipe.param_size {
                    flushed_size
                        .checked_sub(recipe.param_size)
                        .and_then(|size| size.checked_add(recipe.return_size))
                        .ok_or(())?
                } else {
                    recipe.return_size
                };
                let reload_slots = usize::try_from(reload_size / 4).map_err(|_| ())?;
                if reload_size % 4 != 0 || reload_slots > STACK_REGS.len() {
                    trace_direct_call_reject(
                        cursor,
                        "reload",
                        flushed_size,
                        recipe.param_size,
                        recipe.return_size,
                    );
                    self.call_runtime_continuation_op(cursor, RUNTIME_CONT_CURRENT_VM_HANDLER)?;
                    if is_return_call {
                        self.branch_to_epilogue();
                    }
                    return Ok(EmitControl::Continue);
                }
                let expected_layout = pack_call_stack_sizes(recipe);
                let use_wasm_fast_path =
                    !is_return_call && matches!(recipe.target, CallDispatchTarget::Wasm { .. });
                self.flush_stack()?;
                let continued = if use_wasm_fast_path {
                    profile::count(Counter::EmitDirectCallFast);
                    self.call_wasm_direct_fast_helper(
                        recipe.frame.code_addr,
                        continuation_index,
                        expected_layout,
                        reload_slots,
                    )?
                } else {
                    profile::count(Counter::EmitDirectCallHelper);
                    self.call_direct_helper(
                        operand_index,
                        continuation_index,
                        is_return_call,
                        expected_layout,
                        reload_slots,
                    )?
                };
                if !is_return_call {
                    if continued {
                        return Ok(EmitControl::Continue);
                    }
                    return Err(());
                }
                return Ok(EmitControl::Continue);
            }
            BaselineOp::IndirectCall {
                operand_index,
                continuation_index,
                is_return_call,
            } => {
                if is_return_call {
                    self.call_runtime_continuation_op(cursor, RUNTIME_CONT_CURRENT_VM_HANDLER)?;
                    self.branch_to_epilogue();
                    return Ok(EmitControl::Continue);
                }
                let (param_size, return_size) =
                    self.indirect_call_layout_for_operand(operand_index)?;
                let flushed_size =
                    u32::try_from(self.stack_depth.checked_mul(4).ok_or(())?).map_err(|_| ())?;
                let consumed_size = param_size.checked_add(4).ok_or(())?;
                let reload_size = if flushed_size >= consumed_size {
                    flushed_size
                        .checked_sub(consumed_size)
                        .and_then(|size| size.checked_add(return_size))
                        .ok_or(())?
                } else {
                    return_size
                };
                let reload_slots = usize::try_from(reload_size / 4).map_err(|_| ())?;
                if reload_size % 4 != 0 || reload_slots > STACK_REGS.len() {
                    self.call_runtime_continuation_op(cursor, RUNTIME_CONT_CURRENT_VM_HANDLER)?;
                    if is_return_call {
                        self.branch_to_epilogue();
                    }
                    return Ok(EmitControl::Continue);
                }
                let expected_layout = (u64::from(param_size) << 32) | u64::from(return_size);
                profile::count(Counter::EmitIndirectCallHelper);
                self.flush_stack()?;
                let continued = self.call_indirect_helper(
                    operand_index,
                    continuation_index,
                    is_return_call,
                    expected_layout,
                    reload_slots,
                )?;
                if !is_return_call {
                    if continued {
                        return Ok(EmitControl::Continue);
                    }
                    return Err(());
                }
                return Ok(EmitControl::Continue);
            }
            BaselineOp::AtomicFence { shared } => {
                self.mov_x(0, 19);
                self.mov_imm_u32(1, u32::from(shared));
                self.call_ptr(jit_atomic_fence as *const () as usize);
            }
            BaselineOp::RefNull => {
                let dst = self.push_reg()?;
                self.mov_imm_u32(dst, 0);
            }
            BaselineOp::RefIsNull => {
                let value = self.peek_reg()?;
                self.cmp_w_imm(value, 0)?;
                self.cset_w(value, Cond::Eq);
            }
            BaselineOp::RefFunc { funcidx } => {
                self.mov_x(0, 19);
                self.mov_imm_u32(1, funcidx);
                self.call_ptr(jit_ref_func as *const () as usize);
                let dst = self.push_reg()?;
                self.mov_w(dst, 0);
            }
            BaselineOp::Trap { result } => {
                self.return_trap(result);
                return Ok(EmitControl::Stop);
            }
            BaselineOp::RuntimeStub {
                pc_index,
                kind,
                pop_slots,
                push_slots,
            } => {
                let _ = pop_slots;
                self.call_runtime_stack_op(pc_index, kind, push_slots)?;
            }
            BaselineOp::RuntimeContinuationStub { pc_index, kind } => {
                self.call_runtime_continuation_op(pc_index, kind)?;
            }
        }
        Ok(EmitControl::Continue)
    }
}

impl<'a> Emitter<'a> {
    fn finish(mut self) -> Result<Vec<u8>, ()> {
        let epilogue = self.offset();
        self.restore_and_ret();
        for fixup in &self.fixups {
            if fixup.target_index != usize::MAX && self.resolve_label(fixup.target_index).is_none()
            {
                trace_compile_message(format_args!(
                    "compile_reject_missing_label target_index={}",
                    fixup.target_index
                ));
                return Err(());
            }
        }
        let fixups = std::mem::take(&mut self.fixups);
        for fixup in fixups {
            let target = self.resolve_label(fixup.target_index).unwrap_or(epilogue);
            patch_branch(self.masm.as_mut_bytes(), fixup.at, target, fixup.kind)?;
        }
        Ok(self.masm.into_bytes())
    }

    fn resolve_label(&self, target_index: usize) -> Option<usize> {
        self.labels
            .get(target_index)
            .and_then(|label| *label)
            .or_else(|| {
                self.labels
                    .iter()
                    .skip(target_index.saturating_add(1))
                    .find_map(|label| *label)
            })
    }

    fn offset(&self) -> usize {
        self.masm.offset()
    }

    fn prologue(&mut self) {
        self.stp_pre(29, 30);
        self.mov_x_from_sp(29);
        self.stp_pre(19, 20);
        self.stp_pre(21, 22);
        self.stp_pre(23, 24);
        self.stp_pre(25, 26);
        self.stp_pre(27, 28);
        self.mov_x(19, 0);
        self.mov_x(20, 1);
        self.mov_x(21, 2);
    }

    fn restore_and_ret(&mut self) {
        self.ldp_post(27, 28);
        self.ldp_post(25, 26);
        self.ldp_post(23, 24);
        self.ldp_post(21, 22);
        self.ldp_post(19, 20);
        self.ldp_post(29, 30);
        self.ret();
    }

    fn push_reg(&mut self) -> Result<u8, ()> {
        if self.stack_depth == STACK_REGS.len() {
            self.flush_stack()?;
        }
        let reg = STACK_REGS.get(self.stack_depth).copied().ok_or(())?;
        self.stack_depth += 1;
        Ok(reg)
    }

    fn pop_reg(&mut self) -> Result<u8, ()> {
        self.stack_depth = self.stack_depth.checked_sub(1).ok_or(())?;
        Ok(STACK_REGS[self.stack_depth])
    }

    fn peek_reg(&self) -> Result<u8, ()> {
        self.stack_depth
            .checked_sub(1)
            .and_then(|index| STACK_REGS.get(index))
            .copied()
            .ok_or(())
    }

    fn ensure_stack_slots(&mut self, slots: usize) -> Result<(), ()> {
        if self.stack_depth < slots {
            self.flush_stack()?;
            self.reload_stack_slots(slots)?;
        }
        Ok(())
    }

    fn emit_select_slots(&mut self, slots: usize) -> Result<(), ()> {
        if slots == 0 || slots > 3 || self.stack_depth < slots * 2 + 1 {
            return Err(());
        }

        let cond = self.pop_reg()?;
        let mut rhs = [0u8; 3];
        for reg in rhs.iter_mut().take(slots) {
            *reg = self.pop_reg()?;
        }
        let lhs_start = self.stack_depth.checked_sub(slots).ok_or(())?;
        self.cmp_w_imm(cond, 0)?;
        for lane in 0..slots {
            let lhs = STACK_REGS[lhs_start + lane];
            let rhs = rhs[slots - 1 - lane];
            self.csel_w(lhs, lhs, rhs, Cond::Ne);
        }
        Ok(())
    }

    fn flush_stack(&mut self) -> Result<(), ()> {
        profile::count(Counter::EmitStackFlush);
        profile::add(Counter::EmitStackFlushSlot, self.stack_depth as u64);
        let slots = self.stack_depth;
        if slots == 0 {
            return Ok(());
        }
        let byte_len = u64::try_from(slots.checked_mul(4).ok_or(())?).map_err(|_| ())?;
        self.ldr_x_imm(
            9,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, stack_top_ptr),
        )?;
        self.ldr_x_imm(10, 9, 0)?;
        self.add_imm_u64(11, 10, byte_len)?;
        self.ldr_x_imm(
            12,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, stack_memory_len),
        )?;
        self.cmp_x(11, 12);
        let ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ls));
        self.return_trap(VMResult::<()>::StackOverflow);
        let ok_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            ok_branch,
            ok_target,
            FixupKind::BCond(Cond::Ls),
        )?;
        self.ldr_x_imm(
            12,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, stack_memory_ptr),
        )?;
        self.add_x(12, 12, 10);
        for (slot, reg) in STACK_REGS.iter().take(slots).copied().enumerate() {
            self.str_w_imm(reg, 12, slot * 4)?;
        }
        self.str_x_imm(11, 9, 0)?;
        self.stack_depth = 0;
        Ok(())
    }

    fn reload_stack_slots(&mut self, slots: usize) -> Result<(), ()> {
        if slots > STACK_REGS.len() {
            return Err(());
        }
        profile::count(Counter::EmitStackReload);
        profile::add(Counter::EmitStackReloadSlot, slots as u64);
        if slots == 0 {
            return Ok(());
        }
        if self.stack_depth == 0 {
            let byte_len = u64::try_from(slots.checked_mul(4).ok_or(())?).map_err(|_| ())?;
            self.ldr_x_imm(
                9,
                19,
                std::mem::offset_of!(ExecuteContext<'_>, stack_top_ptr),
            )?;
            self.ldr_x_imm(10, 9, 0)?;
            self.mov_imm_u64(11, byte_len);
            self.sub_x(10, 10, 11);
            self.ldr_x_imm(
                12,
                19,
                std::mem::offset_of!(ExecuteContext<'_>, stack_memory_ptr),
            )?;
            self.add_x(12, 12, 10);
            for (slot, reg) in STACK_REGS.iter().take(slots).copied().enumerate() {
                self.ldr_w_imm(reg, 12, slot * 4)?;
            }
            self.str_x_imm(10, 9, 0)?;
            self.stack_depth = slots;
            return Ok(());
        }
        let mut regs = Vec::with_capacity(slots);
        for _ in 0..slots {
            regs.push(self.push_reg()?);
        }
        for reg in regs.into_iter().rev() {
            self.inline_pop_i32(reg)?;
        }
        Ok(())
    }

    fn inline_pop_i32(&mut self, dst: u8) -> Result<(), ()> {
        self.ldr_x_imm(
            9,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, stack_top_ptr),
        )?;
        self.ldr_x_imm(10, 9, 0)?;
        self.mov_imm_u64(11, 4);
        self.sub_x(10, 10, 11);
        self.ldr_x_imm(
            12,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, stack_memory_ptr),
        )?;
        self.add_x(12, 12, 10);
        self.ldr_w(dst, 12);
        self.str_x_imm(10, 9, 0)?;
        Ok(())
    }

    fn inline_global_get4(&mut self, dst: u8, index: u32, lane: u32) -> Result<(), ()> {
        profile::count(Counter::EmitGlobalGetInline);
        self.global_lane_addr(9, index, lane)?;
        self.ldr_w(dst, 9);
        Ok(())
    }

    fn inline_global_set4(&mut self, index: u32, lane: u32, value: u8) -> Result<(), ()> {
        profile::count(Counter::EmitGlobalSetInline);
        self.global_lane_addr(9, index, lane)?;
        self.str_w(value, 9);
        Ok(())
    }

    fn global_lane_addr(&mut self, addr: u8, index: u32, lane: u32) -> Result<(), ()> {
        let layout = GlobalValueJitLayout::get();
        let table_offset = usize::try_from(index)
            .ok()
            .and_then(|index| index.checked_mul(std::mem::size_of::<ObjectRef>()))
            .ok_or(())?;
        self.ldr_x_imm(
            addr,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, current_instance_globals_ptr),
        )?;
        if table_offset % 4 == 0 && table_offset / 4 <= 4095 {
            self.ldr_w_imm(addr, addr, table_offset)?;
        } else {
            self.add_imm_u64(10, addr, u64::try_from(table_offset).map_err(|_| ())?)?;
            self.ldr_w(addr, 10);
        }
        self.ubfm_w(addr, addr, 0, 28);
        self.sub_imm_u32(addr, addr, 1)?;
        if layout.size.is_power_of_two() {
            self.lsl_x_imm(addr, addr, layout.size.trailing_zeros())?;
        } else {
            self.mov_imm_u64(10, u64::try_from(layout.size).map_err(|_| ())?);
            self.mul_x(addr, addr, 10);
        }
        self.ldr_x_imm(
            10,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, global_values_ptr),
        )?;
        self.add_x(addr, 10, addr);
        let lane_offset = layout
            .bytes
            .checked_add(
                usize::try_from(lane)
                    .map_err(|_| ())?
                    .checked_mul(4)
                    .ok_or(())?,
            )
            .ok_or(())?;
        if lane_offset != 0 {
            self.add_imm_u64(addr, addr, u64::try_from(lane_offset).map_err(|_| ())?)?;
        }
        Ok(())
    }

    fn call_memory_fill(&mut self, ptr: u8, data: u8, len: u8) {
        self.mov_x(0, 19);
        self.mov_w(1, ptr);
        self.mov_w(2, data);
        self.mov_w(3, len);
        self.call_ptr(jit_memory_fill as *const () as usize);
        self.return_if_exit();
    }

    fn call_memory_copy(&mut self, dst: u8, src: u8, len: u8) {
        self.mov_x(0, 19);
        self.mov_w(1, dst);
        self.mov_w(2, src);
        self.mov_w(3, len);
        self.call_ptr(jit_memory_copy as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_crc16_update16(&mut self, data_local: u32, crc_local: u32, masked: bool) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, data_local);
        self.mov_imm_u32(2, crc_local);
        self.mov_imm_u32(3, u32::from(masked));
        self.call_ptr(jit_i32_crc16_update16 as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_crc16_update16_call(&mut self, masked: bool) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, u32::from(masked));
        self.call_ptr(jit_call_i32_crc16_update16 as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_list_crc_summary_call(&mut self) {
        self.mov_x(0, 19);
        self.call_ptr(jit_call_i32_list_crc_summary as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_list_crc_summary(&mut self, res_local: u32, finder_idx_local: u32) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, res_local);
        self.mov_imm_u32(2, finder_idx_local);
        self.call_ptr(jit_i32_list_crc_summary as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_list_crc_pair_loop(
        &mut self,
        frame_base_local: u32,
        res_delta: u32,
        iterations_delta: u32,
        crc_delta: u32,
    ) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, frame_base_local);
        self.mov_imm_u32(2, res_delta);
        self.mov_imm_u32(3, iterations_delta);
        self.mov_imm_u32(4, crc_delta);
        self.call_ptr(jit_i32_list_crc_pair_loop as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_core_state_benchmark(&mut self, locals: [u32; 6]) {
        self.mov_x(0, 19);
        for (index, local) in locals.into_iter().enumerate() {
            self.mov_imm_u32(
                u8::try_from(index + 1).expect("argument register fits u8"),
                local,
            );
        }
        self.call_ptr(jit_i32_core_state_benchmark as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_numeric_token_state_transition(&mut self, instr_ref_local: u32, counts_local: u32) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, instr_ref_local);
        self.mov_imm_u32(2, counts_local);
        self.call_ptr(jit_i32_numeric_token_state_transition as *const () as usize);
        self.return_if_exit();
    }

    fn call_i32_select_bit_step4(&mut self, step: &SelectBitStep4) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, step.tmp_local);
        self.mov_imm_u32(2, step.poly);
        self.mov_imm_u32(3, step.source_local);
        self.mov_imm_u32(4, step.source_shift);
        self.mov_imm_u32(5, step.prev_local);
        self.mov_imm_u32(6, step.flags);
        self.mov_imm_u32(7, step.dst_local);
        self.call_ptr(jit_i32_select_bit_step4 as *const () as usize);
        self.return_if_exit();
    }

    fn call_recipe_for_operand(&self, operand_index: usize) -> Result<CallDispatchCache, ()> {
        let recipe_ref = unsafe {
            self.wasm
                .get(operand_index)
                .ok_or(())?
                .operand
                .call_recipe_ref
        };
        if let Some(recipe_slot) = recipe_ref.resolved_recipe_slot() {
            if let Some(recipe) = self.gc.call_recipe(recipe_slot) {
                return Ok(recipe);
            }
        }
        let current_func = self.gc.get_func(self.funcaddr);
        let callee = self
            .gc
            .instance(current_func.instance)
            .funcs
            .as_slice()
            .get(recipe_ref.funcidx as usize)
            .copied()
            .ok_or(())?;
        Ok(self.gc.build_call_recipe(callee))
    }

    fn indirect_call_layout_for_operand(&self, operand_index: usize) -> Result<(u32, u32), ()> {
        let expected_typeidx =
            unsafe { self.wasm.get(operand_index + 1).ok_or(())?.operand.u32 as usize };
        let current_func = self.gc.get_func(self.funcaddr);
        let instance = self.gc.instance(current_func.instance);
        let module = self.gc.get_module(instance.module_addr);
        let functype = module.function_types.get(expected_typeidx).ok_or(())?;
        let param_size = functype.0.iter().map(|ty| ty.stack_size().u32()).sum();
        let return_size = functype.1.iter().map(|ty| ty.stack_size().u32()).sum();
        Ok((param_size, return_size))
    }

    fn call_return_helper(&mut self, return_size: u32) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, return_size);
        self.call_ptr(jit_function_return as *const () as usize);
        self.branch_to_epilogue();
    }

    fn call_block_return_helper(&mut self, stack_top: u32, return_size: u32) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, stack_top);
        self.mov_imm_u32(2, return_size);
        self.call_ptr(jit_block_return as *const () as usize);
    }

    fn call_direct_helper(
        &mut self,
        operand_index: usize,
        _continuation_index: usize,
        is_return_call: bool,
        expected_layout: u64,
        reload_slots: usize,
    ) -> Result<bool, ()> {
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, operand_index);
        self.mov_imm_u64(2, u64::from(is_return_call));
        self.call_ptr(jit_direct_call as *const () as usize);
        if is_return_call {
            self.branch_to_epilogue();
            Ok(false)
        } else {
            self.finish_call_helper_result(expected_layout, reload_slots)
        }
    }

    fn call_wasm_direct_fast_helper(
        &mut self,
        funcaddr: ObjectRef,
        continuation_index: usize,
        expected_layout: u64,
        reload_slots: usize,
    ) -> Result<bool, ()> {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, funcaddr.get());
        self.load_code_ptr_operand(2, continuation_index);
        self.call_ptr(jit_wasm_direct_call_fast as *const () as usize);
        self.finish_call_helper_result(expected_layout, reload_slots)
    }

    fn call_indirect_helper(
        &mut self,
        operand_index: usize,
        _continuation_index: usize,
        is_return_call: bool,
        expected_layout: u64,
        reload_slots: usize,
    ) -> Result<bool, ()> {
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, operand_index);
        self.mov_imm_u64(2, u64::from(is_return_call));
        self.call_ptr(jit_indirect_call as *const () as usize);
        if is_return_call {
            self.branch_to_epilogue();
            Ok(false)
        } else {
            self.finish_call_helper_result(expected_layout, reload_slots)
        }
    }

    fn finish_call_helper_result(
        &mut self,
        expected_layout: u64,
        reload_slots: usize,
    ) -> Result<bool, ()> {
        self.cmp_w_imm(0, JitNativeExit::DONE as u32)?;
        let done_branch = self.branch_placeholder(FixupKind::BCond(Cond::Eq));
        self.return_if_exit();
        let done_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            done_branch,
            done_target,
            FixupKind::BCond(Cond::Eq),
        )?;
        self.mov_imm_u64(16, expected_layout);
        self.cmp_x(1, 16);
        let expected_layout_branch = self.branch_placeholder(FixupKind::BCond(Cond::Eq));
        self.return_trap(VMResult::<()>::InvalidOperand);
        let expected_layout_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            expected_layout_branch,
            expected_layout_target,
            FixupKind::BCond(Cond::Eq),
        )?;
        self.reload_stack_slots(reload_slots)?;
        Ok(true)
    }

    fn call_i32_store_local_base_from_vm_stack(
        &mut self,
        pc_index: usize,
        width: u32,
    ) -> Result<(), ()> {
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, pc_index);
        self.mov_imm_u32(2, width);
        self.call_ptr(jit_i32_store_local_base_from_vm_stack as *const () as usize);
        self.return_if_exit();
        Ok(())
    }

    fn call_runtime_stack_op(
        &mut self,
        pc_index: usize,
        kind: u32,
        push_slots: usize,
    ) -> Result<(), ()> {
        self.flush_stack()?;
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, pc_index);
        self.mov_imm_u32(2, kind);
        self.call_ptr(jit_runtime_stack_op as *const () as usize);
        self.return_if_exit();
        self.reload_stack_slots(push_slots)
    }

    fn call_runtime_continuation_op(&mut self, pc_index: usize, kind: u32) -> Result<(), ()> {
        self.flush_stack()?;
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, pc_index);
        self.load_code_ptr_operand(2, self.next_pc_index(pc_index)?);
        self.mov_imm_u32(3, kind);
        self.call_ptr(jit_runtime_continuation_op as *const () as usize);
        self.return_if_exit();
        Ok(())
    }

    fn call_runtime_continuation_op_reloading_stack(
        &mut self,
        pc_index: usize,
        kind: u32,
        reload_slots: usize,
    ) -> Result<(), ()> {
        self.flush_stack()?;
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, pc_index);
        self.load_code_ptr_operand(2, self.next_pc_index(pc_index)?);
        self.mov_imm_u32(3, kind);
        self.call_ptr(jit_runtime_continuation_op as *const () as usize);
        self.return_if_exit();
        self.reload_stack_slots(reload_slots)
    }

    fn next_pc_index(&self, pc_index: usize) -> Result<usize, ()> {
        let mut cursor = 0usize;
        for len in self.op_lens.iter().copied() {
            if cursor == pc_index {
                return cursor.checked_add(usize::from(len)).ok_or(());
            }
            cursor = cursor.checked_add(usize::from(len)).ok_or(())?;
        }
        Err(())
    }

    fn emit_search_loop(&mut self, plan: SearchLoopPlan) -> Result<(), ()> {
        if plan.field_width != 1 && plan.field_width != 2 {
            return Err(());
        }
        self.load_local4_to_reg(13, plan.rhs_local)?;
        self.mov_imm_u32(17, plan.rhs_mask);
        self.and_w(13, 13, 17);
        self.load_local4_to_reg(12, plan.node_local)?;

        let loop_start = self.offset();

        self.mov_w(14, 12);
        self.add_imm_u32(14, 14, plan.data_delta)?;
        self.inline_i32_load(14, plan.data_memarg.offset, 4, false)?;
        self.store_local4_from_reg(plan.data_local, 14)?;

        self.mov_w(15, 14);
        self.add_imm_u32(15, 15, plan.field_delta)?;
        self.inline_i32_load(15, plan.field_memarg.offset, plan.field_width, false)?;
        self.cmp_w(15, 13);
        self.branch_to(
            plan.match_target,
            FixupKind::BCond(match plan.compare {
                SearchCompare::Eq => Cond::Eq,
                SearchCompare::Ne => Cond::Ne,
            }),
        );

        self.mov_w(16, 12);
        self.add_imm_u32(16, 16, plan.next_delta)?;
        self.inline_i32_load(16, plan.next_memarg.offset, 4, false)?;
        self.store_local4_from_reg(plan.node_local, 16)?;
        self.cmp_w_imm(16, 0)?;
        let fallthrough_branch = if let Some(miss_target) = plan.miss_target {
            self.branch_to(miss_target, FixupKind::BCond(Cond::Eq));
            None
        } else {
            let at = self.branch_placeholder(FixupKind::BCond(Cond::Eq));
            Some(at)
        };
        self.mov_w(12, 16);
        self.branch_to_offset(loop_start, FixupKind::B)?;
        if let Some(at) = fallthrough_branch {
            let target = self.offset();
            patch_branch(
                self.masm.as_mut_bytes(),
                at,
                target,
                FixupKind::BCond(Cond::Eq),
            )?;
        }
        Ok(())
    }

    fn emit_reverse_loop(
        &mut self,
        prev_local: u32,
        saved_local: u32,
        cursor_local: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    ) -> Result<(), ()> {
        self.load_local4_to_reg(12, prev_local)?;
        self.load_local4_to_reg(13, cursor_local)?;
        let loop_start = self.offset();

        self.store_local4_from_reg(saved_local, 12)?;
        self.store_local4_from_reg(prev_local, 13)?;
        self.mov_w(14, 13);
        self.inline_i32_load(14, load_memarg.offset, 4, false)?;
        self.store_local4_from_reg(cursor_local, 14)?;
        self.inline_i32_store(13, store_memarg.offset, 4, 12)?;
        self.cmp_w_imm(14, 0)?;
        let done_branch = self.branch_placeholder(FixupKind::BCond(Cond::Eq));
        self.mov_w(12, 13);
        self.mov_w(13, 14);
        self.branch_to_offset(loop_start, FixupKind::B)?;
        let done_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            done_branch,
            done_target,
            FixupKind::BCond(Cond::Eq),
        )?;
        Ok(())
    }

    fn emit_relink_loop(
        &mut self,
        cursor_local: u32,
        current_local: u32,
        prev_local: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    ) -> Result<(), ()> {
        self.load_local4_to_reg(12, cursor_local)?;
        let loop_start = self.offset();

        self.store_local4_from_reg(current_local, 12)?;
        self.mov_w(13, 12);
        self.inline_i32_load(13, load_memarg.offset, 4, false)?;
        self.store_local4_from_reg(cursor_local, 13)?;
        self.load_local4_to_reg(14, prev_local)?;
        self.inline_i32_store(12, store_memarg.offset, 4, 14)?;
        self.store_local4_from_reg(prev_local, 12)?;
        self.cmp_w_imm(13, 0)?;
        let done_branch = self.branch_placeholder(FixupKind::BCond(Cond::Eq));
        self.mov_w(12, 13);
        self.branch_to_offset(loop_start, FixupKind::B)?;
        let done_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            done_branch,
            done_target,
            FixupKind::BCond(Cond::Eq),
        )?;
        Ok(())
    }

    fn emit_update_store16_loop(&mut self, plan: UpdateStore16LoopPlan) -> Result<(), ()> {
        self.load_local4_to_reg(12, plan.ptr_local)?;
        self.load_local4_to_reg(13, plan.scalar_local)?;
        self.load_local4_to_reg(14, plan.counter_local)?;
        let loop_start = self.offset();

        self.mov_w(15, 12);
        self.add_imm_u32(15, 15, plan.load_delta)?;
        self.inline_i32_load(15, plan.load_memarg.offset, 2, false)?;
        if plan.subtract {
            self.sub_w(15, 15, 13);
        } else {
            self.add_w(15, 15, 13);
        }
        self.mov_w(16, 12);
        self.add_imm_u32(16, 16, plan.store_delta)?;
        self.inline_i32_store(16, plan.store_memarg.offset, 2, 15)?;

        self.add_imm_u32(12, 12, 2)?;
        self.store_local4_from_reg(plan.ptr_local, 12)?;
        self.mov_imm_u32(17, 1);
        self.sub_w(14, 14, 17);
        self.store_local4_from_reg(plan.counter_local, 14)?;
        self.branch_to_offset(loop_start, FixupKind::CbnzW(14))?;
        Ok(())
    }

    fn push_i64_load_from_addr(
        &mut self,
        addr: u8,
        offset: u32,
        width: u32,
        signed: bool,
    ) -> Result<(), ()> {
        self.mov_w(16, addr);
        let low = self.push_reg()?;
        self.mov_w(low, 16);
        let low_signed = signed && width <= 2;
        self.inline_i32_load(low, offset, width.min(4), low_signed)?;
        let high = self.push_reg()?;
        if width == 8 {
            self.mov_w(high, 16);
            self.add_imm_u32(high, high, 4)?;
            self.inline_i32_load(high, offset, 4, false)?;
        } else if signed {
            self.asr_w_imm(high, low, 31);
        } else {
            self.mov_imm_u32(high, 0);
        }
        Ok(())
    }

    fn inline_i32_load(
        &mut self,
        addr: u8,
        offset: u32,
        width: u32,
        signed: bool,
    ) -> Result<(), ()> {
        profile::count(Counter::EmitInlineI32Load);
        self.checked_memory_start(9, 10, addr, offset, width)?;
        self.load_default_memory_data_ptr(11)?;
        self.add_x(11, 11, 9);
        match (width, signed) {
            (1, false) => self.ldrb_w(addr, 11),
            (1, true) => self.ldrsb_w(addr, 11),
            (2, false) => self.ldrh_w(addr, 11),
            (2, true) => self.ldrsh_w(addr, 11),
            (4, false) => self.ldr_w(addr, 11),
            _ => return Err(()),
        }
        Ok(())
    }

    fn inline_i32_store(&mut self, addr: u8, offset: u32, width: u32, value: u8) -> Result<(), ()> {
        profile::count(Counter::EmitInlineI32Store);
        self.checked_memory_start(9, 10, addr, offset, width)?;
        self.load_default_memory_data_ptr(11)?;
        self.add_x(11, 11, 9);
        match width {
            1 => self.strb_w(value, 11),
            2 => self.strh_w(value, 11),
            4 => self.str_w(value, 11),
            _ => return Err(()),
        }
        Ok(())
    }

    fn emit_i32_inc_local_base(
        &mut self,
        base_local: u32,
        store_delta: u32,
        load_delta: u32,
        load_memarg: MemArg,
        store_memarg: MemArg,
    ) -> Result<(), ()> {
        self.load_local_base_addr_to_reg(16, base_local, load_delta)?;
        self.inline_i32_load(16, load_memarg.offset, 4, false)?;
        self.add_imm_u32(16, 16, 1)?;
        self.load_local_base_addr_to_reg(17, base_local, store_delta)?;
        self.inline_i32_store(17, store_memarg.offset, 4, 16)
    }

    fn checked_memory_start(
        &mut self,
        start_x: u8,
        end_x: u8,
        addr_w: u8,
        offset: u32,
        width: u32,
    ) -> Result<(), ()> {
        self.mov_w(start_x, addr_w);
        self.add_imm_u64(start_x, start_x, u64::from(offset))?;
        self.mov_x(end_x, start_x);
        self.add_imm_u64(end_x, end_x, u64::from(width))?;
        self.load_default_memory_data_size(11)?;
        self.cmp_x(end_x, 11);
        let ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ls));
        self.return_trap(VMResult::<()>::MemoryIndexOutOfRange);
        let target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            ok_branch,
            target,
            FixupKind::BCond(Cond::Ls),
        )?;
        Ok(())
    }

    fn load_default_memory_ptr(&mut self, rd: u8) -> Result<(), ()> {
        Ok(self.ldr_x_imm(
            rd,
            19,
            std::mem::offset_of!(ExecuteContext<'_>, default_local_memory_ptr),
        )?)
    }

    fn load_default_memory_data_size(&mut self, rd: u8) -> Result<(), ()> {
        let layout = MemoryJitLayout::get();
        self.load_default_memory_ptr(rd)?;
        self.ldr_w_imm(rd, rd, layout.current_pages)?;
        self.lsl_x_imm(rd, rd, PAGE_SIZE.trailing_zeros())?;
        Ok(())
    }

    fn load_default_memory_data_ptr(&mut self, rd: u8) -> Result<(), ()> {
        let layout = MemoryJitLayout::get();
        self.load_default_memory_ptr(rd)?;
        Ok(self.ldr_x_imm(rd, rd, layout.region_ptr)?)
    }

    fn load_code_ptr_operand(&mut self, rd: u8, operand_index: usize) {
        let byte_offset = operand_index
            .checked_mul(std::mem::size_of::<Instr>())
            .expect("jit operand offset overflow");
        self.add_imm_u64(rd, 20, byte_offset as u64)
            .expect("code pointer operand offset is encodable");
    }

    fn return_if_exit(&mut self) {
        let at = self.branch_placeholder(FixupKind::CbnzX(0));
        self.fixups.push(Fixup {
            at,
            target_index: usize::MAX,
            kind: FixupKind::CbnzX(0),
        });
    }

    fn branch_to_epilogue(&mut self) {
        let at = self.branch_placeholder(FixupKind::B);
        self.fixups.push(Fixup {
            at,
            target_index: usize::MAX,
            kind: FixupKind::B,
        });
    }

    fn branch_to(&mut self, target_index: usize, kind: FixupKind) {
        let at = self.branch_placeholder(kind);
        let target_index = self.skip_end_target_index(target_index);
        self.fixups.push(Fixup {
            at,
            target_index,
            kind,
        });
    }

    fn skip_end_target_index(&self, mut target_index: usize) -> usize {
        while target_index < self.wasm.len()
            && std::ptr::fn_addr_eq(
                unsafe { self.wasm[target_index].op },
                crate::runtime::vm::op_end as crate::common::Op,
            )
        {
            target_index += 1;
        }
        target_index
    }

    fn branch_to_offset(&mut self, target_offset: usize, kind: FixupKind) -> Result<(), ()> {
        let at = self.branch_placeholder(kind);
        patch_branch(self.masm.as_mut_bytes(), at, target_offset, kind)
    }

    fn branch_placeholder(&mut self, kind: FixupKind) -> usize {
        self.masm
            .branch_placeholder(branch_kind(kind))
            .expect("valid branch kind")
    }

    fn branch_table(&mut self, index_reg: u8, targets: &[usize]) -> Result<(), ()> {
        let Some((&default_target, case_targets)) = targets.split_last() else {
            return Err(());
        };
        for (case, target) in case_targets.iter().copied().enumerate() {
            self.cmp_w_u32(index_reg, case as u32);
            self.branch_to(target, FixupKind::BCond(Cond::Eq));
        }
        self.branch_to(default_target, FixupKind::B);
        Ok(())
    }

    fn return_trap<T>(&mut self, result: VMResult<T>) {
        self.mov_imm_u64(0, JitNativeExit::TRAP);
        self.mov_imm_u64(1, vm_result_code(result));
        self.branch_to_epilogue();
    }

    fn call_ptr(&mut self, ptr: usize) {
        profile::count(Counter::EmitHelperCall);
        self.mov_imm_u64(16, ptr as u64);
        self.blr_x(16);
    }

    fn load_local4_to_reg(&mut self, rd: u8, local: u32) -> Result<(), ()> {
        self.load_store_local4(rd, local, true)
    }

    fn store_local4_from_reg(&mut self, local: u32, rs: u8) -> Result<(), ()> {
        self.load_store_local4(rs, local, false)
    }

    fn load_store_local4(&mut self, rt: u8, local: u32, load: bool) -> Result<(), ()> {
        if local <= 255 {
            if load {
                self.ldr_w_unscaled_imm(rt, 21, local)?;
            } else {
                self.str_w_unscaled_imm(rt, 21, local)?;
            }
            return Ok(());
        }
        self.addr_local(17, local)?;
        if load {
            self.ldr_w(rt, 17);
        } else {
            self.str_w(rt, 17);
        }
        Ok(())
    }

    fn addr_local(&mut self, rd: u8, local: u32) -> Result<(), ()> {
        Ok(self.add_imm_u64(rd, 21, u64::from(local))?)
    }

    fn load_local_base_addr_to_reg(&mut self, rd: u8, local: u32, delta: u32) -> Result<(), ()> {
        self.load_local4_to_reg(rd, local)?;
        Ok(self.add_imm_u32(rd, rd, delta)?)
    }

    fn load_local_scaled_index_addr_to_reg(
        &mut self,
        rd: u8,
        base_local: u32,
        index_local: u32,
        scale_log2: u32,
        delta: u32,
    ) -> Result<(), ()> {
        if scale_log2 > 31 {
            return Err(());
        }
        self.load_local4_to_reg(rd, base_local)?;
        self.load_local4_to_reg(17, index_local)?;
        if scale_log2 != 0 {
            self.lsl_w_imm(17, 17, scale_log2);
        }
        self.add_w(rd, rd, 17);
        Ok(self.add_imm_u32(rd, rd, delta)?)
    }

    fn emit_i32_const_binop(&mut self, kind: u32, lhs: u8, rhs: u32) -> Result<(), ()> {
        let Some((op, LocalFastRhsShape::Const)) = decode_local_binop32_kind(kind) else {
            return Err(());
        };
        match op {
            LocalBinop32Op::I32Add => self.add_imm_u32(lhs, lhs, rhs)?,
            LocalBinop32Op::I32Sub => {
                if rhs <= 4095 {
                    self.sub_imm_u32(lhs, lhs, rhs)?;
                } else {
                    self.mov_imm_u32(17, rhs);
                    self.sub_w(lhs, lhs, 17);
                }
            }
            LocalBinop32Op::I32Mul => {
                self.mov_imm_u32(17, rhs);
                self.mul_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32And => {
                self.mov_imm_u32(17, rhs);
                self.and_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32Or => {
                self.mov_imm_u32(17, rhs);
                self.orr_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32Xor => {
                self.mov_imm_u32(17, rhs);
                self.eor_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32Shl => self.lsl_w_imm(lhs, lhs, rhs & 31),
            LocalBinop32Op::I32ShrU => self.lsr_w_imm(lhs, lhs, rhs & 31),
            LocalBinop32Op::I32ShrS => self.asr_w_imm(lhs, lhs, rhs & 31),
            LocalBinop32Op::I32Rotl => {
                self.mov_imm_u32(17, (32u32.wrapping_sub(rhs)) & 31);
                self.rorv_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32Rotr => {
                self.mov_imm_u32(17, rhs & 31);
                self.rorv_w(lhs, lhs, 17);
            }
            LocalBinop32Op::F32Add => {
                self.mov_imm_u32(17, rhs);
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, 17);
                self.fadd_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Sub => {
                self.mov_imm_u32(17, rhs);
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, 17);
                self.fsub_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Mul => {
                self.mov_imm_u32(17, rhs);
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, 17);
                self.fmul_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Div => {
                self.mov_imm_u32(17, rhs);
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, 17);
                self.fdiv_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
        }
        Ok(())
    }

    fn emit_local_get4x3_add_const_binop_add(
        &mut self,
        first: u32,
        second: u32,
        third: u32,
        kind: u32,
        rhs: u32,
        result: u8,
    ) -> Result<(), ()> {
        self.load_local4_to_reg(result, second)?;
        self.load_local4_to_reg(17, third)?;
        self.add_w(result, result, 17);
        self.emit_i32_const_binop(kind, result, rhs)?;
        self.load_local4_to_reg(17, first)?;
        self.add_w(result, result, 17);
        Ok(())
    }

    fn emit_i32_const_cmp(&mut self, kind: u32, lhs: u8, rhs: u32) -> Result<(), ()> {
        let Some((op, LocalFastRhsShape::Const)) = decode_local_cmp32_kind(kind) else {
            return Err(());
        };
        self.cmp_w_u32(lhs, rhs);
        self.cset_w(lhs, i32_cmp_cond(op)?);
        Ok(())
    }

    fn emit_local_binop32(&mut self, kind: u32, lhs: u32, rhs: u32, result: u8) -> Result<(), ()> {
        let Some((op, rhs_shape)) = decode_local_binop32_kind(kind) else {
            return Err(());
        };
        self.load_local4_to_reg(result, lhs)?;
        match rhs_shape {
            LocalFastRhsShape::Local => {
                self.load_local4_to_reg(17, rhs)?;
                self.emit_i32_reg_binop(op, result, 17)
            }
            LocalFastRhsShape::Const => self.emit_i32_const_binop(kind, result, rhs),
        }
    }

    fn emit_i32_reg_binop(&mut self, op: LocalBinop32Op, lhs: u8, rhs: u8) -> Result<(), ()> {
        match op {
            LocalBinop32Op::I32Add => self.add_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Sub => self.sub_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Mul => self.mul_w(lhs, lhs, rhs),
            LocalBinop32Op::I32And => self.and_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Or => self.orr_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Xor => self.eor_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Shl => self.lslv_w(lhs, lhs, rhs),
            LocalBinop32Op::I32ShrS => self.asrv_w(lhs, lhs, rhs),
            LocalBinop32Op::I32ShrU => self.lsrv_w(lhs, lhs, rhs),
            LocalBinop32Op::I32Rotl => {
                self.neg_w(17, rhs);
                self.rorv_w(lhs, lhs, 17);
            }
            LocalBinop32Op::I32Rotr => self.rorv_w(lhs, lhs, rhs),
            LocalBinop32Op::F32Add => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fadd_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Sub => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fsub_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Mul => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fmul_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
            LocalBinop32Op::F32Div => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fdiv_s(0, 0, 1);
                self.fmov_w_from_s(lhs, 0);
            }
        }
        Ok(())
    }

    fn trap_if_i32_divisor_zero(&mut self, rhs: u8) -> Result<(), ()> {
        self.cmp_w_imm(rhs, 0)?;
        let nonzero_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.return_trap(VMResult::<()>::InvalidOperand);
        let nonzero_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            nonzero_branch,
            nonzero_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        Ok(())
    }

    fn trap_if_i32_div_s_overflow(&mut self, lhs: u8, rhs: u8) -> Result<(), ()> {
        self.mov_imm_u32(17, 0x8000_0000);
        self.cmp_w(lhs, 17);
        let lhs_ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.cmp_w_u32(rhs, u32::MAX);
        let rhs_ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.return_trap(VMResult::<()>::InvalidOperand);
        let ok_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            lhs_ok_branch,
            ok_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        patch_branch(
            self.masm.as_mut_bytes(),
            rhs_ok_branch,
            ok_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        Ok(())
    }

    fn trap_if_i64_divisor_zero(&mut self, rhs: u8) -> Result<(), ()> {
        self.mov_imm_u64(9, 0);
        self.cmp_x(rhs, 9);
        let nonzero_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.return_trap(VMResult::<()>::InvalidOperand);
        let nonzero_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            nonzero_branch,
            nonzero_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        Ok(())
    }

    fn trap_if_i64_div_s_overflow(&mut self, lhs: u8, rhs: u8) -> Result<(), ()> {
        self.mov_imm_u64(9, 0x8000_0000_0000_0000);
        self.cmp_x(lhs, 9);
        let lhs_ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.mov_imm_u64(9, u64::MAX);
        self.cmp_x(rhs, 9);
        let rhs_ok_branch = self.branch_placeholder(FixupKind::BCond(Cond::Ne));
        self.return_trap(VMResult::<()>::InvalidOperand);
        let ok_target = self.offset();
        patch_branch(
            self.masm.as_mut_bytes(),
            lhs_ok_branch,
            ok_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        patch_branch(
            self.masm.as_mut_bytes(),
            rhs_ok_branch,
            ok_target,
            FixupKind::BCond(Cond::Ne),
        )?;
        Ok(())
    }

    fn emit_local_cmp32(&mut self, kind: u32, lhs: u32, rhs: u32, result: u8) -> Result<(), ()> {
        let Some((op, rhs_shape)) = decode_local_cmp32_kind(kind) else {
            return Err(());
        };
        self.load_local4_to_reg(result, lhs)?;
        match op {
            LocalCmp32Op::I32Eq
            | LocalCmp32Op::I32Ne
            | LocalCmp32Op::I32LtS
            | LocalCmp32Op::I32LtU
            | LocalCmp32Op::I32GtS
            | LocalCmp32Op::I32GtU
            | LocalCmp32Op::I32LeS
            | LocalCmp32Op::I32LeU
            | LocalCmp32Op::I32GeS
            | LocalCmp32Op::I32GeU => {
                match rhs_shape {
                    LocalFastRhsShape::Local => {
                        self.load_local4_to_reg(17, rhs)?;
                        self.cmp_w(result, 17);
                    }
                    LocalFastRhsShape::Const => self.cmp_w_u32(result, rhs),
                }
                self.cset_w(result, i32_cmp_cond(op)?);
            }
            LocalCmp32Op::F32Eq
            | LocalCmp32Op::F32Ne
            | LocalCmp32Op::F32Lt
            | LocalCmp32Op::F32Gt
            | LocalCmp32Op::F32Le
            | LocalCmp32Op::F32Ge => {
                match rhs_shape {
                    LocalFastRhsShape::Local => self.load_local4_to_reg(17, rhs)?,
                    LocalFastRhsShape::Const => self.mov_imm_u32(17, rhs),
                }
                self.fmov_s_from_w(0, result);
                self.fmov_s_from_w(1, 17);
                self.fcmp_s(0, 1);
                self.cset_w(result, f32_cmp_cond(op)?);
            }
        }
        Ok(())
    }

    fn emit_local_cmp64(&mut self, kind: u32, lhs: u32, rhs: u64, result: u8) -> Result<(), ()> {
        let Some((op, rhs_shape)) = decode_local_cmp64_kind(kind) else {
            return Err(());
        };
        self.load_local_i64_to_x(16, lhs, 9)?;
        self.load_local_binop64_rhs_to_x(17, rhs_shape, rhs, 9)?;
        match op {
            LocalCmp64Op::I64Eq
            | LocalCmp64Op::I64Ne
            | LocalCmp64Op::I64LtS
            | LocalCmp64Op::I64LtU
            | LocalCmp64Op::I64GtS
            | LocalCmp64Op::I64GtU
            | LocalCmp64Op::I64LeS
            | LocalCmp64Op::I64LeU
            | LocalCmp64Op::I64GeS
            | LocalCmp64Op::I64GeU => {
                self.cmp_x(16, 17);
                self.cset_w(result, i64_cmp_cond(op)?);
            }
            LocalCmp64Op::F64Eq
            | LocalCmp64Op::F64Ne
            | LocalCmp64Op::F64Lt
            | LocalCmp64Op::F64Gt
            | LocalCmp64Op::F64Le
            | LocalCmp64Op::F64Ge => {
                self.fmov_d_from_x(0, 16);
                self.fmov_d_from_x(1, 17);
                self.fcmp_d(0, 1);
                self.cset_w(result, f64_cmp_cond(op)?);
            }
        }
        Ok(())
    }

    fn emit_local_unary32(&mut self, kind: u32, src: u32, result: u8) -> Result<(), ()> {
        let op = decode_local_unary32_kind(kind).ok_or(())?;
        self.load_local4_to_reg(result, src)?;
        match op {
            LocalUnary32Op::I32Clz => self.clz_w(result, result),
            LocalUnary32Op::I32Ctz => {
                self.rbit_w(result, result);
                self.clz_w(result, result);
            }
            LocalUnary32Op::I32Popcnt => {
                self.mov_w(0, result);
                self.call_ptr(jit_i32_popcnt_value as *const () as usize);
                self.mov_w(result, 0);
            }
            LocalUnary32Op::F32Abs => {
                self.mov_imm_u32(17, 0x7fff_ffff);
                self.and_w(result, result, 17);
            }
            LocalUnary32Op::F32Neg => {
                self.mov_imm_u32(17, 0x8000_0000);
                self.eor_w(result, result, 17);
            }
            LocalUnary32Op::F32Sqrt => {
                self.fmov_s_from_w(0, result);
                self.fsqrt_s(0, 0);
                self.fmov_w_from_s(result, 0);
            }
            LocalUnary32Op::F32Ceil => {
                self.fmov_s_from_w(0, result);
                self.frintp_s(0, 0);
                self.fmov_w_from_s(result, 0);
            }
            LocalUnary32Op::F32Floor => {
                self.fmov_s_from_w(0, result);
                self.frintm_s(0, 0);
                self.fmov_w_from_s(result, 0);
            }
            LocalUnary32Op::F32Trunc => {
                self.fmov_s_from_w(0, result);
                self.frintz_s(0, 0);
                self.fmov_w_from_s(result, 0);
            }
            LocalUnary32Op::F32Nearest => {
                self.fmov_s_from_w(0, result);
                self.frintn_s(0, 0);
                self.fmov_w_from_s(result, 0);
            }
        }
        Ok(())
    }

    fn emit_local_unary64(&mut self, kind: u32, src: u32, result: u8) -> Result<(), ()> {
        let op = decode_local_unary64_kind(kind).ok_or(())?;
        self.load_local_i64_to_x(result, src, 9)?;
        match op {
            LocalUnary64Op::I64Clz => self.clz_x(result, result),
            LocalUnary64Op::I64Ctz => {
                self.rbit_x(result, result);
                self.clz_x(result, result);
            }
            LocalUnary64Op::I64Popcnt => {
                self.mov_x(0, result);
                self.call_ptr(jit_i64_popcnt_value as *const () as usize);
                self.mov_x(result, 0);
            }
            LocalUnary64Op::F64Abs => {
                self.mov_imm_u64(17, 0x7fff_ffff_ffff_ffff);
                self.and_x(result, result, 17);
            }
            LocalUnary64Op::F64Neg => {
                self.mov_imm_u64(17, 0x8000_0000_0000_0000);
                self.eor_x(result, result, 17);
            }
            LocalUnary64Op::F64Sqrt => {
                self.fmov_d_from_x(0, result);
                self.fsqrt_d(0, 0);
                self.fmov_x_from_d(result, 0);
            }
            LocalUnary64Op::F64Ceil => {
                self.fmov_d_from_x(0, result);
                self.frintp_d(0, 0);
                self.fmov_x_from_d(result, 0);
            }
            LocalUnary64Op::F64Floor => {
                self.fmov_d_from_x(0, result);
                self.frintm_d(0, 0);
                self.fmov_x_from_d(result, 0);
            }
            LocalUnary64Op::F64Trunc => {
                self.fmov_d_from_x(0, result);
                self.frintz_d(0, 0);
                self.fmov_x_from_d(result, 0);
            }
            LocalUnary64Op::F64Nearest => {
                self.fmov_d_from_x(0, result);
                self.frintn_d(0, 0);
                self.fmov_x_from_d(result, 0);
            }
        }
        Ok(())
    }

    fn emit_f32_compare(&mut self, op: FloatCompareOp) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let rhs = self.pop_reg()?;
        let lhs = self.pop_reg()?;
        self.fmov_s_from_w(0, lhs);
        self.fmov_s_from_w(1, rhs);
        self.fcmp_s(0, 1);
        let result = self.push_reg()?;
        self.cset_w(result, float_cmp_cond(op));
        Ok(())
    }

    fn emit_f64_compare(&mut self, op: FloatCompareOp) -> Result<(), ()> {
        self.ensure_stack_slots(4)?;
        let rhs_high = self.pop_reg()?;
        let rhs_low = self.pop_reg()?;
        let lhs_high = self.pop_reg()?;
        let lhs_low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, lhs_low, lhs_high, 9)?;
        self.pack_i64_slots_to_x(17, rhs_low, rhs_high, 9)?;
        self.fmov_d_from_x(0, 16);
        self.fmov_d_from_x(1, 17);
        self.fcmp_d(0, 1);
        let result = self.push_reg()?;
        self.cset_w(result, float_cmp_cond(op));
        Ok(())
    }

    fn emit_f64_binary(&mut self, op: FloatBinaryOp) -> Result<(), ()> {
        self.ensure_stack_slots(4)?;
        let rhs_high = self.pop_reg()?;
        let rhs_low = self.pop_reg()?;
        let lhs_high = self.pop_reg()?;
        let lhs_low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, lhs_low, lhs_high, 9)?;
        self.pack_i64_slots_to_x(17, rhs_low, rhs_high, 9)?;
        match op {
            FloatBinaryOp::Add => {
                self.fmov_d_from_x(0, 16);
                self.fmov_d_from_x(1, 17);
                self.fadd_d(0, 0, 1);
            }
            FloatBinaryOp::Sub => {
                self.fmov_d_from_x(0, 16);
                self.fmov_d_from_x(1, 17);
                self.fsub_d(0, 0, 1);
            }
            FloatBinaryOp::Mul => {
                self.fmov_d_from_x(0, 16);
                self.fmov_d_from_x(1, 17);
                self.fmul_d(0, 0, 1);
            }
            FloatBinaryOp::Div => {
                self.fmov_d_from_x(0, 16);
                self.fmov_d_from_x(1, 17);
                self.fdiv_d(0, 0, 1);
            }
            FloatBinaryOp::Min => {
                self.mov_x(0, 16);
                self.mov_x(1, 17);
                self.call_ptr(jit_f64_min_bits as *const () as usize);
                self.push_x_as_i64_slots(0)?;
                return Ok(());
            }
            FloatBinaryOp::Max => {
                self.mov_x(0, 16);
                self.mov_x(1, 17);
                self.call_ptr(jit_f64_max_bits as *const () as usize);
                self.push_x_as_i64_slots(0)?;
                return Ok(());
            }
            FloatBinaryOp::Copysign => {
                self.mov_imm_u64(9, 0x7fff_ffff_ffff_ffff);
                self.and_x(16, 16, 9);
                self.mov_imm_u64(9, 0x8000_0000_0000_0000);
                self.and_x(17, 17, 9);
                self.orr_x(16, 16, 17);
                self.push_x_as_i64_slots(16)?;
                return Ok(());
            }
        }
        self.fmov_x_from_d(16, 0);
        self.push_x_as_i64_slots(16)?;
        Ok(())
    }

    fn emit_f32_binary(&mut self, op: FloatBinaryOp) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let rhs = self.pop_reg()?;
        let lhs = self.pop_reg()?;
        match op {
            FloatBinaryOp::Add => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fadd_s(0, 0, 1);
            }
            FloatBinaryOp::Sub => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fsub_s(0, 0, 1);
            }
            FloatBinaryOp::Mul => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fmul_s(0, 0, 1);
            }
            FloatBinaryOp::Div => {
                self.fmov_s_from_w(0, lhs);
                self.fmov_s_from_w(1, rhs);
                self.fdiv_s(0, 0, 1);
            }
            FloatBinaryOp::Min => {
                self.mov_w(0, lhs);
                self.mov_w(1, rhs);
                self.call_ptr(jit_f32_min_bits as *const () as usize);
                let result = self.push_reg()?;
                self.mov_w(result, 0);
                return Ok(());
            }
            FloatBinaryOp::Max => {
                self.mov_w(0, lhs);
                self.mov_w(1, rhs);
                self.call_ptr(jit_f32_max_bits as *const () as usize);
                let result = self.push_reg()?;
                self.mov_w(result, 0);
                return Ok(());
            }
            FloatBinaryOp::Copysign => {
                let result = self.push_reg()?;
                self.mov_imm_u32(16, 0x7fff_ffff);
                self.and_w(result, lhs, 16);
                self.mov_imm_u32(16, 0x8000_0000);
                self.and_w(rhs, rhs, 16);
                self.orr_w(result, result, rhs);
                return Ok(());
            }
        }
        let result = self.push_reg()?;
        self.fmov_w_from_s(result, 0);
        Ok(())
    }

    fn emit_f32_unary(&mut self, op: FloatUnaryOp) -> Result<(), ()> {
        self.ensure_stack_slots(1)?;
        let value = self.peek_reg()?;
        self.fmov_s_from_w(0, value);
        match op {
            FloatUnaryOp::Abs => self.fabs_s(0, 0),
            FloatUnaryOp::Neg => self.fneg_s(0, 0),
            FloatUnaryOp::Sqrt => self.fsqrt_s(0, 0),
            FloatUnaryOp::Ceil => self.frintp_s(0, 0),
            FloatUnaryOp::Floor => self.frintm_s(0, 0),
            FloatUnaryOp::Trunc => self.frintz_s(0, 0),
            FloatUnaryOp::Nearest => self.frintn_s(0, 0),
        }
        self.fmov_w_from_s(value, 0);
        Ok(())
    }

    fn emit_f64_unary(&mut self, op: FloatUnaryOp) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let high = self.pop_reg()?;
        let low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, low, high, 9)?;
        self.fmov_d_from_x(0, 16);
        match op {
            FloatUnaryOp::Abs => self.fabs_d(0, 0),
            FloatUnaryOp::Neg => self.fneg_d(0, 0),
            FloatUnaryOp::Sqrt => self.fsqrt_d(0, 0),
            FloatUnaryOp::Ceil => self.frintp_d(0, 0),
            FloatUnaryOp::Floor => self.frintm_d(0, 0),
            FloatUnaryOp::Trunc => self.frintz_d(0, 0),
            FloatUnaryOp::Nearest => self.frintn_d(0, 0),
        }
        self.fmov_x_from_d(16, 0);
        self.push_x_as_i64_slots(16)?;
        Ok(())
    }

    fn emit_f32_convert_i32(&mut self, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(1)?;
        let value = self.pop_reg()?;
        self.cvtf_s_from_w(0, value, signed);
        let result = self.push_reg()?;
        self.fmov_w_from_s(result, 0);
        Ok(())
    }

    fn emit_f32_convert_i64(&mut self, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let high = self.pop_reg()?;
        let low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, low, high, 9)?;
        self.cvtf_s_from_x(0, 16, signed);
        let result = self.push_reg()?;
        self.fmov_w_from_s(result, 0);
        Ok(())
    }

    fn emit_f32_demote_f64(&mut self) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let high = self.pop_reg()?;
        let low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, low, high, 9)?;
        self.fmov_d_from_x(0, 16);
        self.fcvt_s_from_d(0, 0);
        let result = self.push_reg()?;
        self.fmov_w_from_s(result, 0);
        Ok(())
    }

    fn emit_f64_convert_i32(&mut self, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(1)?;
        let value = self.pop_reg()?;
        self.cvtf_d_from_w(0, value, signed);
        self.fmov_x_from_d(16, 0);
        self.push_x_as_i64_slots(16)?;
        Ok(())
    }

    fn emit_f64_convert_i64(&mut self, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(2)?;
        let high = self.pop_reg()?;
        let low = self.pop_reg()?;
        self.pack_i64_slots_to_x(16, low, high, 9)?;
        self.cvtf_d_from_x(0, 16, signed);
        self.fmov_x_from_d(16, 0);
        self.push_x_as_i64_slots(16)?;
        Ok(())
    }

    fn emit_f64_promote_f32(&mut self) -> Result<(), ()> {
        self.ensure_stack_slots(1)?;
        let value = self.pop_reg()?;
        self.fmov_s_from_w(0, value);
        self.fcvt_d_from_s(0, 0);
        self.fmov_x_from_d(16, 0);
        self.push_x_as_i64_slots(16)?;
        Ok(())
    }

    fn emit_i32_trunc_float(&mut self, source: FloatWidth, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(float_width_slots(source))?;
        match source {
            FloatWidth::F32 => {
                let value = self.pop_reg()?;
                self.mov_w(0, value);
                self.mov_imm_u32(1, u32::from(signed));
                self.call_ptr(jit_i32_trunc_f32 as *const () as usize);
            }
            FloatWidth::F64 => {
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.pack_i64_slots_to_x(0, low, high, 9)?;
                self.mov_imm_u32(1, u32::from(signed));
                self.call_ptr(jit_i32_trunc_f64 as *const () as usize);
            }
        }
        self.return_if_exit();
        let result = self.push_reg()?;
        self.mov_w(result, 1);
        Ok(())
    }

    fn emit_i64_trunc_float(
        &mut self,
        source: FloatWidth,
        signed: bool,
        saturating: bool,
    ) -> Result<(), ()> {
        self.ensure_stack_slots(float_width_slots(source))?;
        match source {
            FloatWidth::F32 => {
                let value = self.pop_reg()?;
                self.mov_w(0, value);
                self.mov_imm_u32(1, u32::from(signed));
                self.mov_imm_u32(2, u32::from(saturating));
                self.call_ptr(jit_i64_trunc_f32 as *const () as usize);
            }
            FloatWidth::F64 => {
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.pack_i64_slots_to_x(0, low, high, 9)?;
                self.mov_imm_u32(1, u32::from(signed));
                self.mov_imm_u32(2, u32::from(saturating));
                self.call_ptr(jit_i64_trunc_f64 as *const () as usize);
            }
        }
        self.return_if_exit();
        self.push_x_as_i64_slots(1)?;
        Ok(())
    }

    fn emit_i32_trunc_sat_float(&mut self, source: FloatWidth, signed: bool) -> Result<(), ()> {
        self.ensure_stack_slots(float_width_slots(source))?;
        match source {
            FloatWidth::F32 => {
                let value = self.pop_reg()?;
                self.fmov_s_from_w(0, value);
                let result = self.push_reg()?;
                self.fcvt_w_from_s(result, 0, signed);
            }
            FloatWidth::F64 => {
                let high = self.pop_reg()?;
                let low = self.pop_reg()?;
                self.pack_i64_slots_to_x(16, low, high, 9)?;
                self.fmov_d_from_x(0, 16);
                let result = self.push_reg()?;
                self.fcvt_w_from_d(result, 0, signed);
            }
        }
        Ok(())
    }

    fn pack_i64_slots_to_x(&mut self, rd: u8, low: u8, high: u8, tmp: u8) -> Result<(), ()> {
        self.mov_w(rd, low);
        self.mov_w(tmp, high);
        self.lsl_x_imm(tmp, tmp, 32)?;
        self.orr_x(rd, rd, tmp);
        Ok(())
    }

    fn push_x_as_i64_slots(&mut self, src: u8) -> Result<(), ()> {
        let low = self.push_reg()?;
        self.mov_w(low, src);
        let high = self.push_reg()?;
        self.lsr_x_imm(17, src, 32)?;
        self.mov_w(high, 17);
        Ok(())
    }

    fn load_local_i64_to_x(&mut self, rd: u8, local: u32, tmp: u8) -> Result<(), ()> {
        self.load_local4_to_reg(rd, local)?;
        self.load_local4_to_reg(tmp, local.wrapping_add(4))?;
        self.lsl_x_imm(tmp, tmp, 32)?;
        self.orr_x(rd, rd, tmp);
        Ok(())
    }

    fn store_local_i64_from_x(&mut self, local: u32, src: u8, tmp: u8) -> Result<(), ()> {
        self.store_local4_from_reg(local, src)?;
        self.lsr_x_imm(tmp, src, 32)?;
        self.store_local4_from_reg(local.wrapping_add(4), tmp)?;
        Ok(())
    }

    fn load_local_binop64_rhs_to_x(
        &mut self,
        rd: u8,
        rhs_shape: LocalFastRhsShape,
        rhs: u64,
        tmp: u8,
    ) -> Result<(), ()> {
        match rhs_shape {
            LocalFastRhsShape::Local => {
                let rhs = u32::try_from(rhs).map_err(|_| ())?;
                self.load_local_i64_to_x(rd, rhs, tmp)
            }
            LocalFastRhsShape::Const => {
                self.mov_imm_u64(rd, rhs);
                Ok(())
            }
        }
    }

    fn emit_i64_binop(&mut self, op: LocalBinop64Op, lhs: u8, rhs: u8) -> Result<(), ()> {
        match op {
            LocalBinop64Op::I64Add => self.add_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Sub => self.sub_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Mul => self.mul_x(lhs, lhs, rhs),
            LocalBinop64Op::I64And => self.and_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Or => self.orr_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Xor => self.eor_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Shl => self.lslv_x(lhs, lhs, rhs),
            LocalBinop64Op::I64ShrS => self.asrv_x(lhs, lhs, rhs),
            LocalBinop64Op::I64ShrU => self.lsrv_x(lhs, lhs, rhs),
            LocalBinop64Op::I64Rotl => {
                self.neg_x(9, rhs);
                self.rorv_x(lhs, lhs, 9);
            }
            LocalBinop64Op::I64Rotr => self.rorv_x(lhs, lhs, rhs),
            LocalBinop64Op::F64Add => {
                self.fmov_d_from_x(0, lhs);
                self.fmov_d_from_x(1, rhs);
                self.fadd_d(0, 0, 1);
                self.fmov_x_from_d(lhs, 0);
            }
            LocalBinop64Op::F64Sub => {
                self.fmov_d_from_x(0, lhs);
                self.fmov_d_from_x(1, rhs);
                self.fsub_d(0, 0, 1);
                self.fmov_x_from_d(lhs, 0);
            }
            LocalBinop64Op::F64Mul => {
                self.fmov_d_from_x(0, lhs);
                self.fmov_d_from_x(1, rhs);
                self.fmul_d(0, 0, 1);
                self.fmov_x_from_d(lhs, 0);
            }
            LocalBinop64Op::F64Div => {
                self.fmov_d_from_x(0, lhs);
                self.fmov_d_from_x(1, rhs);
                self.fdiv_d(0, 0, 1);
                self.fmov_x_from_d(lhs, 0);
            }
        }
        Ok(())
    }
}

fn branch_kind(kind: FixupKind) -> BranchKind {
    match kind {
        FixupKind::B => BranchKind::B,
        FixupKind::BCond(cond) => BranchKind::BCond(cond),
        FixupKind::CbnzX(rt) => BranchKind::CbnzX(rt),
        FixupKind::CbnzW(rt) => BranchKind::CbnzW(rt),
    }
}

fn patch_branch(bytes: &mut [u8], at: usize, target: usize, kind: FixupKind) -> Result<(), ()> {
    patch_a64_branch(bytes, at, target, branch_kind(kind)).map_err(|_| ())
}

fn trace_compile_message(args: std::fmt::Arguments<'_>) {
    if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
        eprintln!("[telomere-jit] {args}");
    }
}

fn i32_cmp_cond(op: LocalCmp32Op) -> Result<Cond, ()> {
    match op {
        LocalCmp32Op::I32Eq => Ok(Cond::Eq),
        LocalCmp32Op::I32Ne => Ok(Cond::Ne),
        LocalCmp32Op::I32LtS => Ok(Cond::Lt),
        LocalCmp32Op::I32LtU => Ok(Cond::Lo),
        LocalCmp32Op::I32GtS => Ok(Cond::Gt),
        LocalCmp32Op::I32GtU => Ok(Cond::Hi),
        LocalCmp32Op::I32LeS => Ok(Cond::Le),
        LocalCmp32Op::I32LeU => Ok(Cond::Ls),
        LocalCmp32Op::I32GeS => Ok(Cond::Ge),
        LocalCmp32Op::I32GeU => Ok(Cond::Hs),
        LocalCmp32Op::F32Eq
        | LocalCmp32Op::F32Ne
        | LocalCmp32Op::F32Lt
        | LocalCmp32Op::F32Gt
        | LocalCmp32Op::F32Le
        | LocalCmp32Op::F32Ge => Err(()),
    }
}

fn f32_cmp_cond(op: LocalCmp32Op) -> Result<Cond, ()> {
    match op {
        LocalCmp32Op::F32Eq => Ok(Cond::Eq),
        LocalCmp32Op::F32Ne => Ok(Cond::Ne),
        LocalCmp32Op::F32Lt => Ok(Cond::Lo),
        LocalCmp32Op::F32Gt => Ok(Cond::Gt),
        LocalCmp32Op::F32Le => Ok(Cond::Ls),
        LocalCmp32Op::F32Ge => Ok(Cond::Ge),
        LocalCmp32Op::I32Eq
        | LocalCmp32Op::I32Ne
        | LocalCmp32Op::I32LtS
        | LocalCmp32Op::I32LtU
        | LocalCmp32Op::I32GtS
        | LocalCmp32Op::I32GtU
        | LocalCmp32Op::I32LeS
        | LocalCmp32Op::I32LeU
        | LocalCmp32Op::I32GeS
        | LocalCmp32Op::I32GeU => Err(()),
    }
}

fn i64_cmp_cond(op: LocalCmp64Op) -> Result<Cond, ()> {
    match op {
        LocalCmp64Op::I64Eq => Ok(Cond::Eq),
        LocalCmp64Op::I64Ne => Ok(Cond::Ne),
        LocalCmp64Op::I64LtS => Ok(Cond::Lt),
        LocalCmp64Op::I64LtU => Ok(Cond::Lo),
        LocalCmp64Op::I64GtS => Ok(Cond::Gt),
        LocalCmp64Op::I64GtU => Ok(Cond::Hi),
        LocalCmp64Op::I64LeS => Ok(Cond::Le),
        LocalCmp64Op::I64LeU => Ok(Cond::Ls),
        LocalCmp64Op::I64GeS => Ok(Cond::Ge),
        LocalCmp64Op::I64GeU => Ok(Cond::Hs),
        LocalCmp64Op::F64Eq
        | LocalCmp64Op::F64Ne
        | LocalCmp64Op::F64Lt
        | LocalCmp64Op::F64Gt
        | LocalCmp64Op::F64Le
        | LocalCmp64Op::F64Ge => Err(()),
    }
}

fn f64_cmp_cond(op: LocalCmp64Op) -> Result<Cond, ()> {
    match op {
        LocalCmp64Op::F64Eq => Ok(Cond::Eq),
        LocalCmp64Op::F64Ne => Ok(Cond::Ne),
        LocalCmp64Op::F64Lt => Ok(Cond::Lo),
        LocalCmp64Op::F64Gt => Ok(Cond::Gt),
        LocalCmp64Op::F64Le => Ok(Cond::Ls),
        LocalCmp64Op::F64Ge => Ok(Cond::Ge),
        LocalCmp64Op::I64Eq
        | LocalCmp64Op::I64Ne
        | LocalCmp64Op::I64LtS
        | LocalCmp64Op::I64LtU
        | LocalCmp64Op::I64GtS
        | LocalCmp64Op::I64GtU
        | LocalCmp64Op::I64LeS
        | LocalCmp64Op::I64LeU
        | LocalCmp64Op::I64GeS
        | LocalCmp64Op::I64GeU => Err(()),
    }
}

fn raw_i32_cmp_cond(kind: u32) -> Result<Cond, ()> {
    match kind {
        0 => Ok(Cond::Eq),
        1 => Ok(Cond::Ne),
        2 => Ok(Cond::Lt),
        3 => Ok(Cond::Lo),
        4 => Ok(Cond::Gt),
        5 => Ok(Cond::Hi),
        6 => Ok(Cond::Le),
        7 => Ok(Cond::Ls),
        8 => Ok(Cond::Ge),
        9 => Ok(Cond::Hs),
        _ => Err(()),
    }
}

fn scalar_load_kind(kind: u32) -> Option<(u32, bool)> {
    match kind {
        0 => Some((4, false)),
        1 => Some((1, true)),
        2 => Some((1, false)),
        3 => Some((2, true)),
        4 => Some((2, false)),
        _ => None,
    }
}

fn scalar_store_kind(kind: u32) -> Option<u32> {
    match kind {
        0 => Some(4),
        1 => Some(1),
        2 => Some(2),
        _ => None,
    }
}

fn float_width_slots(width: FloatWidth) -> usize {
    match width {
        FloatWidth::F32 => 1,
        FloatWidth::F64 => 2,
    }
}

fn float_cmp_cond(op: FloatCompareOp) -> Cond {
    match op {
        FloatCompareOp::Eq => Cond::Eq,
        FloatCompareOp::Ne => Cond::Ne,
        FloatCompareOp::Lt => Cond::Lo,
        FloatCompareOp::Gt => Cond::Gt,
        FloatCompareOp::Le => Cond::Ls,
        FloatCompareOp::Ge => Cond::Ge,
    }
}

fn trace_direct_call_reject(
    cursor: usize,
    reason: &str,
    flushed_size: u32,
    param_size: u32,
    return_size: u32,
) {
    if std::env::var_os("TELOMERE_JIT_TRACE_FALLBACK").is_some() {
        eprintln!(
            "[telomere-jit] direct_call_reject pc={cursor} reason={reason} stack={flushed_size} params={param_size} returns={return_size}"
        );
    }
}

fn pack_call_stack_sizes(recipe: CallDispatchCache) -> u64 {
    (u64::from(recipe.param_size) << 32) | u64::from(recipe.return_size)
}
