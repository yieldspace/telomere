use crate::{
    common::{Instr, MemArg, Op, Operand},
    runtime::vm,
};

type NodeId = usize;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ScalarType {
    I32,
    I64,
    F32,
    F64,
}

impl ScalarType {
    fn size(self) -> usize {
        match self {
            Self::I32 | Self::F32 => 4,
            Self::I64 | Self::F64 => 8,
        }
    }
}

#[derive(Clone, Copy)]
enum UnaryKind {
    I32Eqz,
    I64Eqz,
}

impl UnaryKind {
    fn result_ty(self) -> ScalarType {
        ScalarType::I32
    }

    fn raw_op(self) -> Op {
        match self {
            Self::I32Eqz => vm::op_i32_eqz,
            Self::I64Eqz => vm::op_i64_eqz,
        }
    }
}

#[derive(Clone, Copy)]
enum BinaryKind {
    I32Add,
    I32Sub,
    I32Mul,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I64Add,
    I64Sub,
    I64Mul,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Eq,
    F32Ne,
    F32Lt,
    F32Gt,
    F32Le,
    F32Ge,
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Eq,
    F64Ne,
    F64Lt,
    F64Gt,
    F64Le,
    F64Ge,
}

impl BinaryKind {
    fn input_ty(self) -> ScalarType {
        match self {
            Self::I32Add
            | Self::I32Sub
            | Self::I32Mul
            | Self::I32And
            | Self::I32Or
            | Self::I32Xor
            | Self::I32Shl
            | Self::I32ShrS
            | Self::I32ShrU
            | Self::I32Eq
            | Self::I32Ne
            | Self::I32LtS
            | Self::I32LtU
            | Self::I32GtS
            | Self::I32GtU
            | Self::I32LeS
            | Self::I32LeU
            | Self::I32GeS
            | Self::I32GeU => ScalarType::I32,
            Self::I64Add
            | Self::I64Sub
            | Self::I64Mul
            | Self::I64And
            | Self::I64Or
            | Self::I64Xor
            | Self::I64Shl
            | Self::I64ShrS
            | Self::I64ShrU
            | Self::I64Eq
            | Self::I64Ne
            | Self::I64LtS
            | Self::I64LtU
            | Self::I64GtS
            | Self::I64GtU
            | Self::I64LeS
            | Self::I64LeU
            | Self::I64GeS
            | Self::I64GeU => ScalarType::I64,
            Self::F32Add
            | Self::F32Sub
            | Self::F32Mul
            | Self::F32Div
            | Self::F32Eq
            | Self::F32Ne
            | Self::F32Lt
            | Self::F32Gt
            | Self::F32Le
            | Self::F32Ge => ScalarType::F32,
            Self::F64Add
            | Self::F64Sub
            | Self::F64Mul
            | Self::F64Div
            | Self::F64Eq
            | Self::F64Ne
            | Self::F64Lt
            | Self::F64Gt
            | Self::F64Le
            | Self::F64Ge => ScalarType::F64,
        }
    }

    fn result_ty(self) -> ScalarType {
        match self {
            Self::I32Eq
            | Self::I32Ne
            | Self::I32LtS
            | Self::I32LtU
            | Self::I32GtS
            | Self::I32GtU
            | Self::I32LeS
            | Self::I32LeU
            | Self::I32GeS
            | Self::I32GeU
            | Self::I64Eq
            | Self::I64Ne
            | Self::I64LtS
            | Self::I64LtU
            | Self::I64GtS
            | Self::I64GtU
            | Self::I64LeS
            | Self::I64LeU
            | Self::I64GeS
            | Self::I64GeU
            | Self::F32Eq
            | Self::F32Ne
            | Self::F32Lt
            | Self::F32Gt
            | Self::F32Le
            | Self::F32Ge
            | Self::F64Eq
            | Self::F64Ne
            | Self::F64Lt
            | Self::F64Gt
            | Self::F64Le
            | Self::F64Ge => ScalarType::I32,
            _ => self.input_ty(),
        }
    }

    fn is_commutative(self) -> bool {
        matches!(
            self,
            Self::I32Add
                | Self::I32Mul
                | Self::I32And
                | Self::I32Or
                | Self::I32Xor
                | Self::I32Eq
                | Self::I32Ne
                | Self::I64Add
                | Self::I64Mul
                | Self::I64And
                | Self::I64Or
                | Self::I64Xor
                | Self::I64Eq
                | Self::I64Ne
                | Self::F32Add
                | Self::F32Mul
                | Self::F32Eq
                | Self::F32Ne
                | Self::F64Add
                | Self::F64Mul
                | Self::F64Eq
                | Self::F64Ne
        )
    }

    fn raw_op(self) -> Op {
        match self {
            Self::I32Add => vm::op_i32_add,
            Self::I32Sub => vm::op_i32_sub,
            Self::I32Mul => vm::op_i32_mul,
            Self::I32And => vm::op_i32_and,
            Self::I32Or => vm::op_i32_or,
            Self::I32Xor => vm::op_i32_xor,
            Self::I32Shl => vm::op_i32_shl,
            Self::I32ShrS => vm::op_i32_shr_s,
            Self::I32ShrU => vm::op_i32_shr_u,
            Self::I32Eq => vm::op_i32_eq,
            Self::I32Ne => vm::op_i32_ne,
            Self::I32LtS => vm::op_i32_lt_s,
            Self::I32LtU => vm::op_i32_lt_u,
            Self::I32GtS => vm::op_i32_gt_s,
            Self::I32GtU => vm::op_i32_gt_u,
            Self::I32LeS => vm::op_i32_le_s,
            Self::I32LeU => vm::op_i32_le_u,
            Self::I32GeS => vm::op_i32_ge_s,
            Self::I32GeU => vm::op_i32_ge_u,
            Self::I64Add => vm::op_i64_add,
            Self::I64Sub => vm::op_i64_sub,
            Self::I64Mul => vm::op_i64_mul,
            Self::I64And => vm::op_i64_and,
            Self::I64Or => vm::op_i64_or,
            Self::I64Xor => vm::op_i64_xor,
            Self::I64Shl => vm::op_i64_shl,
            Self::I64ShrS => vm::op_i64_shr_s,
            Self::I64ShrU => vm::op_i64_shr_u,
            Self::I64Eq => vm::op_i64_eq,
            Self::I64Ne => vm::op_i64_ne,
            Self::I64LtS => vm::op_i64_lt_s,
            Self::I64LtU => vm::op_i64_lt_u,
            Self::I64GtS => vm::op_i64_gt_s,
            Self::I64GtU => vm::op_i64_gt_u,
            Self::I64LeS => vm::op_i64_le_s,
            Self::I64LeU => vm::op_i64_le_u,
            Self::I64GeS => vm::op_i64_ge_s,
            Self::I64GeU => vm::op_i64_ge_u,
            Self::F32Add => vm::op_f32_add,
            Self::F32Sub => vm::op_f32_sub,
            Self::F32Mul => vm::op_f32_mul,
            Self::F32Div => vm::op_f32_div,
            Self::F32Eq => vm::op_f32_eq,
            Self::F32Ne => vm::op_f32_ne,
            Self::F32Lt => vm::op_f32_lt,
            Self::F32Gt => vm::op_f32_gt,
            Self::F32Le => vm::op_f32_le,
            Self::F32Ge => vm::op_f32_ge,
            Self::F64Add => vm::op_f64_add,
            Self::F64Sub => vm::op_f64_sub,
            Self::F64Mul => vm::op_f64_mul,
            Self::F64Div => vm::op_f64_div,
            Self::F64Eq => vm::op_f64_eq,
            Self::F64Ne => vm::op_f64_ne,
            Self::F64Lt => vm::op_f64_lt,
            Self::F64Gt => vm::op_f64_gt,
            Self::F64Le => vm::op_f64_le,
            Self::F64Ge => vm::op_f64_ge,
        }
    }
}

