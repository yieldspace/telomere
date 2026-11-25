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
    // Canon-op failures inside a core module used to surface as an
    // unreachable guest trap; since #312 the canon-lower trampoline
    // preserves the trap cause, so the runtime's own message reaches the
    // caller. What #317 pins is that the drop traps at all.
    assert!(
        matches!(&error, telomere_component::ComponentError::Trap(message) if message.contains("resource handle 1 is invalid")),
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

// Regression tests for #342: canonical lift/lower must transfer resource
// handles between the handle tables of the two component instances at a
// call boundary. An `own` handle moves (the entry leaves the producer's
// table and is allocated in the consumer's table, keeping its rep and its
// defining instance's destructor); a `borrow` handle is resolved in the
// consumer's terms (the rep itself when the consumer defines the type).

/// The cross-instance case of upstream `resources/handle-table.wast`
/// (line 105): a consumer lowers the definer's `make`/`take`, drops one
/// transferred handle itself, and transfers the other back. Indices in
/// the consumer's table follow the consumer's own allocation discipline,
/// and every drop runs the definer's destructor with the right rep.
#[tokio::test]
async fn own_handles_transfer_between_instances_and_drop_runs_definer_dtor() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "take") (param $h i32) (call $drop (local.get $h))))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "take") (param "r" (own $R)) (canon lift (core func $cm "take")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $D
    (import "c" (instance $c
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "take" (func (param "r" (own $R))))))
    (alias export $c "R" (type $R))
    (canon resource.drop $R (core func $R.resource.drop))
    (canon lower (func $c "make") (core func $make'))
    (canon lower (func $c "take") (core func $take'))
    (core module $DM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "take" (func $take (param i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "run") (result i32)
        (if (i32.ne (i32.const 1) (call $make (i32.const 100))) (then unreachable))
        (if (i32.ne (i32.const 2) (call $make (i32.const 200))) (then unreachable))
        (call $drop (i32.const 2))
        (call $take (i32.const 1))
        (if (i32.ne (i32.const 1) (call $make (i32.const 300))) (then unreachable))
        (call $drop (i32.const 1))
        (i32.const 43)))
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make" (func $make'))
      (export "take" (func $take'))
      (export "drop" (func $R.resource.drop))))))
    (func (export "run") (result u32) (canon lift (core func $dm "run"))))
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "run") (alias export $d "run"))
  (func (export "dtor-reps") (alias export $c "dtor-reps"))
  (func (export "dtor-count") (alias export $c "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(43)]);
    // All three resources were destroyed exactly once, by the definer's
    // destructor, with their reps: 200 (dropped by the consumer), 100
    // (transferred back and dropped by the definer), 300 (dropped by the
    // consumer).
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(3)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(600)]);
}

