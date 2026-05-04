use crate::common::{Instr, ObjectRef, VMResult};
use crate::runtime::jit::abi::{vm_result_code, JitNativeExit};
use crate::runtime::jit::stubs::{
    direct_call as jit_direct_call, function_return as jit_function_return,
    i32_load as jit_i32_load, i32_store as jit_i32_store, push_i32 as jit_push_i32,
};
use crate::{
    common::{decode_local_binop32_kind, LocalBinop32Op, LocalFastRhsShape, Op},
    runtime::vm::{
        op_br, op_br_if, op_call, op_call_jit_lazy, op_end, op_i32_add, op_i32_const,
        op_i32_const_binop, op_i32_const_set4, op_i32_eq, op_i32_eqz, op_i32_load, op_i32_load16_s,
        op_i32_load16_s_local_base, op_i32_load16_u, op_i32_load16_u_local_base, op_i32_load8_s,
        op_i32_load8_s_local_base, op_i32_load8_u, op_i32_load8_u_local_base,
        op_i32_load_local_base, op_i32_lt_s, op_i32_lt_u, op_i32_mul, op_i32_store, op_i32_store16,
        op_i32_store8, op_i32_store_local_base_local_get4, op_i32_sub, op_local_get4,
        op_local_get4_br_if, op_local_get4_i32_const_add, op_local_get4_i32_const_add_set4,
        op_local_get4_i32_const_add_tee4, op_local_get4_local_get4,
        op_local_get4_local_get4_i32_add, op_local_get4_local_get4_i32_add_set4,
        op_local_get4_local_get4_i32_add_tee4, op_local_set4, op_local_tee4, op_return,
        op_return_call, op_return_call_jit_lazy, special_function_return,
    },
};

pub(crate) fn emit_baseline_function(
    _funcaddr: ObjectRef,
    code: &[Instr],
    op_lens: &[u16],
) -> Result<Vec<u8>, ()> {
    let mut emitter = Emitter::new(code, op_lens);
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
    CbnzX(u8),
    CbnzW(u8),
}

#[derive(Clone, Copy)]
enum Cond {
    Eq = 0,
    Ne = 1,
    Lo = 3,
    Hs = 2,
    Lt = 11,
    Ge = 10,
}

impl Cond {
    fn inverted(self) -> Self {
        match self {
            Self::Eq => Self::Ne,
            Self::Ne => Self::Eq,
            Self::Lo => Self::Hs,
            Self::Hs => Self::Lo,
            Self::Lt => Self::Ge,
            Self::Ge => Self::Lt,
        }
    }
}

const STACK_REGS: [u8; 7] = [22, 23, 24, 25, 26, 27, 28];

struct Emitter<'a> {
    wasm: &'a [Instr],
    op_lens: &'a [u16],
    bytes: Vec<u8>,
    labels: Vec<Option<usize>>,
    fixups: Vec<Fixup>,
    stack_depth: usize,
}

impl<'a> Emitter<'a> {
    fn new(wasm: &'a [Instr], op_lens: &'a [u16]) -> Self {
        Self {
            wasm,
            op_lens,
            bytes: Vec::with_capacity(wasm.len() * 16),
            labels: vec![None; wasm.len().saturating_add(1)],
            fixups: Vec::new(),
            stack_depth: 0,
        }
    }

