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

#[cfg(feature = "vm-diagnostics")]
use crate::common::Op;
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
#[cfg(feature = "vm-diagnostics")]
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(feature = "vm-diagnostics")]
use std::sync::OnceLock as DiagnosticsOnceLock;
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

#[cfg(feature = "vm-diagnostics")]
struct DispatchBudget {
    initial: u64,
    remaining: AtomicU64,
    log_every: u64,
}

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

#[cfg(feature = "vm-diagnostics")]
fn parse_nonzero_env_u64(name: &str) -> Option<u64> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value != 0)
}

#[cfg(feature = "vm-diagnostics")]
fn dispatch_budget() -> Option<&'static DispatchBudget> {
    static BUDGET: DiagnosticsOnceLock<Option<DispatchBudget>> = DiagnosticsOnceLock::new();
    BUDGET
        .get_or_init(|| {
            let initial = parse_nonzero_env_u64("TELOMERE_VM_INSTR_BUDGET")?;
            let log_every = parse_nonzero_env_u64("TELOMERE_VM_INSTR_LOG_EVERY").unwrap_or(0);
            Some(DispatchBudget {
                initial,
                remaining: AtomicU64::new(initial),
                log_every,
            })
        })
        .as_ref()
}

#[cfg(feature = "vm-diagnostics")]
fn dispatch_pc_index(tail_code: *const Instr, ctx: &ExecuteContext<'_>) -> Option<usize> {
    let code_base = ctx.current_frame.code_base;
    if code_base.is_null() {
        return None;
    }
    let instr_size = std::mem::size_of::<Instr>();
    let base = code_base as usize;
    let pc = tail_code as usize;
    let delta = pc.checked_sub(base)?;
    (delta % instr_size == 0).then_some(delta / instr_size)
}

