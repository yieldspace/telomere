//! Regression tests for issue #312: canonical ABI loads and stores must
//! trap on unaligned pointers instead of silently succeeding, and the
//! trap cause must survive the canon-lower trampoline (a core caller
//! observes an `unreachable` trap, but the component-level error must
//! keep the canonical reason).

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component WAT must be valid")
}

async fn instantiate(text: &str) -> (telomere::Store, telomere_component::ComponentInstance) {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&compile_component(text))
        .expect("component must compile");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("component must instantiate");
    (store, instance)
}

async fn expect_unaligned_trap(text: &str, export: &str) {
    let (store, instance) = instantiate(text).await;
    let error = instance
        .call(&store, export, &[])
        .await
        .expect_err("misaligned canonical pointer must trap");
    let message = error.to_string();
    assert!(
        message.contains("unaligned pointer") || message.contains("not aligned"),
        "expected an unaligned-pointer trap, got: {message}"
    );
}

/// The issue's original repro: a core function returns a misaligned
/// retptr (1) for a `(tuple u32 u32)` result; lifting the results reads
/// the tuple from address 1, which is not 4-aligned.
#[tokio::test]
async fn misaligned_retptr_traps_on_lift() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func (export "f") (result (tuple u32 u32))
    (canon lift (core func $m "f") (memory $m "mem"))))
"#;
    expect_unaligned_trap(text, "f").await;
}

/// The caller passes a misaligned retptr (1) to a lowered import; storing
/// the `(tuple u32 u32)` result at address 1 must trap.
#[tokio::test]
async fn misaligned_retptr_traps_on_lower() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32) (i32.const 0)))
  (core instance $m (instantiate $M))
  (func $f (result (tuple u32 u32))
    (canon lift (core func $m "f") (memory $m "mem")))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32)))
    (func (export "run") (call $f (i32.const 1))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    expect_unaligned_trap(text, "run").await;
}

/// The caller passes a misaligned argptr (1) for spilled (>16 flat)
/// params; lifting the argument tuple from address 1 must trap.
#[tokio::test]
async fn misaligned_argptr_traps_on_lift() {
    let text = r#"
(component
  (type $big (tuple u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32))
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (param i32))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 8)))
  (core instance $m (instantiate $M))
  (func $f (param "a" $big)
    (canon lift (core func $m "f")
      (memory $m "mem") (realloc (func $m "realloc"))))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32)))
    (func (export "run") (call $f (i32.const 1))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    expect_unaligned_trap(text, "run").await;
}

/// The callee's realloc returns a misaligned pointer (1) for the spilled
/// argument area; the canonical trap raised inside the canon-lower
/// trampoline must keep its cause at the component level.
#[tokio::test]
async fn misaligned_realloc_result_traps_for_spilled_args() {
    let text = r#"
(component
  (type $big (tuple u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32 u32))
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (param i32))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 1)))
  (core instance $m (instantiate $M))
  (func $f (param "a" $big)
    (canon lift (core func $m "f")
      (memory $m "mem") (realloc (func $m "realloc"))))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32)))
    (func (export "run") (call $f (i32.const 8))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    expect_unaligned_trap(text, "run").await;
}

/// A utf16 caller-side string pointer must be 2-aligned, even when the
/// length is zero.
#[tokio::test]
async fn misaligned_utf16_string_pointer_traps_even_when_empty() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (param i32 i32))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 8)))
  (core instance $m (instantiate $M))
  (func $f (param "s" string)
    (canon lift (core func $m "f") string-encoding=utf8
      (memory $m "mem") (realloc (func $m "realloc"))))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) string-encoding=utf16
    (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32 i32)))
    (func (export "run") (call $f (i32.const 1) (i32.const 0))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    expect_unaligned_trap(text, "run").await;
}

/// A latin1+utf16 caller-side string pointer must be 2-aligned for both
/// dynamic representations: untagged (latin1) and tagged (utf16) lengths.
#[tokio::test]
async fn misaligned_latin1_utf16_string_pointer_traps() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (param i32 i32))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 8)))
  (core instance $m (instantiate $M))
  (func $f (param "s" string)
    (canon lift (core func $m "f") string-encoding=utf8
      (memory $m "mem") (realloc (func $m "realloc"))))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) string-encoding=latin1+utf16
    (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32 i32)))
    (func (export "run-latin1") (call $f (i32.const 1) (i32.const 0)))
    (func (export "run-utf16") (call $f (i32.const 1) (i32.const 0x80000000))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run-latin1") (canon lift (core func $main "run-latin1")))
  (func (export "run-utf16") (canon lift (core func $main "run-utf16"))))
"#;
    expect_unaligned_trap(text, "run-latin1").await;
    expect_unaligned_trap(text, "run-utf16").await;
}

/// A list element pointer must be aligned to the element type: a
/// `(list u64)` passed with pointer 2 must trap when its elements are
/// lifted.
#[tokio::test]
async fn misaligned_list_element_pointer_traps() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (param i32 i32))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32) (i32.const 8)))
  (core instance $m (instantiate $M))
  (func $f (param "xs" (list u64))
    (canon lift (core func $m "f")
      (memory $m "mem") (realloc (func $m "realloc"))))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32 i32)))
    (func (export "run") (call $f (i32.const 2) (i32.const 1))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    expect_unaligned_trap(text, "run").await;
}

/// Control: the same shapes with aligned pointers must keep working, so
/// the alignment checks cannot be over-eager.
#[tokio::test]
async fn aligned_pointers_still_work() {
    let text = r#"
(component
  (core module $M
    (memory (export "mem") 1)
    (func (export "f") (result i32) (i32.const 0)))
  (core instance $m (instantiate $M))
  (func $f (export "f") (result (tuple u32 u32))
    (canon lift (core func $m "f") (memory $m "mem")))
  (core module $Libc (memory (export "mem") 1))
  (core instance $libc (instantiate $Libc))
  (core func $f' (canon lower (func $f) (memory $libc "mem")))
  (core module $Main
    (import "" "f" (func $f (param i32)))
    (func (export "run") (call $f (i32.const 8))))
  (core instance $main (instantiate $Main (with "" (instance (export "f" (func $f'))))))
  (func (export "run") (canon lift (core func $main "run"))))
"#;
    let (store, instance) = instantiate(text).await;
    let values = instance
        .call(&store, "f", &[])
        .await
        .expect("aligned lift must succeed");
    assert_eq!(
        values,
        vec![ComponentValue::Tuple(vec![
            ComponentValue::U32(0),
            ComponentValue::U32(0)
        ])]
    );
    instance
        .call(&store, "run", &[])
        .await
        .expect("aligned lower roundtrip must succeed");
}
