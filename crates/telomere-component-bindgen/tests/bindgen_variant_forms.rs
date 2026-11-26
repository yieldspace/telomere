//! Regression tests for the PR #374 Codex review, round 3: the
//! `matches_type` generated for bindgen `variant` / `enum` types must
//! require the declared constructor to match, not just the case shape.
//!
//! Since #314, a type declared as `variant { ready, done }` is the same
//! component type as `enum { ready, done }`, but its values lift as
//! `ComponentValue::Variant`, not `ComponentValue::Enum` (and vice
//! versa). The generated checks only compared case labels and payload
//! absence, so a generated enum binding validated against a declared
//! variant (and a generated variant binding against a declared enum),
//! produced a wrapper that failed when invoked with
//! `InvalidArgument("expected enum result, got Variant ...")` /
//! `InvalidArgument("expected variant result, got Enum ...")`. The
//! generated checks now compare `VariantType::form` as well, mirroring
//! the `option` / `result` extractors fixed in round 1.
//!
//! Round 2's export-ascription handling composes with this: typed
//! validation reads the export's recorded (ascribed) type, and calls
//! through the export convert per that same type, so a binding whose
//! constructor matches the *ascribed* type is accepted and works, in
//! both directions. The last two tests pin that composition.

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

telomere_component_bindgen::bindgen!({
    inline: r#"
        package ex:forms;

        world forms {
            enum status {
                ready,
                done,
            }

            variant status-v {
                ready,
                done,
            }

            variant shape {
                round,
                square,
            }

            export get-status: func() -> status;
            export get-status-v: func() -> status-v;
            export get-shape: func() -> shape;
        }
    "#,
    world: "forms",
    module: "bindings"
});

/// A component exporting `get-{type_export}` returning the type declared
/// with the given constructor. The core function returns discriminant 1
/// (the second case).
fn returning_component(constructor: &str, type_export: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"
(component
  (type $t {constructor})
  (export $t' "{type_export}" (type $t))
  (core module $m
    (func (export "f") (result i32) i32.const 1))
  (core instance $i (instantiate $m))
  (func (export "get-{type_export}") (result $t')
    (canon lift (core func $i "f")))
)
"#
    ))
    .expect("component wat must parse")
}