#[cfg(feature = "vm-diagnostics")]
pub(crate) fn diagnostic_op_label(op: Op) -> &'static str {
    macro_rules! label {
        ($handler:path, $name:literal) => {
            if std::ptr::fn_addr_eq(op, $handler as Op) {
                return $name;
            }
        };
    }

    label!(op_local_get4, "op_local_get4");
    label!(op_local_set4, "op_local_set4");
    label!(op_local_tee4, "op_local_tee4");
    label!(op_local_get4_set4, "op_local_get4_set4");
    label!(op_local_get4_tee4, "op_local_get4_tee4");
    label!(op_local_get4_local_get4, "op_local_get4_local_get4");
    label!(
        op_local_get4_local_get4_local_get4,
        "op_local_get4_local_get4_local_get4"
    );
    label!(op_local_get4_run, "op_local_get4_run");
    label!(op_local_get4_run_skip, "op_local_get4_run_skip");
    label!(
        op_local_get4x3_i32_add_const_binop_i32_add_set4,
        "op_local_get4x3_i32_add_const_binop_i32_add_set4"
    );
    label!(
        op_local_get4x3_i32_add_const_binop_i32_add_tee4,
        "op_local_get4x3_i32_add_const_binop_i32_add_tee4"
    );
    label!(
        op_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store,
        "op_local_get4x3_i32_add_const_binop_i32_add_tee4_i32_const_store"
    );
    label!(op_select4, "op_select4");
    label!(op_select4_set4, "op_select4_set4");
    label!(op_select4_tee4, "op_select4_tee4");
    label!(op_br_if, "op_br_if");
    label!(op_br_table, "op_br_table");
    label!(op_local_get4_br_table, "op_local_get4_br_table");
    label!(
        op_local_get4_i32_const_add_br_table,
        "op_local_get4_i32_const_add_br_table"
    );
    label!(op_if, "op_if");
    label!(op_loop, "op_loop");
    label!(op_end, "op_end");
    label!(op_return, "op_return");
    label!(special_block_return, "special_block_return");
    label!(special_function_return, "special_function_return");
    label!(special_function_vm_end, "special_function_vm_end");
    label!(op_call, "op_call");
    label!(op_i32_const, "op_i32_const");
    label!(op_i32_const_set4, "op_i32_const_set4");
    label!(op_i32_const_tee4, "op_i32_const_tee4");
    label!(op_i32_add, "op_i32_add");
    label!(op_i32_sub, "op_i32_sub");
    label!(op_i32_mul, "op_i32_mul");
    label!(op_i32_and, "op_i32_and");
    label!(op_i32_or, "op_i32_or");
    label!(op_i32_xor, "op_i32_xor");
    label!(op_i32_select_bit_step4, "op_i32_select_bit_step4");
    label!(op_i32_select_bit_step4_run, "op_i32_select_bit_step4_run");
    label!(op_i32_shl, "op_i32_shl");
    label!(op_i32_shr_s, "op_i32_shr_s");
    label!(op_i32_shr_u, "op_i32_shr_u");
    label!(op_i32_eq, "op_i32_eq");
    label!(op_i32_ne, "op_i32_ne");
    label!(op_i32_lt_u, "op_i32_lt_u");
    label!(op_i32_gt_s, "op_i32_gt_s");
    label!(op_i32_gt_u, "op_i32_gt_u");
    label!(op_i32_le_u, "op_i32_le_u");
    label!(op_i32_ge_s, "op_i32_ge_s");
    label!(op_local_binop32, "op_local_binop32");
    label!(op_local_binop32_set4, "op_local_binop32_set4");
    label!(op_local_binop32_tee4, "op_local_binop32_tee4");
    label!(op_local_binop32_br_if, "op_local_binop32_br_if");
    label!(
        op_local_get4_i32_const_add_set4,
        "op_local_get4_i32_const_add_set4"
    );
    label!(
        op_local_get4_i32_const_add_tee4,
        "op_local_get4_i32_const_add_tee4"
    );
    label!(
        op_local_get4_i32_const_add_tee4_br_if,
        "op_local_get4_i32_const_add_tee4_br_if"
    );
    label!(op_local_cmp32, "op_local_cmp32");
    label!(op_local_cmp32_set4, "op_local_cmp32_set4");
    label!(op_local_cmp32_tee4, "op_local_cmp32_tee4");
    label!(op_local_cmp32_br_if, "op_local_cmp32_br_if");
    label!(op_i32_const_binop, "op_i32_const_binop");
    label!(op_i32_const_binop_set4, "op_i32_const_binop_set4");
    label!(op_i32_const_binop_tee4, "op_i32_const_binop_tee4");
    label!(op_i32_const_binop_br_if, "op_i32_const_binop_br_if");
    label!(op_i32_const_cmp, "op_i32_const_cmp");
    label!(op_i32_const_cmp_set4, "op_i32_const_cmp_set4");
    label!(op_i32_const_cmp_tee4, "op_i32_const_cmp_tee4");
    label!(op_i32_const_cmp_br_if, "op_i32_const_cmp_br_if");
    label!(op_i32_load_local_base, "op_i32_load_local_base");
    label!(op_i32_load16_s_local_base, "op_i32_load16_s_local_base");
    label!(op_i32_load16_u_local_base, "op_i32_load16_u_local_base");
    label!(op_i32_load8_u_local_base, "op_i32_load8_u_local_base");
    label!(op_i32_store_local_base, "op_i32_store_local_base");
    label!(op_i32_store16_local_base, "op_i32_store16_local_base");
    label!(op_i32_store8_local_base, "op_i32_store8_local_base");
    label!(op_i32_inc_local_base, "op_i32_inc_local_base");
    label!(
        op_local_get4_i32_inc_local_base,
        "op_local_get4_i32_inc_local_base"
    );
    label!(
        op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4,
        "op_local_get4_i32_inc_local_base_i32_load8_u_local_base_set4"
    );
    label!(
        op_local_get4_i32_load8_u_local_base_set4,
        "op_local_get4_i32_load8_u_local_base_set4"
    );
    label!(
        op_i32_load8_u_local_base_set4_local_get4,
        "op_i32_load8_u_local_base_set4_local_get4"
    );
    label!(
        op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if"
    );
    label!(
        op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if"
    );
    label!(
        op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table,
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4_br_table"
    );
    label!(
        op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table,
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_false_local_get4_br_table"
    );
    label!(
        op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table,
        "op_i32_guarded_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_const_compare_br_table"
    );
    label!(
        op_i32_numeric_token_state_transition,
        "op_i32_numeric_token_state_transition"
    );
    label!(op_i32_core_state_benchmark, "op_i32_core_state_benchmark");
    label!(
        op_i32_matrix_i16_crc_summary,
        "op_i32_matrix_i16_crc_summary"
    );
    label!(op_i32_list_crc_summary, "op_i32_list_crc_summary");
    label!(op_i32_list_crc_pair_loop, "op_i32_list_crc_pair_loop");
    label!(
        op_call_i32_numeric_token_state_transition,
        "op_call_i32_numeric_token_state_transition"
    );
    label!(op_call_i32_crc16_update16, "op_call_i32_crc16_update16");
    label!(
        op_call_i32_crc16_update16_masked,
        "op_call_i32_crc16_update16_masked"
    );
    label!(
        op_call_cached_u16_low7_guard,
        "op_call_cached_u16_low7_guard"
    );
    label!(op_call_i32_list_crc_summary, "op_call_i32_list_crc_summary");
    label!(op_i32_crc16_update16, "op_i32_crc16_update16");
    label!(op_i32_crc16_update16_masked, "op_i32_crc16_update16_masked");
    label!(
        op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4,
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_taken_local_get4"
    );
    label!(
        op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4,
        "op_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if_fallthrough_local_get4"
    );
    label!(
        op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if,
        "op_i32_inc_local_base_i32_load8_u_local_base_set4_local_get4_set4_local_get4_br_if"
    );
    label!(
        op_i32_load16_s_mul_add_local_base_loop,
        "op_i32_load16_s_mul_add_local_base_loop"
    );
    label!(
        op_i32_load16_s_mul_add_local_base_delta_loop,
        "op_i32_load16_s_mul_add_local_base_delta_loop"
    );
    label!(
        op_i32_load16_u_bitmix_acc_local_base_delta_loop,
        "op_i32_load16_u_bitmix_acc_local_base_delta_loop"
    );
    label!(
        op_i32_load16_u_update_store16_local_base_loop,
        "op_i32_load16_u_update_store16_local_base_loop"
    );
    label!(
        op_i32_sum_clip_local_base_loop,
        "op_i32_sum_clip_local_base_loop"
    );
    label!(
        op_i32_load_store_local_base_local_get4,
        "op_i32_load_store_local_base_local_get4"
    );
    label!(
        op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4,
        "op_i32_load16_u_local_base_local_get4_i32_load16_u_local_get4"
    );
    label!(
        op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4,
        "op_i32_load16_s_local_base_local_get4_i32_load16_s_local_get4"
    );
    label!(
        op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u,
        "op_local_get4_i32_load16_u_local_base_local_get4_i32_load16_u"
    );
    label!(
        op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s,
        "op_local_get4_i32_load16_s_local_base_local_get4_i32_load16_s"
    );
    label!(
        op_scalar_copy_local_base_run,
        "op_scalar_copy_local_base_run"
    );
    label!(
        op_i32_load_store_local_base_relink_loop,
        "op_i32_load_store_local_base_relink_loop"
    );
    label!(
        op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if,
        "op_i32_load_local_base_local_get4_i32_load_tee4_cmp_br_if"
    );
    label!(
        op_i32_load16_s_dot4_local_base_loop,
        "op_i32_load16_s_dot4_local_base_loop"
    );
    label!(
        op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if,
        "op_i32_load_local_base_tee4_i32_load8_u_tee4_br_if"
    );
    "unknown"
}

