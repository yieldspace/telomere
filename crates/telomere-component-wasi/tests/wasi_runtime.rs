use telomere_component::{ComponentEngine, ComponentLinker, ComponentValue};
use telomere_component_wasi::{add_to_linker_async, add_to_linker_sync, WasiState};

fn compile_component() -> Vec<u8> {
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
"#,
    )
    .expect("component wat must parse")
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
    let bytes = compile_component();
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