/// A component whose function returns values of the `own` type while the
/// export ascribes the `ascribed` type.
fn ascribed_component(own: &str, ascribed: &str) -> Vec<u8> {
    wat::parse_str(format!(
        r#"
(component
  (type $own {own})
  (type $asc {ascribed})
  (export $asc' "status" (type $asc))
  (core module $m
    (func (export "f") (result i32) i32.const 1))
  (core instance $i (instantiate $m))
  (func $f (result $own)
    (canon lift (core func $i "f")))
  (export "get-status" (func $f) (func (result $asc')))
)
"#
    ))
    .expect("component wat must parse")
}

async fn instantiate(bytes: &[u8]) -> (telomere::Store, telomere_component::ComponentInstance) {
    let engine = ComponentEngine::new();
    let program = engine.compile(bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &ComponentLinker::new())
        .await
        .expect("instantiate should succeed");
    (store, instance)
}

#[tokio::test]
async fn generated_enum_binding_rejects_declared_variant() {
    let bytes = returning_component(r#"(variant (case "ready") (case "done"))"#, "status");
    let (store, instance) = instantiate(&bytes).await;

    let error = instance
        .get_typed_func::<(), bindings::Status>("get-status")
        .err()
        .expect("declared variant must not bind as a generated enum");
    assert!(
        matches!(&error, ComponentError::Link(message) if message.contains("expects enum")),
        "expected a link error naming the enum expectation, got {error:?}"
    );

    // The dynamic API is unaffected: the value is a plain Variant.
    let dynamic = instance
        .call(&store, "get-status", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(
        dynamic,
        vec![ComponentValue::Variant {
            case: "done".to_owned(),
            value: None,
        }]
    );
}

#[tokio::test]
async fn generated_enum_binding_accepts_real_enum() {
    let bytes = returning_component(r#"(enum "ready" "done")"#, "status");
    let (store, instance) = instantiate(&bytes).await;
    let func = instance
        .get_typed_func::<(), bindings::Status>("get-status")
        .expect("real enum type must bind as a generated enum");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        bindings::Status::Done
    );
}

#[tokio::test]
async fn generated_enum_param_rejects_declared_variant() {
    let component = r#"
(component
  (type $t (variant (case "ready") (case "done")))
  (export $t' "status" (type $t))
  (core module $m
    (func (export "echo") (param i32) (result i32) (local.get 0)))
  (core instance $i (instantiate $m))
  (func (export "echo") (param "value" $t') (result u32)
    (canon lift (core func $i "echo")))
)
"#;
    let bytes = wat::parse_str(component).expect("component wat must parse");
    let (_store, instance) = instantiate(&bytes).await;
    let error = instance
        .get_typed_func::<(bindings::Status,), u32>("echo")
        .err()
        .expect("declared variant parameter must not bind as a generated enum");
    assert!(
        matches!(&error, ComponentError::Link(message) if message.contains("expects enum")),
        "expected a link error naming the enum expectation, got {error:?}"
    );
}

#[tokio::test]
async fn generated_variant_binding_rejects_real_enum() {
    let bytes = returning_component(r#"(enum "round" "square")"#, "shape");
    let (store, instance) = instantiate(&bytes).await;

    let error = instance
        .get_typed_func::<(), bindings::Shape>("get-shape")
        .err()
        .expect("declared enum must not bind as a generated variant");
    assert!(
        matches!(&error, ComponentError::Link(message) if message.contains("expects variant")),
        "expected a link error naming the variant expectation, got {error:?}"
    );

    // The dynamic API is unaffected: the value is an Enum.
    let dynamic = instance
        .call(&store, "get-shape", &[])
        .await
        .expect("dynamic call should succeed");
    assert_eq!(dynamic, vec![ComponentValue::Enum("square".to_owned())]);
}

#[tokio::test]
async fn generated_variant_binding_accepts_declared_variant() {
    let bytes = returning_component(r#"(variant (case "round") (case "square"))"#, "shape");
    let (store, instance) = instantiate(&bytes).await;
    let func = instance
        .get_typed_func::<(), bindings::Shape>("get-shape")
        .expect("declared variant must bind as a generated variant");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        bindings::Shape::Square
    );
}

#[tokio::test]
async fn generated_enum_binding_honors_enum_ascription() {
    // The definition returns a declared variant; the export ascribes the
    // enum it desugars to. The public type is the enum, so the generated
    // enum binding is accepted and calls lift per the export type
    // (round 2), while the variant binding is rejected at lookup.
    let bytes = ascribed_component(
        r#"(variant (case "ready") (case "done"))"#,
        r#"(enum "ready" "done")"#,
    );
    let (store, instance) = instantiate(&bytes).await;

    let func = instance
        .get_typed_func::<(), bindings::Status>("get-status")
        .expect("enum-ascribed export must bind as a generated enum");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        bindings::Status::Done
    );
    assert!(
        instance
            .get_typed_func::<(), bindings::StatusV>("get-status")
            .is_err(),
        "variant binding must not bind against an enum-ascribed export"
    );
}

#[tokio::test]
async fn generated_variant_binding_honors_variant_ascription() {
    // The definition returns a real enum; the export ascribes the variant
    // it desugars to. The public type is the variant, so the generated
    // variant binding is accepted and calls lift per the export type,
    // while the enum binding is rejected at lookup.
    let bytes = ascribed_component(
        r#"(enum "ready" "done")"#,
        r#"(variant (case "ready") (case "done"))"#,
    );
    let (store, instance) = instantiate(&bytes).await;

    let func = instance
        .get_typed_func::<(), bindings::StatusV>("get-status")
        .expect("variant-ascribed export must bind as a generated variant");
    assert_eq!(
        func.call(&store, ())
            .await
            .expect("typed call should succeed"),
        bindings::StatusV::Done
    );
    assert!(
        instance
            .get_typed_func::<(), bindings::Status>("get-status")
            .is_err(),
        "enum binding must not bind against a variant-ascribed export"
    );
}
