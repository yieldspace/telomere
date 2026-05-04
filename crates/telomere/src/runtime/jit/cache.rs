use std::{collections::HashMap, sync::Arc};

use parking_lot::Mutex;

use crate::common::{store::FunctionBody, ObjectRef, VMResult};

use super::{
    abi::JitEntry,
    backend,
    code_memory::{CodeArena, ExecutableCode},
};

#[derive(Default)]
pub(crate) struct StoreJitCache {
    inner: Mutex<StoreJitCacheInner>,
}

#[derive(Default)]
struct StoreJitCacheInner {
    compiled: HashMap<ObjectRef, CachedFunction>,
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
}

impl StoreJitCache {
    pub(crate) fn get_or_compile(
        &self,
        funcaddr: ObjectRef,
        body: &FunctionBody,
        max_bytes: u32,
    ) -> VMResult<Arc<CompiledFunction>> {
        let max_bytes = max_bytes as usize;
        if max_bytes == 0 {
            return VMResult::Unimplemented;
        }

        {
            let mut inner = self.inner.lock();
            inner.clock = inner.clock.wrapping_add(1);
            let clock = inner.clock;
            if let Some(cached) = inner.compiled.get_mut(&funcaddr) {
                cached.last_used = clock;
                return VMResult::Success(cached.tiers.active().clone());
            }
        }

        let FunctionBody::Wasm { code, op_lens, .. } = body else {
            return VMResult::Unimplemented;
        };
        let bytes = match backend::emit_baseline_function(funcaddr, code, op_lens) {
            Ok(bytes) => bytes,
            Err(()) => return VMResult::Unimplemented,
        };
        let allocation_len = match CodeArena::allocation_len(bytes.len()) {
            Ok(len) => len,
            Err(()) => return VMResult::Unimplemented,
        };
        if allocation_len > max_bytes {
            return VMResult::Unimplemented;
        }

        let mut inner = self.inner.lock();
        inner.clock = inner.clock.wrapping_add(1);
        let clock = inner.clock;
        if let Some(cached) = inner.compiled.get_mut(&funcaddr) {
            cached.last_used = clock;
            return VMResult::Success(cached.tiers.active().clone());
        }
        inner.evict_until(max_bytes.saturating_sub(allocation_len));
        let compiled = match CompiledFunction::from_bytes(&bytes, &inner.arena) {
            Ok(compiled) => Arc::new(compiled),
            Err(()) => return VMResult::Unimplemented,
        };
        inner.used_bytes = inner.used_bytes.saturating_add(compiled.code_size());
        inner.compiled.insert(
            funcaddr,
            CachedFunction {
                tiers: CompiledTiers::baseline(compiled.clone()),
                last_used: clock,
            },
        );
        VMResult::Success(compiled)
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
            self.used_bytes = self.used_bytes.saturating_sub(cached.tiers.code_size());
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

    fn code_size(&self) -> usize {
        self.baseline.code_size()
            + self
                .optimized
                .as_ref()
                .map(|compiled| compiled.code_size())
                .unwrap_or(0)
    }
}

impl CompiledFunction {
    fn from_bytes(bytes: &[u8], arena: &CodeArena) -> Result<Self, ()> {
        let code = arena.allocate(bytes)?;
        let entry = unsafe { std::mem::transmute::<*mut u8, JitEntry>(code.ptr()) };
        Ok(Self {
            entry,
            code_size: code.len(),
            _code: code,
        })
    }

    pub(crate) fn entry(&self) -> JitEntry {
        self.entry
    }

    fn code_size(&self) -> usize {
        self.code_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Instr;

    #[test]
    fn compiled_tiers_start_with_active_baseline() {
        let code = ExecutableCode::test_stub(32);
        let compiled = Arc::new(CompiledFunction {
            entry: noop_entry,
            code_size: code.len(),
            _code: code,
        });
        let tiers = CompiledTiers::baseline(compiled.clone());

        assert!(Arc::ptr_eq(tiers.active(), &compiled));
        assert_eq!(tiers.code_size(), 32);
    }

    unsafe extern "C" fn noop_entry(
        _ctx: *mut crate::common::ExecuteContext<'_>,
        _code: *const Instr,
        _local_base: *mut u8,
    ) -> super::super::JitNativeExit {
        super::super::JitNativeExit::done()
    }
}