    fn emit(&mut self) -> Result<(), ()> {
        self.prologue();
        let mut cursor = 0usize;
        for &len in self.op_lens {
            if cursor >= self.wasm.len() {
                return Err(());
            }
            self.labels[cursor] = Some(self.offset());
            let op = unsafe { self.wasm[cursor].op };
            if std::ptr::fn_addr_eq(op, op_i32_const as Op) {
                let value = unsafe { self.wasm[cursor + 1].operand.u32 };
                let dst = self.push_reg()?;
                self.mov_imm_u32(dst, value);
            } else if std::ptr::fn_addr_eq(op, op_i32_const_set4 as Op) {
                let value = unsafe { self.wasm[cursor + 1].operand.u32 };
                let local = unsafe { self.wasm[cursor + 2].operand.local_addr };
                self.mov_imm_u32(16, value);
                self.store_local4_from_reg(local, 16)?;
            } else if std::ptr::fn_addr_eq(op, op_i32_const_binop as Op) {
                let kind = unsafe { self.wasm[cursor + 1].operand.u32 };
                let rhs = unsafe { self.wasm[cursor + 2].operand.u32 };
                let lhs = self.peek_reg()?;
                self.emit_i32_const_binop(kind, lhs, rhs)?;
            } else if std::ptr::fn_addr_eq(op, op_local_get4_i32_const_add as Op) {
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let value = unsafe { self.wasm[cursor + 2].operand.u32 };
                let dst = self.push_reg()?;
                self.load_local4_to_reg(dst, local)?;
                self.add_imm_u32(dst, dst, value)?;
            } else if std::ptr::fn_addr_eq(op, op_local_get4_i32_const_add_set4 as Op)
                || std::ptr::fn_addr_eq(op, op_local_get4_i32_const_add_tee4 as Op)
            {
                let src = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let value = unsafe { self.wasm[cursor + 2].operand.u32 };
                let dst = unsafe { self.wasm[cursor + 3].operand.local_addr };
                let value_reg = self.push_reg()?;
                self.load_local4_to_reg(value_reg, src)?;
                self.add_imm_u32(value_reg, value_reg, value)?;
                self.store_local4_from_reg(dst, value_reg)?;
                if std::ptr::fn_addr_eq(op, op_local_get4_i32_const_add_set4 as Op) {
                    self.pop_reg()?;
                }
            } else if std::ptr::fn_addr_eq(op, op_local_get4_local_get4_i32_add as Op) {
                let lhs = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let rhs = unsafe { self.wasm[cursor + 2].operand.local_addr };
                let lhs_reg = self.push_reg()?;
                self.load_local4_to_reg(lhs_reg, lhs)?;
                let rhs_reg = self.push_reg()?;
                self.load_local4_to_reg(rhs_reg, rhs)?;
                let rhs_reg = self.pop_reg()?;
                let lhs_reg = self.peek_reg()?;
                self.add_w(lhs_reg, lhs_reg, rhs_reg);
            } else if std::ptr::fn_addr_eq(op, op_local_get4_local_get4_i32_add_set4 as Op)
                || std::ptr::fn_addr_eq(op, op_local_get4_local_get4_i32_add_tee4 as Op)
            {
                let lhs = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let rhs = unsafe { self.wasm[cursor + 2].operand.local_addr };
                let dst = unsafe { self.wasm[cursor + 3].operand.local_addr };
                let lhs_reg = self.push_reg()?;
                self.load_local4_to_reg(lhs_reg, lhs)?;
                let rhs_reg = self.push_reg()?;
                self.load_local4_to_reg(rhs_reg, rhs)?;
                let rhs_reg = self.pop_reg()?;
                let lhs_reg = self.peek_reg()?;
                self.add_w(lhs_reg, lhs_reg, rhs_reg);
                self.store_local4_from_reg(dst, lhs_reg)?;
                if std::ptr::fn_addr_eq(op, op_local_get4_local_get4_i32_add_set4 as Op) {
                    self.pop_reg()?;
                }
            } else if std::ptr::fn_addr_eq(op, op_local_get4 as Op) {
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let dst = self.push_reg()?;
                self.load_local4_to_reg(dst, local)?;
            } else if std::ptr::fn_addr_eq(op, op_local_get4_local_get4 as Op) {
                let first = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let second = unsafe { self.wasm[cursor + 2].operand.local_addr };
                let first_reg = self.push_reg()?;
                self.load_local4_to_reg(first_reg, first)?;
                let second_reg = self.push_reg()?;
                self.load_local4_to_reg(second_reg, second)?;
            } else if std::ptr::fn_addr_eq(op, op_local_set4 as Op) {
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let src = self.pop_reg()?;
                self.store_local4_from_reg(local, src)?;
            } else if std::ptr::fn_addr_eq(op, op_local_tee4 as Op) {
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let src = self.peek_reg()?;
                self.store_local4_from_reg(local, src)?;
            } else if std::ptr::fn_addr_eq(op, op_i32_add as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.add_w(lhs, lhs, rhs);
            } else if std::ptr::fn_addr_eq(op, op_i32_sub as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.sub_w(lhs, lhs, rhs);
            } else if std::ptr::fn_addr_eq(op, op_i32_mul as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.mul_w(lhs, lhs, rhs);
            } else if std::ptr::fn_addr_eq(op, op_i32_eqz as Op) {
                let value = self.peek_reg()?;
                self.cmp_w_imm(value, 0)?;
                self.cset_w(value, Cond::Eq);
            } else if std::ptr::fn_addr_eq(op, op_i32_eq as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.cmp_w(lhs, rhs);
                self.cset_w(lhs, Cond::Eq);
            } else if std::ptr::fn_addr_eq(op, op_i32_lt_s as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.cmp_w(lhs, rhs);
                self.cset_w(lhs, Cond::Lt);
            } else if std::ptr::fn_addr_eq(op, op_i32_lt_u as Op) {
                let rhs = self.pop_reg()?;
                let lhs = self.peek_reg()?;
                self.cmp_w(lhs, rhs);
                self.cset_w(lhs, Cond::Lo);
            } else if std::ptr::fn_addr_eq(op, op_i32_load as Op) {
                let memarg = unsafe { self.wasm[cursor + 1].operand.memarg };
                let addr = self.peek_reg()?;
                self.call_i32_load_helper(addr, memarg.offset, 4, false);
            } else if std::ptr::fn_addr_eq(op, op_i32_load8_u as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load8_s as Op)
            {
                let memarg = unsafe { self.wasm[cursor + 1].operand.memarg };
                let addr = self.peek_reg()?;
                self.call_i32_load_helper(
                    addr,
                    memarg.offset,
                    1,
                    std::ptr::fn_addr_eq(op, op_i32_load8_s as Op),
                );
            } else if std::ptr::fn_addr_eq(op, op_i32_load16_u as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load16_s as Op)
            {
                let memarg = unsafe { self.wasm[cursor + 1].operand.memarg };
                let addr = self.peek_reg()?;
                self.call_i32_load_helper(
                    addr,
                    memarg.offset,
                    2,
                    std::ptr::fn_addr_eq(op, op_i32_load16_s as Op),
                );
            } else if std::ptr::fn_addr_eq(op, op_i32_load_local_base as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load8_u_local_base as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load8_s_local_base as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load16_u_local_base as Op)
                || std::ptr::fn_addr_eq(op, op_i32_load16_s_local_base as Op)
            {
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let delta = unsafe { self.wasm[cursor + 2].operand.i32 as u32 };
                let memarg = unsafe { self.wasm[cursor + 3].operand.memarg };
                let addr = self.push_reg()?;
                self.load_local4_to_reg(addr, local)?;
                self.add_imm_u32(addr, addr, delta)?;
                let (width, signed) = if std::ptr::fn_addr_eq(op, op_i32_load8_u_local_base as Op)
                    || std::ptr::fn_addr_eq(op, op_i32_load8_s_local_base as Op)
                {
                    (1, std::ptr::fn_addr_eq(op, op_i32_load8_s_local_base as Op))
                } else if std::ptr::fn_addr_eq(op, op_i32_load16_u_local_base as Op)
                    || std::ptr::fn_addr_eq(op, op_i32_load16_s_local_base as Op)
                {
                    (
                        2,
                        std::ptr::fn_addr_eq(op, op_i32_load16_s_local_base as Op),
                    )
                } else {
                    (4, false)
                };
                self.call_i32_load_helper(addr, memarg.offset, width, signed);
            } else if std::ptr::fn_addr_eq(op, op_i32_store as Op)
                || std::ptr::fn_addr_eq(op, op_i32_store8 as Op)
                || std::ptr::fn_addr_eq(op, op_i32_store16 as Op)
            {
                let memarg = unsafe { self.wasm[cursor + 1].operand.memarg };
                let value = self.pop_reg()?;
                let addr = self.pop_reg()?;
                let width = if std::ptr::fn_addr_eq(op, op_i32_store8 as Op) {
                    1
                } else if std::ptr::fn_addr_eq(op, op_i32_store16 as Op) {
                    2
                } else {
                    4
                };
                self.call_i32_store_helper(addr, memarg.offset, width, value);
            } else if std::ptr::fn_addr_eq(op, op_i32_store_local_base_local_get4 as Op) {
                let addr_local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let delta = unsafe { self.wasm[cursor + 2].operand.i32 as u32 };
                let value_local = unsafe { self.wasm[cursor + 3].operand.local_addr };
                let memarg = unsafe { self.wasm[cursor + 4].operand.memarg };
                self.load_local4_to_reg(16, addr_local)?;
                self.add_imm_u32(16, 16, delta)?;
                self.load_local4_to_reg(17, value_local)?;
                self.call_i32_store_helper(16, memarg.offset, 4, 17);
            } else if std::ptr::fn_addr_eq(op, op_br as Op)
                || std::ptr::fn_addr_eq(op, op_return as Op)
            {
                self.flush_stack()?;
                let target = unsafe { self.wasm[cursor + 1].operand.jump_addr } as usize;
                self.branch_to(target, FixupKind::B);
            } else if std::ptr::fn_addr_eq(op, op_br_if as Op) {
                if self.stack_depth > 1 {
                    self.flush_stack_for_fallback()?;
                    self.return_fallback_index(cursor);
                    break;
                } else {
                    let target = unsafe { self.wasm[cursor + 1].operand.jump_addr } as usize;
                    let cond = self.pop_reg()?;
                    self.flush_stack()?;
                    self.branch_to(target, FixupKind::CbnzW(cond));
                }
            } else if std::ptr::fn_addr_eq(op, op_local_get4_br_if as Op) {
                if self.stack_depth > 0 {
                    self.flush_stack_for_fallback()?;
                    self.return_fallback_index(cursor);
                    break;
                }
                let local = unsafe { self.wasm[cursor + 1].operand.local_addr };
                let target = unsafe { self.wasm[cursor + 2].operand.jump_addr } as usize;
                self.load_local4_to_reg(16, local)?;
                self.branch_to(target, FixupKind::CbnzW(16));
            } else if std::ptr::fn_addr_eq(op, op_end as Op) {
            } else if std::ptr::fn_addr_eq(op, special_function_return as Op) {
                let return_size = unsafe { self.wasm[cursor + 1].operand.drop_size };
                self.flush_stack()?;
                self.call_return_helper(return_size);
            } else if std::ptr::fn_addr_eq(op, op_call as Op)
                || std::ptr::fn_addr_eq(op, op_call_jit_lazy as Op)
            {
                self.flush_stack()?;
                self.call_direct_helper(cursor + 1, false);
                break;
            } else if std::ptr::fn_addr_eq(op, op_return_call as Op)
                || std::ptr::fn_addr_eq(op, op_return_call_jit_lazy as Op)
            {
                self.flush_stack()?;
                self.call_direct_helper(cursor + 1, true);
                break;
            } else {
                return Err(());
            }
            cursor += usize::from(len);
        }
        self.return_trap(VMResult::<()>::InvalidOperand);
        Ok(())
    }

