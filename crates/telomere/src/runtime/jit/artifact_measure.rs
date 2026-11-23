//! Measurement harness for the artifact-size side of issue #200.
//!
//! No AoT artifact container exists yet (that is issue #198, building on
//! the precompiled-module codec of #196), so there are no artifact bytes
//! to measure. What can be measured today is the payload an artifact would
//! carry: the native code the baseline emitter produces for every defined
//! function of a module, summed over the module. This test emits exactly
//! that for a defined corpus and prints the totals next to the modules'
//! original wasm bytes. It is a measurement, not a gate: it is
//! `#[ignore]`d and runs only when invoked explicitly with
//! `cargo test -p telomere --release --features jit --lib artifact_measure -- --ignored --nocapture`.
//!
//! Corpus: every module directive in the root-level `.wast` files of the
//! pinned `wasm-testsuite` submodule, plus the core module in
//! `examples/add.wasm`. Modules that fail to parse or to instantiate
//! against an empty registry (imports, malformed binaries, trapping start
//! functions) are counted and skipped, as are functions the emitter
//! rejects; the printed counts keep the ratio honest about its coverage.

use std::path::PathBuf;

use crate::{
    common::store::FunctionBody, instantiate, IoReadBinaryReader, Registry, Store, VMResult,
    WasmParser,
};

use super::backend;

#[derive(Default)]
struct Totals {
    modules_measured: usize,
    modules_parse_failed: usize,
    modules_instantiate_failed: usize,
    functions_emitted: usize,
    functions_rejected: usize,
    wasm_bytes: usize,
    emitted_bytes: usize,
}

fn suite_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/wasm-testsuite")
}

async fn measure_module(source: &[u8], totals: &mut Totals) {
    let mut reader = IoReadBinaryReader::from(source);
    let mut parser = WasmParser::new(&mut reader);
    let module = match parser.parse_module() {
        Ok(module) => module,
        Err(_) => {
            totals.modules_parse_failed += 1;
            return;
        }
    };
    let store = Store::new();
    let registry = Registry::new();
    let handle = match instantiate(module, &store, &registry).await {
        VMResult::Success(handle) => handle,
        _ => {
            totals.modules_instantiate_failed += 1;
            return;
        }
    };
    let runtime = store
        .lock_runtime("artifact_measure")
        .unwrap_or_else(|error| panic!("lock_runtime failed: {error:?}"));
    let funcs = runtime.get_instance(handle.object_ref).funcs.clone();
    let mut module_emitted = 0usize;
    let mut module_funcs = 0usize;
    for funcaddr in funcs {
        let func = runtime.get_func(funcaddr);
        let FunctionBody::Wasm { code, op_lens, .. } = &func.body else {
            continue;
        };
        match backend::emit_baseline_function(funcaddr, code, op_lens, &runtime) {
            Ok(bytes) => {
                module_emitted += bytes.len();
                module_funcs += 1;
            }
            Err(_) => {
                totals.functions_rejected += 1;
            }
        }
    }
    totals.modules_measured += 1;
    totals.functions_emitted += module_funcs;
    totals.wasm_bytes += source.len();
    totals.emitted_bytes += module_emitted;
}

#[tokio::test]
#[ignore = "measurement harness for #200; run with --ignored --nocapture"]
async fn measure_emitted_code_bytes_for_corpus() {
    let mut totals = Totals::default();

    let mut files: Vec<PathBuf> = std::fs::read_dir(suite_dir())
        .expect("wasm-testsuite submodule must be checked out")
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            (path.extension().is_some_and(|ext| ext == "wast")).then_some(path)
        })
        .collect();
    files.sort();
    assert!(!files.is_empty(), "corpus must not be empty");

    for file in &files {
        let text = std::fs::read_to_string(file).expect("wast file must be UTF-8");
        let mut lexer = wast::lexer::Lexer::new(&text);
        lexer.allow_confusing_unicode(true);
        let buf = wast::parser::ParseBuffer::new_with_lexer(lexer)
            .expect("wast lexer must accept the spec suite");
        let wast = wast::parser::parse::<wast::Wast>(&buf)
            .unwrap_or_else(|error| panic!("{} must parse as wast: {error}", file.display()));
        for directive in wast.directives {
            if let wast::WastDirective::Module(mut module) = directive {
                let source = module.encode().expect("module must encode");
                measure_module(&source, &mut totals).await;
            }
        }
    }

    let add_wasm = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/add.wasm");
    let source = std::fs::read(&add_wasm).expect("examples/add.wasm must exist");
    measure_module(&source, &mut totals).await;

    eprintln!(
        "artifact-measure corpus: {} wast files + examples/add.wasm",
        files.len()
    );
    eprintln!("modules measured: {}", totals.modules_measured);
    eprintln!("modules parse-failed: {}", totals.modules_parse_failed);
    eprintln!(
        "modules instantiate-failed: {}",
        totals.modules_instantiate_failed
    );
    eprintln!("functions emitted: {}", totals.functions_emitted);
    eprintln!("functions rejected: {}", totals.functions_rejected);
    eprintln!("wasm bytes: {}", totals.wasm_bytes);
    eprintln!("emitted code bytes: {}", totals.emitted_bytes);
    eprintln!(
        "emitted / wasm ratio: {:.3}",
        totals.emitted_bytes as f64 / totals.wasm_bytes as f64
    );
}