/// A `borrow` passed from a consumer to the component that defines the
/// resource arrives as the rep (the defining component's core code works
/// with reps), and the lender's own handle stays usable after the call:
/// the consumer can still read its rep and drop it.
#[tokio::test]
async fn borrow_handle_crosses_to_defining_instance_as_rep() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (core module $M
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (func (export "make") (result i32) (call $new (i32.const 0x41)))
      (func (export "get-rep") (param $rep i32) (result i32) (local.get $rep)))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))))))
    (func (export "make") (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "get-rep") (param "r" (borrow $R)) (result u32) (canon lift (core func $cm "get-rep")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $D
    (import "c" (instance $c
      (export "R" (type $R (sub resource)))
      (export "make" (func (result (own $R))))
      (export "get-rep" (func (param "r" (borrow $R)) (result u32)))))
    (alias export $c "R" (type $R))
    (canon resource.drop $R (core func $R.resource.drop))
    (canon lower (func $c "make") (core func $make'))
    (canon lower (func $c "get-rep") (core func $get-rep'))
    (core module $DM
      (import "" "make" (func $make (result i32)))
      (import "" "get-rep" (func $get-rep (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make))
        (if (i32.ne (i32.const 1) (local.get $h)) (then unreachable))
        ;; the borrow arrives at the definer as the rep, twice in a row
        (if (i32.ne (i32.const 0x41) (call $get-rep (local.get $h))) (then unreachable))
        (if (i32.ne (i32.const 0x41) (call $get-rep (local.get $h))) (then unreachable))
        ;; the lender's handle is untouched by the borrows: it can be
        ;; borrowed again and finally dropped
        (if (i32.ne (i32.const 0x41) (call $get-rep (local.get $h))) (then unreachable))
        (call $drop (local.get $h))
        (i32.const 44)))
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make" (func $make'))
      (export "get-rep" (func $get-rep'))
      (export "drop" (func $R.resource.drop))))))
    (func (export "run") (result u32) (canon lift (core func $dm "run"))))
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "run") (alias export $d "run"))
  (func (export "dtor-count") (alias export $c "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(44)]);
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
}

/// Handles of two different resource types transferred into one consumer
/// keep their type identity: each type's drop runs its own destructor,
/// and dropping one type's handle through the other type's drop traps.
#[tokio::test]
async fn transferred_handles_keep_their_resource_type() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (core module $M
      (global $sum1 (mut i32) (i32.const 0))
      (global $sum2 (mut i32) (i32.const 0))
      (func (export "dtor1") (param i32)
        (global.set $sum1 (i32.add (global.get $sum1) (local.get 0))))
      (func (export "dtor2") (param i32)
        (global.set $sum2 (i32.add (global.get $sum2) (local.get 0))))
      (func (export "sum1") (result i32) (global.get $sum1))
      (func (export "sum2") (result i32) (global.get $sum2)))
    (core instance $m (instantiate $M))
    (type $R1' (resource (rep i32) (dtor (core func $m "dtor1"))))
    (type $R2' (resource (rep i32) (dtor (core func $m "dtor2"))))
    (export $R1 "R1" (type $R1'))
    (export $R2 "R2" (type $R2'))
    (canon resource.new $R1' (core func $R1.resource.new))
    (canon resource.new $R2' (core func $R2.resource.new))
    (core module $CM
      (import "" "new1" (func $new1 (param i32) (result i32)))
      (import "" "new2" (func $new2 (param i32) (result i32)))
      (func (export "make1") (param $rep i32) (result i32) (call $new1 (local.get $rep)))
      (func (export "make2") (param $rep i32) (result i32) (call $new2 (local.get $rep))))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new1" (func $R1.resource.new))
      (export "new2" (func $R2.resource.new))))))
    (func (export "make1") (param "rep" u32) (result (own $R1)) (canon lift (core func $cm "make1")))
    (func (export "make2") (param "rep" u32) (result (own $R2)) (canon lift (core func $cm "make2")))
    (func (export "sum1") (result u32) (canon lift (core func $m "sum1")))
    (func (export "sum2") (result u32) (canon lift (core func $m "sum2"))))
  (component $D
    (import "c" (instance $c
      (export "R1" (type $R1 (sub resource)))
      (export "R2" (type $R2 (sub resource)))
      (export "make1" (func (param "rep" u32) (result (own $R1))))
      (export "make2" (func (param "rep" u32) (result (own $R2))))))
    (alias export $c "R1" (type $R1))
    (alias export $c "R2" (type $R2))
    (canon resource.drop $R1 (core func $R1.resource.drop))
    (canon resource.drop $R2 (core func $R2.resource.drop))
    (canon lower (func $c "make1") (core func $make1'))
    (canon lower (func $c "make2") (core func $make2'))
    (core module $DM
      (import "" "make1" (func $make1 (param i32) (result i32)))
      (import "" "make2" (func $make2 (param i32) (result i32)))
      (import "" "drop1" (func $drop1 (param i32)))
      (import "" "drop2" (func $drop2 (param i32)))
      (func (export "run") (result i32)
        (local $h1 i32) (local $h2 i32)
        (local.set $h1 (call $make1 (i32.const 10)))
        (local.set $h2 (call $make2 (i32.const 20)))
        (if (i32.ne (i32.const 1) (local.get $h1)) (then unreachable))
        (if (i32.ne (i32.const 2) (local.get $h2)) (then unreachable))
        (call $drop1 (local.get $h1))
        (call $drop2 (local.get $h2))
        (i32.const 45))
      (func (export "run-wrong-drop") (result i32)
        (local $h1 i32)
        (local.set $h1 (call $make1 (i32.const 30)))
        ;; dropping an R1 handle through R2's drop must trap
        (call $drop2 (local.get $h1))
        (i32.const 46)))
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make1" (func $make1'))
      (export "make2" (func $make2'))
      (export "drop1" (func $R1.resource.drop))
      (export "drop2" (func $R2.resource.drop))))))
    (func (export "run") (result u32) (canon lift (core func $dm "run")))
    (func (export "run-wrong-drop") (result u32) (canon lift (core func $dm "run-wrong-drop"))))
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "run") (alias export $d "run"))
  (func (export "run-wrong-drop") (alias export $d "run-wrong-drop"))
  (func (export "sum1") (alias export $c "sum1"))
  (func (export "sum2") (alias export $c "sum2")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance
        .call(&store, "run", &[])
        .await
        .expect("run should succeed");
    assert_eq!(result, vec![ComponentValue::U32(45)]);
    let sum1 = instance
        .call(&store, "sum1", &[])
        .await
        .expect("sum1 should succeed");
    assert_eq!(sum1, vec![ComponentValue::U32(10)]);
    let sum2 = instance
        .call(&store, "sum2", &[])
        .await
        .expect("sum2 should succeed");
    assert_eq!(sum2, vec![ComponentValue::U32(20)]);
    let error = instance
        .call(&store, "run-wrong-drop", &[])
        .await
        .expect_err("dropping an R1 handle through R2's drop must trap");
    // With trap-cause preservation (#312), the runtime's precise
    // wrong-type error surfaces instead of a flattened guest trap.
    assert!(
        matches!(
            &error,
            telomere_component::ComponentError::Trap(message)
                if message.contains("resource handle 2 is invalid")
        ),
        "wrong-type drop should trap, got {error:?}"
    );
}

/// Issue #334 (upstream `resources/borrows.wast` line 161): dropping an
/// owned resource that is currently borrowed must trap instead of
/// succeeding. The borrow here is lent to the very call that performs
/// the drop — one handle passed as both the `borrow` and the `own`
/// parameter of a function whose core implementation drops the own —
/// which is exactly the upstream `lend-trap` shape.
#[tokio::test]
async fn resource_drop_of_borrowed_owned_resource_traps() {
    let bytes = compile_component(
        r#"
(component
  (type $R (resource (rep i32)))
  (export "R" (type $R))
  (canon resource.new $R (core func $R.resource.new))
  (canon resource.drop $R (core func $R.resource.drop))
  (core module $M
    (import "" "new" (func $new (param i32) (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "make") (param i32) (result i32) (call $new (local.get 0)))
    ;; Drops the second (own) parameter while the first (borrow)
    ;; parameter is still lent to this call.
    (func (export "drop-own") (param i32 i32) (call $drop (local.get 1)))
    (func (export "drop") (param i32) (call $drop (local.get 0)))
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new" (func $R.resource.new))
    (export "drop" (func $R.resource.drop))
  ))))
  (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $m "make")))
  (func (export "drop-own") (param "b" (borrow $R)) (param "o" (own $R)) (canon lift (core func $m "drop-own")))
  (func (export "drop") (param "o" (own $R)) (canon lift (core func $m "drop")))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let first = instance
        .call(&store, "make", &[ComponentValue::U32(0x41)])
        .await
        .expect("make should succeed");
    assert_eq!(first, vec![ComponentValue::Own(1)]);
    let second = instance
        .call(&store, "make", &[ComponentValue::U32(0x42)])
        .await
        .expect("make should succeed");
    assert_eq!(second, vec![ComponentValue::Own(2)]);

    // Control: dropping a *different* resource while handle 1 is
    // borrowed is fine.
    instance
        .call(
            &store,
            "drop-own",
            &[ComponentValue::Borrow(1), ComponentValue::Own(2)],
        )
        .await
        .expect("dropping an unborrowed resource while another is borrowed should succeed");

    // The borrowed resource itself must not be dropped mid-call.
    let error = instance
        .call(
            &store,
            "drop-own",
            &[ComponentValue::Borrow(1), ComponentValue::Own(1)],
        )
        .await
        .expect_err("dropping an owned resource while it is borrowed must trap");
    // With #312's trampoline trap-cause preservation in the tree, the
    // canonical message produced by the drop itself now surfaces at the
    // public API (it is also pinned at the handle-table level in the
    // runtime unit tests).
    assert!(
        matches!(error, telomere_component::ComponentError::Trap(ref message) if message.contains("cannot remove owned resource while borrowed")),
        "dropping a borrowed resource should trap, got {error:?}"
    );

    // The trapped call must have released the borrow and left the own
    // handle live: dropping it now succeeds.
    instance
        .call(&store, "drop", &[ComponentValue::Own(1)])
        .await
        .expect("dropping the resource after the borrow was released should succeed");
}

/// Codex review of PR #352 (finding 3): for calls through the public
/// component API, `lower_defined_value` accepts `Own` and `U32` as well
/// as `Borrow` for a borrow-typed parameter (`expect_handle`). The
/// borrow collection behind #334's drop protection must extract the
/// handle with the same rules; with `U32` or `Own` in the borrow
/// position the forbidden drop used to succeed because no borrow was
/// registered.
#[tokio::test]
async fn resource_drop_of_borrowed_owned_resource_traps_for_every_handle_representation() {
    let bytes = compile_component(
        r#"
(component
  (type $R (resource (rep i32)))
  (export "R" (type $R))
  (canon resource.new $R (core func $R.resource.new))
  (canon resource.drop $R (core func $R.resource.drop))
  (core module $M
    (import "" "new" (func $new (param i32) (result i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "make") (param i32) (result i32) (call $new (local.get 0)))
    (func (export "drop-own") (param i32 i32) (call $drop (local.get 1)))
    (func (export "drop") (param i32) (call $drop (local.get 0)))
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new" (func $R.resource.new))
    (export "drop" (func $R.resource.drop))
  ))))
  (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $m "make")))
  (func (export "drop-own") (param "b" (borrow $R)) (param "o" (own $R)) (canon lift (core func $m "drop-own")))
  (func (export "drop") (param "o" (own $R)) (canon lift (core func $m "drop")))
)
"#,
    );
    for kind in 0..3 {
        let borrow_arg = |handle: u32| match kind {
            0 => ComponentValue::Borrow(handle),
            1 => ComponentValue::U32(handle),
            _ => ComponentValue::Own(handle),
        };
        let (store, instance) = instantiate(&bytes).await;
        let first = instance
            .call(&store, "make", &[ComponentValue::U32(0x41)])
            .await
            .expect("make should succeed");
        assert_eq!(first, vec![ComponentValue::Own(1)]);
        let second = instance
            .call(&store, "make", &[ComponentValue::U32(0x42)])
            .await
            .expect("make should succeed");
        assert_eq!(second, vec![ComponentValue::Own(2)]);

        // Control: dropping a *different* resource while handle 1 is
        // borrowed is fine, whatever representation the borrow uses.
        instance
            .call(&store, "drop-own", &[borrow_arg(1), ComponentValue::Own(2)])
            .await
            .expect("dropping an unborrowed resource while another is borrowed should succeed");

        // The borrowed resource itself must not be dropped mid-call.
        let error = instance
            .call(&store, "drop-own", &[borrow_arg(1), ComponentValue::Own(1)])
            .await
            .expect_err("dropping an owned resource while it is borrowed must trap");
        assert!(
            matches!(error, telomere_component::ComponentError::Trap(ref message) if message.contains("cannot remove owned resource while borrowed")),
            "representation {kind}: dropping a borrowed resource should trap, got {error:?}"
        );

        // The trapped call released the borrow and left the own live.
        instance
            .call(&store, "drop", &[ComponentValue::Own(1)])
            .await
            .expect("dropping the resource after the borrow was released should succeed");
    }
}

