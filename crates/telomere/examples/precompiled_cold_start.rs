//! Corpus cold-start comparison for the precompiled-module codec
//! (#196): aggregate `WasmParser::parse_module` time versus aggregate
//! `deserialize_module` time over a directory of `.wasm` corpus files,
//! with the codec's own per-phase timings as the breakdown.
//!
//! The corpus directory can be produced by the fuzz corpus generator:
//!
//! ```sh
//! TELOMERE_FUZZ_CORPUS_OUT=/tmp/fuzz-corpus \
//!   cargo test -p telomere --test fuzz_corpus_replay --release
//! cargo run --release --example precompiled_cold_start -- \
//!   /tmp/fuzz-corpus/parse_core_module
//! ```

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use telomere::{
    deserialize_module_timed, serialize_module_timed, IoReadBinaryReader, PrecompiledTimings,
    WasmParser,
};

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("usage: precompiled_cold_start <corpus-dir>");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read corpus dir {dir}: {error}"))
        .map(|entry| entry.expect("corpus dir entry").path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    assert!(!files.is_empty(), "corpus dir {dir} is empty");

    // Parsing is recursive; use the same generous stack posture as the
    // fuzz corpus replay harness.
    std::thread::Builder::new()
        .stack_size(64 * 1024 * 1024)
        .spawn(move || run(files))
        .expect("spawning measurement thread")
        .join()
        .expect("measurement thread must not panic");
}

fn run(files: Vec<PathBuf>) {
    let mut modules = 0usize;
    let mut skipped = 0usize;
    let mut wasm_bytes = 0u64;
    let mut artifact_bytes = 0u64;
    let mut parse_total = Duration::ZERO;
    let mut serialize_total = Duration::ZERO;
    let mut deserialize_total = Duration::ZERO;
    let mut de_header = Duration::ZERO;
    let mut de_sections = Duration::ZERO;
    let mut de_functions = Duration::ZERO;

    for path in &files {
        let bytes = fs::read(path).expect("corpus file must read");
        let started = Instant::now();
        let mut reader = IoReadBinaryReader::from(&bytes[..]);
        let parsed = WasmParser::new(&mut reader).parse_module();
        let parse_elapsed = started.elapsed();
        let module = match parsed {
            Ok(module) => module,
            Err(_) => {
                skipped += 1;
                continue;
            }
        };
        let mut ser_timings = PrecompiledTimings::default();
        let artifact = serialize_module_timed(&module, Some(&mut ser_timings))
            .expect("a parsed corpus module must serialize");
        let mut de_timings = PrecompiledTimings::default();
        let started = Instant::now();
        let restored = deserialize_module_timed(&artifact, Some(&mut de_timings))
            .expect("a serialized corpus module must deserialize");
        let deserialize_elapsed = started.elapsed();
        drop(restored);

        modules += 1;
        wasm_bytes += bytes.len() as u64;
        artifact_bytes += artifact.len() as u64;
        parse_total += parse_elapsed;
        serialize_total += ser_timings.total;
        deserialize_total += deserialize_elapsed;
        de_header += de_timings.header;
        de_sections += de_timings.sections;
        de_functions += de_timings.functions;
    }

    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    println!("corpus files: {}", files.len());
    println!("modules measured: {modules} (skipped unparseable: {skipped})");
    println!("wasm bytes total: {wasm_bytes}");
    println!("artifact bytes total: {artifact_bytes}");
    println!("parse total: {:.1} ms", ms(parse_total));
    println!("serialize total: {:.1} ms", ms(serialize_total));
    println!("deserialize total: {:.1} ms", ms(deserialize_total));
    println!(
        "deserialize phases: header {:.1} ms, sections {:.1} ms, functions {:.1} ms",
        ms(de_header),
        ms(de_sections),
        ms(de_functions)
    );
    if modules > 0 {
        println!(
            "per module: parse {:.3} ms, deserialize {:.3} ms",
            ms(parse_total) / modules as f64,
            ms(deserialize_total) / modules as f64
        );
    }
}
