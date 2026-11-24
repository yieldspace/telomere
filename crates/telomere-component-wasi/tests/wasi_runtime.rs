use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};
use telomere_component_wasi::{add_to_linker_async, add_to_linker_sync, WasiState};

fn compile_component_with_version(version: &str) -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $env
    (instance
      (export "get-environment" (func (result (list (tuple string string)))))
      (export "get-arguments" (func (result (list string))))
      (export "initial-cwd" (func (result (option string))))
    )
  )
  (import "wasi:cli/environment@0.2.6" (instance $environment (type $env)))
  (alias export $environment "get-environment" (func $get-environment))
  (alias export $environment "get-arguments" (func $get-arguments))
  (alias export $environment "initial-cwd" (func $initial-cwd))

  (type $random
    (instance
      (export "get-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/random@0.2.6" (instance $random-instance (type $random)))
  (alias export $random-instance "get-random-u64" (func $get-random-u64))

  (type $insecure-random
    (instance
      (export "get-insecure-random-u64" (func (result u64)))
    )
  )
  (import "wasi:random/insecure@0.2.6" (instance $insecure-random-instance (type $insecure-random)))
  (alias export $insecure-random-instance "get-insecure-random-u64" (func $get-insecure-random-u64))

  (export "get-environment" (func $get-environment))
  (export "get-arguments" (func $get-arguments))
  (export "initial-cwd" (func $initial-cwd))
  (export "get-random-u64" (func $get-random-u64))
  (export "get-insecure-random-u64" (func $get-insecure-random-u64))
)
"#
        .replace("@0.2.6", &format!("@{version}")),
    )
    .expect("component wat must parse")
}

fn compile_stdout_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $streams
    (instance
      (export "output-stream" (type (sub resource)))
    )
  )
  (import "wasi:io/streams@0.2.6" (instance $streams-instance (type $streams)))
  (alias export $streams-instance "output-stream" (type $output-stream))

  (type $stdout
    (instance
      (alias outer 1 $output-stream (type $stdout-output-stream))
      (export "output-stream" (type (eq $stdout-output-stream)))
      (export "get-stdout" (func (result (own $stdout-output-stream))))
    )
  )
  (import "wasi:cli/stdout@0.2.6" (instance $stdout-instance (type $stdout)))
  (alias export $stdout-instance "get-stdout" (func $get-stdout))

  (core func $get-stdout-lower (canon lower (func $get-stdout)))
  (core module $caller
    (import "" "get-stdout" (func $get-stdout (result i32)))
    (func (export "get-stdout") (result i32)
      (call $get-stdout)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "get-stdout" (func $get-stdout-lower))
      ))
    )
  )
  (func (export "get-stdout") (result (own $output-stream))
    (canon lift (core func $caller "get-stdout"))
  )
)
"#,
    )
    .expect("stdout component wat must parse")
}

fn build_state() -> WasiState {
    WasiState::builder()
        .args(["guest", "one"])
        .env([("FOO", "BAR"), ("BAZ", "QUX")])
        .preopen_dir(".", "sandbox")
        .random_seed(0x1234_5678_u64)
        .build()
}