/// Codex review of PR #352 (finding 1, false positive): a borrow that
/// crosses into the defining instance arrives as the rep, not as a
/// handle index. Resolving it against the defining table must not
/// mistake the rep for an unrelated handle index: here the defining
/// table's handle 1 holds rep 100 while the borrowed resource has
/// rep 1, and dropping handle 1 inside the call is legitimate.
#[tokio::test]
async fn borrow_into_defining_instance_does_not_mark_unrelated_handle() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "setup") (drop (call $new (i32.const 100))))
      (func (export "use-borrow") (param $rep i32) (result i32)
        (call $drop (i32.const 1))
        (i32.const 42)))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "setup") (canon lift (core func $cm "setup")))
    (func (export "use-borrow") (param "b" (borrow $R)) (result u32) (canon lift (core func $cm "use-borrow")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $D
    (import "c" (instance $c
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "setup" (func))
      (export "use-borrow" (func (param "b" (borrow $R)) (result u32)))))
    (alias export $c "R" (type $R))
    (canon resource.drop $R (core func $R.resource.drop))
    (canon lower (func $c "make") (core func $make'))
    (canon lower (func $c "setup") (core func $setup'))
    (canon lower (func $c "use-borrow") (core func $use-borrow'))
    (core module $DM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "setup" (func $setup))
      (import "" "use-borrow" (func $use-borrow (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make (i32.const 1)))
        (if (i32.ne (i32.const 1) (local.get $h)) (then unreachable))
        (call $setup)
        (if (i32.ne (i32.const 42) (call $use-borrow (local.get $h))) (then unreachable))
        (call $drop (local.get $h))
        (i32.const 43)))
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make" (func $make'))
      (export "setup" (func $setup'))
      (export "use-borrow" (func $use-borrow'))
      (export "drop" (func $R.resource.drop))))))
    (func (export "run") (result u32) (canon lift (core func $dm "run"))))
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "run") (alias export $d "run"))
  (func (export "dtor-reps") (alias export $c "dtor-reps"))
  (func (export "dtor-count") (alias export $c "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance.call(&store, "run", &[]).await.expect(
        "run should succeed: the rep-1 borrow must not mark handle 1 (rep 100) as borrowed",
    );
    assert_eq!(result, vec![ComponentValue::U32(43)]);
    // Both resources were destroyed exactly once: rep 100 by the
    // defining instance inside `use-borrow`, rep 1 by the consumer.
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(2)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(101)]);
}

/// Codex review of PR #352 (finding 1, false negative): the mirror
/// image of the previous test — the borrowed resource's own entry is
/// transferred into the defining table (at handle 2, rep 1) in the
/// same call, while handle 1 (rep 100) is an unrelated decoy. The
/// borrow arrives as rep 1 and must protect the rep-1 entry, so
/// dropping the own inside the call traps (the `lend-trap` shape with
/// an index collision in the defining table).
#[tokio::test]
async fn borrow_into_defining_instance_protects_the_rep_entry_not_the_index() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "setup") (drop (call $new (i32.const 100))))
      (func (export "drop-own") (param $b i32) (param $o i32) (result i32)
        (call $drop (local.get $o))
        (i32.const 44)))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "setup") (canon lift (core func $cm "setup")))
    (func (export "drop-own") (param "b" (borrow $R)) (param "o" (own $R)) (result u32) (canon lift (core func $cm "drop-own")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $D
    (import "c" (instance $c
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "setup" (func))
      (export "drop-own" (func (param "b" (borrow $R)) (param "o" (own $R)) (result u32)))))
    (alias export $c "R" (type $R))
    (canon lower (func $c "make") (core func $make'))
    (canon lower (func $c "setup") (core func $setup'))
    (canon lower (func $c "drop-own") (core func $drop-own'))
    (core module $DM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "setup" (func $setup))
      (import "" "drop-own" (func $drop-own (param i32 i32) (result i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make (i32.const 1)))
        (if (i32.ne (i32.const 1) (local.get $h)) (then unreachable))
        (call $setup)
        (drop (call $drop-own (local.get $h) (local.get $h)))
        (i32.const 45)))
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make" (func $make'))
      (export "setup" (func $setup'))
      (export "drop-own" (func $drop-own'))))))
    (func (export "run") (result u32) (canon lift (core func $dm "run"))))
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "run") (alias export $d "run"))
  (func (export "dtor-reps") (alias export $c "dtor-reps"))
  (func (export "dtor-count") (alias export $c "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("dropping the borrowed resource inside the call must trap");
    assert!(
        matches!(&error, telomere_component::ComponentError::Trap(message) if message.contains("cannot remove owned resource while borrowed")),
        "dropping a borrowed resource should trap, got {error:?}"
    );
    // Nothing was destroyed: neither the borrowed rep-1 resource nor
    // the unrelated rep-100 decoy.
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(0)]);
}

/// Codex review of PR #352 (finding 2): an ownership transfer at a
/// call boundary removes the source entry, so it must reject an entry
/// whose borrows are still outstanding — otherwise the destination
/// can destroy the resource while the original borrow is in flight.
/// Here a middle component receives a borrow and an own of the same
/// resource and forwards the own to the defining component's drop
/// function; the call must trap instead of returning `Ok`.
#[tokio::test]
async fn ownership_transfer_of_a_borrowed_resource_is_rejected() {
    let bytes = compile_component(
        r#"
(component
  (component $D
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "drop-it") (param $h i32) (call $drop (local.get $h))))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "drop-it") (param "o" (own $R)) (canon lift (core func $cm "drop-it")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $M
    (import "d" (instance $d
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "drop-it" (func (param "o" (own $R))))))
    (alias export $d "R" (type $R))
    (export "R" (type $R))
    (canon lower (func $d "make") (core func $make'))
    (canon lower (func $d "drop-it") (core func $drop-it'))
    (core module $MM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "drop-it" (func $drop-it (param i32)))
      (func (export "make-m") (param $rep i32) (result i32) (call $make (local.get $rep)))
      (func (export "f") (param $b i32) (param $o i32) (call $drop-it (local.get $o))))
    (core instance $mm (instantiate $MM (with "" (instance
      (export "make" (func $make'))
      (export "drop-it" (func $drop-it'))))))
    (func (export "make-m") (param "rep" u32) (result (own $R)) (canon lift (core func $mm "make-m")))
    (func (export "f") (param "b" (borrow $R)) (param "o" (own $R)) (canon lift (core func $mm "f"))))
  (component $A
    (import "m" (instance $m
      (export "R" (type $R (sub resource)))
      (export "make-m" (func (param "rep" u32) (result (own $R))))
      (export "f" (func (param "b" (borrow $R)) (param "o" (own $R))))))
    (alias export $m "R" (type $R))
    (canon lower (func $m "make-m") (core func $make-m'))
    (canon lower (func $m "f") (core func $f'))
    (core module $AM
      (import "" "make-m" (func $make-m (param i32) (result i32)))
      (import "" "f" (func $f (param i32 i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make-m (i32.const 7)))
        (call $f (local.get $h) (local.get $h))
        (i32.const 42))
      (func (export "run-control") (result i32)
        (local $h1 i32) (local $h2 i32)
        (local.set $h1 (call $make-m (i32.const 7)))
        (local.set $h2 (call $make-m (i32.const 8)))
        (call $f (local.get $h1) (local.get $h2))
        (i32.const 43)))
    (core instance $am (instantiate $AM (with "" (instance
      (export "make-m" (func $make-m'))
      (export "f" (func $f'))))))
    (func (export "run") (result u32) (canon lift (core func $am "run")))
    (func (export "run-control") (result u32) (canon lift (core func $am "run-control"))))
  (instance $d (instantiate $D))
  (instance $m (instantiate $M (with "d" (instance $d))))
  (instance $a (instantiate $A (with "m" (instance $m))))
  (func (export "run") (alias export $a "run"))
  (func (export "run-control") (alias export $a "run-control"))
  (func (export "dtor-reps") (alias export $d "dtor-reps"))
  (func (export "dtor-count") (alias export $d "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    // Control first, on a clean instance: borrowing one resource while
    // a *different* one is transferred onward and destroyed is fine.
    let control = instance
        .call(&store, "run-control", &[])
        .await
        .expect("transferring an unborrowed resource onward should succeed");
    assert_eq!(control, vec![ComponentValue::U32(43)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(8)]);

    // The borrowed resource itself must not be transferred away and
    // destroyed while the borrow is outstanding.
    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("transferring a borrowed resource onward must trap");
    assert!(
        matches!(&error, telomere_component::ComponentError::Trap(message) if message.contains("cannot remove owned resource while borrowed")),
        "transferring a borrowed resource should trap, got {error:?}"
    );
    // The resource was not destroyed: the destructor still ran only
    // for the control case's rep 8.
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(8)]);
}

/// Codex review of PR #352 (round 2): two separately allocated
/// resources may carry the same rep (`resource.new` does not require
/// distinct reps), so a borrow must be registered against the owned
/// entry of the very resource that was lent, never against an
/// unrelated entry that merely holds the same rep. This is the
/// previous test with the control case's second resource also at
/// rep 7: forwarding that second, never-borrowed resource must still
/// succeed, while forwarding the borrowed one still traps.
#[tokio::test]
async fn ownership_transfer_with_duplicate_reps_protects_only_the_borrowed_resource() {
    let bytes = compile_component(
        r#"
(component
  (component $D
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "drop-it") (param $h i32) (call $drop (local.get $h))))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "drop-it") (param "o" (own $R)) (canon lift (core func $cm "drop-it")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $M
    (import "d" (instance $d
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "drop-it" (func (param "o" (own $R))))))
    (alias export $d "R" (type $R))
    (export "R" (type $R))
    (canon lower (func $d "make") (core func $make'))
    (canon lower (func $d "drop-it") (core func $drop-it'))
    (core module $MM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "drop-it" (func $drop-it (param i32)))
      (func (export "make-m") (param $rep i32) (result i32) (call $make (local.get $rep)))
      (func (export "f") (param $b i32) (param $o i32) (call $drop-it (local.get $o))))
    (core instance $mm (instantiate $MM (with "" (instance
      (export "make" (func $make'))
      (export "drop-it" (func $drop-it'))))))
    (func (export "make-m") (param "rep" u32) (result (own $R)) (canon lift (core func $mm "make-m")))
    (func (export "f") (param "b" (borrow $R)) (param "o" (own $R)) (canon lift (core func $mm "f"))))
  (component $A
    (import "m" (instance $m
      (export "R" (type $R (sub resource)))
      (export "make-m" (func (param "rep" u32) (result (own $R))))
      (export "f" (func (param "b" (borrow $R)) (param "o" (own $R))))))
    (alias export $m "R" (type $R))
    (canon lower (func $m "make-m") (core func $make-m'))
    (canon lower (func $m "f") (core func $f'))
    (core module $AM
      (import "" "make-m" (func $make-m (param i32) (result i32)))
      (import "" "f" (func $f (param i32 i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make-m (i32.const 7)))
        (call $f (local.get $h) (local.get $h))
        (i32.const 42))
      (func (export "run-control") (result i32)
        (local $h1 i32) (local $h2 i32)
        (local.set $h1 (call $make-m (i32.const 7)))
        (local.set $h2 (call $make-m (i32.const 7)))
        (call $f (local.get $h1) (local.get $h2))
        (i32.const 43)))
    (core instance $am (instantiate $AM (with "" (instance
      (export "make-m" (func $make-m'))
      (export "f" (func $f'))))))
    (func (export "run") (result u32) (canon lift (core func $am "run")))
    (func (export "run-control") (result u32) (canon lift (core func $am "run-control"))))
  (instance $d (instantiate $D))
  (instance $m (instantiate $M (with "d" (instance $d))))
  (instance $a (instantiate $A (with "m" (instance $m))))
  (func (export "run") (alias export $a "run"))
  (func (export "run-control") (alias export $a "run-control"))
  (func (export "dtor-reps") (alias export $d "dtor-reps"))
  (func (export "dtor-count") (alias export $d "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    // Control first, on a clean instance: borrowing one resource while
    // a *different* one is transferred onward and destroyed is fine.
    let control = instance
        .call(&store, "run-control", &[])
        .await
        .expect("transferring an unborrowed resource onward should succeed");
    assert_eq!(control, vec![ComponentValue::U32(43)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(7)]);

    // The borrowed resource itself must not be transferred away and
    // destroyed while the borrow is outstanding.
    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("transferring a borrowed resource onward must trap");
    assert!(
        matches!(&error, telomere_component::ComponentError::Trap(message) if message.contains("cannot remove owned resource while borrowed")),
        "transferring a borrowed resource should trap, got {error:?}"
    );
    // The resource was not destroyed: the destructor still ran only
    // for the control case's rep 7.
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(7)]);
}

/// Codex review of PR #352 (round 3): the same shape as
/// `ownership_transfer_of_a_borrowed_resource_is_rejected` with the
/// parameter order swapped — the own comes first, the borrow of the
/// same resource second. The handle-transfer walk processes the own
/// first, which removes the source entry before the borrow is
/// resolved; the borrowed identity must nevertheless be collected
/// before any owned argument moves, so the protection cannot depend
/// on parameter order. On the pre-fix code `run` returned
/// `Ok([U32(42)])` — the resource was destroyed while borrowed.
#[tokio::test]
async fn ownership_transfer_of_a_borrowed_resource_is_rejected_with_own_parameter_first() {
    let bytes = compile_component(
        r#"
(component
  (component $D
    (core module $M
      (global $dtor-reps (mut i32) (i32.const 0))
      (global $dtor-count (mut i32) (i32.const 0))
      (func (export "dtor") (param i32)
        (global.set $dtor-reps (i32.add (global.get $dtor-reps) (local.get 0)))
        (global.set $dtor-count (i32.add (global.get $dtor-count) (i32.const 1))))
      (func (export "dtor-reps") (result i32) (global.get $dtor-reps))
      (func (export "dtor-count") (result i32) (global.get $dtor-count)))
    (core instance $m (instantiate $M))
    (type $R' (resource (rep i32) (dtor (core func $m "dtor"))))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (param $rep i32) (result i32) (call $new (local.get $rep)))
      (func (export "drop-it") (param $h i32) (call $drop (local.get $h))))
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))))))
    (func (export "make") (param "rep" u32) (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "drop-it") (param "o" (own $R)) (canon lift (core func $cm "drop-it")))
    (func (export "dtor-reps") (result u32) (canon lift (core func $m "dtor-reps")))
    (func (export "dtor-count") (result u32) (canon lift (core func $m "dtor-count"))))
  (component $M
    (import "d" (instance $d
      (export "R" (type $R (sub resource)))
      (export "make" (func (param "rep" u32) (result (own $R))))
      (export "drop-it" (func (param "o" (own $R))))))
    (alias export $d "R" (type $R))
    (export "R" (type $R))
    (canon lower (func $d "make") (core func $make'))
    (canon lower (func $d "drop-it") (core func $drop-it'))
    (core module $MM
      (import "" "make" (func $make (param i32) (result i32)))
      (import "" "drop-it" (func $drop-it (param i32)))
      (func (export "make-m") (param $rep i32) (result i32) (call $make (local.get $rep)))
      (func (export "f") (param $o i32) (param $b i32) (call $drop-it (local.get $o))))
    (core instance $mm (instantiate $MM (with "" (instance
      (export "make" (func $make'))
      (export "drop-it" (func $drop-it'))))))
    (func (export "make-m") (param "rep" u32) (result (own $R)) (canon lift (core func $mm "make-m")))
    (func (export "f") (param "o" (own $R)) (param "b" (borrow $R)) (canon lift (core func $mm "f"))))
  (component $A
    (import "m" (instance $m
      (export "R" (type $R (sub resource)))
      (export "make-m" (func (param "rep" u32) (result (own $R))))
      (export "f" (func (param "o" (own $R)) (param "b" (borrow $R))))))
    (alias export $m "R" (type $R))
    (canon lower (func $m "make-m") (core func $make-m'))
    (canon lower (func $m "f") (core func $f'))
    (core module $AM
      (import "" "make-m" (func $make-m (param i32) (result i32)))
      (import "" "f" (func $f (param i32 i32)))
      (func (export "run") (result i32)
        (local $h i32)
        (local.set $h (call $make-m (i32.const 7)))
        (call $f (local.get $h) (local.get $h))
        (i32.const 42))
      (func (export "run-control") (result i32)
        (local $h1 i32) (local $h2 i32)
        (local.set $h1 (call $make-m (i32.const 7)))
        (local.set $h2 (call $make-m (i32.const 8)))
        (call $f (local.get $h1) (local.get $h2))
        (i32.const 43)))
    (core instance $am (instantiate $AM (with "" (instance
      (export "make-m" (func $make-m'))
      (export "f" (func $f'))))))
    (func (export "run") (result u32) (canon lift (core func $am "run")))
    (func (export "run-control") (result u32) (canon lift (core func $am "run-control"))))
  (instance $d (instantiate $D))
  (instance $m (instantiate $M (with "d" (instance $d))))
  (instance $a (instantiate $A (with "m" (instance $m))))
  (func (export "run") (alias export $a "run"))
  (func (export "run-control") (alias export $a "run-control"))
  (func (export "dtor-reps") (alias export $d "dtor-reps"))
  (func (export "dtor-count") (alias export $d "dtor-count")))
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    // Control first, on a clean instance: borrowing one resource while
    // a *different* one is transferred onward and destroyed is fine.
    let control = instance
        .call(&store, "run-control", &[])
        .await
        .expect("transferring an unborrowed resource onward should succeed");
    assert_eq!(control, vec![ComponentValue::U32(43)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(7)]);

    // The borrowed resource itself must not be transferred away and
    // destroyed while the borrow is outstanding.
    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("transferring a borrowed resource onward must trap");
    assert!(
        matches!(&error, telomere_component::ComponentError::Trap(message) if message.contains("cannot remove owned resource while borrowed")),
        "transferring a borrowed resource should trap, got {error:?}"
    );
    // The resource was not destroyed: the destructor still ran only
    // for the control case's rep 7.
    let count = instance
        .call(&store, "dtor-count", &[])
        .await
        .expect("dtor-count should succeed");
    assert_eq!(count, vec![ComponentValue::U32(1)]);
    let reps = instance
        .call(&store, "dtor-reps", &[])
        .await
        .expect("dtor-reps should succeed");
    assert_eq!(reps, vec![ComponentValue::U32(7)]);
}