#[derive(Clone, Copy)]
enum Expr {
    Const32 {
        ty: ScalarType,
        bits: u32,
    },
    Const64 {
        ty: ScalarType,
        bits: u64,
    },
    LocalGet4(u32),
    LocalGet8(u32),
    Unary {
        kind: UnaryKind,
        child: NodeId,
    },
    Binary {
        kind: BinaryKind,
        left: NodeId,
        right: NodeId,
    },
    TeeValue4 {
        addr: u32,
        value: NodeId,
    },
    TeeValue8 {
        addr: u32,
        value: NodeId,
    },
}

#[derive(Clone, Copy)]
enum LoadKind {
    I32,
    I64,
    F32,
    F64,
    I32Load8S,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    I64Load8S,
    I64Load8U,
    I64Load16S,
    I64Load16U,
    I64Load32S,
    I64Load32U,
}

impl LoadKind {
    fn raw_op(self) -> Op {
        match self {
            Self::I32 => vm::op_i32_load,
            Self::I64 => vm::op_i64_load,
            Self::F32 => vm::op_f32_load,
            Self::F64 => vm::op_f64_load,
            Self::I32Load8S => vm::op_i32_load8_s,
            Self::I32Load8U => vm::op_i32_load8_u,
            Self::I32Load16S => vm::op_i32_load16_s,
            Self::I32Load16U => vm::op_i32_load16_u,
            Self::I64Load8S => vm::op_i64_load8_s,
            Self::I64Load8U => vm::op_i64_load8_u,
            Self::I64Load16S => vm::op_i64_load16_s,
            Self::I64Load16U => vm::op_i64_load16_u,
            Self::I64Load32S => vm::op_i64_load32_s,
            Self::I64Load32U => vm::op_i64_load32_u,
        }
    }

    fn fused_op(self) -> Op {
        match self {
            Self::I32 => vm::op_local_get4_i32_const_add_i32_load,
            Self::I64 => vm::op_local_get4_i32_const_add_i64_load,
            Self::F32 => vm::op_local_get4_i32_const_add_f32_load,
            Self::F64 => vm::op_local_get4_i32_const_add_f64_load,
            Self::I32Load8S => vm::op_local_get4_i32_const_add_i32_load8_s,
            Self::I32Load8U => vm::op_local_get4_i32_const_add_i32_load8_u,
            Self::I32Load16S => vm::op_local_get4_i32_const_add_i32_load16_s,
            Self::I32Load16U => vm::op_local_get4_i32_const_add_i32_load16_u,
            Self::I64Load8S => vm::op_local_get4_i32_const_add_i64_load8_s,
            Self::I64Load8U => vm::op_local_get4_i32_const_add_i64_load8_u,
            Self::I64Load16S => vm::op_local_get4_i32_const_add_i64_load16_s,
            Self::I64Load16U => vm::op_local_get4_i32_const_add_i64_load16_u,
            Self::I64Load32S => vm::op_local_get4_i32_const_add_i64_load32_s,
            Self::I64Load32U => vm::op_local_get4_i32_const_add_i64_load32_u,
        }
    }
}

#[derive(Clone, Copy)]
enum StoreKind {
    I32,
    I64,
    F32,
    F64,
    I32Store8,
    I32Store16,
    I64Store8,
    I64Store16,
    I64Store32,
}

impl StoreKind {
    fn raw_op(self) -> Op {
        match self {
            Self::I32 => vm::op_i32_store,
            Self::I64 => vm::op_i64_store,
            Self::F32 => vm::op_f32_store,
            Self::F64 => vm::op_f64_store,
            Self::I32Store8 => vm::op_i32_store8,
            Self::I32Store16 => vm::op_i32_store16,
            Self::I64Store8 => vm::op_i64_store8,
            Self::I64Store16 => vm::op_i64_store16,
            Self::I64Store32 => vm::op_i64_store32,
        }
    }

    fn fused_op(self) -> Op {
        match self {
            Self::I32 => vm::op_local_get4_i32_const_add_i32_store,
            Self::I64 => vm::op_local_get4_i32_const_add_i64_store,
            Self::F32 => vm::op_local_get4_i32_const_add_f32_store,
            Self::F64 => vm::op_local_get4_i32_const_add_f64_store,
            Self::I32Store8 => vm::op_local_get4_i32_const_add_i32_store8,
            Self::I32Store16 => vm::op_local_get4_i32_const_add_i32_store16,
            Self::I64Store8 => vm::op_local_get4_i32_const_add_i64_store8,
            Self::I64Store16 => vm::op_local_get4_i32_const_add_i64_store16,
            Self::I64Store32 => vm::op_local_get4_i32_const_add_i64_store32,
        }
    }
}

#[derive(Clone, Copy)]
enum Root {
    Set4 {
        addr: u32,
        value: NodeId,
    },
    Set8 {
        addr: u32,
        value: NodeId,
    },
    Drop4(NodeId),
    Drop8(NodeId),
    Push(NodeId),
    If {
        condition: NodeId,
        jump_addr: u32,
    },
    BrIf {
        condition: NodeId,
        jump_addr: u32,
    },
    Return {
        value: Option<NodeId>,
        jump_addr: u32,
    },
    Load {
        kind: LoadKind,
        memarg: MemArg,
        address: NodeId,
    },
    Store {
        kind: StoreKind,
        memarg: MemArg,
        address: NodeId,
        value: NodeId,
    },
}

struct Region {
    exprs: Vec<Expr>,
    roots: Vec<Root>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Cost {
    cells: usize,
    dispatches: usize,
    transient_stack_bytes: usize,
}

#[derive(Clone, Default)]
struct Plan {
    cost: Cost,
    instrs: Vec<Instr>,
}

impl Plan {
    fn from_instrs(instrs: Vec<Instr>, dispatches: usize, transient_stack_bytes: usize) -> Self {
        Self {
            cost: Cost {
                cells: instrs.len(),
                dispatches,
                transient_stack_bytes,
            },
            instrs,
        }
    }