#[cfg(feature = "vm-diagnostics")]
fn log_dispatch_budget_event(
    event: &str,
    executed: u64,
    tail_code: *const Instr,
    ctx: &ExecuteContext<'_>,
) {
    let pc = dispatch_pc_index(tail_code, ctx)
        .map(|pc| pc.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    let op_label = diagnostic_op_label(unsafe { (*tail_code).op });
    let funcidx = ctx
        .gc
        .instance(ctx.current_frame.instance)
        .funcs
        .iter()
        .position(|addr| *addr == ctx.current_frame.code_addr)
        .map(|index| index.to_string())
        .unwrap_or_else(|| "unknown".to_owned());
    eprintln!(
        "[telomere-vm-diagnostics] {event} executed_instrs={executed} funcidx={funcidx} code_addr={:?} pc={pc} op={op_label} op_addr=0x{:x} task_id={}",
        ctx.current_frame.code_addr,
        unsafe { (*tail_code).op as usize },
        ctx.task_id
    );
}

#[cfg(feature = "vm-diagnostics")]
fn check_dispatch_budget(tail_code: *const Instr, ctx: &ExecuteContext<'_>) -> VMResult<()> {
    let Some(budget) = dispatch_budget() else {
        return VMResult::Success(());
    };
    let previous = budget
        .remaining
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_sub(1)
        })
        .unwrap_or(0);
    if previous == 0 {
        log_dispatch_budget_event(
            "dispatch_budget_already_exhausted",
            budget.initial,
            tail_code,
            ctx,
        );
        return VMResult::InvalidOperand;
    }

    let executed = budget.initial.saturating_sub(previous).saturating_add(1);
    if budget.log_every != 0 && executed % budget.log_every == 0 {
        log_dispatch_budget_event("dispatch_budget_progress", executed, tail_code, ctx);
    }
    if previous == 1 {
        log_dispatch_budget_event("dispatch_budget_exhausted", executed, tail_code, ctx);
        return VMResult::InvalidOperand;
    }
    VMResult::Success(())
}

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
    match memarg.offset.checked_add(offset) {
        Some(sum) => VMResult::Success(sum as usize),
        None => VMResult::MemoryIndexOutOfRange,
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
#[inline(always)]
pub(crate) unsafe fn call_code(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
    #[cfg(feature = "vm-diagnostics")]
    vm_try!(check_dispatch_budget(tail_code, ctx));
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
#[cfg(feature = "jit")]
pub(crate) use call::jit_call_direct;
pub(crate) use call::{
    op_call, op_call_cached_u16_low7_guard, op_call_i32_crc16_update16,
    op_call_i32_crc16_update16_masked, op_call_i32_list_crc_summary,
    op_call_i32_numeric_token_state_transition, op_call_import, op_call_indirect, op_return_call,
    op_return_call_import, op_return_call_indirect, special_start_function_call,
};
#[cfg(feature = "jit")]
pub(crate) use call::{op_call_jit_lazy, op_return_call_jit_lazy, special_start_jit_function_call};
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
    let offset = ctx.stack.pop_u32_fast();
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
    let offset = ctx.stack.pop_u32_fast();
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
    let offset = ctx.stack.pop_u32_fast();
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
    let offset = ctx.stack.pop_u32_fast();
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

#[cfg(feature = "jit")]
pub(crate) const START_JIT_FUNCTION_PROGRAM: [Instr; 1] = [Instr {
    op: special_start_jit_function_call,
}];

pub(crate) fn wasm_entry_pc(_store: &Store) -> StablePc {
    #[cfg(feature = "jit")]
    {
        if crate::runtime::jit::supported() && _store.runtime_config().jit.enabled {
            return StablePc::from_stable_ptr(START_JIT_FUNCTION_PROGRAM.as_ptr());
        }
    }
    StablePc::from_relative_index(0)
}

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
                fp: wasm_entry_pc(store),
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
                fp: wasm_entry_pc(store),
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
        VMResult::Unimplemented => Ok(VMResult::Unimplemented),
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
        VMResult::Unimplemented => VMResult::Unimplemented,
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
    use crate::{IoReadBinaryReader, Registry, ResultValue, Store, WasmParser, WasmValue};

    async fn instantiate_wat(
        wat: &str,
        store: &Store,
        registry: &Registry,
    ) -> crate::common::InstanceHandle {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = IoReadBinaryReader::from(bytes.as_slice());
        let mut parser = WasmParser::new(&mut reader);
        let module = parser.parse_module().expect("module must parse");
        match crate::instantiate(module, store, registry).await {
            VMResult::Success(instance) => instance,
            other => panic!("module must instantiate, got {other:?}"),
        }
    }

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

    #[tokio::test]
    async fn local_get_handlers_contribute_dispatch_profile_labels() {
        let store = Store::new();
        let registry = Registry::new();
        let instance = instantiate_wat(
            r#"
            (module
              (func (export "run") (param i32 i64) (result i64)
                local.get 0
                drop
                local.get 1))
            "#,
            &store,
            &registry,
        )
        .await;

        let _enabled = DispatchProfileTestOverride::enable();
        let result = crate::run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(7), WasmValue::I64(11)]),
        )
        .await;

        match result {
            VMResult::Success(values) => {
                assert_eq!(values, ResultValue::new(vec![WasmValue::I64(11)]));
            }
            other => panic!("profiled local.get module must succeed, got {other:?}"),
        }

        let snapshot =
            take_last_dispatch_profile_snapshot_for_test().expect("profile snapshot must exist");
        assert!(
            count_label(&snapshot.stats, "op_local_get4") > 0,
            "local.get i32 must appear in dispatch profile: {:?}",
            snapshot.stats
        );
        assert!(
            count_label(&snapshot.stats, "op_local_get8") > 0,
            "local.get i64 must appear in dispatch profile: {:?}",
            snapshot.stats
        );
    }
}
