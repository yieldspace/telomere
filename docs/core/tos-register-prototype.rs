// #189 prototype: TOS-in-register threaded dispatch vs byte-stack dispatch.
//
// Two miniature direct-threaded interpreters over the same closed op subset
// (i32 const / local.get / local.set / local.tee / i32 add,sub,mul,lt_s,ge_u /
// br / br_if / drop / end), mirroring telomere's handler shapes:
//   A: OpA = fn(*const InstrA, &mut Ctx) -> u32   (current 2-arg contract;
//      every value round-trips through the byte stack, unaligned LE u32
//      loads/stores exactly like common/stack.rs)
//   B: OpB = fn(*const InstrB, &mut Ctx, u64) -> u32  (TOS threaded in the
//      third integer argument register; the byte stack holds only the
//      values *below* TOS. `tos_live` is a static per-program fact decided
//      at lowering time — encoded here by choosing, for each position, the
//      handler variant compiled for that state, exactly as a real lowering
//      pass that knows the abstract stack depth would.)
// B also has a shim op: a non-converted handler that only understands the
// memory stack. Entering it spills TOS, leaving it reloads TOS — the
// boundary cost the study must bound.
//
// Handlers are #[inline(never)] fn items (telomere's handler-identity
// contract) and dispatch is the last expression, so release codegen must
// turn it into a sibling (tail) jump; verified by disassembly spot-check.

use std::time::Instant;

// ---------------- shared byte stack (mirrors common/stack.rs) -------------
struct Stack {
    mem: Vec<u8>,
    top: usize,
}
impl Stack {
    fn new() -> Self {
        Stack { mem: vec![0u8; 4096], top: 0 }
    }
    #[inline(always)]
    fn push_u32(&mut self, v: u32) {
        unsafe {
            self.mem.as_mut_ptr().add(self.top).cast::<u32>().write_unaligned(v.to_le());
        }
        self.top += 4;
    }
    #[inline(always)]
    fn pop_u32(&mut self) -> u32 {
        self.top -= 4;
        u32::from_le(unsafe { self.mem.as_ptr().add(self.top).cast::<u32>().read_unaligned() })
    }
    #[inline(always)]
    fn peek_u32(&self) -> u32 {
        u32::from_le(unsafe {
            self.mem.as_ptr().add(self.top - 4).cast::<u32>().read_unaligned()
        })
    }
    // reduce_top_i32_add, copied in shape from common/stack.rs:410
    #[inline(always)]
    fn reduce_add(&mut self) {
        let rhs_off = self.top - 4;
        let lhs_off = self.top - 8;
        let lhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(lhs_off).cast::<u32>().read_unaligned()
        });
        let rhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(rhs_off).cast::<u32>().read_unaligned()
        });
        let r = (lhs as i32).wrapping_add(rhs as i32);
        unsafe {
            self.mem.as_mut_ptr().add(lhs_off).cast::<u32>().write_unaligned((r as u32).to_le());
        }
        self.top -= 4;
    }
    #[inline(always)]
    fn reduce_sub(&mut self) {
        let rhs_off = self.top - 4;
        let lhs_off = self.top - 8;
        let lhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(lhs_off).cast::<u32>().read_unaligned()
        });
        let rhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(rhs_off).cast::<u32>().read_unaligned()
        });
        let r = (lhs as i32).wrapping_sub(rhs as i32);
        unsafe {
            self.mem.as_mut_ptr().add(lhs_off).cast::<u32>().write_unaligned((r as u32).to_le());
        }
        self.top -= 4;
    }
    #[inline(always)]
    fn reduce_mul(&mut self) {
        let rhs_off = self.top - 4;
        let lhs_off = self.top - 8;
        let lhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(lhs_off).cast::<u32>().read_unaligned()
        });
        let rhs = u32::from_le(unsafe {
            self.mem.as_ptr().add(rhs_off).cast::<u32>().read_unaligned()
        });
        let r = (lhs as i32).wrapping_mul(rhs as i32);
        unsafe {
            self.mem.as_mut_ptr().add(lhs_off).cast::<u32>().write_unaligned((r as u32).to_le());
        }
        self.top -= 4;
    }
}