    fn seq(parts: impl IntoIterator<Item = Plan>, transient_stack_bytes: usize) -> Self {
        let mut instrs = Vec::new();
        let mut cost = Cost {
            transient_stack_bytes,
            ..Cost::default()
        };
        for part in parts {
            cost.cells += part.cost.cells;
            cost.dispatches += part.cost.dispatches;
            cost.transient_stack_bytes = cost
                .transient_stack_bytes
                .max(part.cost.transient_stack_bytes);
            instrs.extend(part.instrs);
        }
        Self { cost, instrs }
    }
}

pub(crate) fn emit_fused_region(pending: &[Instr], emitted: &mut Vec<Instr>) {
    if pending.is_empty() {
        return;
    }
    let Some(region) = parse_region(pending) else {
        emitted.extend_from_slice(pending);
        return;
    };
    let mut emitter = Emitter::new(&region.exprs);
    for root in region.roots {
        emitted.extend(emitter.emit_root(root).instrs);
    }
}

fn parse_region(pending: &[Instr]) -> Option<Region> {
    let mut exprs = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<NodeId> = Vec::new();
    let mut index = 0usize;

    while index < pending.len() {
        let op = unsafe { pending[index].op };
        if op_eq(op, vm::op_i32_const) {
            let value = unsafe { pending[index + 1].operand.i32 };
            exprs.push(Expr::Const32 {
                ty: ScalarType::I32,
                bits: value as u32,
            });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_i64_const) {
            let value = unsafe { pending[index + 1].operand.i64 };
            exprs.push(Expr::Const64 {
                ty: ScalarType::I64,
                bits: value as u64,
            });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_f32_const) {
            let value = unsafe { pending[index + 1].operand.f32 };
            exprs.push(Expr::Const32 {
                ty: ScalarType::F32,
                bits: value.to_bits(),
            });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_f64_const) {
            let value = unsafe { pending[index + 1].operand.f64 };
            exprs.push(Expr::Const64 {
                ty: ScalarType::F64,
                bits: value.to_bits(),
            });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_get4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            exprs.push(Expr::LocalGet4(addr));
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_get8) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            exprs.push(Expr::LocalGet8(addr));
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_set4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop()?;
            roots.push(Root::Set4 { addr, value });
            index += 2;
        } else if op_eq(op, vm::op_local_set8) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop()?;
            roots.push(Root::Set8 { addr, value });
            index += 2;
        } else if op_eq(op, vm::op_local_tee4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop()?;
            exprs.push(Expr::TeeValue4 { addr, value });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_tee8) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop()?;
            exprs.push(Expr::TeeValue8 { addr, value });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_drop) {
            let size = unsafe { pending[index + 1].operand.drop_size };
            let value = stack.pop()?;
            match size {
                4 => roots.push(Root::Drop4(value)),
                8 => roots.push(Root::Drop8(value)),
                _ => return None,
            }
            index += 2;
        } else if op_eq(op, vm::op_i32_eqz) {
            let child = stack.pop()?;
            exprs.push(Expr::Unary {
                kind: UnaryKind::I32Eqz,
                child,
            });
            stack.push(exprs.len() - 1);
            index += 1;
        } else if op_eq(op, vm::op_i64_eqz) {
            let child = stack.pop()?;
            exprs.push(Expr::Unary {
                kind: UnaryKind::I64Eqz,
                child,
            });
            stack.push(exprs.len() - 1);
            index += 1;
        } else if op_eq(op, vm::op_if) {
            let condition = stack.pop()?;
            if index + 2 != pending.len() {
                return None;
            }
            for value in stack.drain(..) {
                roots.push(Root::Push(value));
            }
            roots.push(Root::If {
                condition,
                jump_addr: unsafe { pending[index + 1].operand.jump_addr },
            });
            index += 2;
        } else if op_eq(op, vm::op_br_if) {
            let condition = stack.pop()?;
            if index + 2 != pending.len() {
                return None;
            }
            for value in stack.drain(..) {
                roots.push(Root::Push(value));
            }
            roots.push(Root::BrIf {
                condition,
                jump_addr: unsafe { pending[index + 1].operand.jump_addr },
            });
            index += 2;
        } else if op_eq(op, vm::op_return) {
            if index + 2 != pending.len() {
                return None;
            }
            let value = stack.pop();
            if !stack.is_empty() {
                return None;
            }
            roots.push(Root::Return {
                value,
                jump_addr: unsafe { pending[index + 1].operand.jump_addr },
            });
            index += 2;
        } else if let Some(kind) = parse_load_kind(op) {
            let address = stack.pop()?;
            if index + 2 != pending.len() {
                return None;
            }
            for value in stack.drain(..) {
                roots.push(Root::Push(value));
            }
            roots.push(Root::Load {
                kind,
                memarg: unsafe { pending[index + 1].operand.memarg },
                address,
            });
            index += 2;
        } else if let Some(kind) = parse_store_kind(op) {
            let value = stack.pop()?;
            let address = stack.pop()?;
            if index + 2 != pending.len() {
                return None;
            }
            for stacked in stack.drain(..) {
                roots.push(Root::Push(stacked));
            }
            roots.push(Root::Store {
                kind,
                memarg: unsafe { pending[index + 1].operand.memarg },
                address,
                value,
            });
            index += 2;
        } else {
            let kind = parse_binary_kind(op)?;
            let right = stack.pop()?;
            let left = stack.pop()?;
            exprs.push(Expr::Binary { kind, left, right });
            stack.push(exprs.len() - 1);
            index += 1;
        }
    }

    for value in stack {
        roots.push(Root::Push(value));
    }

    Some(Region { exprs, roots })
}

struct Emitter<'a> {
    exprs: &'a [Expr],
    value_memo: Vec<Option<Plan>>,
}

impl<'a> Emitter<'a> {
    fn new(exprs: &'a [Expr]) -> Self {
        Self {
            exprs,
            value_memo: vec![None; exprs.len()],
        }
    }

    fn emit_root(&mut self, root: Root) -> Plan {
        match root {
            Root::Set4 { addr, value } => self.emit_set4(addr, value),
            Root::Set8 { addr, value } => self.emit_set8(addr, value),
            Root::Drop4(value) => self.emit_drop4(value),
            Root::Drop8(value) => self.emit_drop8(value),
            Root::Push(value) => self.emit_value(value),
            Root::If {
                condition,
                jump_addr,
            } => self.emit_if(condition, jump_addr),
            Root::BrIf {
                condition,
                jump_addr,
            } => self.emit_br_if(condition, jump_addr),
            Root::Return { value, jump_addr } => self.emit_return(value, jump_addr),
            Root::Load {
                kind,
                memarg,
                address,
            } => self.emit_load(kind, memarg, address),
            Root::Store {
                kind,
                memarg,
                address,
                value,
            } => self.emit_store(kind, memarg, address, value),
        }
    }

    fn emit_value(&mut self, id: NodeId) -> Plan {
        if let Some(plan) = &self.value_memo[id] {
            return plan.clone();
        }

        let plan = match self.exprs[id] {
            Expr::Const32 { ty, bits } => raw_const32(ty, bits),
            Expr::Const64 { ty, bits } => raw_const64(ty, bits),
            Expr::LocalGet4(addr) => raw_local_get4(addr),
            Expr::LocalGet8(addr) => raw_local_get8(addr),
            Expr::Unary { kind, child } => {
                let transient = kind.result_ty().size();
                Plan::seq([self.emit_value(child), raw_unary(kind)], transient)
            }
            Expr::Binary { kind, left, right } => {
                let raw = Plan::seq(
                    [
                        self.emit_value(left),
                        self.emit_value(right),
                        raw_binary(kind),
                    ],
                    kind.input_ty().size() * 2,
                );
                if let Some(matched) = match_local_const_binary(self.exprs, id) {
                    if let Some(op) = binary_value_opcode(kind) {
                        let fused = matched.fused_value_plan(op);
                        choose_better(raw, fused)
                    } else {
                        raw
                    }
                } else {
                    raw
                }
            }
            Expr::TeeValue4 { addr, value } => {
                let raw = Plan::seq([self.emit_value(value), raw_local_tee4(addr)], 4);
                if let Some(bits) = const_bits32(self.exprs, value) {
                    choose_better(raw, fused_const4_local_tee4(addr, bits))
                } else {
                    raw
                }
            }
            Expr::TeeValue8 { addr, value } => {
                let raw = Plan::seq([self.emit_value(value), raw_local_tee8(addr)], 8);
                if let Some(bits) = const_bits64(self.exprs, value) {
                    choose_better(raw, fused_const8_local_tee8(addr, bits))
                } else {
                    raw
                }
            }
        };

        self.value_memo[id] = Some(plan.clone());
        plan
    }

