use crate::{
    common::{Instr, Op, Operand},
    runtime::vm,
};

#[derive(Clone, Copy)]
enum UnaryKind {
    Eqz,
}

#[derive(Clone, Copy)]
enum BinaryKind {
    Add,
    Sub,
    Mul,
    And,
    Or,
    Xor,
    Shl,
    ShrS,
    ShrU,
    Eq,
    Ne,
}

type NodeId = usize;

#[derive(Clone, Copy)]
enum Expr {
    ConstI32(i32),
    LocalGet4(u32),
    Unary {
        kind: UnaryKind,
        child: NodeId,
    },
    Binary {
        kind: BinaryKind,
        left: NodeId,
        right: NodeId,
    },
    TeeValue {
        addr: u32,
        value: NodeId,
    },
}

#[derive(Clone, Copy)]
enum Root {
    Set { addr: u32, value: NodeId },
    Drop(NodeId),
    Push(NodeId),
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
    let region = parse_region(pending);
    let mut emitter = Emitter::new(&region.exprs);
    for root in region.roots {
        emitted.extend(emitter.emit_root(root).instrs);
    }
}

fn parse_region(pending: &[Instr]) -> Region {
    let mut exprs = Vec::new();
    let mut roots = Vec::new();
    let mut stack: Vec<NodeId> = Vec::new();
    let mut index = 0usize;

    while index < pending.len() {
        let op = unsafe { pending[index].op };
        if op_eq(op, vm::op_i32_const) {
            let value = unsafe { pending[index + 1].operand.i32 };
            exprs.push(Expr::ConstI32(value));
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_get4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            exprs.push(Expr::LocalGet4(addr));
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_local_set4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop().expect("local.set4 requires one value");
            roots.push(Root::Set { addr, value });
            index += 2;
        } else if op_eq(op, vm::op_local_tee4) {
            let addr = unsafe { pending[index + 1].operand.local_addr };
            let value = stack.pop().expect("local.tee4 requires one value");
            exprs.push(Expr::TeeValue { addr, value });
            stack.push(exprs.len() - 1);
            index += 2;
        } else if op_eq(op, vm::op_drop) {
            let size = unsafe { pending[index + 1].operand.drop_size };
            debug_assert_eq!(size, 4);
            let value = stack.pop().expect("drop requires one value");
            roots.push(Root::Drop(value));
            index += 2;
        } else if op_eq(op, vm::op_i32_eqz) {
            let child = stack.pop().expect("i32.eqz requires one value");
            exprs.push(Expr::Unary {
                kind: UnaryKind::Eqz,
                child,
            });
            stack.push(exprs.len() - 1);
            index += 1;
        } else {
            let kind = parse_binary_kind(op)
                .unwrap_or_else(|| panic!("unexpected pending opcode in fusion region"));
            let right = stack.pop().expect("binary op requires rhs");
            let left = stack.pop().expect("binary op requires lhs");
            exprs.push(Expr::Binary { kind, left, right });
            stack.push(exprs.len() - 1);
            index += 1;
        }
    }

    for value in stack {
        roots.push(Root::Push(value));
    }

    Region { exprs, roots }
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
            Root::Set { addr, value } => self.emit_set(addr, value),
            Root::Drop(value) => self.emit_drop(value),
            Root::Push(value) => self.emit_value(value),
        }
    }

    fn emit_value(&mut self, id: NodeId) -> Plan {
        if let Some(plan) = &self.value_memo[id] {
            return plan.clone();
        }

        let plan = match self.exprs[id] {
            Expr::ConstI32(value) => raw_const_i32(value),
            Expr::LocalGet4(addr) => raw_local_get4(addr),
            Expr::Unary {
                kind: UnaryKind::Eqz,
                child,
            } => Plan::seq([self.emit_value(child), raw_unary_eqz()], 4),
            Expr::Binary { kind, left, right } => {
                let mut best = Plan::seq(
                    [
                        self.emit_value(left),
                        self.emit_value(right),
                        raw_binary(kind),
                    ],
                    8,
                );

                if let Some((addr, imm, kind, op)) = match_local_get_const_binary(self.exprs, id) {
                    let fused = fused_local_get_const_binary(addr, imm, kind, op);
                    if fused.cost < best.cost {
                        best = fused;
                    }
                }
                best
            }
            Expr::TeeValue { addr, value } => {
                let mut best = Plan::seq([self.emit_value(value), raw_local_tee4(addr)], 4);
                if let Expr::ConstI32(imm) = self.exprs[value] {
                    let fused = fused_const_local_tee4(addr, imm);
                    if fused.cost < best.cost {
                        best = fused;
                    }
                }
                best
            }
        };

        self.value_memo[id] = Some(plan.clone());
        plan
    }

    fn emit_set(&mut self, addr: u32, value: NodeId) -> Plan {
        let mut best = Plan::seq([self.emit_value(value), raw_local_set4(addr)], 4);

        if let Expr::ConstI32(imm) = self.exprs[value] {
            let fused = fused_const_local_set4(addr, imm);
            if fused.cost < best.cost {
                best = fused;
            }
        }

        if let Some((local_addr, imm, kind, op)) = match_local_get_const_binary(self.exprs, value) {
            if local_addr == addr {
                let fused = fused_local_get_const_binary_set(addr, imm, kind, op);
                if fused.cost < best.cost {
                    best = fused;
                }
            }
        }

        best
    }

    fn emit_drop(&mut self, value: NodeId) -> Plan {
        match self.exprs[value] {
            Expr::TeeValue { addr, value } => self.emit_set(addr, value),
            Expr::ConstI32(_) | Expr::LocalGet4(_) | Expr::Unary { .. } | Expr::Binary { .. } => {
                Plan::default()
            }
        }
    }
}

