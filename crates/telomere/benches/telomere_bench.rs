use std::hint::black_box;
use std::time::{Duration, Instant};
use std::{env, fs};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use telomere::{
    component_support::common::{FuncType, ValType},
    host_abi::{
        instantiate_native_module, ExecuteContext, HostFunctionDefinition, Instr, NativeModule,
    },
    instantiate, InstanceHandle, IoReadBinaryReader, Registry, ResultValue, RuntimeConfig, Store,
    VMResult, WasmParser, WasmValue,
};
use tokio::runtime::Runtime;

const RELINK_NODE_COUNT: i32 = 4096;
const RELINK_TAIL: i32 = (RELINK_NODE_COUNT - 1) * 4;

#[derive(Clone, Copy)]
enum BenchmarkMetering {
    Disabled,
    Unlimited,
}

impl BenchmarkMetering {
    /// The default preserves the long-lived Criterion names used for baseline comparisons.
    /// Set `TELOMERE_BENCH_METERING=unlimited` to re-run all six existing workloads with
    /// `MeteringConfig { enabled: true, initial_fuel: None }`; `disabled` is accepted explicitly
    /// for scripted branch comparisons.
    fn from_environment() -> Self {
        match env::var("TELOMERE_BENCH_METERING") {
            Err(env::VarError::NotPresent) => Self::Disabled,
            Ok(value) => match value.as_str() {
                "disabled" => Self::Disabled,
                "unlimited" => Self::Unlimited,
                _ => panic!(
                    "TELOMERE_BENCH_METERING must be `disabled` or `unlimited`, got `{value}`"
                ),
            },
            Err(error) => panic!("could not read TELOMERE_BENCH_METERING: {error}"),
        }
    }

    fn criterion_group_name(self) -> &'static str {
        match self {
            Self::Disabled => "criterion_benchmark",
            Self::Unlimited => "criterion_benchmark_metering_unlimited",
        }
    }
}

// This is the exact loop shape accepted by
// `I32LoadStoreLocalBaseRelinkLoopSpec::emit` in the optimizer. It materializes
// `op_i32_load_store_local_base_relink_loop`, whose native inner loop checks
// `vm_checkpoint!(ctx)` once per linked-list node.
const FUSED_RELINK_LOOP_WAT: &str = r#"
    (module
      (memory 1)
      (func (export "init")
        (local $cursor i32)
        loop $again
          local.get $cursor
          local.get $cursor
          i32.const 4
          i32.add
          i32.store
          local.get $cursor
          i32.const 4
          i32.add
          local.tee $cursor
          i32.const 16384
          i32.lt_u
          br_if $again
        end
        i32.const 16380
        i32.const 0
        i32.store)
      (func (export "reverse") (param $cursor i32) (param $prev i32) (result i32)
        (local $current i32)
        loop $again
          local.get $cursor
          local.tee $current
          i32.load
          local.set $cursor
          local.get $current
          local.get $prev
          i32.store
          local.get $current
          local.set $prev
          local.get $cursor
          br_if $again
        end
        local.get $prev))
"#;

fn parse_wat(wat: &str) -> telomere::Module {
    let source = wat::parse_str(wat).expect("wat must parse");
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module().expect("wat must validate")
}

async fn instantiate_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
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

fn unlimited_metered_store() -> Store {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.metering.enabled = true;
    runtime_config.metering.initial_fuel = None;
    Store::new_with_runtime_config(runtime_config)
}

fn benchmark_store(metering: BenchmarkMetering) -> Store {
    match metering {
        BenchmarkMetering::Disabled => Store::new(),
        BenchmarkMetering::Unlimited => unlimited_metered_store(),
    }
}

async fn instantiate_fused_relink_workload(store: &Store) -> InstanceHandle {
    let registry = Registry::new();
    let handle = instantiate_wat(FUSED_RELINK_LOOP_WAT, store, &registry).await;
    assert_eq!(
        telomere::run_module_function(&handle, store, "init", &ResultValue::new(vec![]))
            .await
            .unwrap(),
        ResultValue::new(vec![])
    );
    handle
}