    fn emit_set4(&mut self, addr: u32, value: NodeId) -> Plan {
        let raw = Plan::seq([self.emit_value(value), raw_local_set4(addr)], 4);
        let best = if let Some(bits) = const_bits32(self.exprs, value) {
            choose_better(raw, fused_const4_local_set4(addr, bits))
        } else {
            raw
        };

        if let Some(matched) = match_local_const_binary(self.exprs, value) {
            if matched.addr == addr {
                if let Some(op) = binary_set_opcode(matched.kind) {
                    return choose_better(best, matched.fused_set_plan(op));
                }
            }
        }
        best
    }

    fn emit_set8(&mut self, addr: u32, value: NodeId) -> Plan {
        let raw = Plan::seq([self.emit_value(value), raw_local_set8(addr)], 8);
        let best = if let Some(bits) = const_bits64(self.exprs, value) {
            choose_better(raw, fused_const8_local_set8(addr, bits))
        } else {
            raw
        };

        if let Some(matched) = match_local_const_binary(self.exprs, value) {
            if matched.addr == addr {
                if let Some(op) = binary_set_opcode(matched.kind) {
                    return choose_better(best, matched.fused_set_plan(op));
                }
            }
        }
        best
    }

    fn emit_drop4(&mut self, value: NodeId) -> Plan {
        match self.exprs[value] {
            Expr::TeeValue4 { addr, value } => self.emit_set4(addr, value),
            _ => Plan::default(),
        }
    }

    fn emit_drop8(&mut self, value: NodeId) -> Plan {
        match self.exprs[value] {
            Expr::TeeValue8 { addr, value } => self.emit_set8(addr, value),
            _ => Plan::default(),
        }
    }

    fn emit_if(&mut self, condition: NodeId, jump_addr: u32) -> Plan {
        let raw = Plan::seq([self.emit_value(condition), raw_if(jump_addr)], 4);
        if let Some(addr) = match_local_get4(self.exprs, condition) {
            return choose_better(
                raw,
                fused_local_get4_branch(vm::op_local_get4_if, addr, jump_addr),
            );
        }
        if let Some(addr) = match_local_get4_i32_eqz(self.exprs, condition) {
            return choose_better(
                raw,
                fused_local_get4_branch(vm::op_local_get4_i32_eqz_if, addr, jump_addr),
            );
        }
        if let Some((addr, bits, op)) = match_i32_compare_branch(self.exprs, condition, true) {
            return choose_better(raw, fused_branch_compare(op, addr, bits, jump_addr));
        }
        raw
    }

    fn emit_br_if(&mut self, condition: NodeId, jump_addr: u32) -> Plan {
        let raw = Plan::seq([self.emit_value(condition), raw_br_if(jump_addr)], 4);
        if let Some(addr) = match_local_get4(self.exprs, condition) {
            return choose_better(
                raw,
                fused_local_get4_branch(vm::op_local_get4_br_if, addr, jump_addr),
            );
        }
        if let Some(addr) = match_local_get4_i32_eqz(self.exprs, condition) {
            return choose_better(
                raw,
                fused_local_get4_branch(vm::op_local_get4_i32_eqz_br_if, addr, jump_addr),
            );
        }
        if let Some((addr, bits, op)) = match_i32_compare_branch(self.exprs, condition, false) {
            return choose_better(raw, fused_branch_compare(op, addr, bits, jump_addr));
        }
        raw
    }

    fn emit_return(&mut self, value: Option<NodeId>, jump_addr: u32) -> Plan {
        match value {
            None => raw_return(jump_addr),
            Some(value) => {
                let raw = Plan::seq(
                    [self.emit_value(value), raw_return(jump_addr)],
                    expr_size(self.exprs, value),
                );
                match self.exprs[value] {
                    Expr::Const32 { bits, .. } => {
                        choose_better(raw, fused_const4_return(bits, jump_addr))
                    }
                    Expr::Const64 { bits, .. } => {
                        choose_better(raw, fused_const8_return(bits, jump_addr))
                    }
                    Expr::LocalGet4(addr) => {
                        choose_better(raw, fused_local_get4_return(addr, jump_addr))
                    }
                    Expr::LocalGet8(addr) => {
                        choose_better(raw, fused_local_get8_return(addr, jump_addr))
                    }
                    _ => raw,
                }
            }
        }
    }

    fn emit_load(&mut self, kind: LoadKind, memarg: MemArg, address: NodeId) -> Plan {
        let raw = Plan::seq([self.emit_value(address), raw_load(kind, memarg)], 4);
        if let Some((addr, imm)) = match_i32_address(self.exprs, address) {
            choose_better(raw, fused_load(kind, addr, imm, memarg))
        } else {
            raw
        }
    }

    fn emit_store(
        &mut self,
        kind: StoreKind,
        memarg: MemArg,
        address: NodeId,
        value: NodeId,
    ) -> Plan {
        let raw = Plan::seq(
            [
                self.emit_value(address),
                self.emit_value(value),
                raw_store(kind, memarg),
            ],
            12,
        );
        if let Some((addr, imm)) = match_i32_address(self.exprs, address) {
            choose_better(
                raw,
                Plan::seq(
                    [self.emit_value(value), fused_store(kind, addr, imm, memarg)],
                    expr_size(self.exprs, value),
                ),
            )
        } else {
            raw
        }
    }
}

fn expr_size(exprs: &[Expr], id: NodeId) -> usize {
    match exprs[id] {
        Expr::Const32 { .. } | Expr::LocalGet4(_) | Expr::Unary { .. } => 4,
        Expr::Const64 { .. } | Expr::LocalGet8(_) => 8,
        Expr::Binary { kind, .. } => kind.result_ty().size(),
        Expr::TeeValue4 { .. } => 4,
        Expr::TeeValue8 { .. } => 8,
    }
}

fn const_bits32(exprs: &[Expr], id: NodeId) -> Option<u32> {
    match exprs[id] {
        Expr::Const32 { bits, .. } => Some(bits),
        _ => None,
    }
}

fn const_bits64(exprs: &[Expr], id: NodeId) -> Option<u64> {
    match exprs[id] {
        Expr::Const64 { bits, .. } => Some(bits),
        _ => None,
    }
}

#[derive(Clone, Copy)]
struct LocalConstMatch {
    kind: BinaryKind,
    addr: u32,
    imm32: Option<u32>,
    imm64: Option<u64>,
}

impl LocalConstMatch {
    fn fused_value_plan(self, op: Op) -> Plan {
        match (self.imm32, self.imm64) {
            (Some(bits), None) => {
                fused_local_const32(op, self.addr, bits, self.kind.result_ty().size())
            }
            (None, Some(bits)) => {
                fused_local_const64(op, self.addr, bits, self.kind.result_ty().size())
            }
            _ => unreachable!("local const match must have exactly one immediate width"),
        }
    }

    fn fused_set_plan(self, op: Op) -> Plan {
        match (self.imm32, self.imm64) {
            (Some(bits), None) => fused_local_const32(op, self.addr, bits, 0),
            (None, Some(bits)) => fused_local_const64(op, self.addr, bits, 0),
            _ => unreachable!("local const match must have exactly one immediate width"),
        }
    }
}