fn match_local_get_const_binary(exprs: &[Expr], id: NodeId) -> Option<(u32, i32, BinaryKind, Op)> {
    let Expr::Binary { kind, left, right } = exprs[id] else {
        return None;
    };

    if let (Expr::LocalGet4(addr), Expr::ConstI32(imm)) = (exprs[left], exprs[right]) {
        return Some((addr, imm, kind, binary_value_opcode(kind)));
    }

    if matches!(
        kind,
        BinaryKind::Add
            | BinaryKind::Mul
            | BinaryKind::And
            | BinaryKind::Or
            | BinaryKind::Xor
            | BinaryKind::Eq
            | BinaryKind::Ne
    ) {
        if let (Expr::ConstI32(imm), Expr::LocalGet4(addr)) = (exprs[left], exprs[right]) {
            return Some((addr, imm, kind, binary_value_opcode(kind)));
        }
    }

    None
}

fn parse_binary_kind(op: Op) -> Option<BinaryKind> {
    if op_eq(op, vm::op_i32_add) {
        Some(BinaryKind::Add)
    } else if op_eq(op, vm::op_i32_sub) {
        Some(BinaryKind::Sub)
    } else if op_eq(op, vm::op_i32_mul) {
        Some(BinaryKind::Mul)
    } else if op_eq(op, vm::op_i32_and) {
        Some(BinaryKind::And)
    } else if op_eq(op, vm::op_i32_or) {
        Some(BinaryKind::Or)
    } else if op_eq(op, vm::op_i32_xor) {
        Some(BinaryKind::Xor)
    } else if op_eq(op, vm::op_i32_shl) {
        Some(BinaryKind::Shl)
    } else if op_eq(op, vm::op_i32_shr_s) {
        Some(BinaryKind::ShrS)
    } else if op_eq(op, vm::op_i32_shr_u) {
        Some(BinaryKind::ShrU)
    } else if op_eq(op, vm::op_i32_eq) {
        Some(BinaryKind::Eq)
    } else if op_eq(op, vm::op_i32_ne) {
        Some(BinaryKind::Ne)
    } else {
        None
    }
}

fn binary_value_opcode(kind: BinaryKind) -> Op {
    match kind {
        BinaryKind::Add => vm::op_local_get4_i32_const_add,
        BinaryKind::Sub => vm::op_local_get4_i32_const_sub,
        BinaryKind::Mul => vm::op_local_get4_i32_const_mul,
        BinaryKind::And => vm::op_local_get4_i32_const_and,
        BinaryKind::Or => vm::op_local_get4_i32_const_or,
        BinaryKind::Xor => vm::op_local_get4_i32_const_xor,
        BinaryKind::Shl => vm::op_local_get4_i32_const_shl,
        BinaryKind::ShrS => vm::op_local_get4_i32_const_shr_s,
        BinaryKind::ShrU => vm::op_local_get4_i32_const_shr_u,
        BinaryKind::Eq => vm::op_local_get4_i32_const_eq,
        BinaryKind::Ne => vm::op_local_get4_i32_const_ne,
    }
}

