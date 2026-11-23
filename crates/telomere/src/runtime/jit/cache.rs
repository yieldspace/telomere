use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

use parking_lot::Mutex;

use crate::common::ObjectRef;
#[cfg(feature = "jit")]
use crate::common::{
    store::{FunctionBody, StoreInner},
    VMResult,
};

#[cfg(feature = "jit")]
use super::backend;
use super::{
    abi::JitEntry,
    profile::{self, Counter},
};
use telomere_jit_codegen::code_memory::{CodeArena, ExecutableCode};

#[derive(Default)]
pub(crate) struct StoreJitCache {
    inner: Mutex<StoreJitCacheInner>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct JitCacheStats {
    pub compiled_functions: usize,
    pub rejected_functions: usize,
    pub used_bytes: usize,
    /// Total bytes held by the cached functions' #209 trap-site tables.
    /// Included in `used_bytes`; reported separately so the table cost
    /// of trap pc attribution stays visible.
    pub trap_table_bytes: usize,
}

#[derive(Default)]
struct StoreJitCacheInner {
    compiled: HashMap<ObjectRef, CachedFunction>,
    disabled: HashSet<ObjectRef>,
    arena: CodeArena,
    used_bytes: usize,
    clock: u64,
}

struct CachedFunction {
    tiers: CompiledTiers,
    last_used: u64,
}

struct CompiledTiers {
    baseline: Arc<CompiledFunction>,
    optimized: Option<Arc<CompiledFunction>>,
    active: ActiveTier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActiveTier {
    Baseline,
    #[allow(dead_code)]
    Optimized,
}

pub(crate) struct CompiledFunction {
    entry: JitEntry,
    code_size: usize,
    _code: ExecutableCode,
    /// #209 trap-site table: `trap_sites[site - 1]` is the
    /// decoded-stream pc index of the instruction that emitted trap
    /// site `site`.
    trap_sites: Box<[u32]>,
}

impl StoreJitCache {
    /// Returns the compiled function installed for `funcaddr`, if any,
    /// refreshing its eviction recency.
    ///
    /// This consult is the whole native-code read path in an `aot-load`-only
    /// build: code arrives in the cache through an AoT loader (or, with the
    /// `jit` feature, through [`StoreJitCache::get_or_compile`]), and a miss
    /// simply means the function has no native code in this store.
    pub(crate) fn get_cached(&self, funcaddr: ObjectRef) -> Option<Arc<CompiledFunction>> {
        let mut inner = self.inner.lock();
        inner.clock = inner.clock.wrapping_add(1);
        let clock = inner.clock;
        let cached = inner.compiled.get_mut(&funcaddr)?;
        profile::count(Counter::CompileHit);
        cached.last_used = clock;
        Some(cached.tiers.active().clone())
    }

