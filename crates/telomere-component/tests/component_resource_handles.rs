//! Regression tests for #317: resource handle tables are per component
//! instance, indices allocate sequentially from 1, and freed indices are
//! reused LIFO (the component-model handle-table discipline asserted by
//! the upstream `resources/handle-table.wast` and `linking/unit.wast`
//! conformance cases).

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

async fn instantiate(bytes: &[u8]) -> (telomere::Store, telomere_component::ComponentInstance) {
    let engine = ComponentEngine::new();
    let program = engine.compile(bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

/// The first component of upstream `resources/handle-table.wast`: inside a
/// single self-contained component, `resource.new` must hand out indices
/// sequentially from 1 and reuse freed indices LIFO.
#[tokio::test]
async fn resource_handles_allocate_sequentially_and_reuse_freed_indices_lifo() {
    let bytes = compile_component(
        r#"
(component
  (type $R (resource (rep i32)))
  (canon resource.new $R (core func $R.resource.new))
  (canon resource.drop $R (core func $R.resource.drop))
  (canon resource.rep $R (core func $R.resource.rep))
  (core module $M
    (import "" "new" (func $new (param i32) (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (import "" "rep" (func $rep (param i32) (result i32)))
    (func (export "run") (result i32)
      (if (i32.ne (i32.const 1) (call $new (i32.const 100))) (then unreachable))
      (if (i32.ne (i32.const 2) (call $new (i32.const 200))) (then unreachable))
      (if (i32.ne (i32.const 3) (call $new (i32.const 300))) (then unreachable))

      (call $drop (i32.const 2))
      (if (i32.ne (i32.const 2) (call $new (i32.const 400))) (then unreachable))
      (if (i32.ne (i32.const 100) (call $rep (i32.const 1))) (then unreachable))
      (if (i32.ne (i32.const 400) (call $rep (i32.const 2))) (then unreachable))
      (if (i32.ne (i32.const 300) (call $rep (i32.const 3))) (then unreachable))

      (call $drop (i32.const 1))
      (call $drop (i32.const 2))
      (call $drop (i32.const 3))
      (if (i32.ne (i32.const 3) (call $new (i32.const 500))) (then unreachable))
      (if (i32.ne (i32.const 2) (call $new (i32.const 600))) (then unreachable))
      (if (i32.ne (i32.const 1) (call $new (i32.const 700))) (then unreachable))
      (if (i32.ne (i32.const 500) (call $rep (i32.const 3))) (then unreachable))
      (if (i32.ne (i32.const 600) (call $rep (i32.const 2))) (then unreachable))
      (if (i32.ne (i32.const 700) (call $rep (i32.const 1))) (then unreachable))

      (if (i32.ne (i32.const 4) (call $new (i32.const 800))) (then unreachable))

      (i32.const 42)
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new" (func $R.resource.new))
    (export "drop" (func $R.resource.drop))
    (export "rep" (func $R.resource.rep))
  ))))
  (func (export "run") (result u32) (canon lift (core func $m "run")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

/// The per-instance case of upstream `linking/unit.wast` (lines 1020-1022):
/// two instances of the same component allocate handle indices from their
/// own tables, so the sequence make-1, make-1, make-2, make-1, make-2
/// yields 1, 2, 1, 3, 2.
#[tokio::test]
async fn resource_handle_tables_are_per_component_instance() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (type $R (resource (rep i32)))
    (core func $new (canon resource.new $R))
    (core module $M
      (import "canon" "new" (func $new (param i32) (result i32)))
      (func (export "make") (result i32) (call $new (i32.const 0))))
    (core instance $m (instantiate $M
      (with "canon" (instance (export "new" (func $new))))))
    (func (export "make") (result u32) (canon lift (core func $m "make"))))
  (instance $c1 (instantiate $C))
  (instance $c2 (instantiate $C))
  (func (export "make-1") (alias export $c1 "make"))
  (func (export "make-2") (alias export $c2 "make"))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    for (export, expected) in [
        ("make-1", 1),
        ("make-1", 2),
        ("make-2", 1),
        ("make-1", 3),
        ("make-2", 2),
    ] {
        let result = instance
            .call(&store, export, &[])
            .await
            .expect("make should succeed");
        assert_eq!(
            result,
            vec![ComponentValue::U32(expected)],
            "{export} should allocate from its own instance table"
        );
    }
}

/// A handle index allocated in one instance is unknown in a sibling
/// instance of the same definition: dropping it there must trap, while
/// dropping it in the owning instance succeeds.
#[tokio::test]
async fn resource_handle_from_sibling_instance_is_unknown() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (type $R (resource (rep i32)))
    (canon resource.new $R (core func $R.resource.new))
    (canon resource.drop $R (core func $R.resource.drop))
    (core module $M
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "alloc") (drop (call $new (i32.const 100))))
      (func (export "drop-first") (call $drop (i32.const 1)))
    )
    (core instance $m (instantiate $M (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))
    ))))
    (func (export "alloc") (canon lift (core func $m "alloc")))
    (func (export "drop-first") (canon lift (core func $m "drop-first")))
  )
  (instance $c1 (instantiate $C))
  (instance $c2 (instantiate $C))
  (func (export "alloc-in1") (alias export $c1 "alloc"))
  (func (export "drop-in1") (alias export $c1 "drop-first"))
  (func (export "drop-in2") (alias export $c2 "drop-first"))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    instance
        .call(&store, "alloc-in1", &[])
        .await
        .expect("alloc in instance 1 should succeed");
    let error = instance
        .call(&store, "drop-in2", &[])
        .await
        .expect_err("index 1 is unknown in instance 2 and must trap");
    // Canon-op failures inside a core module surface as an unreachable
    // guest trap (the same shape the host-resource drop tests pin).
    assert!(
        matches!(error, telomere_component::ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable")),
        "dropping a sibling instance's handle should trap, got {error:?}"
    );
    instance
        .call(&store, "drop-in1", &[])
        .await
        .expect("dropping the handle in its owning instance should succeed");
}

