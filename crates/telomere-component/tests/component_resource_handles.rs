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
    // The canon-op trap cause is preserved across the core host
    // trampoline (#312) and phrased canonically (#335): the caller sees
    // the `unknown handle index` trap itself, not a bare `unreachable`.
    assert!(
        matches!(error, telomere_component::ComponentError::Trap(ref message) if message.contains("unknown handle index 1")),
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

// Regression tests for #335: canonical handle operations validate the
// handle index against the instance's handle table. Unknown,
// already-dropped, zero, and max indices trap with the canonical
// `unknown handle index N` text, and using a handle through another
// resource type's canon op traps with the canonical wrong-type text,
// instead of reaching the guest or silently succeeding (upstream
// `resources/handle-table.wast` lines 200-212, 260, 292, 321, 323).
//
// The canon ops below run inside core modules, so their traps cross the
// core host trampoline; the trap *cause* text is preserved across that
// boundary by the #312 trampoline fix (PR #340). Until that lands, the
// in-core cases surface as a bare `unreachable` guest trap at the public
// API; the assertions here pin the canonical text the runtime produces
// once the cause is preserved (verified against an integrated tree).

fn expect_call_trap_text(
    result: Result<Vec<ComponentValue>, telomere_component::ComponentError>,
    expected: &str,
    context: &str,
) {
    let error = result.expect_err(context);
    let text = format!("{error:?}");
    assert!(
        text.contains(expected),
        "{context}: expected an error containing `{expected}`, got {text}"
    );
}

/// The `$Traps` component of upstream `resources/handle-table.wast`:
/// `resource.drop` / `resource.rep` of a never-allocated, already-dropped,
/// zero, or max handle index must trap `unknown handle index N`.
#[tokio::test]
async fn canon_handle_ops_trap_on_unknown_handle_indices() {
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
    (func (export "drop-never-allocated") (call $drop (i32.const 5)))
    (func (export "rep-never-allocated") (drop (call $rep (i32.const 5))))
    (func (export "double-drop")
      (if (i32.ne (i32.const 1) (call $new (i32.const 100))) (then unreachable))
      (call $drop (i32.const 1))
      (call $drop (i32.const 1))
    )
    (func (export "drop-zero") (call $drop (i32.const 0)))
    (func (export "drop-max") (call $drop (i32.const 0xffffffff)))
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "new" (func $R.resource.new))
    (export "drop" (func $R.resource.drop))
    (export "rep" (func $R.resource.rep))
  ))))
  (func (export "drop-never-allocated") (canon lift (core func $m "drop-never-allocated")))
  (func (export "rep-never-allocated") (canon lift (core func $m "rep-never-allocated")))
  (func (export "double-drop") (canon lift (core func $m "double-drop")))
  (func (export "drop-zero") (canon lift (core func $m "drop-zero")))
  (func (export "drop-max") (canon lift (core func $m "drop-max")))
)
"#,
    );
    for (export, expected) in [
        ("drop-never-allocated", "unknown handle index 5"),
        ("rep-never-allocated", "unknown handle index 5"),
        ("double-drop", "unknown handle index 1"),
        ("drop-zero", "unknown handle index 0"),
        ("drop-max", "unknown handle index 4294967295"),
    ] {
        let (store, instance) = instantiate(&bytes).await;
        let result = instance.call(&store, export, &[]).await;
        expect_call_trap_text(result, expected, export);
    }
}

