#[macro_use]
pub(crate) mod traps;
#[cfg(feature = "threads")]
mod atomics;
mod bulk_memory;
mod call;
mod control;
mod globals;
mod locals;
mod memory;
mod numeric;
mod refs;
#[cfg(feature = "simd")]
pub(crate) mod simd;
mod superinstructions;
mod tables;

use crate::{
    common::store::{CallDispatchCache, CallDispatchTarget},
    common::{
        execute_elem_init_const_expr, CallFrameCache, ElemInit, ExecuteContext, ExportDesc,
        InstanceHandle, Instr, LocalReference, MemArg, ObjectRef, ResultType, ResultValue,
        StablePc, Stack, VMResult, ValType, WasmValue, TABLE_UNINITIALIZED,
    },
    runtime::{
        memory_effect::{HostCallPending, PendingOp},
        scheduler::{ExecutionDriver, ReadyFlag, Scheduler, SyncRunError, Task, TokioDriver},
    },
    Store,
};
#[cfg(all(test, feature = "vm-profile"))]
use std::cell::Cell;
#[cfg(feature = "vm-profile")]
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::OnceLock,
    time::{Duration, Instant},
};

#[cfg(feature = "vm-profile")]
thread_local! {
    static DISPATCH_PROFILE_SESSION: RefCell<Option<DispatchProfileSession>> = const { RefCell::new(None) };
    #[cfg(test)]
    static DISPATCH_PROFILE_TEST_ENABLED: Cell<bool> = const { Cell::new(false) };
    #[cfg(test)]
    static LAST_DISPATCH_PROFILE_SNAPSHOT: RefCell<Option<DispatchProfileSnapshot>> = const { RefCell::new(None) };
}

#[cfg(feature = "vm-profile")]
#[derive(Clone, Copy)]
struct DispatchProfileConfig {
    enabled: bool,
    top_n: usize,
}

#[cfg(feature = "vm-profile")]
const VM_PROFILE_TOP_K: usize = 16;

#[cfg(feature = "vm-profile")]
const HANDLER_LAYOUT_STABILITY_PHASES: usize = 2;

#[cfg(feature = "vm-profile")]
#[derive(Debug, Default, Clone, Copy)]
struct DispatchProfileStat {
    count: u64,
}

#[cfg(feature = "vm-profile")]
struct DispatchProfileSession {
    started_at: Instant,
    total_instrs: u64,
    stats: HashMap<&'static str, DispatchProfileStat>,
    pairs: HashMap<(&'static str, &'static str), DispatchProfileStat>,
    triples: HashMap<(&'static str, &'static str, &'static str), DispatchProfileStat>,
    last_label: Option<&'static str>,
    last_pair: Option<(&'static str, &'static str)>,
}

#[cfg(feature = "vm-profile")]
impl DispatchProfileSession {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            total_instrs: 0,
            stats: HashMap::new(),
            pairs: HashMap::new(),
            triples: HashMap::new(),
            last_label: None,
            last_pair: None,
        }
    }
}

#[cfg(feature = "vm-profile")]
#[derive(Clone)]
struct DispatchProfileSnapshot {
    elapsed: Duration,
    total_instrs: u64,
    stats: Vec<(&'static str, DispatchProfileStat)>,
    pairs: Vec<((&'static str, &'static str), DispatchProfileStat)>,
    triples: Vec<(
        (&'static str, &'static str, &'static str),
        DispatchProfileStat,
    )>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[allow(dead_code)]
enum HandlerLayoutGroup {
    Locals,
    Superinstructions,
    Memory,
    Call,
    Control,
    Numeric,
    Globals,
    Tables,
    Refs,
    BulkMemory,
    Atomics,
    Simd,
    Traps,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[allow(dead_code)]
enum DispatchProfileFamilyGroup {
    LocalControl,
    Memory,
    CallSelect,
}

#[allow(dead_code)]
impl DispatchProfileFamilyGroup {
    const ORDER: [Self; 3] = [Self::LocalControl, Self::Memory, Self::CallSelect];

    const fn index(self) -> usize {
        match self {
            Self::LocalControl => 0,
            Self::Memory => 1,
            Self::CallSelect => 2,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::LocalControl => "local/control",
            Self::Memory => "memory",
            Self::CallSelect => "call/select",
        }
    }
}

#[allow(dead_code)]
impl HandlerLayoutGroup {
    const fn rank(self) -> usize {
        match self {
            Self::Locals => 0,
            Self::Superinstructions => 1,
            Self::Memory => 2,
            Self::Call => 3,
            Self::Control => 4,
            Self::Numeric => 5,
            Self::Globals => 6,
            Self::Tables => 7,
            Self::Refs => 8,
            Self::BulkMemory => 9,
            Self::Atomics => 10,
            Self::Simd => 11,
            Self::Traps => 12,
            Self::Other => 13,
        }
    }
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
struct HandlerDescriptor {
    layout_group: HandlerLayoutGroup,
    family_group: DispatchProfileFamilyGroup,
}

#[allow(dead_code)]
fn handler_descriptor(label: &'static str) -> HandlerDescriptor {
    if label == "op_unreachable" {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Traps,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("special_")
        || matches!(
            label,
            "op_return"
                | "op_end"
                | "op_br"
                | "op_else"
                | "op_br_if"
                | "op_br_table"
                | "op_loop"
                | "op_if"
        )
    {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Control,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_local_get4_i32_const_add")
        || label.starts_with("op_local_get4_local_get4_i32_add")
        || label.starts_with("op_local_binop32")
        || label.starts_with("op_local_binop64")
        || label.starts_with("op_local_cmp32")
        || label.starts_with("op_local_cmp64")
        || label.starts_with("op_local_unary32")
        || label.starts_with("op_local_unary64")
        || matches!(
            label,
            "op_local_get4_br_if"
                | "op_local_get4_i32_eqz_br_if"
                | "op_local_get4_i32_const_compare_br_if"
                | "op_local_get4_local_get4_compare_br_if"
        )
    {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Superinstructions,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_mem_") || label == "op_data_drop" {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::BulkMemory,
            family_group: DispatchProfileFamilyGroup::Memory,
        };
    }
    if label.starts_with("op_atomic") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Atomics,
            family_group: DispatchProfileFamilyGroup::Memory,
        };
    }
    if label.starts_with("op_call") || label.starts_with("op_return_call") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Call,
            family_group: DispatchProfileFamilyGroup::CallSelect,
        };
    }
    if label.contains("_load") || label.contains("_store") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Memory,
            family_group: DispatchProfileFamilyGroup::Memory,
        };
    }
    if label.starts_with("op_local_") || label.starts_with("op_select") || label == "op_drop" {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Locals,
            family_group: if label.starts_with("op_select") {
                DispatchProfileFamilyGroup::CallSelect
            } else {
                DispatchProfileFamilyGroup::LocalControl
            },
        };
    }
    if label.starts_with("op_global_") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Globals,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_table_") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Tables,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_ref_") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Refs,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_v128") || label.contains("x") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Simd,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    if label.starts_with("op_i") || label.starts_with("op_f") {
        return HandlerDescriptor {
            layout_group: HandlerLayoutGroup::Numeric,
            family_group: DispatchProfileFamilyGroup::LocalControl,
        };
    }
    HandlerDescriptor {
        layout_group: HandlerLayoutGroup::Other,
        family_group: DispatchProfileFamilyGroup::LocalControl,
    }
}

