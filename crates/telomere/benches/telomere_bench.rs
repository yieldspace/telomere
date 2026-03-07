use std::{
    fs,
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use telomere::{ResultValue, WasmValue};
use tokio::runtime::Runtime;
use wast::{QuoteWat, Wast, WastDirective, Wat};

fn collect_component_sources(path: &std::path::Path) -> Vec<Vec<u8>> {
    let text = fs::read_to_string(path).unwrap();
    let buffer = wast::parser::ParseBuffer::new(&text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buffer).unwrap();
    let mut sources = Vec::new();
    for directive in wast.directives {
        match directive {
            WastDirective::Module(mut module) | WastDirective::ModuleDefinition(mut module)
                if is_component_quote(&module) =>
            {
                sources.push(module.encode().unwrap());
            }
            _ => {}
        }
    }
    assert!(
        !sources.is_empty(),
        "expected at least one component source in {}",
        path.display()
    );
    sources
}

fn is_component_quote(module: &QuoteWat<'_>) -> bool {
    matches!(
        module,
        QuoteWat::Wat(Wat::Component(_)) | QuoteWat::QuoteComponent(_, _)
    )
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

    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let upstream_strings = collect_component_sources(
        &manifest_dir.join(
            "tests/component_model_upstream/c7176a512c0bbe4654849f4ba221c1a71c7cf514/values/strings.wast",
        ),
    );
    let very_nested = fs::read(
        manifest_dir.join(
            "tests/component_model_upstream/precompiled/c7176a512c0bbe4654849f4ba221c1a71c7cf514/wasm-tools/very-nested.0.wasm",
        ),
    )
    .unwrap();
    let mut component_group = c.benchmark_group("component_compile");
    component_group.sampling_mode(SamplingMode::Flat);
    component_group.bench_function("upstream_strings", |b| {
        let sources = upstream_strings.clone();
        b.iter(|| {
            let engine = telomere::component::ComponentEngine::new();
            for source in &sources {
                let program = engine.compile(black_box(source)).unwrap();
                black_box(program);
            }
        })
    });
    component_group.bench_function("very_nested_precompiled", |b| {
        let source = very_nested.clone();
        b.iter(|| {
            let engine = telomere::component::ComponentEngine::new();
            let program = engine.compile(black_box(&source)).unwrap();
            black_box(program);
        })
    });
    component_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