async fn run_fused_relink_roundtrip(handle: &InstanceHandle, store: &Store) {
    assert_eq!(
        black_box(
            telomere::run_module_function(
                handle,
                store,
                "reverse",
                &ResultValue::new(vec![WasmValue::I32(0), WasmValue::I32(0)]),
            )
            .await
        )
        .unwrap(),
        ResultValue::new(vec![WasmValue::I32(RELINK_TAIL)])
    );
    assert_eq!(
        black_box(
            telomere::run_module_function(
                handle,
                store,
                "reverse",
                &ResultValue::new(vec![WasmValue::I32(RELINK_TAIL), WasmValue::I32(0)]),
            )
            .await
        )
        .unwrap(),
        ResultValue::new(vec![WasmValue::I32(0)])
    );
}

async fn time_fused_relink_roundtrips(store: Store, iters: u64) -> Duration {
    let handle = instantiate_fused_relink_workload(&store).await;
    let mut duration = Duration::ZERO;
    for _ in 0..iters {
        let start = Instant::now();
        run_fused_relink_roundtrip(&handle, &store).await;
        duration += start.elapsed();
    }
    duration
}

pub fn criterion_benchmark(c: &mut Criterion) {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("benches/telomere-benchmark.wasm");
    let file = fs::File::open(path).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(file);
    let mut parser = telomere::WasmParser::new(&mut reader);
    let module = parser.parse_module().unwrap();
    let rt = Runtime::new().unwrap();
    let metering_mode = BenchmarkMetering::from_environment();
    let mut group = c.benchmark_group(metering_mode.criterion_group_name());
    group.sampling_mode(SamplingMode::Flat);
    group.bench_function("fib", |b| {
        b.to_async(&rt).iter_custom(|iters| {
            let module = module.clone();
            async move {
                let module = module.clone();
                let mut duration = Duration::new(0, 0);
                for _ in 0..iters {
                    let store = benchmark_store(metering_mode);
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
            let store = benchmark_store(metering_mode);
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

    group.bench_function("tail_recursive_accumulate", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = benchmark_store(metering_mode);
            let registry = Registry::new();
            let handle = instantiate_wat(
                r#"
                (module
                  (func (export "run") (param i32) (result i32)
                    local.get 0
                    i32.const 0
                    call 1)
                  (func (param $n i32) (param $acc i32) (result i32)
                    local.get $n
                    i32.eqz
                    if
                      local.get $acc
                      return
                    end
                    local.get $n
                    i32.const 1
                    i32.sub
                    local.get $acc
                    local.get $n
                    i32.add
                    return_call 1))
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
                    ResultValue::new(vec![WasmValue::I32(524800)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });

    group.bench_function("sync_host_roundtrip_i32", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = benchmark_store(metering_mode);
            let mut registry = Registry::new();
            let host = match instantiate_native_module(
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
            let store = benchmark_store(metering_mode);
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

    group.bench_function("scalar_local_loop", |b| {
        b.to_async(&rt).iter_custom(|iters| async move {
            let store = benchmark_store(metering_mode);
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
                    ResultValue::new(vec![WasmValue::I32(523776)])
                );
                duration += start.elapsed();
            }
            duration
        })
    });
    group.finish();

    let mut metering_group = c.benchmark_group("metering_overhead");
    metering_group.sampling_mode(SamplingMode::Flat);
    metering_group.bench_function("fused_relink_loop_disabled", |b| {
        b.to_async(&rt)
            .iter_custom(|iters| time_fused_relink_roundtrips(Store::new(), iters))
    });
    metering_group.bench_function("fused_relink_loop_unlimited", |b| {
        b.to_async(&rt)
            .iter_custom(|iters| time_fused_relink_roundtrips(unlimited_metered_store(), iters))
    });
    metering_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