fn match_local_const_binary(exprs: &[Expr], id: NodeId) -> Option<LocalConstMatch> {
    let Expr::Binary { kind, left, right } = exprs[id] else {
        return None;
    };

    if let Some(found) = match_local_const_binary_ordered(exprs, kind, left, right) {
        return Some(found);
    }

    if kind.is_commutative() {
        return match_local_const_binary_ordered(exprs, kind, right, left);
    }

    None
}

fn match_local_const_binary_ordered(
    exprs: &[Expr],
    kind: BinaryKind,
    local_id: NodeId,
    const_id: NodeId,
) -> Option<LocalConstMatch> {
    match (exprs[local_id], exprs[const_id], kind.input_ty()) {
        (Expr::LocalGet4(addr), Expr::Const32 { ty, bits }, input_ty)
            if input_ty == ty && input_ty.size() == 4 =>
        {
            Some(LocalConstMatch {
                kind,
                addr,
                imm32: Some(bits),
                imm64: None,
            })
        }
        (Expr::LocalGet8(addr), Expr::Const64 { ty, bits }, input_ty)
            if input_ty == ty && input_ty.size() == 8 =>
        {
            Some(LocalConstMatch {
                kind,
                addr,
                imm32: None,
                imm64: Some(bits),
            })
        }
        _ => None,
    }
}

fn match_local_get4(exprs: &[Expr], id: NodeId) -> Option<u32> {
    match exprs[id] {
        Expr::LocalGet4(addr) => Some(addr),
        _ => None,
    }
}

fn match_local_get4_i32_eqz(exprs: &[Expr], id: NodeId) -> Option<u32> {
    match exprs[id] {
        Expr::Unary {
            kind: UnaryKind::I32Eqz,
            child,
        } => match exprs[child] {
            Expr::LocalGet4(addr) => Some(addr),
            _ => None,
        },
        _ => None,
    }
}

fn match_i32_compare_branch(exprs: &[Expr], id: NodeId, for_if: bool) -> Option<(u32, u32, Op)> {
    let matched = match_local_const_binary(exprs, id)?;
    let op = match (matched.kind, for_if) {
        (BinaryKind::I32Eq, true) => vm::op_local_get4_i32_const_eq_if,
        (BinaryKind::I32Eq, false) => vm::op_local_get4_i32_const_eq_br_if,
        (BinaryKind::I32Ne, true) => vm::op_local_get4_i32_const_ne_if,
        (BinaryKind::I32Ne, false) => vm::op_local_get4_i32_const_ne_br_if,
        (BinaryKind::I32LtS, true) => vm::op_local_get4_i32_const_lt_s_if,
        (BinaryKind::I32LtS, false) => vm::op_local_get4_i32_const_lt_s_br_if,
        (BinaryKind::I32LtU, true) => vm::op_local_get4_i32_const_lt_u_if,
        (BinaryKind::I32LtU, false) => vm::op_local_get4_i32_const_lt_u_br_if,
        (BinaryKind::I32GtS, true) => vm::op_local_get4_i32_const_gt_s_if,
        (BinaryKind::I32GtS, false) => vm::op_local_get4_i32_const_gt_s_br_if,
        (BinaryKind::I32GtU, true) => vm::op_local_get4_i32_const_gt_u_if,
        (BinaryKind::I32GtU, false) => vm::op_local_get4_i32_const_gt_u_br_if,
        (BinaryKind::I32LeS, true) => vm::op_local_get4_i32_const_le_s_if,
        (BinaryKind::I32LeS, false) => vm::op_local_get4_i32_const_le_s_br_if,
        (BinaryKind::I32LeU, true) => vm::op_local_get4_i32_const_le_u_if,
        (BinaryKind::I32LeU, false) => vm::op_local_get4_i32_const_le_u_br_if,
        (BinaryKind::I32GeS, true) => vm::op_local_get4_i32_const_ge_s_if,
        (BinaryKind::I32GeS, false) => vm::op_local_get4_i32_const_ge_s_br_if,
        (BinaryKind::I32GeU, true) => vm::op_local_get4_i32_const_ge_u_if,
        (BinaryKind::I32GeU, false) => vm::op_local_get4_i32_const_ge_u_br_if,
        _ => return None,
    };
    Some((matched.addr, matched.imm32?, op))
}

fn match_i32_address(exprs: &[Expr], id: NodeId) -> Option<(u32, i32)> {
    match exprs[id] {
        Expr::LocalGet4(addr) => Some((addr, 0)),
        Expr::Binary {
            kind: BinaryKind::I32Add,
            left,
            right,
        } => {
            if let (
                Expr::LocalGet4(addr),
                Expr::Const32 {
                    ty: ScalarType::I32,
                    bits,
                },
            ) = (exprs[left], exprs[right])
            {
                return Some((addr, bits as i32));
            }
            if let (
                Expr::Const32 {
                    ty: ScalarType::I32,
                    bits,
                },
                Expr::LocalGet4(addr),
            ) = (exprs[left], exprs[right])
            {
                return Some((addr, bits as i32));
            }
            None
        }
        _ => None,
    }
}