#[allow(dead_code)]
fn handler_layout_group(label: &'static str) -> HandlerLayoutGroup {
    handler_descriptor(label).layout_group
}

#[allow(dead_code)]
fn dispatch_profile_family_group(label: &'static str) -> DispatchProfileFamilyGroup {
    handler_descriptor(label).family_group
}

#[cfg(feature = "vm-profile")]
fn grouped_profile_stats(
    stats: &[(&'static str, DispatchProfileStat)],
) -> [Vec<(&'static str, DispatchProfileStat)>; 3] {
    let mut grouped: [Vec<(&'static str, DispatchProfileStat)>; 3] =
        std::array::from_fn(|_| Vec::new());
    let top_n = dispatch_profile_config().top_n;
    for entry in stats.iter().copied() {
        let group = dispatch_profile_family_group(entry.0);
        let bucket = &mut grouped[group.index()];
        if bucket.len() < top_n {
            bucket.push(entry);
        }
    }
    grouped
}

#[cfg(feature = "vm-profile")]
fn grouped_profile_pairs(
    pairs: &[((&'static str, &'static str), DispatchProfileStat)],
) -> [Vec<((&'static str, &'static str), DispatchProfileStat)>; 3] {
    let mut grouped: [Vec<((&'static str, &'static str), DispatchProfileStat)>; 3] =
        std::array::from_fn(|_| Vec::new());
    let top_n = dispatch_profile_config().top_n;
    for entry in pairs.iter().copied() {
        let group = dispatch_profile_family_group((entry.0).0);
        let bucket = &mut grouped[group.index()];
        if bucket.len() < top_n {
            bucket.push(entry);
        }
    }
    grouped
}

#[cfg(feature = "vm-profile")]
fn grouped_profile_triples(
    triples: &[(
        (&'static str, &'static str, &'static str),
        DispatchProfileStat,
    )],
) -> [Vec<(
    (&'static str, &'static str, &'static str),
    DispatchProfileStat,
)>; 3] {
    let mut grouped: [Vec<(
        (&'static str, &'static str, &'static str),
        DispatchProfileStat,
    )>; 3] = std::array::from_fn(|_| Vec::new());
    let top_n = dispatch_profile_config().top_n;
    for entry in triples.iter().copied() {
        let group = dispatch_profile_family_group((entry.0).0);
        let bucket = &mut grouped[group.index()];
        if bucket.len() < top_n {
            bucket.push(entry);
        }
    }
    grouped
}

#[cfg(feature = "vm-profile")]
fn layout_span_for_pair(pair: (&'static str, &'static str)) -> usize {
    let lhs = handler_layout_group(pair.0).rank();
    let rhs = handler_layout_group(pair.1).rank();
    lhs.max(rhs) - lhs.min(rhs)
}

#[cfg(feature = "vm-profile")]
fn layout_span_for_triple(triple: (&'static str, &'static str, &'static str)) -> usize {
    let a = handler_layout_group(triple.0).rank();
    let b = handler_layout_group(triple.1).rank();
    let c = handler_layout_group(triple.2).rank();
    let min = a.min(b).min(c);
    let max = a.max(b).max(c);
    max - min
}

#[cfg(feature = "vm-profile")]
struct DispatchProfileRunGuard {
    active: bool,
}

#[cfg(not(feature = "vm-profile"))]
struct DispatchProfileRunGuard;

#[cfg(feature = "vm-profile")]
impl DispatchProfileRunGuard {
    fn new() -> Self {
        let active = dispatch_profile_enabled();
        if active {
            DISPATCH_PROFILE_SESSION.with(|session| {
                *session.borrow_mut() = Some(DispatchProfileSession::new());
            });
        }
        Self { active }
    }
}

#[cfg(not(feature = "vm-profile"))]
impl DispatchProfileRunGuard {
    fn new() -> Self {
        Self
    }
}

#[cfg(feature = "vm-profile")]
impl Drop for DispatchProfileRunGuard {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let Some(snapshot) = finish_dispatch_profile_session() else {
            return;
        };
        #[cfg(test)]
        LAST_DISPATCH_PROFILE_SNAPSHOT.with(|last| {
            *last.borrow_mut() = Some(snapshot.clone());
        });
        if cfg!(test) {
            return;
        }
        eprintln!(
            "[telomere-vm-profile] total_instrs={} elapsed_ms={:.3} top_k={} layout_stability_phases={}",
            snapshot.total_instrs,
            snapshot.elapsed.as_secs_f64() * 1000.0,
            VM_PROFILE_TOP_K,
            HANDLER_LAYOUT_STABILITY_PHASES,
        );
        let grouped_stats = grouped_profile_stats(&snapshot.stats);
        let grouped_pairs = grouped_profile_pairs(&snapshot.pairs);
        let grouped_triples = grouped_profile_triples(&snapshot.triples);
        for &(label, stat) in &snapshot.stats {
            let share = if snapshot.total_instrs == 0 {
                0.0
            } else {
                stat.count as f64 / snapshot.total_instrs as f64 * 100.0
            };
            let approx_elapsed_ms = if snapshot.total_instrs == 0 {
                0.0
            } else {
                snapshot.elapsed.as_secs_f64() * 1000.0 * stat.count as f64
                    / snapshot.total_instrs as f64
            };
            eprintln!(
                "[telomere-vm-profile] family={} count={} elapsed_ms={:.3} share_pct={:.2}",
                label, stat.count, approx_elapsed_ms, share
            );
        }
        for &((lhs, rhs), stat) in &snapshot.pairs {
            eprintln!(
                "[telomere-vm-profile] pair={}=>{} count={} layout_span={} groups={:?}=>{:?}",
                lhs,
                rhs,
                stat.count,
                layout_span_for_pair((lhs, rhs)),
                handler_layout_group(lhs),
                handler_layout_group(rhs)
            );
        }
        for &((a, b, c), stat) in &snapshot.triples {
            eprintln!(
                "[telomere-vm-profile] triple={}=>{}=>{} count={} layout_span={} groups={:?}=>{:?}=>{:?}",
                a,
                b,
                c,
                stat.count,
                layout_span_for_triple((a, b, c)),
                handler_layout_group(a),
                handler_layout_group(b),
                handler_layout_group(c)
            );
        }
        for group in DispatchProfileFamilyGroup::ORDER {
            let stats = grouped_stats[group.index()]
                .iter()
                .map(|(label, stat)| format!("{label}:count={}", stat.count))
                .collect::<Vec<_>>()
                .join(",");
            let pairs = grouped_pairs[group.index()]
                .iter()
                .map(|((lhs, rhs), stat)| {
                    format!(
                        "{lhs}=>{rhs}:count={},layout_span={}",
                        stat.count,
                        layout_span_for_pair((*lhs, *rhs))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let triples = grouped_triples[group.index()]
                .iter()
                .map(|((a, b, c), stat)| {
                    format!(
                        "{a}=>{b}=>{c}:count={},layout_span={}",
                        stat.count,
                        layout_span_for_triple((*a, *b, *c))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            eprintln!(
                "[telomere-vm-profile] family_group={} top_k={} stats=[{}] pairs=[{}] triples=[{}]",
                group.label(),
                dispatch_profile_config().top_n,
                stats,
                pairs,
                triples,
            );
        }
    }
}

#[cfg(feature = "vm-profile")]
fn dispatch_profile_config() -> DispatchProfileConfig {
    static CONFIG: OnceLock<DispatchProfileConfig> = OnceLock::new();
    *CONFIG.get_or_init(|| {
        let enabled = std::env::var("TELOMERE_VM_PROFILE")
            .ok()
            .is_some_and(|value| value != "0");
        let top_n = std::env::var("TELOMERE_VM_PROFILE_TOP")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value != 0)
            .unwrap_or(VM_PROFILE_TOP_K);
        DispatchProfileConfig { enabled, top_n }
    })
}

#[cfg(feature = "vm-profile")]
fn finish_dispatch_profile_session() -> Option<DispatchProfileSnapshot> {
    DISPATCH_PROFILE_SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let profile = session.take()?;
        let now = Instant::now();
        let mut stats = profile.stats.into_iter().collect::<Vec<_>>();
        let mut pairs = profile.pairs.into_iter().collect::<Vec<_>>();
        let mut triples = profile.triples.into_iter().collect::<Vec<_>>();
        stats.sort_by_key(|(_, stat)| std::cmp::Reverse(stat.count));
        pairs.sort_by_key(|(_, stat)| std::cmp::Reverse(stat.count));
        triples.sort_by_key(|(_, stat)| std::cmp::Reverse(stat.count));
        stats.truncate(dispatch_profile_config().top_n);
        pairs.truncate(dispatch_profile_config().top_n);
        triples.truncate(dispatch_profile_config().top_n);
        Some(DispatchProfileSnapshot {
            elapsed: now.saturating_duration_since(profile.started_at),
            total_instrs: profile.total_instrs,
            stats,
            pairs,
            triples,
        })
    })
}

#[inline(always)]
#[cfg(feature = "vm-profile")]
fn dispatch_profile_enabled() -> bool {
    #[cfg(test)]
    if DISPATCH_PROFILE_TEST_ENABLED.with(|enabled| enabled.get()) {
        return true;
    }
    if !cfg!(any(debug_assertions, test)) {
        return false;
    }
    dispatch_profile_config().enabled
}

#[inline(always)]
#[cfg(feature = "vm-profile")]
pub(crate) fn dispatch_profile_count(label: &'static str) {
    if !dispatch_profile_enabled() {
        return;
    }
    DISPATCH_PROFILE_SESSION.with(|session| {
        let mut session = session.borrow_mut();
        let Some(profile) = session.as_mut() else {
            return;
        };
        let stat = profile.stats.entry(label).or_default();
        stat.count = stat.count.saturating_add(1);
        if let Some(previous) = profile.last_label {
            let pair = (previous, label);
            let pair_stat = profile.pairs.entry(pair).or_default();
            pair_stat.count = pair_stat.count.saturating_add(1);
            if let Some((first, second)) = profile.last_pair {
                let triple = (first, second, label);
                let triple_stat = profile.triples.entry(triple).or_default();
                triple_stat.count = triple_stat.count.saturating_add(1);
            }
            profile.last_pair = Some(pair);
        }
        profile.last_label = Some(label);
        profile.total_instrs = profile.total_instrs.saturating_add(1);
    });
}

#[inline(always)]
#[cfg(not(feature = "vm-profile"))]
pub(crate) fn dispatch_profile_count(_label: &'static str) {}

#[cfg(all(test, feature = "vm-profile"))]
struct DispatchProfileTestOverride {
    previous: bool,
}

#[cfg(all(test, feature = "vm-profile"))]
impl DispatchProfileTestOverride {
    fn enable() -> Self {
        LAST_DISPATCH_PROFILE_SNAPSHOT.with(|last| {
            last.borrow_mut().take();
        });
        let previous = DISPATCH_PROFILE_TEST_ENABLED.with(|enabled| {
            let previous = enabled.get();
            enabled.set(true);
            previous
        });
        Self { previous }
    }
}

#[cfg(all(test, feature = "vm-profile"))]
impl Drop for DispatchProfileTestOverride {
    fn drop(&mut self) {
        DISPATCH_PROFILE_TEST_ENABLED.with(|enabled| enabled.set(self.previous));
    }
}

#[cfg(all(test, feature = "vm-profile"))]
fn take_last_dispatch_profile_snapshot_for_test() -> Option<DispatchProfileSnapshot> {
    LAST_DISPATCH_PROFILE_SNAPSHOT.with(|last| last.borrow_mut().take())
}

#[inline(always)]
fn wasm_shift_mask32(rhs: u32) -> u32 {
    rhs & 31
}

#[inline(always)]
fn wasm_shift_mask64(rhs: u32) -> u32 {
    rhs & 63
}

#[inline(always)]
fn wasm_i32_shl(lhs: i32, rhs: i32) -> i32 {
    lhs.wrapping_shl(wasm_shift_mask32(rhs as u32))
}

#[inline(always)]
fn wasm_i32_shr_s(lhs: i32, rhs: i32) -> i32 {
    lhs >> wasm_shift_mask32(rhs as u32)
}

#[inline(always)]
fn wasm_i32_shr_u(lhs: u32, rhs: u32) -> u32 {
    lhs.wrapping_shr(wasm_shift_mask32(rhs))
}

#[inline(always)]
fn wasm_i64_shl(lhs: i64, rhs: i64) -> i64 {
    lhs.wrapping_shl(wasm_shift_mask64(rhs as u32))
}

#[inline(always)]
fn wasm_i64_shr_s(lhs: i64, rhs: i64) -> i64 {
    lhs >> wasm_shift_mask64(rhs as u32)
}

#[inline(always)]
fn wasm_i64_shr_u(lhs: u64, rhs: u64) -> u64 {
    lhs.wrapping_shr(wasm_shift_mask64(rhs as u32))
}

pub(crate) enum StoreBytes {
    Write1([u8; 1]),
    Write2([u8; 2]),
    Write4([u8; 4]),
    Write8([u8; 8]),
    Write16([u8; 16]),
}

impl StoreBytes {
    #[inline(always)]
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Write1(bytes) => bytes,
            Self::Write2(bytes) => bytes,
            Self::Write4(bytes) => bytes,
            Self::Write8(bytes) => bytes,
            Self::Write16(bytes) => bytes,
        }
    }
}

#[inline(always)]
pub(crate) fn compute_memory_offset(memarg: MemArg, offset: u32) -> VMResult<usize> {
    let sum = memarg.offset as u64 + offset as u64;
    if sum <= u32::MAX as u64 {
        VMResult::Success(sum as usize)
    } else {
        VMResult::MemoryIndexOutOfRange
    }
}

enum CallOutcome {
    Immediate(*const Instr),
    Pending,
}

/// Telomere runtime helper `call_code`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Updates `ctx.cont` and performs the direct-threaded tail jump into the next instruction.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn call_code(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    ctx.cont = tail_code;
    let op = (*tail_code).op;
    op(tail_code.offset(1), ctx)
}

#[inline(always)]
/// Telomere runtime helper `call_next`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: internal runtime continuation dispatch.
/// Traps: propagates the trap behavior of the target instruction.
/// Notes: Advances from the current instruction by `consumed` operands and delegates to `call_code` without introducing non-tail cleanup.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn call_next(
    tail_code: *const Instr,
    consumed: isize,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    call_code(tail_code.offset(consumed), ctx)
}

fn result_type_size(ty: &ResultType) -> usize {
    ty.iter().map(|value| value.stack_size().usize()).sum()
}

fn push_typed_value(stack: &mut Stack, ty: ValType, value: &WasmValue) -> VMResult<()> {
    match (ty, value) {
        (ValType::I32, WasmValue::I32(value)) => stack.push_i32(*value),
        (ValType::I64, WasmValue::I64(value)) => stack.push_i64(*value),
        (ValType::F32, WasmValue::F32(value)) => stack.push_f32(*value),
        (ValType::F64, WasmValue::F64(value)) => stack.push_f64(*value),
        (ValType::V128, WasmValue::V128(value)) => stack.push_u128(*value),
        (ValType::FuncRef, WasmValue::FuncRef(value)) => stack.push_u32(*value),
        (ValType::ExternRef, WasmValue::ExternRef(value)) => stack.push_u32(*value),
        _ => VMResult::InvalidOperand,
    }
}

fn push_result_values(stack: &mut Stack, types: &ResultType, values: &ResultValue) -> VMResult<()> {
    if types.0.len() != values.len() {
        return VMResult::InvalidOperand;
    }
    for (ty, value) in types.iter().zip(values.iter()) {
        vm_try!(push_typed_value(stack, *ty, value));
    }
    VMResult::Success(())
}

fn pop_result_values(stack: &mut Stack, ty: &ResultType) -> ResultValue {
    let mut result = ty
        .stack_pop_iter()
        .map(|t| match t {
            ValType::I32 => WasmValue::I32(stack.pop_i32()),
            ValType::I64 => WasmValue::I64(stack.pop_i64()),
            ValType::F32 => WasmValue::F32(stack.pop_f32()),
            ValType::F64 => WasmValue::F64(stack.pop_f64()),
            ValType::FuncRef => WasmValue::FuncRef(stack.pop_u32()),
            ValType::ExternRef => WasmValue::ExternRef(stack.pop_u32()),
            ValType::V128 => WasmValue::V128(stack.pop_u128()),
        })
        .collect::<Vec<_>>();
    result.reverse();
    ResultValue::new(result)
}

fn start_async_host_call_with(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
    async_host: crate::common::AsyncHostFunction,
) -> VMResult<CallOutcome> {
    let task_id = ctx.task_id;
    let future = async_host(ctx);
    ctx.effect
        .push_pending(PendingOp::HostCall(HostCallPending { task_id, future }));
    ctx.cont = return_addr;
    VMResult::Success(CallOutcome::Pending)
}

fn start_async_host_call(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    let async_host = ctx.func().async_host_code_pointer();
    start_async_host_call_with(return_addr, ctx, async_host)
}

fn invoke_sync_host_function_with(
    _return_addr: *const Instr,
    ctx: &mut ExecuteContext,
    fp: crate::common::HostFunction,
) -> VMResult<CallOutcome> {
    let return_addr = vm_try!(fp(ctx));
    VMResult::Success(CallOutcome::Immediate(return_addr))
}

fn invoke_host_function(
    return_addr: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<CallOutcome> {
    if ctx.func().is_async_host_func() {
        start_async_host_call(return_addr, ctx)
    } else {
        let fp = ctx.func().host_code_pointer();
        invoke_sync_host_function_with(return_addr, ctx, fp)
    }
}

#[cfg(feature = "threads")]
pub(crate) use atomics::*;
pub(crate) use bulk_memory::{
    op_data_drop, op_mem_copy_indexed_local_local, op_mem_copy_indexed_local_shared,
    op_mem_copy_indexed_shared_local, op_mem_copy_indexed_shared_shared, op_mem_copy_local,
    op_mem_copy_shared, op_mem_fill_indexed_local, op_mem_fill_indexed_shared, op_mem_fill_local,
    op_mem_fill_shared, op_mem_init_indexed_local, op_mem_init_indexed_shared, op_mem_init_local,
    op_mem_init_shared,
};
pub(crate) use call::{
    op_call, op_call_import, op_call_indirect, op_return_call, op_return_call_import,
    op_return_call_indirect, special_start_function_call,
};
pub use control::special_function_return;
pub(crate) use control::*;
pub(crate) use globals::*;
pub(crate) use locals::*;
pub(crate) use memory::*;
pub(crate) use numeric::*;
pub(crate) use refs::*;
pub(crate) use superinstructions::*;
pub(crate) use tables::*;

/// Telomere runtime helper `store_internal_local`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Materializes the store payload before consuming the address so the write can tail-dispatch through `call_next`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
/// - `make_operation` must not retain references into `ctx` after it returns because the helper will continue by tail-dispatching immediately after the write.
pub(crate) unsafe fn store_internal_local(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    trace!("op_store: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(unsafe { ctx.default_local_memory_mut_unchecked() }.write_bytes(start, bytes));
    call_next(tail_code, 1, ctx)
}

/// Telomere runtime helper `store_internal_shared`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/spec/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Materializes the store payload before consuming the address so the write can tail-dispatch through `call_next`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The active frame must have shared default memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
/// - `make_operation` must not retain references into `ctx` after it returns because the helper will continue by tail-dispatching immediately after the write.
pub(crate) unsafe fn store_internal_shared(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    trace!("op_store_shared: {:?} {}", memarg, offset);
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.default_shared_memory_id_unchecked(), start, bytes,));
    call_next(tail_code, 1, ctx)
}

/// Telomere runtime helper `store_internal_local_indexed`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the pre-decoded indexed local-memory fast path and tail-dispatches after consuming `memarg + memidx`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The memory index operand at `tail_code.add(1)` must be in-bounds and refer to a local memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn store_internal_local_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .local_write_bytes(ctx.local_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 2, ctx)
}

/// Telomere runtime helper `store_internal_shared_indexed`.
///
/// Related spec:
/// - Execution: https://webassembly.github.io/multi-memory/core/exec/instructions.html
///
/// Stack effect: `[i32, value] -> []`.
/// Traps: traps on out-of-bounds memory access.
/// Notes: Uses the pre-decoded indexed shared-memory fast path and tail-dispatches after consuming `memarg + memidx`.
///
/// # Safety
/// - `tail_code` must reference the active decoded instruction stream for the current frame.
/// - `ctx` must reference a live execution context for the same store, frame, and validated locals/stack layout.
/// - The memory index operand at `tail_code.add(1)` must be in-bounds and refer to a shared memory.
/// - Callers must not preserve borrows, locks, or guards across any tail-dispatch that this helper performs.
pub(crate) unsafe fn store_internal_shared_indexed(
    tail_code: *const Instr,
    ctx: &mut ExecuteContext,
    _label: &'static str,
    make_operation: impl FnOnce(&mut ExecuteContext) -> StoreBytes,
) -> VMResult<()> {
    let memarg = (*tail_code).operand.memarg;
    let memidx = (*tail_code.add(1)).operand.u32;
    let operation = make_operation(ctx);
    let offset = ctx.stack.pop_u32();
    let bytes = operation.as_slice();
    let start = vm_try!(compute_memory_offset(memarg, offset));
    vm_try!(ctx
        .gc
        .shared_write_bytes(ctx.shared_memory_id_at_unchecked(memidx), start, bytes));
    call_next(tail_code, 2, ctx)
}
pub(crate) const VM_END: Instr = Instr {
    op: special_function_vm_end,
};

pub(crate) const START_HOST_FUNCTION_PROGRAM: [Instr; 1] = [Instr {
    op: special_start_function_call,
}];

pub async fn run_module_function(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: &ResultValue,
) -> VMResult<ResultValue> {
    let mut driver = TokioDriver::new();
    run_module_function_with_driver(instance, store, name, args, &mut driver).await
}

pub async fn run_module_function_with_driver<D: ExecutionDriver>(
    instance: &InstanceHandle,
    store: &Store,
    name: &str,
    args: &ResultValue,
    driver: &mut D,
) -> VMResult<ResultValue> {
    let _dispatch_profile_guard = DispatchProfileRunGuard::new();
    if store.has_active_gc_on_current_thread() {
        tracing::error!(
            "run_module_function is unsupported while the same store GC is already active"
        );
        return VMResult::Unlinkable;
    }
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let gc = store.lock_gc();
        let instance = gc.get_instance(vm_try!(VMResult::from_option(
            instance.object_ref_for_store(store),
            || { VMResult::Unlinkable }
        )));
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = *vm_try!(VMResult::from_option(
                instance.funcs.as_slice().get(idx.0 as usize),
                || { VMResult::Unlinkable }
            ));
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(
                code_addr,
                funcinst,
                func_instance
                    .memory_slots
                    .first()
                    .copied()
                    .and_then(|slot| slot.handle()),
            );
            let mut stack = Stack::new(128 * 1024);
            let tidx = *vm_try!(VMResult::from_option(
                module_inst.functions.get(idx.0 as usize),
                || { VMResult::Unlinkable }
            ));
            let ft = vm_try!(VMResult::from_option(
                module_inst.function_types.get(tidx.0 as usize),
                || { VMResult::Unlinkable }
            ))
            .clone();
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            vm_try!(push_result_values(&mut stack, &ft.0, args));

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = vm_try!(stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &VM_END as *const Instr,
                &gc,
            ));

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            return VMResult::Unlinkable;
        };
        ft
    };
    scheduler.run_with_driver(driver).await;
    let ct = scheduler.completed_tasks.pop().unwrap();
    vm_try!(ct.result);
    let mut stack = ct.stack;
    VMResult::Success(pop_result_values(&mut stack, &ft.1))
}

pub(crate) fn run_module_function_sync_with_gc(
    instance: &InstanceHandle,
    store: &Store,
    gc: &mut crate::common::StoreInner,
    name: &str,
    args: &ResultValue,
) -> Result<VMResult<ResultValue>, SyncRunError> {
    let _dispatch_profile_guard = DispatchProfileRunGuard::new();
    let mut scheduler: Scheduler<'_> = Scheduler::new(store);

    let ft = {
        let instance = gc.get_instance(match instance.object_ref_for_store(store) {
            Some(object_ref) => object_ref,
            None => return Ok(VMResult::Unlinkable),
        });
        let module_inst = gc.get_module(instance.module_addr);
        trace!("{:?}", module_inst.exports);
        let ft = if let Some(ExportDesc::Func(idx)) = module_inst.exports.find(name) {
            let code_addr = match instance.funcs.as_slice().get(idx.0 as usize) {
                Some(code_addr) => *code_addr,
                None => return Ok(VMResult::Unlinkable),
            };
            let funcinst = gc.get_func(code_addr);
            let func_instance = gc.instance(funcinst.instance);
            let frame = CallFrameCache::from_parts(
                code_addr,
                funcinst,
                func_instance
                    .memory_slots
                    .first()
                    .copied()
                    .and_then(|slot| slot.handle()),
            );
            let mut stack = Stack::new(128 * 1024);
            let tidx = match module_inst.functions.get(idx.0 as usize) {
                Some(tidx) => *tidx,
                None => return Ok(VMResult::Unlinkable),
            };
            let ft = match module_inst.function_types.get(tidx.0 as usize) {
                Some(ft) => ft.clone(),
                None => return Ok(VMResult::Unlinkable),
            };
            let param_size = result_type_size(&ft.0);

            let locals_data = funcinst.locals();
            let local_size = locals_data.byte_size();
            let push_result = push_result_values(&mut stack, &ft.0, args);
            if !matches!(push_result, VMResult::Success(())) {
                return Ok(vm_result_err_into_result_value(push_result));
            }

            tracing::trace!("run_module_function: {name} {local_size}");
            let local_reference = match stack.function_call(
                param_size,
                local_size,
                frame,
                LocalReference {
                    local_size: 0,
                    local_top: 0,
                },
                &VM_END as *const Instr,
                gc,
            ) {
                VMResult::Success(local_reference) => local_reference,
                other => return Ok(vm_result_err_into_result_value(other)),
            };

            scheduler.push(Task {
                fp: StablePc::from_relative_index(0),
                task_id: 0,
                stack,
                local_reference,
                ready_flag: ReadyFlag::Ready,
                pending_effects: 0,
                terminal_result: None,
            });
            ft
        } else {
            return Ok(VMResult::Unlinkable);
        };
        ft
    };

    scheduler.run_sync_with_gc(gc)?;
    let ct = scheduler.completed_tasks.pop().unwrap();
    match ct.result {
        VMResult::Success(()) => {
            let mut stack = ct.stack;
            Ok(VMResult::Success(pop_result_values(&mut stack, &ft.1)))
        }
        VMResult::Unreachable => Ok(VMResult::Unreachable),
        VMResult::StackOverflow => Ok(VMResult::StackOverflow),
        VMResult::MemoryIndexOutOfRange => Ok(VMResult::MemoryIndexOutOfRange),
        VMResult::TableIndexOutOfRange => Ok(VMResult::TableIndexOutOfRange),
        VMResult::CallIndirectInvalidType => Ok(VMResult::CallIndirectInvalidType),
        VMResult::TableUninitialized => Ok(VMResult::TableUninitialized),
        VMResult::Unlinkable => Ok(VMResult::Unlinkable),
        VMResult::InvalidOperand => Ok(VMResult::InvalidOperand),
        VMResult::UnalignedAtomic => Ok(VMResult::UnalignedAtomic),
    }
}

fn vm_result_err_into_result_value<T>(result: VMResult<T>) -> VMResult<ResultValue> {
    match result {
        VMResult::Success(_) => unreachable!(),
        VMResult::Unreachable => VMResult::Unreachable,
        VMResult::StackOverflow => VMResult::StackOverflow,
        VMResult::MemoryIndexOutOfRange => VMResult::MemoryIndexOutOfRange,
        VMResult::TableIndexOutOfRange => VMResult::TableIndexOutOfRange,
        VMResult::CallIndirectInvalidType => VMResult::CallIndirectInvalidType,
        VMResult::TableUninitialized => VMResult::TableUninitialized,
        VMResult::Unlinkable => VMResult::Unlinkable,
        VMResult::InvalidOperand => VMResult::InvalidOperand,
        VMResult::UnalignedAtomic => VMResult::UnalignedAtomic,
    }
}

fn read_global_value(bytes: &[u8], ty: ValType) -> Option<WasmValue> {
    match ty {
        ValType::I32 => Some(WasmValue::I32(i32::from_le_bytes(bytes.try_into().ok()?))),
        ValType::I64 => Some(WasmValue::I64(i64::from_le_bytes(bytes.try_into().ok()?))),
        ValType::F32 => Some(WasmValue::F32(f32::from_bits(u32::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::F64 => Some(WasmValue::F64(f64::from_bits(u64::from_le_bytes(
            bytes.try_into().ok()?,
        )))),
        ValType::V128 => Some(WasmValue::V128(u128::from_le_bytes(bytes.try_into().ok()?))),
        ValType::FuncRef => Some(WasmValue::FuncRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
        ValType::ExternRef => Some(WasmValue::ExternRef(u32::from_le_bytes(
            bytes.try_into().ok()?,
        ))),
    }
}

pub fn get_global(instance: &InstanceHandle, store: &Store, name: &str) -> VMResult<WasmValue> {
    if store.has_active_gc_on_current_thread() {
        tracing::error!("get_global is unsupported while the same store GC is already active");
        return VMResult::Unlinkable;
    }
    let gc = store.lock_gc();

    let instance = unsafe {
        &*gc.get_instance_unchecked(vm_try!(VMResult::from_option(
            instance.object_ref_for_store(store),
            || { VMResult::Unlinkable }
        )))
    };
    let module_inst = gc.get_module(instance.module_addr);
    let Some(ExportDesc::Global(idx)) = module_inst.exports.find(name) else {
        return VMResult::Unlinkable;
    };
    let addr = *vm_try!(VMResult::from_option(
        instance.globals.as_slice().get(idx.0 as usize),
        || { VMResult::Unlinkable }
    ));
    let gt = *vm_try!(VMResult::from_option(
        module_inst.globals.get(idx.0 as usize),
        || { VMResult::Unlinkable }
    ));
    let Some(value) = read_global_value(gc.get_global(addr), gt.0) else {
        return VMResult::Unlinkable;
    };
    VMResult::Success(value)
}

#[cfg(all(test, feature = "vm-profile"))]
mod tests {
    use super::*;

    fn count_label(stats: &[(&'static str, DispatchProfileStat)], label: &'static str) -> u64 {
        stats
            .iter()
            .find_map(|(candidate, stat)| (*candidate == label).then_some(stat.count))
            .unwrap_or_default()
    }

    #[test]
    fn dispatch_profile_tracks_pairs_triples_and_hot_layout_spans() {
        let _enabled = DispatchProfileTestOverride::enable();
        {
            let _guard = DispatchProfileRunGuard::new();
            dispatch_profile_count("op_local_get4");
            dispatch_profile_count("op_local_get4_i32_eqz_br_if");
            dispatch_profile_count("op_i32_load_local_base");
            dispatch_profile_count("op_i32_store_local_base");
            dispatch_profile_count("op_i32_load_local_base");
        }

        let snapshot =
            take_last_dispatch_profile_snapshot_for_test().expect("profile snapshot must exist");
        assert_eq!(count_label(&snapshot.stats, "op_local_get4"), 1);
        assert_eq!(count_label(&snapshot.stats, "op_i32_load_local_base"), 2);

        let pair = snapshot
            .pairs
            .iter()
            .find_map(|((lhs, rhs), stat)| {
                (*lhs == "op_local_get4_i32_eqz_br_if" && *rhs == "op_i32_load_local_base")
                    .then_some(stat.count)
            })
            .expect("hot pair must be recorded");
        assert_eq!(pair, 1);
        assert_eq!(
            layout_span_for_pair(("op_local_get4_i32_eqz_br_if", "op_i32_load_local_base")),
            1
        );

        let triple = snapshot
            .triples
            .iter()
            .find_map(|((a, b, c), stat)| {
                (*a == "op_local_get4_i32_eqz_br_if"
                    && *b == "op_i32_load_local_base"
                    && *c == "op_i32_store_local_base")
                    .then_some(stat.count)
            })
            .expect("hot triple must be recorded");
        assert_eq!(triple, 1);
        assert_eq!(
            layout_span_for_triple((
                "op_local_get4_i32_eqz_br_if",
                "op_i32_load_local_base",
                "op_i32_store_local_base",
            )),
            1
        );
    }

    #[test]
    fn handler_layout_groups_match_hot_path_modules() {
        assert_eq!(
            handler_layout_group("op_local_get4"),
            HandlerLayoutGroup::Locals
        );
        assert_eq!(
            handler_layout_group("op_local_get4_i32_eqz_br_if"),
            HandlerLayoutGroup::Superinstructions
        );
        assert_eq!(
            handler_layout_group("op_i32_store_local_base"),
            HandlerLayoutGroup::Memory
        );
        assert_eq!(handler_layout_group("op_call"), HandlerLayoutGroup::Call);
        assert_eq!(
            handler_layout_group("op_br_if"),
            HandlerLayoutGroup::Control
        );
    }

    #[test]
    fn dispatch_profile_groups_hot_stats_by_phase0_buckets() {
        let _enabled = DispatchProfileTestOverride::enable();
        {
            let _guard = DispatchProfileRunGuard::new();
            dispatch_profile_count("op_local_get4_i32_eqz_br_if");
            dispatch_profile_count("op_i32_load_local_base");
            dispatch_profile_count("op_call");
            dispatch_profile_count("op_select4");
            dispatch_profile_count("op_i32_load_local_base");
        }

        let snapshot =
            take_last_dispatch_profile_snapshot_for_test().expect("profile snapshot must exist");
        let grouped_stats = grouped_profile_stats(&snapshot.stats);

        assert_eq!(
            grouped_stats[DispatchProfileFamilyGroup::LocalControl.index()][0].0,
            "op_local_get4_i32_eqz_br_if"
        );
        assert_eq!(
            grouped_stats[DispatchProfileFamilyGroup::Memory.index()][0].0,
            "op_i32_load_local_base"
        );
        assert_eq!(
            dispatch_profile_family_group("op_select4"),
            DispatchProfileFamilyGroup::CallSelect
        );
        let call_select_labels = grouped_stats[DispatchProfileFamilyGroup::CallSelect.index()]
            .iter()
            .map(|(label, _)| *label)
            .collect::<Vec<_>>();
        assert!(call_select_labels.contains(&"op_call"));
        assert!(call_select_labels.contains(&"op_select4"));
    }
}