    #[cfg(feature = "jit")]
    pub(crate) fn get_or_compile(
        &self,
        funcaddr: ObjectRef,
        body: &FunctionBody,
        runtime: &StoreInner,
        max_bytes: u32,
        metered: bool,
    ) -> VMResult<Arc<CompiledFunction>> {
        profile::count(Counter::CompileAttempt);
        let max_bytes = max_bytes as usize;
        if max_bytes == 0 {
            profile::count(Counter::CompileRejectMaxZero);
            trace_compile_reject("max_zero", 0, 0, max_bytes);
            return VMResult::Unimplemented;
        }

        {
            let inner = self.inner.lock();
            if inner.disabled.contains(&funcaddr) {
                profile::count(Counter::CompileRejectDisabled);
                trace_compile_reject("disabled", 0, 0, max_bytes);
                return VMResult::Unimplemented;
            }
        }
        if let Some(compiled) = self.get_cached(funcaddr) {
            return VMResult::Success(compiled);
        }

        let FunctionBody::Wasm { code, op_lens, .. } = body else {
            return VMResult::Unimplemented;
        };
        let emitted =
            match backend::emit_baseline_function(funcaddr, code, op_lens, runtime, metered) {
                Ok(emitted) => emitted,
                Err(backend::EmitBaselineError::Verify) => {
                    profile::count(Counter::CompileRejectVerify);
                    trace_compile_reject("verify", 0, 0, max_bytes);
                    runtime.mark_jit_rejected_func(funcaddr);
                    self.mark_rejected(funcaddr);
                    return VMResult::Unimplemented;
                }
                Err(backend::EmitBaselineError::Emit) => {
                    profile::count(Counter::CompileRejectEmit);
                    trace_compile_reject("emit", 0, 0, max_bytes);
                    runtime.mark_jit_rejected_func(funcaddr);
                    self.mark_rejected(funcaddr);
                    return VMResult::Unimplemented;
                }
            };
        // The cap is charged in emitted code bytes (16-byte aligned), not
        // in page-rounded mapping sizes: functions are packed into code
        // slabs, so residency is bounded by code size, not page count.
        let allocation_len = match CodeArena::accounted_len(emitted.code.len()) {
            Ok(len) => len,
            Err(_) => {
                profile::count(Counter::CompileRejectAllocationLen);
                trace_compile_reject("allocation_len", emitted.code.len(), 0, max_bytes);
                runtime.mark_jit_rejected_func(funcaddr);
                self.mark_rejected(funcaddr);
                return VMResult::Unimplemented;
            }
        };
        // #209: the trap-site table is resident per compiled function,
        // so it is charged against the same cap as the code itself.
        let table_bytes = emitted.trap_sites.len() * std::mem::size_of::<u32>();
        let resident_len = allocation_len + table_bytes;
        if resident_len > max_bytes {
            profile::count(Counter::CompileRejectTooLarge);
            trace_compile_reject("too_large", emitted.code.len(), resident_len, max_bytes);
            runtime.mark_jit_rejected_func(funcaddr);
            self.mark_rejected(funcaddr);
            return VMResult::Unimplemented;
        }

        let mut inner = self.inner.lock();
        inner.clock = inner.clock.wrapping_add(1);
        let clock = inner.clock;
        if inner.disabled.contains(&funcaddr) {
            profile::count(Counter::CompileRejectDisabledAfterEmit);
            trace_compile_reject(
                "disabled_after_emit",
                emitted.code.len(),
                resident_len,
                max_bytes,
            );
            return VMResult::Unimplemented;
        }
        if let Some(cached) = inner.compiled.get_mut(&funcaddr) {
            profile::count(Counter::CompileHit);
            cached.last_used = clock;
            return VMResult::Success(cached.tiers.active().clone());
        }
        inner.evict_until(max_bytes.saturating_sub(resident_len));
        let compiled =
            match CompiledFunction::from_bytes(&emitted.code, emitted.trap_sites, &inner.arena) {
                Ok(compiled) => Arc::new(compiled),
                Err(()) => {
                    profile::count(Counter::CompileRejectFromBytes);
                    trace_compile_reject("from_bytes", emitted.code.len(), resident_len, max_bytes);
                    runtime.mark_jit_rejected_func(funcaddr);
                    inner.disabled.insert(funcaddr);
                    return VMResult::Unimplemented;
                }
            };
        profile::count(Counter::CompileAccepted);
        profile::add(Counter::CompileBytes, emitted.code.len() as u64);
        inner.used_bytes = inner.used_bytes.saturating_add(compiled.resident_bytes());
        inner.compiled.insert(
            funcaddr,
            CachedFunction {
                tiers: CompiledTiers::baseline(compiled.clone()),
                last_used: clock,
            },
        );
        VMResult::Success(compiled)
    }

    pub(crate) fn touch_compiled(
        &self,
        funcaddr: ObjectRef,
        compiled: &Arc<CompiledFunction>,
    ) -> bool {
        let mut inner = self.inner.lock();
        inner.clock = inner.clock.wrapping_add(1);
        let clock = inner.clock;
        let Some(cached) = inner.compiled.get_mut(&funcaddr) else {
            return false;
        };
        if !Arc::ptr_eq(cached.tiers.active(), compiled) {
            return false;
        }
        cached.last_used = clock;
        true
    }

