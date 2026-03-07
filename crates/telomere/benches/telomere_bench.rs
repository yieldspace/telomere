use std::fs;
use std::hint::black_box;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use telomere::{ResultValue, WasmValue};
use tokio::runtime::Runtime;

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
                    let mut store = telomere::Store::new();
                    let registry = telomere::Registry::new();
                    let handle = telomere::instantiate(module.clone(), &mut store, &registry)
                        .await
                        .unwrap();
                    let start = Instant::now();
                    assert_eq!(
                        black_box(
                            telomere::run_module_function(
                                &handle,
                                &mut store,
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
    group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
