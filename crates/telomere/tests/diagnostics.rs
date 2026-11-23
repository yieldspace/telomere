//! Regression coverage for the embedder observability contract (#144):
//! Telomere must not statically disable release tracing, and a guest trap
//! must be observable as a structured `tracing` event on the documented
//! target without any diagnostic feature build.
//!
//! CI runs the workspace test suite in release mode, so
//! `trap_event_is_emitted_with_context` doubles as the
//! `release_max_level_off` regression guard: with that feature enabled, the
//! `WARN` event below is compiled out of release builds entirely and the
//! capture stays empty.

mod common;

use common::instantiate_wat;
use std::sync::{Arc, Mutex};
use telomere::{
    diagnostics, run_module_function, Registry, ResultValue, Store, TrapKind, VMResult,
};
use tracing::{
    field::{Field, Visit},
    span::{Attributes, Id, Record},
    Event, Metadata, Subscriber,
};

#[derive(Debug, Default)]
struct CapturedEvent {
    target: String,
    kind: Option<String>,
}

#[derive(Default)]
struct Capture {
    events: Mutex<Vec<CapturedEvent>>,
}

struct CaptureSubscriber(Arc<Capture>);

#[derive(Default)]
struct FieldVisitor {
    kind: Option<String>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "kind" {
            self.kind = Some(value.to_owned());
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

impl Subscriber for CaptureSubscriber {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
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
            kind: visitor.kind,
        });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

fn with_capture<R>(run: impl FnOnce() -> R) -> (R, Vec<CapturedEvent>) {
    let capture = Arc::new(Capture::default());
    let subscriber = CaptureSubscriber(Arc::clone(&capture));
    let result = tracing::subscriber::with_default(subscriber, run);
    let events = std::mem::take(&mut *capture.events.lock().unwrap());
    (result, events)
}

fn trap_events(events: &[CapturedEvent]) -> Vec<&CapturedEvent> {
    events
        .iter()
        .filter(|event| event.target == diagnostics::TARGET_TRAP)
        .collect()
}

#[test]
fn diagnostics_targets_are_the_documented_strings() {
    // The strings are the contract embedders filter on; docs/core/diagnostics.md
    // and the module documentation name exactly these values.
    assert_eq!(diagnostics::TARGET_TRAP, "telomere::trap");
    assert_eq!(diagnostics::TARGET_JIT, "telomere::jit");
    assert_eq!(diagnostics::TARGET_STACK, "telomere::stack");
}

#[test]
fn trap_event_is_emitted_with_context() {
    let (_store, events) = with_capture(|| {
        futures::executor::block_on(async {
            let store = Store::new();
            let registry = Registry::new();
            let instance = instantiate_wat(
                r#"
                (module
                  (func (export "ok") (result i32) (i32.const 7))
                  (func (export "boom") unreachable)
                  (func (export "div") (result i32)
                    (i32.div_s (i32.const 1) (i32.const 0))))
                "#,
                &store,
                &registry,
            )
            .await;

            // A successful call records no trap and emits no trap event.
            let ok = run_module_function(&instance, &store, "ok", &ResultValue::new(vec![])).await;
            assert!(matches!(ok, VMResult::Success(_)));
            assert!(store.take_last_trap().is_none());

            let trapped =
                run_module_function(&instance, &store, "boom", &ResultValue::new(vec![])).await;
            assert!(matches!(trapped, VMResult::Unreachable));
            let info = store
                .take_last_trap()
                .expect("the trap record stays available next to the event");
            assert_eq!(info.kind, TrapKind::Unreachable);

            let divided =
                run_module_function(&instance, &store, "div", &ResultValue::new(vec![])).await;
            assert!(matches!(divided, VMResult::DivideByZero));
            assert_eq!(
                store.take_last_trap().map(|info| info.kind),
                Some(TrapKind::DivideByZero)
            );
            store
        })
    });

    let traps = trap_events(&events);
    assert_eq!(
        traps.len(),
        2,
        "expected one trap event per trapping call, got {events:?}"
    );
    assert_eq!(
        traps[0].kind.as_deref(),
        Some(TrapKind::Unreachable.as_str())
    );
    assert_eq!(
        traps[1].kind.as_deref(),
        Some(TrapKind::DivideByZero.as_str())
    );
}