fn parse_binary_kind(op: Op) -> Option<BinaryKind> {
    if op_eq(op, vm::op_i32_add) {
        Some(BinaryKind::I32Add)
    } else if op_eq(op, vm::op_i32_sub) {
        Some(BinaryKind::I32Sub)
    } else if op_eq(op, vm::op_i32_mul) {
        Some(BinaryKind::I32Mul)
    } else if op_eq(op, vm::op_i32_and) {
        Some(BinaryKind::I32And)
    } else if op_eq(op, vm::op_i32_or) {
        Some(BinaryKind::I32Or)
    } else if op_eq(op, vm::op_i32_xor) {
        Some(BinaryKind::I32Xor)
    } else if op_eq(op, vm::op_i32_shl) {
        Some(BinaryKind::I32Shl)
    } else if op_eq(op, vm::op_i32_shr_s) {
        Some(BinaryKind::I32ShrS)
    } else if op_eq(op, vm::op_i32_shr_u) {
        Some(BinaryKind::I32ShrU)
    } else if op_eq(op, vm::op_i32_eq) {
        Some(BinaryKind::I32Eq)
    } else if op_eq(op, vm::op_i32_ne) {
        Some(BinaryKind::I32Ne)
    } else if op_eq(op, vm::op_i32_lt_s) {
        Some(BinaryKind::I32LtS)
    } else if op_eq(op, vm::op_i32_lt_u) {
        Some(BinaryKind::I32LtU)
    } else if op_eq(op, vm::op_i32_gt_s) {
        Some(BinaryKind::I32GtS)
    } else if op_eq(op, vm::op_i32_gt_u) {
        Some(BinaryKind::I32GtU)
    } else if op_eq(op, vm::op_i32_le_s) {
        Some(BinaryKind::I32LeS)
    } else if op_eq(op, vm::op_i32_le_u) {
        Some(BinaryKind::I32LeU)
    } else if op_eq(op, vm::op_i32_ge_s) {
        Some(BinaryKind::I32GeS)
    } else if op_eq(op, vm::op_i32_ge_u) {
        Some(BinaryKind::I32GeU)
    } else if op_eq(op, vm::op_i64_add) {
        Some(BinaryKind::I64Add)
    } else if op_eq(op, vm::op_i64_sub) {
        Some(BinaryKind::I64Sub)
    } else if op_eq(op, vm::op_i64_mul) {
        Some(BinaryKind::I64Mul)
    } else if op_eq(op, vm::op_i64_and) {
        Some(BinaryKind::I64And)
    } else if op_eq(op, vm::op_i64_or) {
        Some(BinaryKind::I64Or)
    } else if op_eq(op, vm::op_i64_xor) {
        Some(BinaryKind::I64Xor)
    } else if op_eq(op, vm::op_i64_shl) {
        Some(BinaryKind::I64Shl)
    } else if op_eq(op, vm::op_i64_shr_s) {
        Some(BinaryKind::I64ShrS)
    } else if op_eq(op, vm::op_i64_shr_u) {
        Some(BinaryKind::I64ShrU)
    } else if op_eq(op, vm::op_i64_eq) {
        Some(BinaryKind::I64Eq)
    } else if op_eq(op, vm::op_i64_ne) {
        Some(BinaryKind::I64Ne)
    } else if op_eq(op, vm::op_i64_lt_s) {
        Some(BinaryKind::I64LtS)
    } else if op_eq(op, vm::op_i64_lt_u) {
        Some(BinaryKind::I64LtU)
    } else if op_eq(op, vm::op_i64_gt_s) {
        Some(BinaryKind::I64GtS)
    } else if op_eq(op, vm::op_i64_gt_u) {
        Some(BinaryKind::I64GtU)
    } else if op_eq(op, vm::op_i64_le_s) {
        Some(BinaryKind::I64LeS)
    } else if op_eq(op, vm::op_i64_le_u) {
        Some(BinaryKind::I64LeU)
    } else if op_eq(op, vm::op_i64_ge_s) {
        Some(BinaryKind::I64GeS)
    } else if op_eq(op, vm::op_i64_ge_u) {
        Some(BinaryKind::I64GeU)
    } else if op_eq(op, vm::op_f32_add) {
        Some(BinaryKind::F32Add)
    } else if op_eq(op, vm::op_f32_sub) {
        Some(BinaryKind::F32Sub)
    } else if op_eq(op, vm::op_f32_mul) {
        Some(BinaryKind::F32Mul)
    } else if op_eq(op, vm::op_f32_div) {
        Some(BinaryKind::F32Div)
    } else if op_eq(op, vm::op_f32_eq) {
        Some(BinaryKind::F32Eq)
    } else if op_eq(op, vm::op_f32_ne) {
        Some(BinaryKind::F32Ne)
    } else if op_eq(op, vm::op_f32_lt) {
        Some(BinaryKind::F32Lt)
    } else if op_eq(op, vm::op_f32_gt) {
        Some(BinaryKind::F32Gt)
    } else if op_eq(op, vm::op_f32_le) {
        Some(BinaryKind::F32Le)
    } else if op_eq(op, vm::op_f32_ge) {
        Some(BinaryKind::F32Ge)
    } else if op_eq(op, vm::op_f64_add) {
        Some(BinaryKind::F64Add)
    } else if op_eq(op, vm::op_f64_sub) {
        Some(BinaryKind::F64Sub)
    } else if op_eq(op, vm::op_f64_mul) {
        Some(BinaryKind::F64Mul)
    } else if op_eq(op, vm::op_f64_div) {
        Some(BinaryKind::F64Div)
    } else if op_eq(op, vm::op_f64_eq) {
        Some(BinaryKind::F64Eq)
    } else if op_eq(op, vm::op_f64_ne) {
        Some(BinaryKind::F64Ne)
    } else if op_eq(op, vm::op_f64_lt) {
        Some(BinaryKind::F64Lt)
    } else if op_eq(op, vm::op_f64_gt) {
        Some(BinaryKind::F64Gt)
    } else if op_eq(op, vm::op_f64_le) {
        Some(BinaryKind::F64Le)
    } else if op_eq(op, vm::op_f64_ge) {
        Some(BinaryKind::F64Ge)
    } else {
        None
    }
}

fn parse_load_kind(op: Op) -> Option<LoadKind> {
    if op_eq(op, vm::op_i32_load) {
        Some(LoadKind::I32)
    } else if op_eq(op, vm::op_i64_load) {
        Some(LoadKind::I64)
    } else if op_eq(op, vm::op_f32_load) {
        Some(LoadKind::F32)
    } else if op_eq(op, vm::op_f64_load) {
        Some(LoadKind::F64)
    } else if op_eq(op, vm::op_i32_load8_s) {
        Some(LoadKind::I32Load8S)
    } else if op_eq(op, vm::op_i32_load8_u) {
        Some(LoadKind::I32Load8U)
    } else if op_eq(op, vm::op_i32_load16_s) {
        Some(LoadKind::I32Load16S)
    } else if op_eq(op, vm::op_i32_load16_u) {
        Some(LoadKind::I32Load16U)
    } else if op_eq(op, vm::op_i64_load8_s) {
        Some(LoadKind::I64Load8S)
    } else if op_eq(op, vm::op_i64_load8_u) {
        Some(LoadKind::I64Load8U)
    } else if op_eq(op, vm::op_i64_load16_s) {
        Some(LoadKind::I64Load16S)
    } else if op_eq(op, vm::op_i64_load16_u) {
        Some(LoadKind::I64Load16U)
    } else if op_eq(op, vm::op_i64_load32_s) {
        Some(LoadKind::I64Load32S)
    } else if op_eq(op, vm::op_i64_load32_u) {
        Some(LoadKind::I64Load32U)
    } else {
        None
    }
}

fn parse_store_kind(op: Op) -> Option<StoreKind> {
    if op_eq(op, vm::op_i32_store) {
        Some(StoreKind::I32)
    } else if op_eq(op, vm::op_i64_store) {
        Some(StoreKind::I64)
    } else if op_eq(op, vm::op_f32_store) {
        Some(StoreKind::F32)
    } else if op_eq(op, vm::op_f64_store) {
        Some(StoreKind::F64)
    } else if op_eq(op, vm::op_i32_store8) {
        Some(StoreKind::I32Store8)
    } else if op_eq(op, vm::op_i32_store16) {
        Some(StoreKind::I32Store16)
    } else if op_eq(op, vm::op_i64_store8) {
        Some(StoreKind::I64Store8)
    } else if op_eq(op, vm::op_i64_store16) {
        Some(StoreKind::I64Store16)
    } else if op_eq(op, vm::op_i64_store32) {
        Some(StoreKind::I64Store32)
    } else {
        None
    }
}