struct Ctx {
    stack: Stack,
    locals: [u32; 8],
    result: u32,
}
impl Ctx {
    fn new() -> Self {
        Ctx { stack: Stack::new(), locals: [0; 8], result: 0 }
    }
}

// ---------------- variant A: current contract ------------------------------
type OpA = unsafe fn(*const InstrA, &mut Ctx) -> u32;
#[derive(Clone, Copy)]
struct InstrA {
    op: OpA,
    operand: u32,
}
const OK: u32 = 0;
const END: u32 = 1;

macro_rules! next_a {
    ($pc:expr, $ctx:expr) => {{
        let n = $pc.add(1);
        (n.read().op)(n, $ctx)
    }};
}

#[inline(never)]
unsafe fn a_const(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.stack.push_u32(pc.read().operand);
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_local_get(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let v = ctx.locals[pc.read().operand as usize];
    ctx.stack.push_u32(v);
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_local_set(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let v = ctx.stack.pop_u32();
    ctx.locals[pc.read().operand as usize] = v;
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_local_tee(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let v = ctx.stack.peek_u32();
    ctx.locals[pc.read().operand as usize] = v;
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_add(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.stack.reduce_add();
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_sub(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.stack.reduce_sub();
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_mul(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.stack.reduce_mul();
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_ge_u(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let rhs = ctx.stack.pop_u32();
    let lhs = ctx.stack.pop_u32();
    ctx.stack.push_u32((lhs >= rhs) as u32);
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_br_if(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let c = ctx.stack.pop_u32();
    if c != 0 {
        let t = pc.offset(pc.read().operand as i32 as isize);
        (t.read().op)(t, ctx)
    } else {
        next_a!(pc, ctx)
    }
}
#[inline(never)]
unsafe fn a_br(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let t = pc.offset(pc.read().operand as i32 as isize);
    (t.read().op)(t, ctx)
}
#[inline(never)]
unsafe fn a_drop(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.stack.top -= 4;
    next_a!(pc, ctx)
}
// Non-converted stack-only op (stands in for any unconverted handler):
// mixes the top two stack values in place.
#[inline(never)]
unsafe fn a_opaque(pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    let rhs = ctx.stack.pop_u32();
    let lhs = ctx.stack.pop_u32();
    ctx.stack.push_u32(lhs ^ (rhs.wrapping_mul(0x9e37_79b1)));
    next_a!(pc, ctx)
}
#[inline(never)]
unsafe fn a_end(_pc: *const InstrA, ctx: &mut Ctx) -> u32 {
    ctx.result = ctx.stack.pop_u32();
    END
}

// ---------------- variant B: TOS in register -------------------------------
type OpB = unsafe fn(*const InstrB, &mut Ctx, u64) -> u32;
#[derive(Clone, Copy)]
struct InstrB {
    op: OpB,
    operand: u32,
}

macro_rules! next_b {
    ($pc:expr, $ctx:expr, $tos:expr) => {{
        let n = $pc.add(1);
        (n.read().op)(n, $ctx, $tos)
    }};
}

// Producers when TOS is dead: result simply becomes TOS (no memory traffic).
#[inline(never)]
unsafe fn b_const(pc: *const InstrB, ctx: &mut Ctx, _tos: u64) -> u32 {
    next_b!(pc, ctx, pc.read().operand as u64)
}
#[inline(never)]
unsafe fn b_local_get(pc: *const InstrB, ctx: &mut Ctx, _tos: u64) -> u32 {
    let v = ctx.locals[pc.read().operand as usize];
    next_b!(pc, ctx, v as u64)
}
// Producers when TOS is live: spill TOS below, new value becomes TOS.
#[inline(never)]
unsafe fn b_const_spill(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.stack.push_u32(tos as u32);
    next_b!(pc, ctx, pc.read().operand as u64)
}
#[inline(never)]
unsafe fn b_local_get_spill(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.stack.push_u32(tos as u32);
    let v = ctx.locals[pc.read().operand as usize];
    next_b!(pc, ctx, v as u64)
}
// Dyadic ops: rhs = TOS (register), lhs = top of byte stack (one load, no store).
#[inline(never)]
unsafe fn b_add(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    let lhs = ctx.stack.pop_u32();
    next_b!(pc, ctx, (lhs as i32).wrapping_add(tos as i32) as u32 as u64)
}
#[inline(never)]
unsafe fn b_sub(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    let lhs = ctx.stack.pop_u32();
    next_b!(pc, ctx, (lhs as i32).wrapping_sub(tos as i32) as u32 as u64)
}
#[inline(never)]
unsafe fn b_mul(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    let lhs = ctx.stack.pop_u32();
    next_b!(pc, ctx, (lhs as i32).wrapping_mul(tos as i32) as u32 as u64)
}
#[inline(never)]
unsafe fn b_ge_u(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    let lhs = ctx.stack.pop_u32();
    next_b!(pc, ctx, (lhs >= tos as u32) as u64)
}
// local.set consumes TOS; TOS is dead afterwards (next handler is a
// dead-TOS variant, which ignores the register).
#[inline(never)]
unsafe fn b_local_set(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.locals[pc.read().operand as usize] = tos as u32;
    next_b!(pc, ctx, 0)
}
#[inline(never)]
unsafe fn b_local_tee(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.locals[pc.read().operand as usize] = tos as u32;
    next_b!(pc, ctx, tos)
}
#[inline(never)]
unsafe fn b_br_if(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    if tos != 0 {
        let t = pc.offset(pc.read().operand as i32 as isize);
        (t.read().op)(t, ctx, 0)
    } else {
        next_b!(pc, ctx, 0)
    }
}
#[inline(never)]
unsafe fn b_br(pc: *const InstrB, ctx: &mut Ctx, _tos: u64) -> u32 {
    let t = pc.offset(pc.read().operand as i32 as isize);
    (t.read().op)(t, ctx, 0)
}
#[inline(never)]
unsafe fn b_drop(pc: *const InstrB, ctx: &mut Ctx, _tos: u64) -> u32 {
    next_b!(pc, ctx, 0)
}
// Shim boundary: spill TOS, run the stack-only op, reload TOS.
#[inline(never)]
unsafe fn b_shim_opaque(pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.stack.push_u32(tos as u32); // spill
    let rhs = ctx.stack.pop_u32();
    let lhs = ctx.stack.pop_u32();
    ctx.stack.push_u32(lhs ^ (rhs.wrapping_mul(0x9e37_79b1)));
    let new_tos = ctx.stack.pop_u32(); // reload
    next_b!(pc, ctx, new_tos as u64)
}
#[inline(never)]
unsafe fn b_end(_pc: *const InstrB, ctx: &mut Ctx, tos: u64) -> u32 {
    ctx.result = tos as u32;
    END
}

// ---------------- programs --------------------------------------------------
// Locals: 0 = n (input), 1 = i, 2 = acc/a, 3 = b
// Program builders take a `shim` flag: when set, the loop body's add is
// replaced by produce->opaque crossing (worst case for the shim).

fn prog_a_scalar_loop(shim: bool) -> Vec<InstrA> {
    // acc = sum_{i<n} i   (same shape as telomere's scalar_local_loop bench)
    let mut p = Vec::new();
    let mut emit = |op: OpA, operand: u32| p.push(InstrA { op, operand });
    // loop head at index 0
    emit(a_local_get, 1); // i
    emit(a_local_get, 0); // n
    emit(a_ge_u, 0);
    emit(a_br_if, 9); // -> end-ish (index 13) : offset from br_if idx 3 => +10? fixed below
    if shim {
        emit(a_local_get, 2);
        emit(a_local_get, 1);
        emit(a_opaque, 0); // acc ^ i*K  (different math; used only for timing parity A/B)
        emit(a_local_set, 2);
    } else {
        emit(a_local_get, 2);
        emit(a_local_get, 1);
        emit(a_add, 0);
        emit(a_local_set, 2);
    }
    emit(a_local_get, 1);
    emit(a_const, 1);
    emit(a_add, 0);
    emit(a_local_set, 1);
    emit(a_br, 0); // placeholder, fixed below
    emit(a_local_get, 2);
    emit(a_end, 0);
    // fix branch targets (operand = relative offset from the branch instr)
    let br_if_idx = 3usize;
    let exit_idx = p.len() - 2; // local_get acc
    p[br_if_idx].operand = (exit_idx - br_if_idx) as u32;
    let br_idx = p.len() - 3;
    p[br_idx].operand = (0isize - br_idx as isize) as u32; // wraps; handled specially? no:
    p[br_idx].op = a_br;
    // a_br adds operand as usize; for backward jump store two's complement via
    // a dedicated encoding: we instead place loop head at 0 and use wrapping add.
    p
}

fn main() {
    // NOTE: backward branch uses wrapping pointer arithmetic in a_br/b_br via
    // `pc.add(operand as usize)` where operand is the two's-complement offset.
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("bench");
    match mode {
        "check" => check(),
        _ => bench(),
    }
}

fn run_a(p: &[InstrA], n: u32) -> u32 {
    let mut ctx = Ctx::new();
    ctx.locals[0] = n;
    unsafe { (p[0].op)(p.as_ptr(), &mut ctx) };
    ctx.result
}
fn run_b(p: &[InstrB], n: u32) -> u32 {
    let mut ctx = Ctx::new();
    ctx.locals[0] = n;
    unsafe { (p[0].op)(p.as_ptr(), &mut ctx, 0) };
    ctx.result
}

fn prog_b_scalar_loop(shim: bool) -> Vec<InstrB> {
    let mut p: Vec<InstrB> = Vec::new();
    let mut emit = |op: OpB, operand: u32| p.push(InstrB { op, operand });
    // 0: loop head. TOS dead at loop entry (br/br_if dispatch with dead TOS).
    emit(b_local_get, 1); // i -> TOS
    emit(b_local_get_spill, 0); // spill i, n -> TOS
    emit(b_ge_u, 0); // lhs=i popped, TOS = i>=n
    emit(b_br_if, 0); // consumes TOS; target fixed below
    if shim {
        emit(b_local_get, 2); // acc -> TOS
        emit(b_local_get_spill, 1); // spill acc, i -> TOS
        emit(b_shim_opaque, 0); // stack-only op on (acc, i), TOS reloaded
        emit(b_local_set, 2);
    } else {
        emit(b_local_get, 2);
        emit(b_local_get_spill, 1);
        emit(b_add, 0);
        emit(b_local_set, 2);
    }
    emit(b_local_get, 1);
    emit(b_const_spill, 1);
    emit(b_add, 0);
    emit(b_local_set, 1);
    emit(b_br, 0);
    emit(b_local_get, 2);
    emit(b_end, 0);
    let br_if_idx = 3usize;
    let exit_idx = p.len() - 2;
    p[br_if_idx].operand = (exit_idx - br_if_idx) as u32;
    let br_idx = p.len() - 3;
    p[br_idx].operand = (0isize - br_idx as isize) as u32;
    p
}

// fib, iterative: a=0,b=1; loop n times { t=a+b; a=b; b=t } result a
fn prog_a_fib() -> Vec<InstrA> {
    let mut p = Vec::new();
    let mut emit = |op: OpA, operand: u32| p.push(InstrA { op, operand });
    // locals: 0=n, 1=i, 2=a, 3=b, 4=t
    emit(a_const, 0); emit(a_local_set, 2); // a=0   (indices 0,1)
    emit(a_const, 1); emit(a_local_set, 3); // b=1   (2,3)
    // loop head = 4
    emit(a_local_get, 1); emit(a_local_get, 0); emit(a_ge_u, 0); emit(a_br_if, 0); // 4..7
    emit(a_local_get, 2); emit(a_local_get, 3); emit(a_add, 0); emit(a_local_set, 4); // t=a+b 8..11
    emit(a_local_get, 3); emit(a_local_set, 2); // a=b 12,13
    emit(a_local_get, 4); emit(a_local_set, 3); // b=t 14,15
    emit(a_local_get, 1); emit(a_const, 1); emit(a_add, 0); emit(a_local_set, 1); // i++ 16..19
    emit(a_br, 0); // 20 -> back to 4
    emit(a_local_get, 2); emit(a_end, 0); // 21,22
    p[7].operand = (21 - 7) as u32;
    p[20].operand = (4isize - 20) as u32;
    p
}
fn prog_b_fib() -> Vec<InstrB> {
    let mut p: Vec<InstrB> = Vec::new();
    let mut emit = |op: OpB, operand: u32| p.push(InstrB { op, operand });
    emit(b_const, 0); emit(b_local_set, 2);
    emit(b_const, 1); emit(b_local_set, 3);
    emit(b_local_get, 1); emit(b_local_get_spill, 0); emit(b_ge_u, 0); emit(b_br_if, 0); // 4..7
    emit(b_local_get, 2); emit(b_local_get_spill, 3); emit(b_add, 0); emit(b_local_set, 4); // 8..11
    emit(b_local_get, 3); emit(b_local_set, 2);
    emit(b_local_get, 4); emit(b_local_set, 3);
    emit(b_local_get, 1); emit(b_const_spill, 1); emit(b_add, 0); emit(b_local_set, 1);
    emit(b_br, 0);
    emit(b_local_get, 2); emit(b_end, 0);
    p[7].operand = (21 - 7) as u32;
    p[20].operand = (4isize - 20) as u32;
    p
}

fn check() {
    let a = prog_a_scalar_loop(false);
    let b = prog_b_scalar_loop(false);
    for n in [0u32, 1, 10, 1024] {
        let (ra, rb) = (run_a(&a, n), run_b(&b, n));
        assert_eq!(ra, rb, "scalar_loop n={n}");
        assert_eq!(ra, (0..n).fold(0u32, |s, i| s.wrapping_add(i)), "scalar_loop math n={n}");
        println!("scalar_loop n={n}: {ra} OK");
    }
    let a = prog_a_scalar_loop(true);
    let b = prog_b_scalar_loop(true);
    for n in [0u32, 1, 10, 257] {
        assert_eq!(run_a(&a, n), run_b(&b, n), "scalar_loop_shim n={n}");
        println!("scalar_loop_shim n={n}: {} OK", run_a(&a, n));
    }
    let (fa, fb) = (prog_a_fib(), prog_b_fib());
    for n in [0u32, 1, 2, 10, 20] {
        let (ra, rb) = (run_a(&fa, n), run_b(&fb, n));
        assert_eq!(ra, rb, "fib n={n}");
        println!("fib_iter n={n}: {ra} OK");
    }
    println!("ALL CHECKS PASS");
}

fn timeit<F: Fn() -> u32>(label: &str, f: F, reps: usize) {
    // interleaved-friendly: caller alternates A/B per round
    let mut acc = 0u32;
    let t = Instant::now();
    for _ in 0..reps {
        acc = acc.wrapping_add(f());
    }
    let dt = t.elapsed();
    println!("{label}: {:.1} ns/run  (checksum {})", dt.as_nanos() as f64 / reps as f64, acc);
}

fn bench() {
    let n_loop = 20_000u32;
    let n_fib = 2_000_000u32; // iterations of fib body loop per run (n argument)
    let a = prog_a_scalar_loop(false);
    let b = prog_b_scalar_loop(false);
    let as_ = prog_a_scalar_loop(true);
    let bs = prog_b_scalar_loop(true);
    let fa = prog_a_fib();
    let fb = prog_b_fib();
    // warmup
    assert_eq!(run_a(&a, 10), run_b(&b, 10));
    // Paired rounds: A,B,B,A alternating to cancel drift.
    for round in 0..5 {
        println!("-- round {round}");
        if round % 2 == 0 {
            timeit("A scalar_loop", || run_a(&a, n_loop), 200);
            timeit("B scalar_loop", || run_b(&b, n_loop), 200);
            timeit("A scalar_loop_shim", || run_a(&as_, n_loop), 200);
            timeit("B scalar_loop_shim", || run_b(&bs, n_loop), 200);
            timeit("A fib_body", || run_a(&fa, n_fib), 3);
            timeit("B fib_body", || run_b(&fb, n_fib), 3);
        } else {
            timeit("B scalar_loop", || run_b(&b, n_loop), 200);
            timeit("A scalar_loop", || run_a(&a, n_loop), 200);
            timeit("B scalar_loop_shim", || run_b(&bs, n_loop), 200);
            timeit("A scalar_loop_shim", || run_a(&as_, n_loop), 200);
            timeit("B fib_body", || run_b(&fb, n_fib), 3);
            timeit("A fib_body", || run_a(&fa, n_fib), 3);
        }
    }
}
