//! Regression tests for issue #314: a *declared* variant whose cases happen
//! to match the `option` / `result` / `enum` sugar shapes must still be
//! represented as `ComponentValue::Variant` by the dynamic API. Only a type
//! actually declared with the sugar constructor may use the sugar
//! representation (`Option` / `Result` / `Enum`).

use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

async fn instantiate_component(
    text: &str,
    linker: &ComponentLinker,
) -> (telomere::Store, telomere_component::ComponentInstance) {
    let bytes = compile_component(text);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, linker)
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

/// A component that passes two flat core values to a host import typed with
/// `sig_ty`. Lifting the flat arguments for the host callback exercises the
/// flat lift path; no memory is involved.
fn flat_import_component(type_decls: &str, sig_ty: &str) -> String {
    format!(
        r#"
(component
  {type_decls}
  (type $t (func (param "value" {sig_ty})))
  (import "host" (func $host (type $t)))
  (core func $host-lower (canon lower (func $host)))
  (core module $m
    (import "" "host" (func $host (param i32 i32)))
    (func (export "run") (param i32 i32) (result i32)
      (call $host (local.get 0) (local.get 1))
      (local.get 0)
    )
  )
  (core instance $i (instantiate $m (with "" (instance (export "host" (func $host-lower))))))
  (func (export "run") (param "disc" u32) (param "payload" u32) (result u32)
    (canon lift (core func $i "run"))
  )
)
"#,
        type_decls = type_decls,
        sig_ty = sig_ty,
    )
}

async fn assert_component_echo(text: &str, input: ComponentValue) {
    let (store, instance) = instantiate_component(text, &ComponentLinker::new()).await;
    let result = instance
        .call(&store, "run", std::slice::from_ref(&input))
        .await
        .expect("echo call should succeed");
    assert_eq!(result, vec![input]);
}

async fn assert_flat_import_args(text: &str, disc: u32, payload: u32, expected: ComponentValue) {
    let mut linker = ComponentLinker::new();
    linker.register_import("host", move |_store, args| {
        assert_eq!(args, std::slice::from_ref(&expected));
        Ok(vec![])
    });
    let (store, instance) = instantiate_component(text, &linker).await;
    let result = instance
        .call(
            &store,
            "run",
            &[ComponentValue::U32(disc), ComponentValue::U32(payload)],
        )
        .await
        .expect("run call should succeed");
    assert_eq!(result, vec![ComponentValue::U32(disc)]);
}

/// A component whose exported `run` echoes a single-flat-value argument
/// through a core function (usable for payloadless variants and enums,
/// whose flattened form is just the discriminant).
fn flat_echo_component(type_decls: &str, sig_ty: &str) -> String {
    format!(
        r#"
(component
  {type_decls}
  (core module $m
    (func (export "run") (param i32) (result i32) (local.get 0))
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" {sig_ty}) (result {sig_ty})
    (canon lift (core func $i "run"))
  )
)
"#,
        type_decls = type_decls,
        sig_ty = sig_ty,
    )
}

async fn assert_flat_echo(text: &str, input: ComponentValue) {
    let (store, instance) = instantiate_component(text, &ComponentLinker::new()).await;
    let result = instance
        .call(&store, "run", std::slice::from_ref(&input))
        .await
        .expect("echo call should succeed");
    assert_eq!(result, vec![input]);
}