/// The `$WrongType` component of upstream `resources/handle-table.wast`:
/// two resource types share one table, but each entry remembers its type.
/// Dropping an $R1 handle through $R2's `resource.drop`, and returning an
/// $R1 handle from a function typed as returning `own $R2`, must both
/// trap with the canonical wrong-type text.
#[tokio::test]
async fn canon_handle_ops_trap_on_wrong_type_handles() {
    let bytes = compile_component(
        r#"
(component
  (type $R1 (resource (rep i32)))
  (type $R2 (resource (rep i32)))
  (canon resource.new $R1 (core func $R1.resource.new))
  (canon resource.drop $R2 (core func $R2.resource.drop))
  (core module $M
    (import "" "R1.new" (func $R1.new (param i32) (result i32)))
    (import "" "R2.drop" (func $R2.drop (param i32)))
    (func (export "drop-R1-as-R2")
      (if (i32.ne (i32.const 1) (call $R1.new (i32.const 100))) (then unreachable))
      (call $R2.drop (i32.const 1))
    )
    (func (export "return-R1-as-R2") (result i32)
      (call $R1.new (i32.const 100))
    )
  )
  (core instance $m (instantiate $M (with "" (instance
    (export "R1.new" (func $R1.resource.new))
    (export "R2.drop" (func $R2.resource.drop))
  ))))
  (export $R2' "R2" (type $R2))
  (func (export "drop-R1-as-R2") (canon lift (core func $m "drop-R1-as-R2")))
  (func (export "return-R1-as-R2") (result (own $R2')) (canon lift (core func $m "return-R1-as-R2")))
)
"#,
    );
    const WRONG_TYPE: &str = "handle index 1 used with the wrong type, expected \
        guest-defined resource but found a different guest-defined resource";
    let (store, instance) = instantiate(&bytes).await;
    let result = instance.call(&store, "drop-R1-as-R2", &[]).await;
    expect_call_trap_text(result, WRONG_TYPE, "drop-R1-as-R2");
    let (store, instance) = instantiate(&bytes).await;
    let result = instance.call(&store, "return-R1-as-R2", &[]).await;
    expect_call_trap_text(result, WRONG_TYPE, "return-R1-as-R2");
}

/// The middleman section of upstream `resources/handle-table.wast`
/// (lines 186-212): after $D drops the handle it received, passing the
/// same index to a lowered `consume` (own) is not reached in these cases;
/// the pinned cases here are a `borrow` argument naming an index that
/// was never valid in the caller's table, which must trap
/// `unknown handle index 3` at the canon-lower boundary instead of
/// reaching the callee, and a valid borrow of a live handle, which must
/// keep working.
#[tokio::test]
async fn canon_lower_borrow_args_validate_handle_indices() {
    let bytes = compile_component(
        r#"
(component
  (component $C
    (type $R' (resource (rep i32)))
    (export $R "R" (type $R'))
    (canon resource.new $R' (core func $R.resource.new))
    (canon resource.drop $R' (core func $R.resource.drop))
    (core module $CM
      (import "" "new" (func $new (param i32) (result i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "make") (result i32) (call $new (i32.const 100)))
      (func (export "consume") (param $h i32) (call $drop (local.get $h)))
      (func (export "use") (param $rep i32))
    )
    (core instance $cm (instantiate $CM (with "" (instance
      (export "new" (func $R.resource.new))
      (export "drop" (func $R.resource.drop))
    ))))
    (func (export "make") (result (own $R)) (canon lift (core func $cm "make")))
    (func (export "consume") (param "r" (own $R)) (canon lift (core func $cm "consume")))
    (func (export "use") (param "r" (borrow $R)) (canon lift (core func $cm "use")))
  )
  (component $D
    (import "c" (instance $c
      (export "R" (type $R (sub resource)))
      (export "make" (func (result (own $R))))
      (export "consume" (func (param "r" (own $R))))
      (export "use" (func (param "r" (borrow $R))))
    ))
    (alias export $c "R" (type $R))
    (canon resource.drop $R (core func $R.resource.drop))
    (canon lower (func $c "make") (core func $make'))
    (canon lower (func $c "consume") (core func $consume'))
    (canon lower (func $c "use") (core func $use'))
    (core module $DM
      (import "" "make" (func $make (result i32)))
      (import "" "consume" (func $consume (param i32)))
      (import "" "use" (func $use (param i32)))
      (import "" "drop" (func $drop (param i32)))
      (func (export "own-use-after-drop")
        (if (i32.ne (i32.const 1) (call $make)) (then unreachable))
        (call $drop (i32.const 1))
        (call $consume (i32.const 1))
      )
      (func (export "borrow-never-valid")
        (call $use (i32.const 3))
      )
      (func (export "borrow-valid")
        (if (i32.ne (i32.const 1) (call $make)) (then unreachable))
        (call $use (i32.const 1))
      )
    )
    (core instance $dm (instantiate $DM (with "" (instance
      (export "make" (func $make'))
      (export "consume" (func $consume'))
      (export "use" (func $use'))
      (export "drop" (func $R.resource.drop))
    ))))
    (func (export "own-use-after-drop") (canon lift (core func $dm "own-use-after-drop")))
    (func (export "borrow-never-valid") (canon lift (core func $dm "borrow-never-valid")))
    (func (export "borrow-valid") (canon lift (core func $dm "borrow-valid")))
  )
  (instance $c (instantiate $C))
  (instance $d (instantiate $D (with "c" (instance $c))))
  (func (export "own-use-after-drop") (alias export $d "own-use-after-drop"))
  (func (export "borrow-never-valid") (alias export $d "borrow-never-valid"))
  (func (export "borrow-valid") (alias export $d "borrow-valid"))
)
"#,
    );
    let (store, instance) = instantiate(&bytes).await;
    let result = instance.call(&store, "borrow-never-valid", &[]).await;
    expect_call_trap_text(result, "unknown handle index 3", "borrow-never-valid");
    let (store, instance) = instantiate(&bytes).await;
    let result = instance.call(&store, "own-use-after-drop", &[]).await;
    expect_call_trap_text(result, "unknown handle index 1", "own-use-after-drop");
    let (store, instance) = instantiate(&bytes).await;
    instance
        .call(&store, "borrow-valid", &[])
        .await
        .expect("borrowing a live handle through canon lower should succeed");
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
    // wrong-type error surfaces instead of a flattened guest trap;
    // since #335 that text is the canonical wrong-type trap.
    assert!(
        matches!(
            &error,
            telomere_component::ComponentError::Trap(message)
                if message.contains("handle index 2 used with the wrong type")
        ),
        "wrong-type drop should trap, got {error:?}"
    );
}