    fn mark_rejected(&self, funcaddr: ObjectRef) {
        let mut inner = self.inner.lock();
        if let Some(cached) = inner.compiled.remove(&funcaddr) {
            inner.used_bytes = inner
                .used_bytes
                .saturating_sub(cached.tiers.resident_bytes());
        }
        inner.disabled.insert(funcaddr);
    }

    pub(crate) fn stats(&self) -> JitCacheStats {
        let inner = self.inner.lock();
        JitCacheStats {
            compiled_functions: inner.compiled.len(),
            rejected_functions: inner.disabled.len(),
            used_bytes: inner.used_bytes,
            trap_table_bytes: inner
                .compiled
                .values()
                .map(|cached| cached.tiers.trap_table_bytes())
                .sum(),
        }
    }
}

fn trace_compile_reject(reason: &'static str, bytes_len: usize, allocation_len: usize, max: usize) {
    if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
        static COMPILE_TRACE_COUNT: AtomicUsize = AtomicUsize::new(0);
        let max_count = std::env::var("TELOMERE_JIT_TRACE_COMPILE_MAX")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(64);
        let index = COMPILE_TRACE_COUNT.fetch_add(1, Ordering::Relaxed);
        if index >= max_count {
            return;
        }
        eprintln!(
            "[telomere-jit] compile_reject reason={reason} bytes={bytes_len} allocation={allocation_len} max={max}"
        );
    }
    if tracing::level_enabled!(tracing::Level::DEBUG) {
        tracing::debug!(
            target: crate::diagnostics::TARGET_JIT,
            reason,
            bytes = bytes_len,
            allocation = allocation_len,
            max,
            "jit compilation rejected"
        );
    }
}

impl StoreJitCacheInner {
    fn evict_until(&mut self, target_used_bytes: usize) {
        while self.used_bytes > target_used_bytes {
            let Some((&funcaddr, _)) = self
                .compiled
                .iter()
                .min_by_key(|(_, cached)| cached.last_used)
            else {
                break;
            };
            let Some(cached) = self.compiled.remove(&funcaddr) else {
                break;
            };
            self.used_bytes = self
                .used_bytes
                .saturating_sub(cached.tiers.resident_bytes());
        }
    }
}

impl CompiledTiers {
    fn baseline(baseline: Arc<CompiledFunction>) -> Self {
        Self {
            baseline,
            optimized: None,
            active: ActiveTier::Baseline,
        }
    }

    fn active(&self) -> &Arc<CompiledFunction> {
        match self.active {
            ActiveTier::Baseline => &self.baseline,
            ActiveTier::Optimized => self.optimized.as_ref().unwrap_or(&self.baseline),
        }
    }

    #[cfg(test)]
    fn code_size(&self) -> usize {
        self.baseline.code_size()
            + self
                .optimized
                .as_ref()
                .map(|compiled| compiled.code_size())
                .unwrap_or(0)
    }

    fn resident_bytes(&self) -> usize {
        self.baseline.resident_bytes()
            + self
                .optimized
                .as_ref()
                .map(|compiled| compiled.resident_bytes())
                .unwrap_or(0)
    }

    fn trap_table_bytes(&self) -> usize {
        self.baseline.trap_table_bytes()
            + self
                .optimized
                .as_ref()
                .map(|compiled| compiled.trap_table_bytes())
                .unwrap_or(0)
    }
}

impl CompiledFunction {
    fn from_bytes(bytes: &[u8], trap_sites: Vec<u32>, arena: &CodeArena) -> Result<Self, ()> {
        let code = arena.allocate(bytes).map_err(|_| ())?;
        let entry = unsafe { std::mem::transmute::<usize, JitEntry>(code.ptr() as usize) };
        Ok(Self {
            entry,
            code_size: code.len(),
            _code: code,
            trap_sites: trap_sites.into_boxed_slice(),
        })
    }

    pub(crate) fn entry(&self) -> JitEntry {
        self.entry
    }

    fn code_size(&self) -> usize {
        self.code_size
    }

    /// Bytes charged to the code-cache budget for this function: the
    /// emitted code plus its #209 trap-site table.
    fn resident_bytes(&self) -> usize {
        self.code_size() + self.trap_table_bytes()
    }

