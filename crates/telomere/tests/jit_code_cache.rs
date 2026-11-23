//! JIT code cache residency tests for #195.
//!
//! The cache used to account every compiled function at its page-rounded
//! mapping size, so the default 4 MiB cap held at most 256 functions on
//! macOS (16 KiB pages) and 1024 on Linux (4 KiB pages) no matter how small
//! the functions were, and hot sets larger than that recompiled forever.
//! These tests pin the slab-packed behaviour: many small functions stay
//! resident at the default cap, are compiled once, and are accounted in
//! code bytes rather than pages.

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
use telomere::{InstanceHandle, Registry, ResultValue, VMResult, WasmValue};
use telomere::{JitConfig, RuntimeConfig, Store};

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn parse_module(wat: &str) -> telomere::Module {
    let bytes = wat::parse_str(wat).expect("wat should parse");
    let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);
    parser.parse_module().expect("module should parse")
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn jit_store() -> Store {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    Store::new_with_runtime_config(runtime_config)
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
async fn instantiate_jit_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
    let module = parse_module(wat);
    match telomere::instantiate(module, store, registry).await {
        VMResult::Success(instance) => instance,
        other => panic!("instantiate must succeed, got {other:?}"),
    }
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn assert_success_i32(result: VMResult<ResultValue>, expected: i32) {
    match result {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(expected)]));
        }
        other => panic!("expected success with {expected}, got {other:?}"),
    }
}