fn binary_value_opcode(kind: BinaryKind) -> Option<Op> {
    Some(match kind {
        BinaryKind::I32Add => vm::op_local_get4_i32_const_add,
        BinaryKind::I32Sub => vm::op_local_get4_i32_const_sub,
        BinaryKind::I32Mul => vm::op_local_get4_i32_const_mul,
        BinaryKind::I32And => vm::op_local_get4_i32_const_and,
        BinaryKind::I32Or => vm::op_local_get4_i32_const_or,
        BinaryKind::I32Xor => vm::op_local_get4_i32_const_xor,
        BinaryKind::I32Shl => vm::op_local_get4_i32_const_shl,
        BinaryKind::I32ShrS => vm::op_local_get4_i32_const_shr_s,
        BinaryKind::I32ShrU => vm::op_local_get4_i32_const_shr_u,
        BinaryKind::I32Eq => vm::op_local_get4_i32_const_eq,
        BinaryKind::I32Ne => vm::op_local_get4_i32_const_ne,
        BinaryKind::I32LtS => vm::op_local_get4_i32_const_lt_s,
        BinaryKind::I32LtU => vm::op_local_get4_i32_const_lt_u,
        BinaryKind::I32GtS => vm::op_local_get4_i32_const_gt_s,
        BinaryKind::I32GtU => vm::op_local_get4_i32_const_gt_u,
        BinaryKind::I32LeS => vm::op_local_get4_i32_const_le_s,
        BinaryKind::I32LeU => vm::op_local_get4_i32_const_le_u,
        BinaryKind::I32GeS => vm::op_local_get4_i32_const_ge_s,
        BinaryKind::I32GeU => vm::op_local_get4_i32_const_ge_u,
        BinaryKind::I64Add => vm::op_local_get8_i64_const_add,
        BinaryKind::I64Sub => vm::op_local_get8_i64_const_sub,
        BinaryKind::I64Mul => vm::op_local_get8_i64_const_mul,
        BinaryKind::I64And => vm::op_local_get8_i64_const_and,
        BinaryKind::I64Or => vm::op_local_get8_i64_const_or,
        BinaryKind::I64Xor => vm::op_local_get8_i64_const_xor,
        BinaryKind::I64Shl => vm::op_local_get8_i64_const_shl,
        BinaryKind::I64ShrS => vm::op_local_get8_i64_const_shr_s,
        BinaryKind::I64ShrU => vm::op_local_get8_i64_const_shr_u,
        BinaryKind::I64Eq => vm::op_local_get8_i64_const_eq,
        BinaryKind::I64Ne => vm::op_local_get8_i64_const_ne,
        BinaryKind::I64LtS => vm::op_local_get8_i64_const_lt_s,
        BinaryKind::I64LtU => vm::op_local_get8_i64_const_lt_u,
        BinaryKind::I64GtS => vm::op_local_get8_i64_const_gt_s,
        BinaryKind::I64GtU => vm::op_local_get8_i64_const_gt_u,
        BinaryKind::I64LeS => vm::op_local_get8_i64_const_le_s,
        BinaryKind::I64LeU => vm::op_local_get8_i64_const_le_u,
        BinaryKind::I64GeS => vm::op_local_get8_i64_const_ge_s,
        BinaryKind::I64GeU => vm::op_local_get8_i64_const_ge_u,
        BinaryKind::F32Add => vm::op_local_get4_f32_const_add,
        BinaryKind::F32Sub => vm::op_local_get4_f32_const_sub,
        BinaryKind::F32Mul => vm::op_local_get4_f32_const_mul,
        BinaryKind::F32Div => vm::op_local_get4_f32_const_div,
        BinaryKind::F32Eq => vm::op_local_get4_f32_const_eq,
        BinaryKind::F32Ne => vm::op_local_get4_f32_const_ne,
        BinaryKind::F32Lt => vm::op_local_get4_f32_const_lt,
        BinaryKind::F32Gt => vm::op_local_get4_f32_const_gt,
        BinaryKind::F32Le => vm::op_local_get4_f32_const_le,
        BinaryKind::F32Ge => vm::op_local_get4_f32_const_ge,
        BinaryKind::F64Add => vm::op_local_get8_f64_const_add,
        BinaryKind::F64Sub => vm::op_local_get8_f64_const_sub,
        BinaryKind::F64Mul => vm::op_local_get8_f64_const_mul,
        BinaryKind::F64Div => vm::op_local_get8_f64_const_div,
        BinaryKind::F64Eq => vm::op_local_get8_f64_const_eq,
        BinaryKind::F64Ne => vm::op_local_get8_f64_const_ne,
        BinaryKind::F64Lt => vm::op_local_get8_f64_const_lt,
        BinaryKind::F64Gt => vm::op_local_get8_f64_const_gt,
        BinaryKind::F64Le => vm::op_local_get8_f64_const_le,
        BinaryKind::F64Ge => vm::op_local_get8_f64_const_ge,
    })
}

fn binary_set_opcode(kind: BinaryKind) -> Option<Op> {
    Some(match kind {
        BinaryKind::I32Add => vm::op_local_get4_i32_const_add_local_set4,
        BinaryKind::I32Sub => vm::op_local_get4_i32_const_sub_local_set4,
        BinaryKind::I32Mul => vm::op_local_get4_i32_const_mul_local_set4,
        BinaryKind::I32And => vm::op_local_get4_i32_const_and_local_set4,
        BinaryKind::I32Or => vm::op_local_get4_i32_const_or_local_set4,
        BinaryKind::I32Xor => vm::op_local_get4_i32_const_xor_local_set4,
        BinaryKind::I32Shl => vm::op_local_get4_i32_const_shl_local_set4,
        BinaryKind::I32ShrS => vm::op_local_get4_i32_const_shr_s_local_set4,
        BinaryKind::I32ShrU => vm::op_local_get4_i32_const_shr_u_local_set4,
        BinaryKind::I32Eq => vm::op_local_get4_i32_const_eq_local_set4,
        BinaryKind::I32Ne => vm::op_local_get4_i32_const_ne_local_set4,
        BinaryKind::I32LtS => vm::op_local_get4_i32_const_lt_s_local_set4,
        BinaryKind::I32LtU => vm::op_local_get4_i32_const_lt_u_local_set4,
        BinaryKind::I32GtS => vm::op_local_get4_i32_const_gt_s_local_set4,
        BinaryKind::I32GtU => vm::op_local_get4_i32_const_gt_u_local_set4,
        BinaryKind::I32LeS => vm::op_local_get4_i32_const_le_s_local_set4,
        BinaryKind::I32LeU => vm::op_local_get4_i32_const_le_u_local_set4,
        BinaryKind::I32GeS => vm::op_local_get4_i32_const_ge_s_local_set4,
        BinaryKind::I32GeU => vm::op_local_get4_i32_const_ge_u_local_set4,
        BinaryKind::I64Add => vm::op_local_get8_i64_const_add_local_set8,
        BinaryKind::I64Sub => vm::op_local_get8_i64_const_sub_local_set8,
        BinaryKind::I64Mul => vm::op_local_get8_i64_const_mul_local_set8,
        BinaryKind::I64And => vm::op_local_get8_i64_const_and_local_set8,
        BinaryKind::I64Or => vm::op_local_get8_i64_const_or_local_set8,
        BinaryKind::I64Xor => vm::op_local_get8_i64_const_xor_local_set8,
        BinaryKind::I64Shl => vm::op_local_get8_i64_const_shl_local_set8,
        BinaryKind::I64ShrS => vm::op_local_get8_i64_const_shr_s_local_set8,
        BinaryKind::I64ShrU => vm::op_local_get8_i64_const_shr_u_local_set8,
        BinaryKind::F32Add => vm::op_local_get4_f32_const_add_local_set4,
        BinaryKind::F32Sub => vm::op_local_get4_f32_const_sub_local_set4,
        BinaryKind::F32Mul => vm::op_local_get4_f32_const_mul_local_set4,
        BinaryKind::F32Div => vm::op_local_get4_f32_const_div_local_set4,
        BinaryKind::F64Add => vm::op_local_get8_f64_const_add_local_set8,
        BinaryKind::F64Sub => vm::op_local_get8_f64_const_sub_local_set8,
        BinaryKind::F64Mul => vm::op_local_get8_f64_const_mul_local_set8,
        BinaryKind::F64Div => vm::op_local_get8_f64_const_div_local_set8,
        _ => return None,
    })
}

