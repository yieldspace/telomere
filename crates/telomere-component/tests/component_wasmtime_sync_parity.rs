mod common;

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component wat must be valid")
}

fn host_roundtrip_component(
    type_decls: &str,
    func_sig: &str,
    host_sig: &str,
    run_sig: &str,
    run_body: &str,
) -> String {
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
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
  )
  (core instance $libc (instantiate $libc))
  (core func $host-lower
    (canon lower (func $host) (memory $libc "memory") (realloc (func $libc "realloc")))
  )
  (core module $forward
    (import "" "host" (func $host {host_sig}))
    (import "" "realloc" (func $realloc (param i32 i32 i32 i32) (result i32)))
    (func (export "run") {run_sig}
      {run_body}
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
    (canon lift (core func $forward "run") (memory $libc "memory") (realloc (func $libc "realloc")))
  )
)
"#,
        type_decls = type_decls,
        func_sig = func_sig,
        host_sig = host_sig,
        run_sig = run_sig,
        run_body = run_body,
    )
}

fn local_gets(count: usize) -> String {
    (0..count)
        .map(|index| format!("(local.get {index})"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn indirect_forward_body(param_count: usize, result_size: u32) -> String {
    let args = local_gets(param_count);
    let args = if args.is_empty() {
        String::new()
    } else {
        format!(" {args}")
    };
    format!(
        r#"
      (local $ret i32)
      (local.set $ret (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const {result_size})))
      (call $host{args} (local.get $ret))
      (local.get $ret)
"#,
        args = args,
        result_size = result_size,
    )
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

async fn assert_component_echo(text: &str, input: ComponentValue) {
    let (store, instance) = instantiate_component(text, &ComponentLinker::new()).await;
    let result = instance
        .call(&store, "run", std::slice::from_ref(&input))
        .await
        .expect("self echo call should succeed");
    assert_eq!(result, vec![input]);
}

fn large_record_type_defs(field_count: usize) -> String {
    let fields = (0..field_count)
        .map(|index| format!("(field \"f-{index}\" u32)"))
        .collect::<Vec<_>>()
        .join(" ");
    format!("(type $value (record {fields}))\n  (export \"value\" (type $value))")
}

fn large_record_value(field_count: usize) -> ComponentValue {
    ComponentValue::Record(
        (0..field_count)
            .map(|index| (format!("f-{index}"), ComponentValue::U32(index as u32 + 1)))
            .collect(),
    )
}

#[tokio::test]
async fn component_dynamic_values_roundtrip_matches_wasmtime_supported_sync_values() {
    let list = host_roundtrip_component(
        "",
        r#"(param "value" (list u32)) (result (list u32))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 32),
    );
    assert_dynamic_roundtrip(
        &list,
        ComponentValue::List(vec![
            ComponentValue::U32(32343),
            ComponentValue::U32(79_023_439),
            ComponentValue::U32(2_084_037_802),
        ]),
    )
    .await;

    let record = r#"
(component
  (type $inner (record (field "d" bool) (field "e" u32)))
  (export "inner" (type $inner))
  (type $value (record (field "a" u32) (field "b" float64) (field "c" $inner)))
  (export "value" (type $value))
  (core module $m
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
    (func (export "run") (param i32 f64 i32 i32) (result i32)
      (local $ret i32)
      (local.set $ret (call 0 (i32.const 0) (i32.const 0) (i32.const 8) (i32.const 24)))
      (i32.store (local.get $ret) (local.get 0))
      (f64.store (i32.add (local.get $ret) (i32.const 8)) (local.get 1))
      (i32.store8 (i32.add (local.get $ret) (i32.const 16)) (local.get 2))
      (i32.store (i32.add (local.get $ret) (i32.const 20)) (local.get 3))
      (local.get $ret)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run") (memory $i "memory") (realloc (func $i "realloc")))
  )
)
"#;
    assert_component_echo(
        record,
        ComponentValue::Record(vec![
            ("a".to_owned(), ComponentValue::U32(32343)),
            ("b".to_owned(), ComponentValue::F64(std::f64::consts::PI)),
            (
                "c".to_owned(),
                ComponentValue::Record(vec![
                    ("d".to_owned(), ComponentValue::Bool(false)),
                    ("e".to_owned(), ComponentValue::U32(314159265)),
                ]),
            ),
        ]),
    )
    .await;

    let tuple = host_roundtrip_component(
        "",
        r#"(param "value" (tuple u32 u32)) (result (tuple u32 u32))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    assert_dynamic_roundtrip(
        &tuple,
        ComponentValue::Tuple(vec![ComponentValue::U32(42), ComponentValue::U32(24)]),
    )
    .await;

    let variant = r#"
(component
  (type $inner (record (field "d" bool) (field "e" u32)))
  (export "inner" (type $inner))
  (type $value (variant (case "a" u32) (case "b" float64) (case "c" $inner)))
  (export "value" (type $value))
  (core module $m
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
    (func (export "run") (param i32 i64 i32) (result i32)
      (local $ret i32)
      (local.set $ret (call 0 (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 16)))
      (i32.store8 (local.get $ret) (local.get 0))
      (i32.store8
        (i32.add (local.get $ret) (i32.const 8))
        (i32.wrap_i64 (local.get 1))
      )
      (i32.store (i32.add (local.get $ret) (i32.const 12)) (local.get 2))
      (local.get $ret)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run") (memory $i "memory") (realloc (func $i "realloc")))
  )
)
"#;
    assert_component_echo(
        variant,
        ComponentValue::Variant {
            case: "c".to_owned(),
            value: Some(Box::new(ComponentValue::Record(vec![
                ("d".to_owned(), ComponentValue::Bool(true)),
                ("e".to_owned(), ComponentValue::U32(271828182)),
            ]))),
        },
    )
    .await;

    let enum_component = r#"
(component
  (type $value (enum "a" "b"))
  (export "value" (type $value))
  (core module $m
    (func (export "run") (param i32) (result i32)
      (local.get 0))
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run"))
  )
)
"#;
    assert_component_echo(enum_component, ComponentValue::Enum("b".to_owned())).await;

    let flags = r#"
(component
  (type $value (flags "a" "b" "c" "d" "e"))
  (export "value" (type $value))
  (core module $m
    (func (export "run") (param i32) (result i32)
      (local.get 0))
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run"))
  )
)
"#;
    assert_component_echo(
        flags,
        ComponentValue::Flags(vec!["b".to_owned(), "d".to_owned()]),
    )
    .await;

    let option = host_roundtrip_component(
        "",
        r#"(param "value" (option u32)) (result (option u32))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    assert_dynamic_roundtrip(
        &option,
        ComponentValue::Option(Some(Box::new(ComponentValue::U32(314159265)))),
    )
    .await;

    let result_component = host_roundtrip_component(
        "",
        r#"(param "value" (result string (error string))) (result (result string (error string)))"#,
        r#"(param i32 i32 i32 i32)"#,
        r#"(param i32 i32 i32) (result i32)"#,
        &indirect_forward_body(3, 32),
    );
    assert_dynamic_roundtrip(
        &result_component,
        ComponentValue::Result {
            ok: None,
            err: Some(Box::new(ComponentValue::String("nope".to_owned()))),
        },
    )
    .await;
}

#[tokio::test]
async fn component_dynamic_values_support_indirect_parameter_areas() {
    let type_defs = large_record_type_defs(17);
    let component = format!(
        r#"
(component
  {type_defs}
  (core module $m
    (memory (export "memory") 1)
    (global $bump (mut i32) (i32.const 32))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32)
      (local.set $ptr (global.get $bump))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      (local.get $ptr)
    )
    (func (export "run") (param i32) (result i32)
      (local.get 0)
    )
  )
  (core instance $i (instantiate $m))
  (func (export "run") (param "value" $value) (result $value)
    (canon lift (core func $i "run") (memory $i "memory") (realloc (func $i "realloc")))
  )
)
"#,
        type_defs = type_defs,
    );
    assert_component_echo(&component, large_record_value(17)).await;
}

#[tokio::test]
async fn component_typed_funcs_roundtrip_supported_values() {
    let string_component = host_roundtrip_component(
        "",
        r#"(param "value" string) (result string)"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (text,): (String,)| Ok(text));
    let (store, instance) = instantiate_component(&string_component, &linker).await;
    let func = instance
        .get_func("run")
        .expect("dynamic func lookup should succeed")
        .typed::<(String,), String>()
        .expect("typed view should succeed");
    assert_eq!(
        func.call(&store, ("hello".to_owned(),))
            .await
            .expect("typed call should succeed"),
        "hello".to_owned()
    );

    let list_component = host_roundtrip_component(
        "",
        r#"(param "value" (list u8)) (result (list u8))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed_async("host", |_store, (bytes,): (Vec<u8>,)| {
        Box::pin(async move { Ok(bytes) })
    });
    let (store, instance) = instantiate_component(&list_component, &linker).await;
    let func = instance
        .get_typed_func::<(Vec<u8>,), Vec<u8>>("run")
        .expect("typed func lookup should succeed");
    assert_eq!(
        func.call(&store, (vec![1, 2, 3, 4],))
            .await
            .expect("typed list call should succeed"),
        vec![1, 2, 3, 4]
    );

    let tuple_component = host_roundtrip_component(
        "",
        r#"(param "value" (tuple u8 s8)) (result (tuple u8 s8))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (value,): ((u8, i8),)| Ok(value));
    let (store, instance) = instantiate_component(&tuple_component, &linker).await;
    let func = instance
        .get_typed_func::<((u8, i8),), (u8, i8)>("run")
        .expect("tuple typed func lookup should succeed");
    assert_eq!(
        func.call(&store, ((7, -3),))
            .await
            .expect("tuple typed call should succeed"),
        (7, -3)
    );

    let option_component = host_roundtrip_component(
        "",
        r#"(param "value" (option u32)) (result (option u32))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (value,): (Option<u32>,)| Ok(value));
    let (store, instance) = instantiate_component(&option_component, &linker).await;
    let func = instance
        .get_typed_func::<(Option<u32>,), Option<u32>>("run")
        .expect("option typed func lookup should succeed");
    assert_eq!(
        func.call(&store, (Some(123),))
            .await
            .expect("option typed call should succeed"),
        Some(123)
    );

    let result_component = host_roundtrip_component(
        "",
        r#"(param "value" (result string (error string))) (result (result string (error string)))"#,
        r#"(param i32 i32 i32 i32)"#,
        r#"(param i32 i32 i32) (result i32)"#,
        &indirect_forward_body(3, 32),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (value,): (Result<String, String>,)| {
        Ok(value)
    });
    let (store, instance) = instantiate_component(&result_component, &linker).await;
    let func = instance
        .get_typed_func::<(Result<String, String>,), Result<String, String>>("run")
        .expect("result typed func lookup should succeed");
    assert_eq!(
        func.call(&store, (Err("boom".to_owned()),))
            .await
            .expect("result typed call should succeed"),
        Err("boom".to_owned())
    );
}

#[tokio::test]
async fn component_typed_funcs_validate_signatures_like_wasmtime() {
    let component = host_roundtrip_component(
        "",
        r#"(param "value" (list u8)) (result (list u8))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (bytes,): (Vec<u8>,)| Ok(bytes));
    let (store, instance) = instantiate_component(&component, &linker).await;
    let func = instance
        .get_func("run")
        .expect("func lookup should succeed");
    assert!(func.typed::<(), Vec<u8>>().is_err());
    assert!(func.typed::<(Vec<u8>,), Vec<u8>>().is_ok());
    assert!(instance
        .get_typed_func::<(Vec<u16>,), Vec<u8>>("run")
        .is_err());
    assert!(instance
        .get_typed_func::<(Vec<u8>,), Vec<u8>>("run")
        .is_ok());

    let typed = instance
        .get_typed_func::<(Vec<u8>,), Vec<u8>>("run")
        .expect("typed func lookup should succeed");
    assert_eq!(
        typed
            .call(&store, (vec![9, 8, 7],))
            .await
            .expect("typed call should succeed"),
        vec![9, 8, 7]
    );
}

#[tokio::test]
async fn component_typed_integer_bindings_match_wasmtime_reinterpretation() {
    let bytes = compile_component(
        r#"
(component
  (core module $m
    (func (export "take-i32-100") (param i32)
      local.get 0
      i32.const 100
      i32.eq
      br_if 0
      unreachable)
    (func (export "retm1-i32") (result i32)
      i32.const -1)
    (func (export "retbig-i32") (result i32)
      i32.const 100000)
  )
  (core instance $i (instantiate $m))
  (func (export "take-u8") (param "value" u8)
    (canon lift (core func $i "take-i32-100")))
  (func (export "retm1-u8") (result u8)
    (canon lift (core func $i "retm1-i32")))
  (func (export "retbig-u8") (result u8)
    (canon lift (core func $i "retbig-i32")))
)
"#,
    );
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &ComponentLinker::new())
        .await
        .expect("instantiate should succeed");

    let take_u8 = instance
        .get_typed_func::<(u8,), ()>("take-u8")
        .expect("typed func lookup should succeed");
    take_u8
        .call(&store, (100,))
        .await
        .expect("100 should pass through");
    let error = take_u8
        .call(&store, (101,))
        .await
        .expect_err("101 should trap");
    assert!(matches!(error, ComponentError::Trap(message) if message.contains("unreachable")));

    let retm1 = instance
        .get_typed_func::<(), u8>("retm1-u8")
        .expect("typed func lookup should succeed");
    assert_eq!(
        retm1.call(&store, ()).await.expect("call should succeed"),
        0xff
    );

    let retbig = instance
        .get_typed_func::<(), u8>("retbig-u8")
        .expect("typed func lookup should succeed");
    assert_eq!(
        retbig.call(&store, ()).await.expect("call should succeed"),
        100000u32 as u8
    );
}

#[tokio::test]
async fn component_fixed_length_lists_roundtrip_and_reject_wrong_length() {
    let component = host_roundtrip_component(
        "",
        r#"(param "value" (list u8 3)) (result (list u8 3))"#,
        r#"(param i32 i32 i32)"#,
        r#"(param i32 i32) (result i32)"#,
        &indirect_forward_body(2, 16),
    );
    let mut linker = ComponentLinker::new();
    linker.register_import_typed("host", |_store, (bytes,): (Vec<u8>,)| Ok(bytes));
    let (store, instance) = instantiate_component(&component, &linker).await;
    let typed = instance
        .get_typed_func::<(Vec<u8>,), Vec<u8>>("run")
        .expect("typed func lookup should succeed");
    assert_eq!(
        typed
            .call(&store, (vec![1, 2, 3],))
            .await
            .expect("fixed-length list should roundtrip"),
        vec![1, 2, 3]
    );

    let error = typed
        .call(&store, (vec![1, 2],))
        .await
        .expect_err("wrong list length should fail");
    assert!(
        matches!(error, ComponentError::InvalidArgument(message) if message.contains("expected list length 3"))
    );
}

#[test]
fn component_nested_names_match_wasmtime_supported_import_forms() {
    let engine = ComponentEngine::new();
    let ok = compile_component(
        r#"
(component
  (import "a:b:c:d/e" (func))
  (import "a:b-c:d-e:f-g/h-i/j-k/l-m/n/o/p@1.0.0" (func))
  (import "unlocked-dep=<a:b:c:d/e/f/g@{>=1.2.3 <2.0.0}>" (func))
  (import "locked-dep=<a:b:c:d/e/f/g@1.2.3>,integrity=<sha256-a>" (func))
)
"#,
    );
    engine.compile(&ok).expect("nested names should compile");

    let bad = compile_component(
        r#"
(component
  (import "a:b:c:d/E" (func))
)
"#,
    );
    let error = engine
        .compile(&bad)
        .expect_err("invalid nested name should fail");
    assert!(
        matches!(error, ComponentError::Validation(message) if message.contains("Invalid import name") || message.contains("Invalid words"))
    );
}