/// A component whose exported `run` forwards its argument through memory to
/// a host import and back, exercising the indirect (memory) lift/lower paths
/// on both sides of the call.
fn host_roundtrip_component(type_decls: &str, func_sig: &str) -> String {
    // The forwarded value flattens to two words; the lowered host import
    // takes those two words plus the result-area pointer.
    let run_params = "i32 i32";
    let host_params = "i32 i32 i32";
    let gets = "(local.get 0) (local.get 1)";
    format!(
        r#"
(component
  {type_decls}
  (type $t (func {func_sig}))
  (import "host" (func $host (type $t)))
  (core module $libc
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr
        (i32.and
          (i32.add (global.get $bump) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) (memory (core memory $libc "memory")) (realloc (core func $libc "realloc")))
  )
  (core module $forward
    (import "" "host" (func $host (param {host_params})))
    (import "" "realloc" (func $realloc (param i32 i32 i32 i32) (result i32)))
    (func (export "run") (param {run_params}) (result i32)
      (local $ret i32)
      (local.set $ret (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 16)))
      (call $host {gets} (local.get $ret))
      (local.get $ret)
    )
  )
  (core instance $forward
    (instantiate $forward
      (with "" (instance
        (export "host" (func $host-lower))
        (export "realloc" (func $libc "realloc"))
      ))
    )
  )
  (func (export "run") (type $t)
    (canon lift (core func $forward "run") (memory (core memory $libc "memory")) (realloc (core func $libc "realloc")))
  )
)
"#,
        type_decls = type_decls,
        func_sig = func_sig,
        run_params = run_params,
        host_params = host_params,
        gets = gets,
    )
}

async fn assert_dynamic_roundtrip(text: &str, input: ComponentValue) {
    let mut linker = ComponentLinker::new();
    let expected = input.clone();
    linker.register_import("host", move |_store, args| {
        assert_eq!(args, std::slice::from_ref(&expected));
        Ok(vec![expected.clone()])
    });

    let (store, instance) = instantiate_component(text, &linker).await;
    let result = instance
        .call(&store, "run", std::slice::from_ref(&input))
        .await
        .expect("roundtrip call should succeed");
    assert_eq!(result, vec![input]);
}

const NONE_SOME_VARIANT: &str = r#"(type $value (variant (case "none") (case "some" u32)))"#;

#[tokio::test]
async fn declared_variant_with_option_shape_lifts_as_variant_flat() {
    let component = flat_import_component(NONE_SOME_VARIANT, "$value");
    assert_flat_import_args(
        &component,
        1,
        42,
        ComponentValue::Variant {
            case: "some".to_owned(),
            value: Some(Box::new(ComponentValue::U32(42))),
        },
    )
    .await;
    assert_flat_import_args(
        &component,
        0,
        0,
        ComponentValue::Variant {
            case: "none".to_owned(),
            value: None,
        },
    )
    .await;
}

#[tokio::test]
async fn declared_variant_with_option_shape_lifts_as_variant_through_memory() {
    let component = host_roundtrip_component(
        NONE_SOME_VARIANT,
        r#"(param "value" $value) (result $value)"#,
    );
    assert_dynamic_roundtrip(
        &component,
        ComponentValue::Variant {
            case: "some".to_owned(),
            value: Some(Box::new(ComponentValue::U32(314159265))),
        },
    )
    .await;
    assert_dynamic_roundtrip(
        &component,
        ComponentValue::Variant {
            case: "none".to_owned(),
            value: None,
        },
    )
    .await;
}

#[tokio::test]
async fn declared_variant_value_lowers_as_variant_flat() {
    // The dynamic API lowers host-supplied `Variant` values for a declared
    // variant parameter into the flat discriminant/payload words.
    let component = r#"
(component
  (type $value (variant (case "none") (case "some" u32)))
  (core module $m
    (func (export "run") (param i32 i32) (result i32) (local.get 0))
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result u32)
    (canon lift (core func $i "run"))
  )
)
"#;
    let (store, instance) = instantiate_component(component, &ComponentLinker::new()).await;
    let some = instance
        .call(
            &store,
            "run",
            &[ComponentValue::Variant {
                case: "some".to_owned(),
                value: Some(Box::new(ComponentValue::U32(42))),
            }],
        )
        .await
        .expect("run call should succeed");
    assert_eq!(some, vec![ComponentValue::U32(1)]);
    let none = instance
        .call(
            &store,
            "run",
            &[ComponentValue::Variant {
                case: "none".to_owned(),
                value: None,
            }],
        )
        .await
        .expect("run call should succeed");
    assert_eq!(none, vec![ComponentValue::U32(0)]);
}

