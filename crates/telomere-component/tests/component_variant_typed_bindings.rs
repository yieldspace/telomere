//! Regression tests for the PR #374 Codex review: typed bindings
//! (`get_typed_func`, `ComponentFunc::typed`) must not accept a *declared
//! variant* whose cases merely have the `option` / `result` shape. Since
//! #314, values of such a type lift as `ComponentValue::Variant`, so a
//! `Option<T>` / `Result<T, E>` binding validated only by case shape
//! produced a wrapper that failed when invoked. Typed signature validation
//! now requires the declared constructor to be `option` / `result` itself,
//! matching wasmtime, whose typed conversions distinguish the constructors.
//!
//! Typed *host imports* have no signature-validation step in this engine
//! (bindings are type-erased at registration; every typed-import mismatch
//! surfaces at call time). The last test pins that the declared-variant
//! argument is at least not silently reinterpreted as a `Result` there.
//!
//! Round 2 (export ascriptions): an export may ascribe a supertype to the
//! function it exports, and that ascribed type is the function's public
//! type — the one typed bindings validate against. When the ascription
//! differs from the definition's type only in the declared constructors
//! of variant-shaped types (a declared `variant` exported as the
//! `result` / `option` it desugars to), calls through the export must
//! convert values per the export type, or a binding accepted at lookup
//! fails when invoked. The tests at the bottom pin that the ascribed
//! representation is honored on the flat and memory paths, in both
//! directions, and that an ascription naming the variant itself changes
//! nothing.

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

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

/// An exported function returning a payloadless two-case type declared with
/// the given constructor (`variant` for the bug shape, `result` for the
/// control). The core function returns the discriminant.
fn payloadless_export(constructor: &str) -> String {
    format!(
        r#"
(component
  (type $v {constructor})
  (core module $m
    (func (export "f") (result i32) i32.const 0))
  (core instance $i (instantiate $m))
  (func (export "f") (result $v)
    (canon lift (core func $i "f")))
)
"#,
        constructor = constructor,
    )
}

#[tokio::test]
async fn typed_result_binding_rejects_declared_variant_with_result_shape() {
    let component = payloadless_export(r#"(variant (case "ok") (case "err"))"#);
    let (store, instance) = instantiate_component(&component, &ComponentLinker::new()).await;

    let error = instance
        .get_typed_func::<(), Result<(), ()>>("f")
        .err()
        .expect("declared variant must not bind as Result");
    assert!(
        matches!(&error, ComponentError::Link(message) if message.contains("expects result")),
        "expected a link error naming the result expectation, got {error:?}"
    );

    // The dynamic API is unaffected: the value is a plain Variant.
    let dynamic = instance
        .call(&store, "f", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(
        dynamic,
        vec![ComponentValue::Variant {
            case: "ok".to_owned(),
            value: None,
        }]
    );
}

#[tokio::test]
async fn typed_result_binding_accepts_real_result_type() {
    let component = payloadless_export("(result)");
    let (store, instance) = instantiate_component(&component, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(), Result<(), ()>>("f")
        .expect("real result type must bind as Result");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        Ok(())
    );
}

#[tokio::test]
async fn typed_result_param_rejects_declared_variant_with_result_shape() {
    let component = r#"
(component
  (type $v (variant (case "ok") (case "err")))
  (core module $m
    (func (export "run") (param i32) (result i32) (local.get 0)))
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $v) (result u32)
    (canon lift (core func $i "run")))
)
"#;
    let (store, instance) = instantiate_component(component, &ComponentLinker::new()).await;
    assert!(
        instance
            .get_typed_func::<(Result<(), ()>,), u32>("run")
            .is_err(),
        "declared variant parameter must not bind as Result"
    );
    // Dynamic call with a Variant value still works.
    let dynamic = instance
        .call(
            &store,
            "run",
            &[ComponentValue::Variant {
                case: "ok".to_owned(),
                value: None,
            }],
        )
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::U32(0)]);
}

/// A component whose exported `run` forwards its argument through memory to
/// a host import and back (same shape as the #314 roundtrip tests).
fn host_roundtrip_component(type_decls: &str, func_sig: &str) -> String {
    host_roundtrip_component_words(type_decls, func_sig, 2)
}

