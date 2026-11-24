use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicU64, AtomicUsize, Ordering},
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
    /// Monotonic tick source for LRU recency stamps.
    ///
    /// #191: recency used to be refreshed by locking `inner` on every
    /// native-to-native call (`touch_compiled`). The stamp now lives on
    /// [`CompiledFunction`] as an atomic and this clock lives outside the
    /// mutex, so the direct call path can refresh recency with one
    /// `fetch_add` and a sampled store instead of a lock + hash lookup.
    /// The clock is only ever advanced by this store's own execution and
    /// cache maintenance (a store executes under its runtime lock), and
    /// the stamps are recency hints, so `Relaxed` ordering is enough: a
    /// stale read can only make eviction marginally less precise.
    clock: AtomicU64,
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
}

struct CachedFunction {
    tiers: CompiledTiers,
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
    /// LRU recency stamp, in [`StoreJitCache`] clock ticks.
    ///
    /// #191: this lives on the compiled function itself (not behind the
    /// cache mutex) so the direct native call path can refresh it
    /// without locking. It is a recency hint only; see
    /// [`StoreJitCache::note_direct_call`].
    last_used: AtomicU64,
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
        let clock = self.next_tick();
        let inner = self.inner.lock();
        let cached = inner.compiled.get(&funcaddr)?;
        profile::count(Counter::CompileHit);
        let active = cached.tiers.active().clone();
        active.note_used(clock);
        Some(active)
    }

    fn next_tick(&self) -> u64 {
        self.clock.fetch_add(1, Ordering::Relaxed).wrapping_add(1)
    }

    /// #191: recency refresh for the direct native-to-native call path.
    ///
    /// The caller already holds the `Arc<CompiledFunction>` it is about
    /// to execute (obtained by upgrading the per-store slot in
    /// `StoreInner`), so keeping the code alive never depended on this
    /// stamp; the stamp only steers eviction. To keep the hot path at
    /// one atomic read-modify-write, the store clock advances on every
    /// direct call but the callee's stamp is rewritten only once it is
    /// at least `DIRECT_CALL_TOUCH_INTERVAL` ticks stale, which bounds
    /// stamp stores to one per interval per function while keeping
    /// eviction order accurate to within that many calls.
    pub(crate) fn note_direct_call(&self, compiled: &CompiledFunction) {
        let clock = self.next_tick();
        if clock.wrapping_sub(compiled.last_used()) >= DIRECT_CALL_TOUCH_INTERVAL {
            compiled.note_used(clock);
        }
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

        let clock = self.next_tick();
        let mut inner = self.inner.lock();
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
        if let Some(cached) = inner.compiled.get(&funcaddr) {
            profile::count(Counter::CompileHit);
            let active = cached.tiers.active().clone();
            active.note_used(clock);
            return VMResult::Success(active);
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
        compiled.note_used(clock);
        inner.used_bytes = inner.used_bytes.saturating_add(compiled.resident_bytes());
        inner.compiled.insert(
            funcaddr,
            CachedFunction {
                tiers: CompiledTiers::baseline(compiled.clone()),
            },
        );
        VMResult::Success(compiled)
    }

    /// Identity + recency check for the interpreter-side entry path
    /// (`enter_current_frame_raw`): returns `true` when the cache still
    /// holds exactly this compiled function as the active tier for
    /// `funcaddr`, refreshing its recency stamp.
    ///
    /// #191: native-to-native calls no longer come through here (they
    /// use [`StoreJitCache::note_direct_call`] on the `Arc` they already
    /// hold); this remains for entries that start from the interpreter,
    /// where the per-frame slot may name code the cache has since
    /// evicted or replaced.
    pub(crate) fn touch_compiled(
        &self,
        funcaddr: ObjectRef,
        compiled: &Arc<CompiledFunction>,
    ) -> bool {
        let clock = self.next_tick();
        let inner = self.inner.lock();
        let Some(cached) = inner.compiled.get(&funcaddr) else {
            return false;
        };
        if !Arc::ptr_eq(cached.tiers.active(), compiled) {
            return false;
        }
        compiled.note_used(clock);
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

    /// Installs a batch of already-patched native functions
    /// atomically (#198 AoT loader): either every entry lands in the
    /// cache or none does. Entries naming a disabled (JIT-rejected)
    /// function, a batch that names one function twice, or a batch
    /// whose resident bytes exceed `max_bytes` fail with
    /// `Err(needed_bytes)` and leave the cache untouched.
    ///
    /// An entry naming an already-cached function is *accepted*, not
    /// an error: with native execution enabled, instantiation runs
    /// the module's start function before the caller can install, so
    /// lazy compilation may already have cached part of the batch.
    /// The cached entry is the same function's native code, so it is
    /// kept (recency refreshed) and its bytes are not charged twice.
    ///
    /// Every fresh mapping is allocated *before* any eviction or
    /// insertion: arena allocation never depends on eviction (the
    /// arena grows by fresh slabs), so an allocation failure still
    /// leaves the cache map and its accounting exactly as they were.
    /// Eviction of *other* cached functions to make room follows the
    /// same policy as on-demand compilation; batch entries themselves
    /// are never eviction victims.
    pub(crate) fn install_batch(
        &self,
        batch: &[(ObjectRef, Vec<u8>, Vec<u32>)],
        max_bytes: usize,
    ) -> Result<Vec<Arc<CompiledFunction>>, usize> {
        let mut resident_total: usize = 0;
        for (_, code, trap_sites) in batch {
            let allocation_len = CodeArena::accounted_len(code.len()).map_err(|_| usize::MAX)?;
            resident_total = resident_total
                .saturating_add(allocation_len)
                .saturating_add(trap_sites.len() * std::mem::size_of::<u32>());
        }
        if resident_total > max_bytes {
            return Err(resident_total);
        }
        let clock = self.next_tick();
        let mut inner = self.inner.lock();
        let mut seen: HashSet<ObjectRef> = HashSet::with_capacity(batch.len());
        for (funcaddr, _, _) in batch {
            if inner.disabled.contains(funcaddr) || !seen.insert(*funcaddr) {
                return Err(resident_total);
            }
        }
        // Partition into already-cached entries (accepted as-is) and
        // fresh entries (to allocate), charging the budget for the
        // union: cached bytes are already resident, fresh bytes are
        // new, and together they must fit.
        let mut existing_bytes: usize = 0;
        let mut fresh_resident: usize = 0;
        let mut installed = Vec::with_capacity(batch.len());
        for (funcaddr, code, trap_sites) in batch {
            if let Some(cached) = inner.compiled.get(funcaddr) {
                let active = cached.tiers.active().clone();
                active.note_used(clock);
                existing_bytes = existing_bytes.saturating_add(cached.tiers.resident_bytes());
                installed.push(active);
            } else {
                let allocation_len =
                    CodeArena::accounted_len(code.len()).map_err(|_| resident_total)?;
                fresh_resident = fresh_resident
                    .saturating_add(allocation_len)
                    .saturating_add(trap_sites.len() * std::mem::size_of::<u32>());
            }
        }
        if existing_bytes.saturating_add(fresh_resident) > max_bytes {
            return Err(existing_bytes.saturating_add(fresh_resident));
        }
        // Allocate every fresh mapping before evicting or inserting
        // any entry, so an allocation failure leaves the cache
        // untouched.
        let mut compiled_batch = Vec::with_capacity(batch.len());
        for (funcaddr, code, trap_sites) in batch {
            if inner.compiled.contains_key(funcaddr) {
                continue;
            }
            let compiled = CompiledFunction::from_bytes(code, trap_sites.clone(), &inner.arena)
                .map_err(|_| resident_total)?;
            compiled_batch.push((*funcaddr, Arc::new(compiled)));
        }
        // Evict non-batch entries until the fresh entries fit. The
        // fit check above guarantees this terminates without touching
        // a batch entry: evicting everything else leaves at most
        // `existing_bytes`, and existing + fresh <= max_bytes.
        let target_used = max_bytes.saturating_sub(fresh_resident);
        while inner.used_bytes > target_used {
            let victim = inner
                .compiled
                .iter()
                .filter(|(addr, _)| !seen.contains(*addr))
                .min_by_key(|(_, cached)| cached.tiers.active().last_used())
                .map(|(addr, _)| *addr);
            let Some(victim) = victim else {
                break;
            };
            if let Some(cached) = inner.compiled.remove(&victim) {
                inner.used_bytes = inner
                    .used_bytes
                    .saturating_sub(cached.tiers.resident_bytes());
            }
        }
        for (funcaddr, compiled) in compiled_batch {
            compiled.note_used(clock);
            inner.used_bytes = inner.used_bytes.saturating_add(compiled.resident_bytes());
            inner.compiled.insert(
                funcaddr,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled.clone()),
                },
            );
            installed.push(compiled);
        }
        Ok(installed)
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
        // The count cap applies only to this stderr copy. The structured
        // event below must still fire past the cap: subscribers do their
        // own filtering (docs/core/diagnostics.md).
        if index < max_count {
            eprintln!(
                "[telomere-jit] compile_reject reason={reason} bytes={bytes_len} allocation={allocation_len} max={max}"
            );
        }
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
                .min_by_key(|(_, cached)| cached.tiers.active().last_used())
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

/// #191: direct-call recency stamps are refreshed at most once per
/// this many store-clock ticks per function. The clock advances once
/// per direct call (and once per cache slow-path operation), so a
/// function's eviction rank is accurate to within this many calls.
const DIRECT_CALL_TOUCH_INTERVAL: u64 = 32;

impl CompiledFunction {
    fn from_bytes(bytes: &[u8], trap_sites: Vec<u32>, arena: &CodeArena) -> Result<Self, ()> {
        let code = arena.allocate(bytes).map_err(|_| ())?;
        let entry = unsafe { std::mem::transmute::<usize, JitEntry>(code.ptr() as usize) };
        Ok(Self {
            entry,
            code_size: code.len(),
            _code: code,
            trap_sites: trap_sites.into_boxed_slice(),
            last_used: AtomicU64::new(0),
        })
    }

    pub(crate) fn entry(&self) -> JitEntry {
        self.entry
    }

    /// Records a use at cache-clock tick `tick`. Recency is a hint for
    /// eviction only, so a plain `Relaxed` store is enough.
    fn note_used(&self, tick: u64) {
        self.last_used.store(tick, Ordering::Relaxed);
    }

    fn last_used(&self) -> u64 {
        self.last_used.load(Ordering::Relaxed)
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
                },
            );
        }

        let hit = cache
            .get_cached(func)
            .expect("installed function must be consultable");
        assert!(Arc::ptr_eq(&hit, &compiled));

        assert_eq!(
            compiled.last_used(),
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

        compiled_a.note_used(1);
        compiled_b.note_used(2);
        cache.clock.store(2, Ordering::Relaxed);
        {
            let mut inner = cache.inner.lock();
            inner.used_bytes = compiled_a.code_size() + compiled_b.code_size();
            inner.compiled.insert(
                func_a,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled_a.clone()),
                },
            );
            inner.compiled.insert(
                func_b,
                CachedFunction {
                    tiers: CompiledTiers::baseline(compiled_b.clone()),
                },
            );
        }

        assert!(cache.touch_compiled(func_a, &compiled_a));

        let mut inner = cache.inner.lock();
        let target_used_bytes = inner.used_bytes.saturating_sub(compiled_b.code_size());
        inner.evict_until(target_used_bytes);

        assert!(inner.compiled.contains_key(&func_a));
        assert!(!inner.compiled.contains_key(&func_b));
        assert_eq!(compiled_a.last_used(), 3);
    }

    #[test]
    fn touch_compiled_rejects_stale_weak_entry() {
        let cache = StoreJitCache::default();
        let func = ObjectRef(3);
        let current = test_compiled(32);
        let stale = test_compiled(32);

        current.note_used(1);
        cache.clock.store(1, Ordering::Relaxed);
        {
            let mut inner = cache.inner.lock();
            inner.used_bytes = current.code_size();
            inner.compiled.insert(
                func,
                CachedFunction {
                    tiers: CompiledTiers::baseline(current.clone()),
                },
            );
        }

        assert!(!cache.touch_compiled(func, &stale));

        assert_eq!(current.last_used(), 1);
    }

    #[test]
    fn note_direct_call_refreshes_recency_without_the_cache_lock() {
        let cache = StoreJitCache::default();
        let compiled = test_compiled(32);

        // The stamp starts at 0; the first direct call ticks the clock
        // to 1, still within the sampling interval of stamp 0, so no
        // store happens yet.
        cache.note_direct_call(&compiled);
        assert_eq!(compiled.last_used(), 0);

        // Advancing the clock past the sampling interval makes the next
        // direct call refresh the stamp to the current tick.
        cache
            .clock
            .store(DIRECT_CALL_TOUCH_INTERVAL + 1, Ordering::Relaxed);
        cache.note_direct_call(&compiled);
        assert_eq!(compiled.last_used(), DIRECT_CALL_TOUCH_INTERVAL + 2);
    }

    #[test]
    fn install_batch_accepts_already_cached_entries() {
        let cache = StoreJitCache::default();
        let func_a = ObjectRef(11);
        let func_b = ObjectRef(12);
        let code = vec![0x90u8; 32];

        let first = cache
            .install_batch(&[(func_a, code.clone(), Vec::new())], 4096)
            .expect("first install must succeed");
        assert_eq!(first.len(), 1);
        let used_after_first = cache.stats().used_bytes;

        // Installing a batch that overlaps the cached entry (the
        // start-function case: lazy compilation got there first) must
        // succeed, keep the original entry, and not double-charge it.
        let second = cache
            .install_batch(
                &[
                    (func_a, code.clone(), Vec::new()),
                    (func_b, code.clone(), Vec::new()),
                ],
                4096,
            )
            .expect("install overlapping the cache must succeed");
        assert_eq!(second.len(), 2);
        assert!(Arc::ptr_eq(&second[0], &first[0]));
        let stats = cache.stats();
        assert_eq!(stats.compiled_functions, 2);
        assert_eq!(stats.used_bytes, used_after_first * 2);
    }

    #[test]
    fn install_batch_failure_leaves_cache_untouched() {
        let cache = StoreJitCache::default();
        let resident = ObjectRef(21);
        let newcomer = ObjectRef(22);
        let rejected = ObjectRef(23);
        let code = vec![0x90u8; 32];
        cache
            .install_batch(&[(resident, code.clone(), Vec::new())], 4096)
            .expect("seed install must succeed");
        {
            let mut inner = cache.inner.lock();
            inner.disabled.insert(rejected);
        }
        let before = cache.stats();

        // A batch naming a disabled function fails wholesale: the
        // valid newcomer in the same batch must not be installed and
        // the resident entry must survive.
        let result = cache.install_batch(
            &[
                (newcomer, code.clone(), Vec::new()),
                (rejected, code.clone(), Vec::new()),
            ],
            4096,
        );
        assert!(result.is_err());
        let after = cache.stats();
        assert_eq!(after.compiled_functions, before.compiled_functions);
        assert_eq!(after.used_bytes, before.used_bytes);
        assert!(cache.get_cached(resident).is_some());
        assert!(cache.get_cached(newcomer).is_none());
    }

    #[test]
    fn install_batch_evicts_others_but_never_batch_entries() {
        let cache = StoreJitCache::default();
        let kept = ObjectRef(31);
        let evictable = ObjectRef(32);
        let newcomer = ObjectRef(33);
        let code = vec![0x90u8; 32];
        // Two 32-byte residents exactly fill a 64-byte budget.
        cache
            .install_batch(
                &[
                    (kept, code.clone(), Vec::new()),
                    (evictable, code.clone(), Vec::new()),
                ],
                64,
            )
            .expect("seed install must succeed");

        // Re-installing `kept` together with a newcomer under the
        // same budget must evict only the non-batch entry.
        cache
            .install_batch(
                &[
                    (kept, code.clone(), Vec::new()),
                    (newcomer, code.clone(), Vec::new()),
                ],
                64,
            )
            .expect("batch install must succeed");
        assert!(cache.get_cached(kept).is_some());
        assert!(cache.get_cached(newcomer).is_some());
        assert!(cache.get_cached(evictable).is_none());
        assert_eq!(cache.stats().used_bytes, 64);
    }

    fn test_compiled(code_size: usize) -> Arc<CompiledFunction> {
        let code = ExecutableCode::test_stub(code_size);
        Arc::new(CompiledFunction {
            entry: noop_entry,
            code_size: code.len(),
            _code: code,
            trap_sites: Box::default(),
            last_used: AtomicU64::new(0),
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
