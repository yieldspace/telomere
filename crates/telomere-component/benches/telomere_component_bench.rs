use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
};

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};
use wast::{QuoteWat, Wast, WastDirective, Wat};

fn collect_component_sources(path: &Path) -> Vec<Vec<u8>> {
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
    sources
}

fn collect_component_corpus(root: &Path) -> Vec<Vec<u8>> {
    let mut files = fs::read_dir(root)
        .unwrap()
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|ext| ext == "wast"))
        .collect::<Vec<_>>();
    files.sort();

    let mut sources = Vec::new();
    for path in files {
        sources.extend(collect_component_sources(&path));
    }

    assert!(
        !sources.is_empty(),
        "expected at least one component source in {}",
        root.display()
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
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let testsuite_corpus =
        collect_component_corpus(&manifest_dir.join("tests/component_model_testsuite"));

    let mut component_group = c.benchmark_group("component_compile");
    component_group.sampling_mode(SamplingMode::Flat);
    component_group.bench_function("local_testsuite_corpus", |b| {
        let sources = testsuite_corpus.clone();
        b.iter(|| {
            let engine = telomere_component::ComponentEngine::new();
            for source in &sources {
                let program = engine.compile(black_box(source)).unwrap();
                black_box(program);
            }
        })
    });
    component_group.finish();
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