/// Same roundtrip, for values flattening to `words` core words.
fn host_roundtrip_component_words(type_decls: &str, func_sig: &str, words: usize) -> String {
    let run_params = vec!["i32"; words].join(" ");
    let host_params = vec!["i32"; words + 1].join(" ");
    let gets = (0..words)
        .map(|index| format!("(local.get {index})"))
        .collect::<Vec<_>>()
        .join(" ");
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

#[tokio::test]
async fn typed_option_binding_rejects_declared_variant_with_option_shape() {
    let component = host_roundtrip_component(
        r#"(type $value (variant (case "none") (case "some" u32)))"#,
        r#"(param "value" $value) (result $value)"#,
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (value,): (Option<u32>,)| Ok(value));
    let (_store, instance) = instantiate_component(&component, &linker).await;
    let error = instance
        .get_typed_func::<(Option<u32>,), Option<u32>>("run")
        .err()
        .expect("declared variant must not bind as Option");
    assert!(
        matches!(&error, ComponentError::Link(message) if message.contains("expects option")),
        "expected a link error naming the option expectation, got {error:?}"
    );
}

#[tokio::test]
async fn typed_option_binding_rejects_option_of_declared_result_shaped_variant() {
    // The outer type is a real `option`, but its payload is a declared
    // variant with the result shape: the nested typed conversion must be
    // rejected as well.
    let component = host_roundtrip_component_words(
        r#"(type $inner (variant (case "ok" u32) (case "err" u32)))"#,
        r#"(param "value" (option $inner)) (result (option $inner))"#,
        3,
    );
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| Ok(args.to_vec()));
    let (_store, instance) = instantiate_component(&component, &linker).await;
    assert!(
        instance
            .get_typed_func::<(Option<Result<u32, u32>>,), Option<Result<u32, u32>>>("run")
            .is_err(),
        "option of a declared result-shaped variant must not bind as Option<Result>"
    );
}

#[tokio::test]
async fn typed_option_binding_accepts_real_option_type() {
    let component =
        host_roundtrip_component("", r#"(param "value" (option u32)) (result (option u32))"#);
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (value,): (Option<u32>,)| Ok(value));
    let (store, instance) = instantiate_component(&component, &linker).await;
    let func = instance
        .get_typed_func::<(Option<u32>,), Option<u32>>("run")
        .expect("real option type must bind as Option");
    assert_eq!(
        func.call(&store, (Some(123),))
            .await
            .expect("typed call should succeed"),
        Some(123)
    );
    assert_eq!(
        func.call(&store, (None,))
            .await
            .expect("typed call should succeed"),
        None
    );
}

#[tokio::test]
async fn typed_host_import_does_not_silently_convert_declared_variant() {
    // Typed host imports are not signature-validated in this engine (the
    // binding is type-erased at registration, so -- as with every other
    // typed-import mismatch -- the conversion failure surfaces when the
    // guest calls the import). What must not happen is the pre-#314
    // behaviour: silently reinterpreting the declared variant's value as a
    // `Result` and invoking the host function with it.
    let component = r#"
(component
  (type $v (variant (case "ok") (case "err")))
  (type $t (func (param "value" $v)))
  (import "host" (func $host (type $t)))
  (core func $host-lower (canon lower (func $host)))
  (core module $m
    (import "" "host" (func $host (param i32)))
    (func (export "run") (result i32)
      (call $host (i32.const 0))
      (i32.const 0))
  )
  (core instance $i (instantiate $m (with "" (instance (export "host" (func $host-lower))))))
  (func (export "run") (result u32)
    (canon lift (core func $i "run")))
)
"#;
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (_value,): (Result<(), ()>,)| Ok(()));
    let (store, instance) = instantiate_component(component, &linker).await;
    assert!(
        instance.call(&store, "run", &[]).await.is_err(),
        "typed host import must not receive a declared variant as a Result"
    );

    // Control: a dynamic host import receives the Variant value as-is.
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert_eq!(
            args,
            &[ComponentValue::Variant {
                case: "ok".to_owned(),
                value: None,
            }]
        );
        Ok(vec![])
    });
    let (store, instance) = instantiate_component(component, &linker).await;
    assert_eq!(
        instance
            .call(&store, "run", &[])
            .await
            .expect("dynamic call should succeed"),
        vec![ComponentValue::U32(0)]
    );
}

/// The Codex round-2 shape: a function whose own type returns a declared
/// variant, exported under an ascription naming the `result` type the
/// variant desugars to. The core function returns the discriminant.
fn result_ascribed_export() -> &'static str {
    r#"
(component
  (type $v (variant (case "ok") (case "err")))
  (core module $m
    (func (export "f") (result i32) i32.const 0))
  (core instance $i (instantiate $m))
  (func $f (result $v)
    (canon lift (core func $i "f")))
  (export "f" (func $f) (func (result (result))))
)
"#
}