/// All resource types of one instance share a single table: indices form
/// one sequence across types, and an index freed by one type is reused by
/// the next allocation of any type.
#[tokio::test]
async fn resource_types_share_one_handle_table_per_instance() {
    let bytes = compile_component(
        r#"
(component
  (type $R1 (resource (rep i32)))
  (type $R2 (resource (rep i32)))
  (canon resource.new $R1 (core func $R1.resource.new))
  (canon resource.new $R2 (core func $R2.resource.new))
  (canon resource.drop $R1 (core func $R1.resource.drop))
  (canon resource.rep $R2 (core func $R2.resource.rep))
  (core module $M
    (import "" "new1" (func $new1 (param i32) (result i32)))
    (import "" "new2" (func $new2 (param i32) (result i32)))
    (import "" "drop1" (func $drop1 (param i32)))
    (import "" "rep2" (func $rep2 (param i32) (result i32)))
    (func (export "run") (result i32)
      (if (i32.ne (i32.const 1) (call $new1 (i32.const 100))) (then unreachable))
      (if (i32.ne (i32.const 2) (call $new2 (i32.const 200))) (then unreachable))
      (call $drop1 (i32.const 1))
      (if (i32.ne (i32.const 1) (call $new2 (i32.const 300))) (then unreachable))
      (if (i32.ne (i32.const 300) (call $rep2 (i32.const 1))) (then unreachable))
      (if (i32.ne (i32.const 200) (call $rep2 (i32.const 2))) (then unreachable))
      (i32.const 42)
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new1" (func $R1.resource.new))
    (export "new2" (func $R2.resource.new))
    (export "drop1" (func $R1.resource.drop))
    (export "rep2" (func $R2.resource.rep))
  ))))
  (func (export "run") (result u32) (canon lift (core func $m "run")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

// Regression tests for the Codex P1 on PR #341: while `resource.drop`
// runs a destructor, the destructor may re-enter the same instance's
// canon resource ops. The handle-table borrow taken by the drop must be
// released before the destructor is invoked, or the nested op panics
// with `RefCell already borrowed`.
//
// Each component below defines a second resource type `$S` (whose canon
// ops the destructor can call without a definition cycle) and a resource
// `$R` whose destructor calls one of them. All resource types of an
// instance share the one handle table, so the nested op borrows the
// same `RefCell` the drop was holding.

/// The destructor allocates another resource in the same instance: the
/// nested `resource.new` must succeed and reuse the index the dropped
/// resource just freed.
#[tokio::test]
async fn resource_destructor_may_allocate_in_same_instance() {
    let bytes = compile_component(
        r#"
(component
  (type $S (resource (rep i32)))
  (core func $S.new (canon resource.new $S))
  (core func $S.rep (canon resource.rep $S))
  (core module $D
    (import "" "new" (func $new (param i32) (result i32)))
    (func (export "dtor") (param i32)
      ;; Runs while $R's drop is in flight; handle 1 is the index $R just
      ;; freed, so the nested allocation must hand it back with rep 99.
      (if (i32.ne (i32.const 1) (call $new (i32.const 99))) (then unreachable))
    )
  )
  (core instance $d (instantiate $D (with "" (instance
    (export "new" (func $S.new))
  ))))
  (type $R (resource (rep i32) (dtor (core func $d "dtor"))))
  (core func $R.new (canon resource.new $R))
  (core func $R.drop (canon resource.drop $R))
  (core module $M
    (import "" "new-r" (func $new-r (param i32) (result i32)))
    (import "" "drop-r" (func $drop-r (param i32)))
    (import "" "rep-s" (func $rep-s (param i32) (result i32)))
    (func (export "run") (result i32)
      (call $drop-r (call $new-r (i32.const 7)))
      (if (i32.ne (i32.const 99) (call $rep-s (i32.const 1))) (then unreachable))
      (i32.const 42)
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new-r" (func $R.new))
    (export "drop-r" (func $R.drop))
    (export "rep-s" (func $S.rep))
  ))))
  (func (export "run") (result u32) (canon lift (core func $m "run")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

/// The destructor reads the rep of another live resource in the same
/// instance: the nested `resource.rep` (a shared borrow) must not trip
/// over the drop's mutable borrow.
#[tokio::test]
async fn resource_destructor_may_rep_in_same_instance() {
    let bytes = compile_component(
        r#"
(component
  (type $S (resource (rep i32)))
  (core func $S.new (canon resource.new $S))
  (core func $S.rep (canon resource.rep $S))
  (core module $D
    (import "" "rep" (func $rep (param i32) (result i32)))
    (func (export "dtor") (param i32)
      ;; Handle 1 is the $S resource the caller created first, rep 50.
      (if (i32.ne (i32.const 50) (call $rep (i32.const 1))) (then unreachable))
    )
  )
  (core instance $d (instantiate $D (with "" (instance
    (export "rep" (func $S.rep))
  ))))
  (type $R (resource (rep i32) (dtor (core func $d "dtor"))))
  (core func $R.new (canon resource.new $R))
  (core func $R.drop (canon resource.drop $R))
  (core module $M
    (import "" "new-s" (func $new-s (param i32) (result i32)))
    (import "" "new-r" (func $new-r (param i32) (result i32)))
    (import "" "drop-r" (func $drop-r (param i32)))
    (func (export "run") (result i32)
      (drop (call $new-s (i32.const 50)))
      (call $drop-r (call $new-r (i32.const 7)))
      (i32.const 42)
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new-s" (func $S.new))
    (export "new-r" (func $R.new))
    (export "drop-r" (func $R.drop))
  ))))
  (func (export "run") (result u32) (canon lift (core func $m "run")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}

/// The destructor drops another live resource in the same instance: the
/// nested `resource.drop` must succeed, and the freed index must be back
/// in the LIFO free list afterwards.
#[tokio::test]
async fn resource_destructor_may_drop_in_same_instance() {
    let bytes = compile_component(
        r#"
(component
  (type $S (resource (rep i32)))
  (core func $S.new (canon resource.new $S))
  (core func $S.drop (canon resource.drop $S))
  (core func $S.rep (canon resource.rep $S))
  (core module $D
    (import "" "drop" (func $drop (param i32)))
    (func (export "dtor") (param i32)
      ;; Drop the $S resource (handle 1) the caller created first.
      (call $drop (i32.const 1))
    )
  )
  (core instance $d (instantiate $D (with "" (instance
    (export "drop" (func $S.drop))
  ))))
  (type $R (resource (rep i32) (dtor (core func $d "dtor"))))
  (core func $R.new (canon resource.new $R))
  (core func $R.drop (canon resource.drop $R))
  (core module $M
    (import "" "new-s" (func $new-s (param i32) (result i32)))
    (import "" "new-r" (func $new-r (param i32) (result i32)))
    (import "" "drop-r" (func $drop-r (param i32)))
    (import "" "rep-s" (func $rep-s (param i32) (result i32)))
    (func (export "run") (result i32)
      (drop (call $new-s (i32.const 50)))
      (call $drop-r (call $new-r (i32.const 7)))
      ;; The dtor freed handle 1 after $R freed handle 2, so the LIFO
      ;; free list hands handle 1 to the next allocation.
      (if (i32.ne (i32.const 1) (call $new-s (i32.const 60))) (then unreachable))
      (if (i32.ne (i32.const 60) (call $rep-s (i32.const 1))) (then unreachable))
      (i32.const 42)
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new-s" (func $S.new))
    (export "new-r" (func $R.new))
    (export "drop-r" (func $R.drop))
    (export "rep-s" (func $S.rep))
  ))))
  (func (export "run") (result u32) (canon lift (core func $m "run")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(42)]);
}