fn raw_const32(ty: ScalarType, bits: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: match ty {
                    ScalarType::I32 => vm::op_i32_const,
                    ScalarType::F32 => vm::op_f32_const,
                    _ => unreachable!("invalid 32-bit scalar type"),
                },
            },
            Instr {
                operand: match ty {
                    ScalarType::I32 => Operand { i32: bits as i32 },
                    ScalarType::F32 => Operand {
                        f32: f32::from_bits(bits),
                    },
                    _ => unreachable!("invalid 32-bit scalar type"),
                },
            },
        ],
        1,
        4,
    )
}

fn raw_const64(ty: ScalarType, bits: u64) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: match ty {
                    ScalarType::I64 => vm::op_i64_const,
                    ScalarType::F64 => vm::op_f64_const,
                    _ => unreachable!("invalid 64-bit scalar type"),
                },
            },
            Instr {
                operand: match ty {
                    ScalarType::I64 => Operand { i64: bits as i64 },
                    ScalarType::F64 => Operand {
                        f64: f64::from_bits(bits),
                    },
                    _ => unreachable!("invalid 64-bit scalar type"),
                },
            },
        ],
        1,
        8,
    )
}

fn raw_local_get4(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_get4,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        4,
    )
}

fn raw_local_get8(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_get8,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        8,
    )
}

fn raw_local_set4(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_set4,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        4,
    )
}

fn raw_local_set8(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_set8,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        8,
    )
}

fn raw_local_tee4(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_tee4,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        4,
    )
}

fn raw_local_tee8(addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_tee8,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
        ],
        1,
        8,
    )
}

fn raw_unary(kind: UnaryKind) -> Plan {
    Plan::from_instrs(
        vec![Instr { op: kind.raw_op() }],
        1,
        kind.result_ty().size(),
    )
}

fn raw_binary(kind: BinaryKind) -> Plan {
    Plan::from_instrs(
        vec![Instr { op: kind.raw_op() }],
        1,
        kind.input_ty().size() * 2,
    )
}

fn raw_if(jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op: vm::op_if },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        4,
    )
}

fn raw_br_if(jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op: vm::op_br_if },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        4,
    )
}

fn raw_return(jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op: vm::op_return },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        0,
    )
}

fn raw_load(kind: LoadKind, memarg: MemArg) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op: kind.raw_op() },
            Instr {
                operand: Operand { memarg },
            },
        ],
        1,
        4,
    )
}

fn raw_store(kind: StoreKind, memarg: MemArg) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op: kind.raw_op() },
            Instr {
                operand: Operand { memarg },
            },
        ],
        1,
        4,
    )
}

fn pack_local_u32(addr: u32, bits: u32) -> Operand {
    let mut encoded = [0u8; 8];
    encoded[..4].copy_from_slice(&addr.to_le_bytes());
    encoded[4..].copy_from_slice(&bits.to_le_bytes());
    Operand { encoded }
}

fn pack_local_i32(addr: u32, imm: i32) -> Operand {
    pack_local_u32(addr, imm as u32)
}

fn fused_local_const32(op: Op, addr: u32, bits: u32, transient_stack_bytes: usize) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op },
            Instr {
                operand: pack_local_u32(addr, bits),
            },
        ],
        1,
        transient_stack_bytes,
    )
}

fn fused_local_const64(op: Op, addr: u32, bits: u64, transient_stack_bytes: usize) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op },
            Instr {
                operand: Operand { local_addr: addr },
            },
            Instr {
                operand: Operand { u64: bits },
            },
        ],
        1,
        transient_stack_bytes,
    )
}

fn fused_const4_local_set4(addr: u32, bits: u32) -> Plan {
    fused_local_const32(vm::op_const4_local_set4, addr, bits, 0)
}

fn fused_const4_local_tee4(addr: u32, bits: u32) -> Plan {
    fused_local_const32(vm::op_const4_local_tee4, addr, bits, 4)
}

fn fused_const8_local_set8(addr: u32, bits: u64) -> Plan {
    fused_local_const64(vm::op_const8_local_set8, addr, bits, 0)
}

fn fused_const8_local_tee8(addr: u32, bits: u64) -> Plan {
    fused_local_const64(vm::op_const8_local_tee8, addr, bits, 8)
}

fn fused_const4_return(bits: u32, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_const4_return,
            },
            Instr {
                operand: Operand { u32: bits },
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        4,
    )
}

fn fused_const8_return(bits: u64, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_const8_return,
            },
            Instr {
                operand: Operand { u64: bits },
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        8,
    )
}

fn fused_local_get4_return(addr: u32, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_get4_return,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        4,
    )
}

fn fused_local_get8_return(addr: u32, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_local_get8_return,
            },
            Instr {
                operand: Operand { local_addr: addr },
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        8,
    )
}

fn fused_local_get4_branch(op: Op, addr: u32, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op },
            Instr {
                operand: Operand { local_addr: addr },
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        0,
    )
}

fn fused_branch_compare(op: Op, addr: u32, bits: u32, jump_addr: u32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op },
            Instr {
                operand: pack_local_u32(addr, bits),
            },
            Instr {
                operand: Operand { jump_addr },
            },
        ],
        1,
        0,
    )
}

fn fused_load(kind: LoadKind, addr: u32, imm: i32, memarg: MemArg) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: kind.fused_op(),
            },
            Instr {
                operand: pack_local_i32(addr, imm),
            },
            Instr {
                operand: Operand { memarg },
            },
        ],
        1,
        0,
    )
}

fn fused_store(kind: StoreKind, addr: u32, imm: i32, memarg: MemArg) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: kind.fused_op(),
            },
            Instr {
                operand: pack_local_i32(addr, imm),
            },
            Instr {
                operand: Operand { memarg },
            },
        ],
        1,
        0,
    )
}

fn choose_better(current: Plan, candidate: Plan) -> Plan {
    if candidate.cost < current.cost {
        candidate
    } else {
        current
    }
}

fn op_eq(lhs: Op, rhs: Op) -> bool {
    lhs as usize == rhs as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn emitted_len(pending: &[Instr]) -> usize {
        let mut emitted = Vec::new();
        emit_fused_region(pending, &mut emitted);
        emitted.len()
    }

    #[test]
    fn const_local_set4_is_fused() {
        let pending = vec![
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: 7 },
            },
            Instr {
                op: vm::op_local_set4,
            },
            Instr {
                operand: Operand { local_addr: 0 },
            },
        ];
        assert_eq!(emitted_len(&pending), 2);
    }

    #[test]
    fn local_get_const_add_load_is_fused() {
        let pending = vec![
            Instr {
                op: vm::op_local_get4,
            },
            Instr {
                operand: Operand { local_addr: 0 },
            },
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: 4 },
            },
            Instr { op: vm::op_i32_add },
            Instr {
                op: vm::op_i32_load,
            },
            Instr {
                operand: Operand {
                    memarg: MemArg {
                        align: 4,
                        offset: 0,
                    },
                },
            },
        ];
        assert_eq!(emitted_len(&pending), 3);
    }

    #[test]
    fn local_get_eqz_if_is_fused() {
        let pending = vec![
            Instr {
                op: vm::op_local_get4,
            },
            Instr {
                operand: Operand { local_addr: 0 },
            },
            Instr { op: vm::op_i32_eqz },
            Instr { op: vm::op_if },
            Instr {
                operand: Operand { jump_addr: 9 },
            },
        ];
        assert_eq!(emitted_len(&pending), 3);
    }
}