#[tokio::test]
async fn typed_result_binding_honors_export_ascription() {
    let (store, instance) =
        instantiate_component(result_ascribed_export(), &ComponentLinker::new()).await;

    // The binding validates against the export's ascribed type and the
    // call must now succeed: values are lifted per the export type.
    let func = instance
        .get_typed_func::<(), Result<(), ()>>("f")
        .expect("ascribed result type must bind as Result");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        Ok(())
    );

    // The dynamic API sees the same representation as a function whose
    // type is declared with the `result` constructor outright.
    let (control_store, control) =
        instantiate_component(&payloadless_export("(result)"), &ComponentLinker::new()).await;
    let control_dynamic = control
        .call(&control_store, "f", &[])
        .await
        .expect("control dynamic call should succeed");
    assert!(matches!(
        control_dynamic.as_slice(),
        [ComponentValue::Result { .. }]
    ));
    let dynamic = instance
        .call(&store, "f", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, control_dynamic);
}

#[tokio::test]
async fn variant_ascription_keeps_variant_representation() {
    // Ascribing the very variant type the function already has must not
    // change anything: the export type is still a declared variant, so a
    // Result binding is rejected at lookup and dynamic values stay
    // Variants.
    let component = r#"
(component
  (type $v (variant (case "ok") (case "err")))
  (core module $m
    (func (export "f") (result i32) i32.const 0))
  (core instance $i (instantiate $m))
  (func $f (result $v)
    (canon lift (core func $i "f")))
  (export "f" (func $f) (func (result $v)))
)
"#;
    let (store, instance) = instantiate_component(component, &ComponentLinker::new()).await;
    assert!(
        instance.get_typed_func::<(), Result<(), ()>>("f").is_err(),
        "variant-ascribed export must not bind as Result"
    );
    let dynamic = instance
        .call(&store, "f", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(
        dynamic,
        vec![ComponentValue::Variant {
            case: "ok".to_owned(),
            value: None,
        }]
    );
}

#[tokio::test]
async fn typed_result_param_honors_export_ascription() {
    // Parameter direction: the function's own type takes the declared
    // variant; the export ascribes the `result` it desugars to. The core
    // function echoes the discriminant it receives.
    let component = r#"
(component
  (type $v (variant (case "ok") (case "err")))
  (core module $m
    (func (export "run") (param i32) (result i32) (local.get 0)))
  (core instance $i (instantiate $m))
  (func $f (param "value" $v) (result u32)
    (canon lift (core func $i "run")))
  (export "run" (func $f) (func (param "value" (result)) (result u32)))
)
"#;
    let (store, instance) = instantiate_component(component, &ComponentLinker::new()).await;
    let func = instance
        .get_typed_func::<(Result<(), ()>,), u32>("run")
        .expect("ascribed result parameter must bind as Result");
    assert_eq!(
        func.call(&store, (Ok(()),))
            .await
            .expect("typed call should succeed"),
        0
    );
    assert_eq!(
        func.call(&store, (Err(()),))
            .await
            .expect("typed call should succeed"),
        1
    );
}

/// A component exporting two functions that return an option-shaped
/// declared variant through memory, ascribed as returning `(option u32)`.
/// `some` writes discriminant 1 / payload 42 at offset 0; `none` points
/// at zeroed memory (discriminant 0).
fn option_ascribed_exports() -> &'static str {
    r#"
(component
  (type $v (variant (case "none") (case "some" u32)))
  (core module $m
    (memory (export "memory") 1)
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      i32.const 1024)
    (func (export "some") (result i32)
      i32.const 0
      i32.const 1
      i32.store
      i32.const 4
      i32.const 42
      i32.store
      i32.const 0)
    (func (export "none") (result i32)
      i32.const 64)
  )
  (core instance $i (instantiate $m))
  (func $some (result $v)
    (canon lift (core func $i "some") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (func $none (result $v)
    (canon lift (core func $i "none") (memory (core memory $i "memory")) (realloc (core func $i "realloc"))))
  (export "some" (func $some) (func (result (option u32))))
  (export "none" (func $none) (func (result (option u32))))
)
"#
}

#[tokio::test]
async fn typed_option_binding_honors_export_ascription() {
    let (store, instance) =
        instantiate_component(option_ascribed_exports(), &ComponentLinker::new()).await;

    let some = instance
        .get_typed_func::<(), Option<u32>>("some")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        some.call(&store, ())
            .await
            .expect("typed call should succeed"),
        Some(42)
    );
    let none = instance
        .get_typed_func::<(), Option<u32>>("none")
        .expect("ascribed option type must bind as Option");
    assert_eq!(
        none.call(&store, ())
            .await
            .expect("typed call should succeed"),
        None
    );

    // Dynamic calls through the exports produce Option values too.
    let dynamic = instance
        .call(&store, "some", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(
        dynamic,
        vec![ComponentValue::Option(Some(Box::new(ComponentValue::U32(
            42
        ))))]
    );
    let dynamic = instance
        .call(&store, "none", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Option(None)]);
}