/// Builds a module with `callees` tiny functions `f_i(x) = x + i` and one
/// exported `run` that chains a call to every callee, so `run(0)` touches
/// every function and returns `sum(0..callees)`.
#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
fn many_function_module(callees: usize) -> String {
    let mut wat = String::from("(module\n");
    for i in 0..callees {
        wat.push_str(&format!(
            "  (func $f{i} (param i32) (result i32)\n    local.get 0\n    i32.const {i}\n    i32.add)\n"
        ));
    }
    wat.push_str("  (func (export \"run\") (param i32) (result i32)\n    local.get 0\n");
    for i in 0..callees {
        wat.push_str(&format!("    call $f{i}\n"));
    }
    wat.push_str("  )\n)\n");
    wat
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_code_cache_keeps_many_small_functions_resident() {
    // Comfortably above the old resident ceilings at the default 4 MiB cap:
    // 256 functions on 16 KiB-page targets, 1024 on 4 KiB-page targets.
    const CALLEES: usize = 3000;
    let wat = many_function_module(CALLEES);
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(&wat, &store, &registry).await;
    let expected = (CALLEES * (CALLEES - 1) / 2) as i32;

    let mut stats_after_pass = Vec::new();
    for _ in 0..3 {
        let result = telomere::run_module_function(
            &instance,
            &store,
            "run",
            &ResultValue::new(vec![WasmValue::I32(0)]),
        )
        .await;
        assert_success_i32(result, expected);
        stats_after_pass.push(store.jit_cache_stats());
    }

    let after_first = stats_after_pass[0];
    assert_eq!(
        after_first.rejected_functions, 0,
        "no function should be rejected, stats={after_first:?}"
    );
    assert_eq!(
        after_first.compiled_functions,
        CALLEES + 1,
        "every called function should stay resident at the default cap, stats={after_first:?}"
    );
    // The whole hot set fits the default cap in code bytes; page accounting
    // needed (CALLEES + 1) * page_size (>= 12 MiB) for the same set, which is
    // exactly what forced the eviction churn this test guards against. The
    // per-byte accounting ratio itself is pinned by the small-module test
    // below.
    assert!(
        after_first.used_bytes <= 4 * 1024 * 1024,
        "used_bytes should track code bytes, not page-rounded mappings, stats={after_first:?}"
    );
    // Steady state: later passes neither evict nor recompile anything, so
    // the cache state is byte-for-byte stable.
    assert_eq!(
        stats_after_pass[1], after_first,
        "second pass must not churn the cache, first={after_first:?} second={:?}",
        stats_after_pass[1]
    );
    assert_eq!(
        stats_after_pass[2], after_first,
        "third pass must not churn the cache, first={after_first:?} third={:?}",
        stats_after_pass[2]
    );
}

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
#[tokio::test]
async fn jit_code_cache_accounts_small_module_in_code_bytes() {
    let store = jit_store();
    let registry = Registry::new();
    let instance = instantiate_jit_wat(
        r#"
        (module
          (func $add1 (param i32) (result i32)
            local.get 0
            i32.const 1
            i32.add)
          (func (export "run") (param i32) (result i32)
            local.get 0
            call $add1
            call $add1
            i32.const 3
            i32.add))
        "#,
        &store,
        &registry,
    )
    .await;

    let result = telomere::run_module_function(
        &instance,
        &store,
        "run",
        &ResultValue::new(vec![WasmValue::I32(10)]),
    )
    .await;
    assert_success_i32(result, 15);

    let stats = store.jit_cache_stats();
    assert_eq!(stats.compiled_functions, 2, "stats={stats:?}");
    assert_eq!(stats.rejected_functions, 0, "stats={stats:?}");
    // Two tiny functions emit ~2.5 KB of native code in total (measured on
    // x86_64-linux: compile.bytes = 2482). Page accounting charged
    // 2 * page_size (8192 on Linux, 32768 on macOS) for the same module, so
    // anything under one 4096-byte page proves code-byte accounting.
    assert!(
        stats.used_bytes < 4096,
        "used_bytes should be proportional to emitted code, stats={stats:?}"
    );
}

// ---------------------------------------------------------------------------
// #144 regression: the stderr compile-trace count cap must not suppress the
// structured `telomere::jit` events. The diagnostics contract
// (docs/core/diagnostics.md) says the historical filters and count caps
// apply only to the stderr copies; the events are left for the subscriber
// to filter.
// ---------------------------------------------------------------------------

#[cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]
mod compile_reject_events {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tracing::level_filters::LevelFilter;
    use tracing::{
        field::{Field, Visit},
        span::{Attributes, Id, Record},
        Event, Metadata, Subscriber,
    };

    #[derive(Debug)]
    struct CapturedEvent {
        target: String,
        reason: Option<String>,
    }

    #[derive(Default)]
    struct Capture {
        events: Mutex<Vec<CapturedEvent>>,
    }

    struct CaptureSubscriber(Arc<Capture>);

    #[derive(Default)]
    struct FieldVisitor {
        reason: Option<String>,
    }

    impl Visit for FieldVisitor {
        fn record_str(&mut self, field: &Field, value: &str) {
            if field.name() == "reason" {
                self.reason = Some(value.to_owned());
            }
        }

        fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
            if field.name() == "reason" {
                self.reason = Some(format!("{value:?}"));
            }
        }
    }

    impl Subscriber for CaptureSubscriber {
        fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
            true
        }

        fn max_level_hint(&self) -> Option<LevelFilter> {
            Some(LevelFilter::TRACE)
        }

        fn new_span(&self, _span: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }

        fn record(&self, _span: &Id, _values: &Record<'_>) {}

        fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

        fn event(&self, event: &Event<'_>) {
            let mut visitor = FieldVisitor::default();
            event.record(&mut visitor);
            self.0.events.lock().unwrap().push(CapturedEvent {
                target: event.metadata().target().to_owned(),
                reason: visitor.reason,
            });
        }

        fn enter(&self, _span: &Id) {}

        fn exit(&self, _span: &Id) {}
    }

    /// Sets the compile-trace env vars for the duration of the test and
    /// restores their previous values on drop. The vars are process-global,
    /// but with `_MAX=0` the stderr branch is deterministically silent and
    /// the shared trace counter cannot change the outcome; the other tests
    /// in this binary do not read these variables.
    struct EnvGuard {
        saved: Vec<(&'static str, Option<String>)>,
    }

    impl EnvGuard {
        fn set(vars: &[(&'static str, &str)]) -> Self {
            let saved = vars
                .iter()
                .map(|(name, value)| {
                    let previous = std::env::var(name).ok();
                    std::env::set_var(name, value);
                    (*name, previous)
                })
                .collect();
            Self { saved }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (name, previous) in &self.saved {
                match previous {
                    Some(value) => std::env::set_var(name, value),
                    None => std::env::remove_var(name),
                }
            }
        }
    }

    #[test]
    fn compile_reject_events_survive_the_stderr_count_cap() {
        // `_MAX=0` puts every rejection past the stderr cap: the stderr
        // trace prints nothing at all. The structured events must still
        // reach subscribers.
        let _env = EnvGuard::set(&[
            ("TELOMERE_JIT_TRACE_COMPILE", "1"),
            ("TELOMERE_JIT_TRACE_COMPILE_MAX", "0"),
        ]);
        let capture = Arc::new(Capture::default());
        let subscriber = CaptureSubscriber(Arc::clone(&capture));
        tracing::subscriber::with_default(subscriber, || {
            futures::executor::block_on(async {
                // A zero-byte code-cache cap rejects every compilation
                // attempt with reason "max_zero".
                let mut runtime_config = RuntimeConfig::default();
                runtime_config.jit = JitConfig {
                    enabled: true,
                    code_cache_max_bytes: 0,
                };
                let store = Store::new_with_runtime_config(runtime_config);
                let registry = Registry::new();
                let instance = instantiate_jit_wat(
                    r#"
                    (module
                      (func $answer (result i32) (i32.const 42))
                      (func (export "run") (result i32) (call $answer)))
                    "#,
                    &store,
                    &registry,
                )
                .await;
                // The compile is rejected; execution falls back to the
                // interpreter and still succeeds.
                let result = telomere::run_module_function(
                    &instance,
                    &store,
                    "run",
                    &ResultValue::new(vec![]),
                )
                .await;
                assert_success_i32(result, 42);
            });
        });
        let events = capture.events.lock().unwrap();
        let rejects: Vec<_> = events
            .iter()
            .filter(|event| {
                event.target == telomere::diagnostics::TARGET_JIT
                    && event.reason.as_deref() == Some("max_zero")
            })
            .collect();
        assert!(
            !rejects.is_empty(),
            "expected a telomere::jit compile-reject event despite \
             TELOMERE_JIT_TRACE_COMPILE_MAX=0, got {events:?}"
        );
    }
}