#[tokio::test]
async fn declared_variant_with_result_shape_lifts_as_variant_flat() {
    let component = flat_import_component(
        r#"(type $value (variant (case "ok" u32) (case "err" u32)))"#,
        "$value",
    );
    assert_flat_import_args(
        &component,
        1,
        7,
        ComponentValue::Variant {
            case: "err".to_owned(),
            value: Some(Box::new(ComponentValue::U32(7))),
        },
    )
    .await;
}

#[tokio::test]
async fn declared_variant_with_payloadless_cases_lifts_as_variant_flat() {
    let component = flat_echo_component(
        r#"(type $value (variant (case "a") (case "b")))
           (export "value" (type $value))"#,
        "$value",
    );
    assert_flat_echo(
        &component,
        ComponentValue::Variant {
            case: "b".to_owned(),
            value: None,
        },
    )
    .await;
}

#[tokio::test]
async fn nested_declared_variant_keeps_variant_form_through_memory() {
    // The inner variant is itself option-shaped; both levels must keep the
    // declared `Variant` representation when the value is read back from
    // memory. The core function stores its three flat argument words into
    // a fresh allocation and returns the pointer.
    let component = r#"
(component
  (type $inner (variant (case "none") (case "some" u32)))
  (export "inner" (type $inner))
  (type $value (variant (case "wrap" $inner) (case "empty")))
  (export "value" (type $value))
  (core module $m
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func $realloc (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr
        (i32.and
          (i32.add (global.get $bump) (i32.sub (local.get $align) (i32.const 1)))
          (i32.sub (i32.const 0) (local.get $align))))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
    (func (export "run") (param i32 i32 i32) (result i32)
      (local $ret i32)
      (local.set $ret (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 12)))
      (i32.store (local.get $ret) (local.get 0))
      (i32.store offset=4 (local.get $ret) (local.get 1))
      (i32.store offset=8 (local.get $ret) (local.get 2))
      (local.get $ret)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run") (memory (core memory $i "memory")) (realloc (core func $i "realloc")))
  )
)
"#;
    assert_component_echo(
        component,
        ComponentValue::Variant {
            case: "wrap".to_owned(),
            value: Some(Box::new(ComponentValue::Variant {
                case: "some".to_owned(),
                value: Some(Box::new(ComponentValue::U32(99))),
            })),
        },
    )
    .await;
}

#[tokio::test]
async fn option_type_still_lifts_as_option_flat() {
    let component = flat_import_component("", "(option u32)");
    assert_flat_import_args(
        &component,
        1,
        42,
        ComponentValue::Option(Some(Box::new(ComponentValue::U32(42)))),
    )
    .await;
    assert_flat_import_args(&component, 0, 0, ComponentValue::Option(None)).await;
}

#[tokio::test]
async fn option_type_still_lifts_as_option_through_memory() {
    let component =
        host_roundtrip_component("", r#"(param "value" (option u32)) (result (option u32))"#);
    assert_dynamic_roundtrip(
        &component,
        ComponentValue::Option(Some(Box::new(ComponentValue::U32(314159265)))),
    )
    .await;
}

#[tokio::test]
async fn result_and_enum_types_keep_their_representations_flat() {
    let result_component = flat_import_component("", "(result u32 (error u32))");
    assert_flat_import_args(
        &result_component,
        1,
        7,
        ComponentValue::Result {
            ok: None,
            err: Some(Box::new(ComponentValue::U32(7))),
        },
    )
    .await;

    let enum_component = flat_echo_component(
        r#"(type $value (enum "a" "b"))
           (export "value" (type $value))"#,
        "$value",
    );
    assert_flat_echo(&enum_component, ComponentValue::Enum("b".to_owned())).await;
}
