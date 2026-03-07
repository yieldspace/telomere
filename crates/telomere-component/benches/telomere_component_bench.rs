use std::{fs, hint::black_box};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
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
            let engine = telomere_component::ComponentEngine::new();
            for source in &sources {
                let program = engine.compile(black_box(source)).unwrap();
                black_box(program);
            }
        })
    });
    component_group.bench_function("very_nested_precompiled", |b| {
        let source = very_nested.clone();
        b.iter(|| {
            let engine = telomere_component::ComponentEngine::new();
            let program = engine.compile(black_box(&source)).unwrap();
            black_box(program);
        })
    });
    component_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