fn expected_random(seed: u64) -> u64 {
    let mut value = seed.wrapping_add(0x9e3779b97f4a7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    value ^ (value >> 31)
}

fn assert_snapshot(
    secure_random: &ComponentValue,
    insecure_random: &ComponentValue,
    args: &[ComponentValue],
    env: &[ComponentValue],
    cwd: &ComponentValue,
) {
    assert!(matches!(secure_random, ComponentValue::U64(_)));
    assert_eq!(
        insecure_random,
        &ComponentValue::U64(expected_random(0x1234_5678_u64))
    );
    assert_eq!(
        args,
        &[
            ComponentValue::String("guest".to_owned()),
            ComponentValue::String("one".to_owned())
        ]
    );
    assert_eq!(env.len(), 2);
    assert!(env.iter().any(|entry| {
        matches!(
            entry,
            ComponentValue::Tuple(values)
                if values
                    == &[
                        ComponentValue::String("BAZ".to_owned()),
                        ComponentValue::String("QUX".to_owned()),
                    ]
        )
    }));
    assert!(env.iter().any(|entry| {
        matches!(
            entry,
            ComponentValue::Tuple(values)
                if values
                    == &[
                        ComponentValue::String("FOO".to_owned()),
                        ComponentValue::String("BAR".to_owned()),
                    ]
        )
    }));
    assert_eq!(
        cwd,
        &ComponentValue::Option(Some(Box::new(ComponentValue::String("sandbox".to_owned()))))
    );
}

async fn run_runtime(
    add: fn(&mut ComponentLinker, WasiState) -> Result<(), telomere_component::ComponentError>,
) {
    run_runtime_with_version(add, "0.2.6").await;
}

async fn run_runtime_with_version(
    add: fn(&mut ComponentLinker, WasiState) -> Result<(), telomere_component::ComponentError>,
    version: &str,
) {
    let bytes = compile_component_with_version(version);
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    add(&mut linker, build_state()).expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let random = instance
        .call(&store, "get-random-u64", &[])
        .await
        .expect("random call should succeed");
    let insecure_random = instance
        .call(&store, "get-insecure-random-u64", &[])
        .await
        .expect("insecure random call should succeed");
    let args = instance
        .call(&store, "get-arguments", &[])
        .await
        .expect("arguments call should succeed");
    let env = instance
        .call(&store, "get-environment", &[])
        .await
        .expect("environment call should succeed");
    let cwd = instance
        .call(&store, "initial-cwd", &[])
        .await
        .expect("cwd call should succeed");

    assert_snapshot(
        &random[0],
        &insecure_random[0],
        &match &args[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &match &env[0] {
            ComponentValue::List(values) => values.clone(),
            other => panic!("expected list, got {other:?}"),
        },
        &cwd[0],
    );
}

#[tokio::test]
async fn wasi_linker_sync_supports_environment_and_random() {
    run_runtime(add_to_linker_sync).await;
}

#[tokio::test]
async fn wasi_linker_async_supports_environment_and_random() {
    run_runtime(add_to_linker_async).await;
}

#[tokio::test]
async fn wasi_linker_sync_resolves_older_semver_imports() {
    // The provider registers its interfaces at 0.2.6; a guest built
    // against 0.2.0 (the version stock Rust wasm32-wasip2 components
    // name) imports the same interfaces at 0.2.0 and must still link
    // (#258).
    run_runtime_with_version(add_to_linker_sync, "0.2.0").await;
}

#[tokio::test]
async fn wasi_linker_sync_lowers_get_stdout_owned_resource() {
    let bytes = compile_stdout_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");
    let mut linker = ComponentLinker::new();
    add_to_linker_sync(&mut linker, build_state())
        .expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    let result = instance
        .call(&store, "get-stdout", &[])
        .await
        .expect("get-stdout should succeed");

    assert!(matches!(result.as_slice(), [ComponentValue::Own(_)]));
}

fn compile_stdout_drop_component() -> Vec<u8> {
    compile_stdout_drop_component_with_version("0.2.6")
}

fn compile_stdout_drop_component_with_version(version: &str) -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $ty-wasi:io/error@0.2.6
    (instance
      (export "error" (type (sub resource)))
    )
  )
  (import "wasi:io/error@0.2.6" (instance $wasi:io/error@0.2.6 (type $ty-wasi:io/error@0.2.6)))
  (alias export $wasi:io/error@0.2.6 "error" (type $error))

  (type $ty-wasi:io/streams@0.2.6
    (instance
      (export "output-stream" (type (sub resource)))
      (alias outer 1 $error (type $streams-error))
      (export "error" (type (eq $streams-error)))
      (type $own-error (own $streams-error))
      (type $stream-error
        (variant
          (case "last-operation-failed" $own-error)
          (case "closed")
        )
      )
      (export "stream-error" (type (eq $stream-error)))
      (type $borrow-stream (borrow 0))
      (type $bytes (list u8))
      (type $write-result (result (error $stream-error)))
      (export "[method]output-stream.blocking-write-and-flush"
        (func
          (param "self" $borrow-stream)
          (param "contents" $bytes)
          (result $write-result)
        )
      )
    )
  )
  (import "wasi:io/streams@0.2.6" (instance $wasi:io/streams@0.2.6 (type $ty-wasi:io/streams@0.2.6)))
  (alias export $wasi:io/streams@0.2.6 "output-stream" (type $output-stream))
  (alias export $wasi:io/streams@0.2.6 "[method]output-stream.blocking-write-and-flush" (func $write))

  (type $ty-wasi:cli/stdout@0.2.6
    (instance
      (alias outer 1 $output-stream (type $stdout-output-stream))
      (export "output-stream" (type (eq $stdout-output-stream)))
      (export "get-stdout" (func (result (own $stdout-output-stream))))
    )
  )
  (import "wasi:cli/stdout@0.2.6" (instance $wasi:cli/stdout@0.2.6 (type $ty-wasi:cli/stdout@0.2.6)))
  (alias export $wasi:cli/stdout@0.2.6 "get-stdout" (func $get-stdout))

  (core module $memmod
    (memory (export "mem") 1)
    (global $bump (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $p i32)
      (local.set $p (global.get $bump))
      (global.set $bump (i32.add (local.get $p) (local.get 3)))
      (local.get $p)
    )
    (data (i32.const 0) "hello from a dropped stream\n")
  )
  (core instance $meminst (instantiate $memmod))
  (alias core export $meminst "mem" (core memory $mem))
  (alias core export $meminst "realloc" (core func $realloc))

  (core func $get-lower (canon lower (func $get-stdout)))
  (core func $write-lower (canon lower (func $write) (memory $mem) (realloc $realloc)))
  (core func $drop (canon resource.drop $output-stream))

  (core module $caller
    (import "" "get" (func $get (result i32)))
    (import "" "write" (func $write (param i32 i32 i32 i32)))
    (import "" "drop" (func $drop (param i32)))
    (func (export "write-and-drop") (result i32)
      (local $h i32)
      (local.set $h (call $get))
      (call $write (local.get $h) (i32.const 0) (i32.const 28) (i32.const 1024))
      (call $drop (local.get $h))
      (i32.const 0)
    )
    (func (export "write-after-drop") (result i32)
      (local $h i32)
      (local.set $h (call $get))
      (call $drop (local.get $h))
      (call $write (local.get $h) (i32.const 0) (i32.const 28) (i32.const 1024))
      (i32.const 0)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "get" (func $get-lower))
        (export "write" (func $write-lower))
        (export "drop" (func $drop))
      ))
    )
  )
  (func (export "write-and-drop") (result s32)
    (canon lift (core func $caller "write-and-drop"))
  )
  (func (export "write-after-drop") (result s32)
    (canon lift (core func $caller "write-after-drop"))
  )
)
"#.replace("@0.2.6", &format!("@{version}")),
    )
    .expect("stdout drop component wat must parse")
}