fn binary_set_opcode(kind: BinaryKind) -> Op {
    match kind {
        BinaryKind::Add => vm::op_local_get4_i32_const_add_local_set4,
        BinaryKind::Sub => vm::op_local_get4_i32_const_sub_local_set4,
        BinaryKind::Mul => vm::op_local_get4_i32_const_mul_local_set4,
        BinaryKind::And => vm::op_local_get4_i32_const_and_local_set4,
        BinaryKind::Or => vm::op_local_get4_i32_const_or_local_set4,
        BinaryKind::Xor => vm::op_local_get4_i32_const_xor_local_set4,
        BinaryKind::Shl => vm::op_local_get4_i32_const_shl_local_set4,
        BinaryKind::ShrS => vm::op_local_get4_i32_const_shr_s_local_set4,
        BinaryKind::ShrU => vm::op_local_get4_i32_const_shr_u_local_set4,
        BinaryKind::Eq => vm::op_local_get4_i32_const_eq_local_set4,
        BinaryKind::Ne => vm::op_local_get4_i32_const_ne_local_set4,
    }
}

fn raw_const_i32(value: i32) -> Plan {
    Plan::from_instrs(
        vec![
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: value },
            },
        ],
        1,
        4,
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

fn raw_unary_eqz() -> Plan {
    Plan::from_instrs(vec![Instr { op: vm::op_i32_eqz }], 1, 4)
}

fn raw_binary(kind: BinaryKind) -> Plan {
    Plan::from_instrs(
        vec![Instr {
            op: match kind {
                BinaryKind::Add => vm::op_i32_add,
                BinaryKind::Sub => vm::op_i32_sub,
                BinaryKind::Mul => vm::op_i32_mul,
                BinaryKind::And => vm::op_i32_and,
                BinaryKind::Or => vm::op_i32_or,
                BinaryKind::Xor => vm::op_i32_xor,
                BinaryKind::Shl => vm::op_i32_shl,
                BinaryKind::ShrS => vm::op_i32_shr_s,
                BinaryKind::ShrU => vm::op_i32_shr_u,
                BinaryKind::Eq => vm::op_i32_eq,
                BinaryKind::Ne => vm::op_i32_ne,
            },
        }],
        1,
        8,
    )
}

fn fused_const_local_set4(addr: u32, imm: i32) -> Plan {
    fused_with_packed_operand(vm::op_i32_const_local_set4, addr, imm, 0)
}

fn fused_const_local_tee4(addr: u32, imm: i32) -> Plan {
    fused_with_packed_operand(vm::op_i32_const_local_tee4, addr, imm, 4)
}

fn fused_local_get_const_binary(addr: u32, imm: i32, _kind: BinaryKind, op: Op) -> Plan {
    fused_with_packed_operand(op, addr, imm, 4)
}

fn fused_local_get_const_binary_set(addr: u32, imm: i32, kind: BinaryKind, _op: Op) -> Plan {
    fused_with_packed_operand(binary_set_opcode(kind), addr, imm, 0)
}

fn fused_with_packed_operand(op: Op, addr: u32, imm: i32, transient_stack_bytes: usize) -> Plan {
    Plan::from_instrs(
        vec![
            Instr { op },
            Instr {
                operand: pack_local_i32(addr, imm),
            },
        ],
        1,
        transient_stack_bytes,
    )
}

fn pack_local_i32(addr: u32, imm: i32) -> Operand {
    let mut encoded = [0u8; 8];
    encoded[..4].copy_from_slice(&addr.to_le_bytes());
    encoded[4..].copy_from_slice(&imm.to_le_bytes());
    Operand { encoded }
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
    fn const_local_set_is_fused() {
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
    fn local_get_const_add_set_is_fused() {
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
                operand: Operand { i32: 1 },
            },
            Instr { op: vm::op_i32_add },
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
    fn tee_drop_becomes_set() {
        let pending = vec![
            Instr {
                op: vm::op_i32_const,
            },
            Instr {
                operand: Operand { i32: 1 },
            },
            Instr {
                op: vm::op_local_tee4,
            },
            Instr {
                operand: Operand { local_addr: 0 },
            },
            Instr { op: vm::op_drop },
            Instr {
                operand: Operand { drop_size: 4 },
            },
        ];
        assert_eq!(emitted_len(&pending), 2);
    }

    #[test]
    fn pure_tree_drop_is_elided() {
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
                operand: Operand { i32: 3 },
            },
            Instr { op: vm::op_i32_add },
            Instr { op: vm::op_drop },
            Instr {
                operand: Operand { drop_size: 4 },
            },
        ];
        assert_eq!(emitted_len(&pending), 0);
    }
}