    fn finish(mut self) -> Result<Vec<u8>, ()> {
        let epilogue = self.offset();
        self.restore_and_ret();
        for fixup in self.fixups {
            let target = self
                .labels
                .get(fixup.target_index)
                .and_then(|label| *label)
                .unwrap_or(epilogue);
            patch_branch(&mut self.bytes, fixup.at, target, fixup.kind)?;
        }
        Ok(self.bytes)
    }

    fn offset(&self) -> usize {
        self.bytes.len()
    }

    fn prologue(&mut self) {
        self.stp_pre(29, 30);
        self.insn(0x910003fd);
        self.stp_pre(19, 20);
        self.stp_pre(21, 22);
        self.stp_pre(23, 24);
        self.stp_pre(25, 26);
        self.stp_pre(27, 28);
        self.insn(0xaa0003f3);
        self.insn(0xaa0103f4);
        self.insn(0xaa0203f5);
    }

    fn restore_and_ret(&mut self) {
        self.ldp_post(27, 28);
        self.ldp_post(25, 26);
        self.ldp_post(23, 24);
        self.ldp_post(21, 22);
        self.ldp_post(19, 20);
        self.ldp_post(29, 30);
        self.insn(0xd65f03c0);
    }

    fn push_reg(&mut self) -> Result<u8, ()> {
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

    fn flush_stack(&mut self) -> Result<(), ()> {
        for reg in STACK_REGS.iter().take(self.stack_depth) {
            self.call_helper_reg(jit_push_i32 as *const () as usize, *reg);
        }
        self.stack_depth = 0;
        Ok(())
    }

    fn flush_stack_for_fallback(&mut self) -> Result<(), ()> {
        for reg in STACK_REGS.iter().take(self.stack_depth) {
            self.call_helper_reg(jit_push_i32 as *const () as usize, *reg);
        }
        Ok(())
    }

    fn call_helper_reg(&mut self, helper: usize, rn: u8) {
        self.mov_x(0, 19);
        self.mov_w(1, rn);
        self.call_ptr(helper);
        self.return_if_exit();
    }

    fn call_return_helper(&mut self, return_size: u32) {
        self.mov_x(0, 19);
        self.mov_imm_u32(1, return_size);
        self.call_ptr(jit_function_return as *const () as usize);
        self.branch_to_epilogue();
    }

    fn call_direct_helper(&mut self, operand_index: usize, is_return_call: bool) {
        self.mov_x(0, 19);
        self.load_code_ptr_operand(1, operand_index);
        self.mov_imm_u64(2, u64::from(is_return_call));
        self.call_ptr(jit_direct_call as *const () as usize);
        if is_return_call {
            self.branch_to_epilogue();
        } else {
            self.return_if_exit();
        }
    }

    fn call_i32_load_helper(&mut self, addr: u8, offset: u32, width: u32, signed: bool) {
        self.mov_x(0, 19);
        self.mov_w(1, addr);
        self.mov_imm_u32(2, offset);
        self.mov_imm_u32(3, width);
        self.mov_imm_u32(4, u32::from(signed));
        self.call_ptr(jit_i32_load as *const () as usize);
        self.return_if_exit();
        self.mov_w(addr, 1);
    }

    fn call_i32_store_helper(&mut self, addr: u8, offset: u32, width: u32, value: u8) {
        self.mov_x(0, 19);
        self.mov_w(1, addr);
        self.mov_imm_u32(2, offset);
        self.mov_imm_u32(3, width);
        self.mov_w(4, value);
        self.call_ptr(jit_i32_store as *const () as usize);
        self.return_if_exit();
    }

    fn load_code_ptr_operand(&mut self, rd: u8, operand_index: usize) {
        let byte_offset = operand_index
            .checked_mul(std::mem::size_of::<Instr>())
            .expect("jit operand offset overflow");
        if byte_offset <= 4095 {
            self.insn(0x91000000 | ((byte_offset as u32) << 10) | (20 << 5) | u32::from(rd));
        } else {
            self.mov_imm_u64(rd, byte_offset as u64);
            self.insn(0x8b000000 | (u32::from(rd) << 16) | (20 << 5) | u32::from(rd));
        }
    }

    fn return_if_exit(&mut self) {
        let at = self.offset();
        self.insn(0xb5000000);
        self.fixups.push(Fixup {
            at,
            target_index: usize::MAX,
            kind: FixupKind::CbnzX(0),
        });
    }

    fn branch_to_epilogue(&mut self) {
        let at = self.offset();
        self.insn(0x14000000);
        self.fixups.push(Fixup {
            at,
            target_index: usize::MAX,
            kind: FixupKind::B,
        });
    }

    fn branch_to(&mut self, target_index: usize, kind: FixupKind) {
        let at = self.offset();
        self.insn(match kind {
            FixupKind::B => 0x14000000,
            FixupKind::CbnzX(rt) => 0xb5000000 | u32::from(rt),
            FixupKind::CbnzW(rt) => 0x35000000 | u32::from(rt),
        });
        self.fixups.push(Fixup {
            at,
            target_index,
            kind,
        });
    }

    fn return_trap<T>(&mut self, result: VMResult<T>) {
        self.mov_imm_u64(0, JitNativeExit::TRAP);
        self.mov_imm_u64(1, vm_result_code(result));
        self.branch_to_epilogue();
    }

    fn return_fallback_index(&mut self, index: usize) {
        self.mov_imm_u64(0, JitNativeExit::FALLBACK_INDEX);
        self.mov_imm_u64(1, index as u64);
        self.branch_to_epilogue();
    }

    fn call_ptr(&mut self, ptr: usize) {
        self.mov_imm_u64(16, ptr as u64);
        self.insn(0xd63f0200);
    }

    fn mov_x(&mut self, rd: u8, rn: u8) {
        self.insn(0xaa0003e0 | (u32::from(rn) << 16) | u32::from(rd));
    }

    fn mov_w(&mut self, rd: u8, rn: u8) {
        self.insn(0x2a0003e0 | (u32::from(rn) << 16) | u32::from(rd));
    }

    fn mov_imm_u32(&mut self, rd: u8, value: u32) {
        self.insn(0x52800000 | ((value & 0xffff) << 5) | u32::from(rd));
        let hi = (value >> 16) & 0xffff;
        if hi != 0 {
            self.insn(0x72800000 | (1 << 21) | (hi << 5) | u32::from(rd));
        }
    }

    fn mov_imm_u64(&mut self, rd: u8, value: u64) {
        self.insn(0xd2800000 | (((value & 0xffff) as u32) << 5) | u32::from(rd));
        for hw in 1..4 {
            let part = ((value >> (hw * 16)) & 0xffff) as u32;
            if part != 0 {
                self.insn(0xf2800000 | ((hw as u32) << 21) | (part << 5) | u32::from(rd));
            }
        }
    }

    fn insn(&mut self, insn: u32) {
        self.bytes.extend_from_slice(&insn.to_le_bytes());
    }

    fn stp_pre(&mut self, rt: u8, rt2: u8) {
        self.insn(0xa9800000 | (0x7e << 15) | (u32::from(rt2) << 10) | (31 << 5) | u32::from(rt));
    }

    fn ldp_post(&mut self, rt: u8, rt2: u8) {
        self.insn(0xa8c00000 | (2 << 15) | (u32::from(rt2) << 10) | (31 << 5) | u32::from(rt));
    }

    fn load_local4_to_reg(&mut self, rd: u8, local: u32) -> Result<(), ()> {
        self.load_store_local4(rd, local, true)
    }

    fn store_local4_from_reg(&mut self, local: u32, rs: u8) -> Result<(), ()> {
        self.load_store_local4(rs, local, false)
    }

    fn load_store_local4(&mut self, rt: u8, local: u32, load: bool) -> Result<(), ()> {
        if local <= 255 {
            let base = if load { 0xb8400000 } else { 0xb8000000 };
            self.insn(base | (local << 12) | (21 << 5) | u32::from(rt));
            return Ok(());
        }
        self.addr_local(17, local)?;
        let base = if load { 0xb9400000 } else { 0xb9000000 };
        self.insn(base | (17 << 5) | u32::from(rt));
        Ok(())
    }

    fn addr_local(&mut self, rd: u8, local: u32) -> Result<(), ()> {
        if local <= 4095 {
            self.insn(0x91000000 | (local << 10) | (21 << 5) | u32::from(rd));
        } else {
            self.mov_imm_u64(rd, u64::from(local));
            self.insn(0x8b000000 | (u32::from(rd) << 16) | (21 << 5) | u32::from(rd));
        }
        Ok(())
    }

    fn add_imm_u32(&mut self, rd: u8, rn: u8, imm: u32) -> Result<(), ()> {
        if imm <= 4095 {
            self.insn(0x11000000 | (imm << 10) | (u32::from(rn) << 5) | u32::from(rd));
        } else {
            self.mov_imm_u32(17, imm);
            self.add_w(rd, rn, 17);
        }
        Ok(())
    }

    fn emit_i32_const_binop(&mut self, kind: u32, lhs: u8, rhs: u32) -> Result<(), ()> {
        let Some((op, LocalFastRhsShape::Const)) = decode_local_binop32_kind(kind) else {
            return Err(());
        };
        match op {
            LocalBinop32Op::I32Add => self.add_imm_u32(lhs, lhs, rhs)?,
            LocalBinop32Op::I32Sub => {
                if rhs <= 4095 {
                    self.insn(0x51000000 | (rhs << 10) | (u32::from(lhs) << 5) | u32::from(lhs));
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
            LocalBinop32Op::I32Shl if rhs < 32 => self.lsl_w_imm(lhs, lhs, rhs),
            LocalBinop32Op::I32ShrU if rhs < 32 => self.lsr_w_imm(lhs, lhs, rhs),
            LocalBinop32Op::I32ShrS if rhs < 32 => self.asr_w_imm(lhs, lhs, rhs),
            LocalBinop32Op::I32Rotl | LocalBinop32Op::I32Rotr if rhs == 0 => {}
            LocalBinop32Op::I32Rotl
            | LocalBinop32Op::I32Rotr
            | LocalBinop32Op::I32Shl
            | LocalBinop32Op::I32ShrS
            | LocalBinop32Op::I32ShrU
            | LocalBinop32Op::F32Add
            | LocalBinop32Op::F32Sub
            | LocalBinop32Op::F32Mul
            | LocalBinop32Op::F32Div => return Err(()),
        }
        Ok(())
    }

    fn add_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x0b000000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn sub_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x4b000000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn mul_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x1b007c00 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn and_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x0a000000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn orr_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x2a000000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn eor_w(&mut self, rd: u8, rn: u8, rm: u8) {
        self.insn(0x4a000000 | (u32::from(rm) << 16) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn cmp_w(&mut self, rn: u8, rm: u8) {
        self.insn(0x6b00001f | (u32::from(rm) << 16) | (u32::from(rn) << 5));
    }

    fn cmp_w_imm(&mut self, rn: u8, imm: u32) -> Result<(), ()> {
        if imm > 4095 {
            return Err(());
        }
        self.insn(0x7100001f | (imm << 10) | (u32::from(rn) << 5));
        Ok(())
    }

    fn cset_w(&mut self, rd: u8, cond: Cond) {
        let inverted = cond.inverted() as u32;
        self.insn(0x1a800400 | (31 << 16) | (inverted << 12) | (31 << 5) | u32::from(rd));
    }

    fn lsl_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.ubfm_w(rd, rn, (32 - shift) & 31, 31 - shift);
    }

    fn lsr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.ubfm_w(rd, rn, shift, 31);
    }

    fn asr_w_imm(&mut self, rd: u8, rn: u8, shift: u32) {
        self.sbfm_w(rd, rn, shift, 31);
    }

    fn ubfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.insn(0x53000000 | (immr << 16) | (imms << 10) | (u32::from(rn) << 5) | u32::from(rd));
    }

    fn sbfm_w(&mut self, rd: u8, rn: u8, immr: u32, imms: u32) {
        self.insn(0x13000000 | (immr << 16) | (imms << 10) | (u32::from(rn) << 5) | u32::from(rd));
    }
}

fn patch_branch(bytes: &mut [u8], at: usize, target: usize, kind: FixupKind) -> Result<(), ()> {
    let delta = target as isize - at as isize;
    if delta % 4 != 0 {
        return Err(());
    }
    let words = delta / 4;
    let insn = match kind {
        FixupKind::B => {
            if !(-(1 << 25)..(1 << 25)).contains(&words) {
                return Err(());
            }
            0x14000000 | ((words as i32 as u32) & 0x03ff_ffff)
        }
        FixupKind::CbnzX(rt) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(());
            }
            0xb5000000 | (((words as i32 as u32) & 0x7ffff) << 5) | u32::from(rt)
        }
        FixupKind::CbnzW(rt) => {
            if !(-(1 << 18)..(1 << 18)).contains(&words) {
                return Err(());
            }
            0x35000000 | (((words as i32 as u32) & 0x7ffff) << 5) | u32::from(rt)
        }
    };
    bytes[at..at + 4].copy_from_slice(&insn.to_le_bytes());
    Ok(())
}