#[tokio::test]
async fn wasi_stdout_handle_can_be_dropped_and_is_released() {
    run_stdout_drop(compile_stdout_drop_component()).await;
}

#[tokio::test]
async fn wasi_stdout_handle_drop_resolves_older_semver_imports() {
    // The same guest importing at 0.2.0 instead of the provider's 0.2.6:
    // both instance resolution and the resource-drop registration must
    // follow the semver compatible track (#258).
    run_stdout_drop(compile_stdout_drop_component_with_version("0.2.0")).await;
}

async fn run_stdout_drop(bytes: Vec<u8>) {
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let state = WasiState::builder().build();
    let mut linker = ComponentLinker::new();
    add_to_linker_sync(&mut linker, state.clone())
        .expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let result = instance
        .call(&store, "write-and-drop", &[])
        .await
        .expect("writing to stdout and dropping its handle should succeed");
    assert_eq!(result, vec![ComponentValue::S32(0)]);
    assert_eq!(state.stdout(), b"hello from a dropped stream\n");

    // The drop must actually release the host-side stream: writing through
    // the same handle afterwards fails in the provider because the entry is
    // gone. A no-op drop would let this call succeed.
    let error = instance
        .call(&store, "write-after-drop", &[])
        .await
        .expect_err("writing through a dropped stdout handle should fail");
    assert!(
        matches!(error, telomere_component::ComponentError::GuestTrap { kind: telomere::TrapKind::Unreachable, ref message } if message.contains("unreachable")),
        "write after drop should trap, got {error:?}"
    );
    assert_eq!(state.stdout(), b"hello from a dropped stream\n");
}

fn compile_sockets_component() -> Vec<u8> {
    wat::parse_str(
        r#"
(component
  (type $instance-network
    (instance
      (export "network" (type $network (sub resource)))
      (export "instance-network" (func (result (own $network))))
    )
  )
  (import "wasi:sockets/instance-network@0.2.6" (instance $instance-network-instance (type $instance-network)))
  (alias export $instance-network-instance "network" (type $network))
  (alias export $instance-network-instance "instance-network" (func $instance-network))

  (core func $instance-network-lower (canon lower (func $instance-network)))
  (core module $caller
    (import "" "instance-network" (func $instance-network (result i32)))
    (func (export "call") (result i32)
      (call $instance-network)
    )
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance
        (export "instance-network" (func $instance-network-lower))
      ))
    )
  )
  (func (export "call") (result (own $network))
    (canon lift (core func $caller "call"))
  )
)
"#,
    )
    .expect("sockets component wat must parse")
}

async fn run_sockets_link_failure(
    add: fn(&mut ComponentLinker, WasiState) -> Result<(), telomere_component::ComponentError>,
) {
    // The default provider world excludes wasi:sockets (docs/core/sockets-scope.md,
    // #135): a component importing a sockets interface must fail when the
    // runtime resolves its imports during instantiation, with an error naming
    // the unresolved import, not link successfully and trap on first use.
    let bytes = compile_sockets_component();
    let engine = ComponentEngine::new();
    let program = engine.compile(&bytes).expect("compile should succeed");

    let mut linker = ComponentLinker::new();
    add(&mut linker, build_state()).expect("wasi linker registration should succeed");

    let store = telomere::Store::new();
    let error = match engine.instantiate(&program, &store, &linker).await {
        Ok(_) => panic!("sockets-importing component must not instantiate"),
        Err(error) => error,
    };
    assert!(
        matches!(error, telomere_component::ComponentError::Unsupported(ref message) if message.contains("wasi:sockets/instance-network@0.2.6")),
        "expected an unsupported-import error naming the unresolved sockets import, got {error:?}"
    );
}

#[tokio::test]
async fn wasi_linker_sync_rejects_sockets_imports() {
    run_sockets_link_failure(add_to_linker_sync).await;
}

#[tokio::test]
async fn wasi_linker_async_rejects_sockets_imports() {
    run_sockets_link_failure(add_to_linker_async).await;
}