    /// Bytes held by this function's #209 trap-site table.
    pub(crate) fn trap_table_bytes(&self) -> usize {
        self.trap_sites.len() * std::mem::size_of::<u32>()
    }

    /// #209: resolves a 1-based trap-site id to the decoded-stream pc
    /// index recorded for it. Site 0 is the no-site encoding and never
    /// resolves; out-of-range ids return `None` so a trap degrades to
    /// an unattributed pc instead of a wrong one.
    pub(crate) fn trap_pc(&self, site: u64) -> Option<u32> {
        let index = usize::try_from(site.checked_sub(1)?).ok()?;
        self.trap_sites.get(index).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Instr;

    #[test]
    fn compiled_tiers_start_with_active_baseline() {
        let compiled = test_compiled(32);
        let tiers = CompiledTiers::baseline(compiled.clone());

        assert!(Arc::ptr_eq(tiers.active(), &compiled));
        assert_eq!(tiers.code_size(), 32);
    }

    #[test]
    fn get_cached_consults_installed_code_without_compiling() {
        let cache = StoreJitCache::default();
        let func = ObjectRef(7);
        assert!(cache.get_cached(func).is_none());

        let compiled = test_compiled(32);
        {
            let mut inner = cache.inner.lock();
            inner.used_bytes = compiled.code_size();
            inner.compiled.insert(
                func,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled.clone()),
                    last_used: 0,
                },
            );
        }

        let hit = cache
            .get_cached(func)
            .expect("installed function must be consultable");
        assert!(Arc::ptr_eq(&hit, &compiled));

        let inner = cache.inner.lock();
        assert_eq!(
            inner.compiled.get(&func).unwrap().last_used,
            2,
            "a consult refreshes eviction recency (one miss + one hit on the clock)"
        );
    }

    #[test]
    fn touch_compiled_refreshes_lru_before_eviction() {
        let cache = StoreJitCache::default();
        let func_a = ObjectRef(1);
        let func_b = ObjectRef(2);
        let compiled_a = test_compiled(32);
        let compiled_b = test_compiled(32);

        {
            let mut inner = cache.inner.lock();
            inner.clock = 2;
            inner.used_bytes = compiled_a.code_size() + compiled_b.code_size();
            inner.compiled.insert(
                func_a,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled_a.clone()),
                    last_used: 1,
                },
            );
            inner.compiled.insert(
                func_b,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled_b.clone()),
                    last_used: 2,
                },
            );
        }

        assert!(cache.touch_compiled(func_a, &compiled_a));

        let mut inner = cache.inner.lock();
        let target_used_bytes = inner.used_bytes.saturating_sub(compiled_b.code_size());
        inner.evict_until(target_used_bytes);

        assert!(inner.compiled.contains_key(&func_a));
        assert!(!inner.compiled.contains_key(&func_b));
        assert_eq!(inner.compiled.get(&func_a).unwrap().last_used, 3);
    }

    #[test]
    fn touch_compiled_rejects_stale_weak_entry() {
        let cache = StoreJitCache::default();
        let func = ObjectRef(3);
        let current = test_compiled(32);
        let stale = test_compiled(32);

        {
            let mut inner = cache.inner.lock();
            inner.clock = 1;
            inner.used_bytes = current.code_size();
            inner.compiled.insert(
                func,
                CachedFunction {
                    tiers: CompiledTiers::baseline(current),
                    last_used: 1,
                },
            );
        }

        assert!(!cache.touch_compiled(func, &stale));

        let inner = cache.inner.lock();
        assert_eq!(inner.compiled.get(&func).unwrap().last_used, 1);
    }

    fn test_compiled(code_size: usize) -> Arc<CompiledFunction> {
        let code = ExecutableCode::test_stub(code_size);
        Arc::new(CompiledFunction {
            entry: noop_entry,
            code_size: code.len(),
            _code: code,
            trap_sites: Box::default(),
        })
    }

    unsafe extern "C" fn noop_entry(
        _ctx: *mut crate::common::ExecuteContext<'_>,
        _code: *const Instr,
        _local_base: *mut u8,
    ) -> super::super::JitNativeExit {
        super::super::JitNativeExit::done()
    }
}
