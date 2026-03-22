use std::fs;
use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use telomere::{
    common::{ExecuteContext, FuncType, HostFunctionDefinition, Instr, NativeModule, ValType},
    instantiate, IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmValue,
};
use tokio::runtime::Runtime;

fn parse_wat(wat: &str) -> telomere::Module {
    let source = wat::parse_str(wat).expect("wat must parse");
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module().expect("wat must validate")
}

async fn instantiate_wat(
    wat: &str,
    store: &Store,
    registry: &Registry,
) -> telomere::common::InstanceHandle {
    match instantiate(parse_wat(wat), store, registry).await {
        VMResult::Success(instance) => instance,
        other => panic!("wat module must instantiate: {other:?}"),
    }
}

fn bench_add_one(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let value = i32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), 0, 4)
            .try_into()
            .unwrap(),
    );
    let slot = ctx.return_slot();
    slot.write(&(value + 1).to_le_bytes());
    let (prev_local_ref, return_addr) =
        ctx.stack
            .function_return_in_place(&ctx.local_reference, 4, ctx.gc);
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("benches/telomere-benchmark.wasm");
    let file = fs::File::open(path).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(file);
    let mut parser = telomere::WasmParser::new(&mut reader);
    let module = parser.parse_module().unwrap();
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("criterion_benchmark");
    group.sampling_mode(SamplingMode::Flat);
    group.bench_function("fib", |b| {
        b.to_async(&rt).iter_custom(|iters| {
            let module = module.clone();
            async move {
                let module = module.clone();
                let mut duration = Duration::new(0, 0);
                for _ in 0..iters {
                    let store = telomere::Store::new();
                    let registry = telomere::Registry::new();
                    let handle = telomere::instantiate(module.clone(), &store, &registry)
                        .await
                        .unwrap();
                    let start = Instant::now();
                    assert_eq!(
                        black_box(
                            telomere::run_module_function(
                                &handle,
                                &store,
                                "run",
                                &telomere::ResultValue::new(vec![WasmValue::I32(20)]),
                            )
                            .await,
                        )
                        .unwrap(),
                        ResultValue::new(vec![WasmValue::I32(6765)])
                    );
                    duration += start.elapsed();
                }
                duration
            }
        })
    });

    group.bench_function("return_call_chain", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func $leaf (param i32) (result i32)
                    local.get 0
                    i32.const 1
                    i32.add)
                  (func $step3 (param i32) (result i32)
                    local.get 0
                    return_call $leaf)
                  (func $step2 (param i32) (result i32)
                    local.get 0
                    return_call $step3)
                  (func $step1 (param i32) (result i32)
                    local.get 0
                    return_call $step2)
                  (func (export "run") (param i32) (result i32)
                    local.get 0
                    return_call $step1))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(41)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(42)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("sync_host_roundtrip_i32", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let mut registry = Registry::new();
            let host = match telomere::runtime::instantiate_native_module(
                NativeModule {
                    functions: vec![HostFunctionDefinition {
                        name: Some("add-one".to_owned()),
                        signature: FuncType::new(vec![ValType::I32], vec![ValType::I32]),
                        fp: bench_add_one,
                    }],
                },
                &store,
                &registry,
            )
            .await
            {
                VMResult::Success(instance) => instance,
                other => panic!("host module must instantiate: {other:?}"),
            };
            registry.register("host", host);
            let handle = instantiate_wat(
                r#"
                (module
                  (import "host" "add-one" (func $add-one (param i32) (result i32)))
                  (func (export "run") (param i32) (result i32)
                    local.get 0
                    return_call $add-one))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(41)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(42)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("memory_load_store_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        i32.const 0
                        local.get $i
                        i32.store
                        i32.const 0
                        i32.load
                        local.get $acc
                        i32.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("local_addr_load_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i32)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        i32.load
                        local.get $acc
                        i32.add
                        local.set $acc
                        local.get $addr
                        i32.const 4
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_local_addr_load_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i32)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        i32.const 0
                        i32.add
                        i32.load
                        local.get $acc
                        i32.add
                        local.set $acc
                        local.get $addr
                        i32.const 4
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("local_local_store_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i32)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        local.get $i
                        i32.store
                        local.get $addr
                        i32.load
                        local.get $acc
                        i32.add
                        local.set $acc
                        local.get $addr
                        i32.const 4
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_local_local_store_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i32)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        i32.const 0
                        i32.add
                        local.get $i
                        i32.store
                        local.get $addr
                        i32.const 0
                        i32.add
                        i32.load
                        local.get $acc
                        i32.add
                        local.set $acc
                        local.get $addr
                        i32.const 4
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("bitpack_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i32.const 255
                        i32.and
                        local.set $acc
                        local.get $acc
                        i32.const 3
                        i32.shl
                        local.tee $acc
                        local.get $i
                        i32.const 1
                        i32.shr_u
                        i32.add
                        local.set $acc
                        local.get $acc
                        local.get $i
                        i32.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(3574)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_bitpack_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i32.const 255
                        i32.and
                        i32.const 0
                        i32.add
                        local.set $acc
                        local.get $acc
                        i32.const 3
                        i32.shl
                        i32.const 0
                        i32.add
                        local.tee $acc
                        local.get $i
                        i32.const 1
                        i32.shr_u
                        i32.add
                        i32.const 0
                        i32.add
                        local.set $acc
                        local.get $acc
                        local.get $i
                        i32.add
                        i32.const 0
                        i32.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        i32.const 0
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(3574)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("i64_local_addr_load_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i64)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        i64.load
                        local.get $acc
                        i64.add
                        local.set $acc
                        local.get $addr
                        i32.const 8
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_i64_local_addr_load_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i64)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $addr
                        i32.const 0
                        i32.add
                        i64.load
                        local.get $acc
                        i64.add
                        local.set $acc
                        local.get $addr
                        i32.const 8
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("i64_local_local_store_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i64)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i64)
                    (local $value i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i64.extend_i32_u
                        local.set $value
                        local.get $addr
                        local.get $value
                        i64.store
                        local.get $addr
                        i64.load
                        local.get $acc
                        i64.add
                        local.set $acc
                        local.get $addr
                        i32.const 8
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_i64_local_local_store_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (memory 1)
                  (func (export "run") (param $n i32) (result i64)
                    (local $addr i32)
                    (local $i i32)
                    (local $acc i64)
                    (local $value i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i64.extend_i32_u
                        local.set $value
                        local.get $addr
                        i32.const 0
                        i32.add
                        local.get $value
                        i64.store
                        local.get $addr
                        i32.const 0
                        i32.add
                        i64.load
                        local.get $acc
                        i64.add
                        local.set $acc
                        local.get $addr
                        i32.const 8
                        i32.add
                        local.set $addr
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("i64_scalar_mix_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i64) (result i64)
                    (local $i i64)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i64.ge_u
                        br_if $exit
                        local.get $acc
                        i64.const 5
                        i64.add
                        local.set $acc
                        local.get $acc
                        i64.const 1
                        i64.shl
                        local.tee $acc
                        local.get $i
                        i64.add
                        local.set $acc
                        local.get $i
                        i64.const 1
                        i64.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I64(32)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(47_244_640_213)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_i64_scalar_mix_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i64) (result i64)
                    (local $i i64)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i64.ge_u
                        br_if $exit
                        local.get $acc
                        i64.const 5
                        i64.add
                        i64.const 0
                        i64.add
                        local.set $acc
                        local.get $acc
                        i64.const 1
                        i64.shl
                        i64.const 0
                        i64.add
                        local.tee $acc
                        local.get $i
                        i64.add
                        i64.const 0
                        i64.add
                        local.set $acc
                        local.get $i
                        i64.const 1
                        i64.add
                        i64.const 0
                        i64.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I64(32)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(47_244_640_213)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("f64_scalar_mix_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result f64)
                    (local $i i32)
                    (local $acc f64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $acc
                        f64.const 1.5
                        f64.add
                        local.set $acc
                        local.get $acc
                        f64.const 2
                        f64.mul
                        local.tee $acc
                        f64.const 1
                        f64.div
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(20)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::F64(3_145_725.0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_f64_scalar_mix_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result f64)
                    (local $i i32)
                    (local $acc f64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $acc
                        f64.const 1.5
                        f64.add
                        f64.const 0
                        f64.add
                        local.set $acc
                        local.get $acc
                        f64.const 2
                        f64.mul
                        f64.const 0
                        f64.add
                        local.tee $acc
                        f64.const 1
                        f64.div
                        f64.const 0
                        f64.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(20)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::F64(3_145_725.0)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("compare_branch_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i64) (result i64)
                    (local $i i64)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i64.ge_u
                        br_if $exit
                        local.get $acc
                        local.get $i
                        i64.add
                        local.set $acc
                        local.get $i
                        i64.const 1
                        i64.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I64(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_compare_branch_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i64) (result i64)
                    (local $i i64)
                    (local $acc i64)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i64.ge_u
                        i32.const 0
                        i32.or
                        br_if $exit
                        local.get $acc
                        local.get $i
                        i64.add
                        local.set $acc
                        local.get $i
                        i64.const 1
                        i64.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I64(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I64(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("divrem_loop_nontrap", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    (local $tmp i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i32.const 7
                        i32.add
                        local.set $tmp
                        local.get $tmp
                        i32.const 3
                        i32.div_u
                        local.set $tmp
                        local.get $tmp
                        i32.const 5
                        i32.rem_u
                        local.set $tmp
                        local.get $acc
                        local.get $tmp
                        i32.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(2050)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("baseline_divrem_loop_nontrap", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = Store::new();
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param $n i32) (result i32)
                    (local $i i32)
                    (local $acc i32)
                    (local $tmp i32)
                    block $exit
                      loop $loop
                        local.get $i
                        local.get $n
                        i32.ge_u
                        br_if $exit
                        local.get $i
                        i32.const 7
                        i32.add
                        i32.const 0
                        i32.add
                        local.set $tmp
                        local.get $tmp
                        i32.const 3
                        i32.div_u
                        i32.const 0
                        i32.add
                        local.set $tmp
                        local.get $tmp
                        i32.const 5
                        i32.rem_u
                        i32.const 0
                        i32.add
                        local.set $tmp
                        local.get $acc
                        local.get $tmp
                        i32.add
                        local.set $acc
                        local.get $i
                        i32.const 1
                        i32.add
                        local.set $i
                        br $loop
                      end
                    end
                    local.get $acc))
                "#,
                &store,
                &registry,
            )
            .await;
            let mut duration = Duration::new(0, 0);
            for _ in 0..iters {
                let start = Instant::now();
                assert_eq!(
                    black_box(
                        telomere::run_module_function(
                            &handle,
                            &store,
                            "run",
                            &ResultValue::new(vec![WasmValue::I32(1024)]),
                        )
                        .await
                    )
                    .unwrap(),
                    ResultValue::new(vec![WasmValue::I32(2050)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
