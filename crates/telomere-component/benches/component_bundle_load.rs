//! Component bundle load measurement (#201).
//!
//! For each component fixture this prints, per load:
//!
//! * `compile` — the plain path: `ComponentEngine::compile` over the
//!   component binary (component-envelope decode + one core
//!   `parse_module` per embedded core module).
//! * `core parse (sum)` — the core front end alone: the fixture's
//!   CoreModule section payloads extracted with the bundle section
//!   walker and timed through `WasmParser::parse_module` one by one.
//! * `envelope (derived)` — `compile − core parse (sum)`: everything
//!   the component layer itself costs (section decode, validation,
//!   type construction, the per-module contract check). Derived, not
//!   directly timed; the two timed inputs are printed alongside.
//! * `bundle load` — the bundle path: `ComponentEngine::compile_bundle`
//!   over a bundle built from the same fixture (envelope parse,
//!   signature posture, keyed-set pre-check, per-artifact open,
//!   component decode with substitution).
//! * `artifact open (sum)` — the per-artifact share of the bundle
//!   path: each section's standalone artifact opened through
//!   `telomere::open_aot_artifact`.
//! * `bundle build` — the offline producer cost (parse + emit + sign
//!   path), reported for completeness; it is not part of load.
//!
//! Every figure is the median of the repetitions shown, after a
//! warm-up round, measured with `std::time::Instant` on the current
//! thread. Run with:
//!
//! ```sh
//! cargo bench -p telomere-component --features aot-bundle \
//!     --bench component_bundle_load
//! ```

use std::time::{Duration, Instant};

use telomere::{AotLoadPolicy, IoReadBinaryReader, WasmParser};
use telomere_component::bundle::enumerate_core_module_sections;
use telomere_component::{build_component_bundle, ComponentEngine};

const REPS: usize = 51;

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    samples[samples.len() / 2]
}

fn time<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
    // Warm-up.
    let _ = f();
    let mut samples = Vec::with_capacity(REPS);
    let mut out = None;
    for _ in 0..REPS {
        let start = Instant::now();
        out = Some(f());
        samples.push(start.elapsed());
    }
    (out.expect("at least one run"), median(samples))
}

/// Like [`time`], but the closure returns the duration to record
/// itself (for phases timed as a sum over sub-steps inside the
/// closure, where the closure also does untimed setup).
fn time_total(mut f: impl FnMut() -> Duration) -> Duration {
    let _ = f();
    let samples = (0..REPS).map(|_| f()).collect();
    median(samples)
}

fn main() {
    let fixtures = [
        "component-add.wasm",
        "wasi-component-args.wasm",
        "wasi-component-stdout.wasm",
    ];
    let engine = ComponentEngine::new();
    println!(
        "{:<28} {:>9} {:>10} {:>10} {:>12} {:>10} {:>12} {:>12}",
        "fixture",
        "bytes",
        "sections",
        "compile",
        "core parse",
        "envelope~",
        "bundle load",
        "art. open"
    );
    for name in fixtures {
        let path = format!("{}/../../examples/{name}", env!("CARGO_MANIFEST_DIR"));
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {path}: {e}"));
        let sections = enumerate_core_module_sections(&bytes).expect("walk fixture");

        let (_, compile) = time(|| engine.compile(&bytes).expect("compile fixture"));
        let parse_sum = time_total(|| {
            let mut total = Duration::ZERO;
            for section in &sections {
                let payload = &bytes[section.range.clone()];
                let start = Instant::now();
                let mut reader = IoReadBinaryReader::from(payload);
                let mut parser = WasmParser::new(&mut reader);
                parser.parse_module().expect("parse section");
                total += start.elapsed();
            }
            total
        });
        let envelope = compile.saturating_sub(parse_sum);

        let bundle = build_component_bundle(&bytes, None).expect("build bundle");
        let policy = AotLoadPolicy {
            allow_unsigned: true,
        };
        let (loaded, bundle_load) = time(|| {
            engine
                .compile_bundle(&bundle, None, policy)
                .expect("load bundle")
        });
        assert_eq!(
            loaded.report().substituted_ordinals.len(),
            sections.len(),
            "every section substituted"
        );
        let open_sum = time_total(|| {
            let mut total = Duration::ZERO;
            for section in &sections {
                let payload = &bytes[section.range.clone()];
                let mut reader = IoReadBinaryReader::from(payload);
                let mut parser = WasmParser::new(&mut reader);
                let module = parser.parse_module().expect("parse section");
                let artifact = telomere::build_aot_artifact(&module, None).expect("build");
                let start = Instant::now();
                let _ = telomere::open_aot_artifact(&artifact, None, policy).expect("open");
                total += start.elapsed();
            }
            total
        });
        let (_, bundle_build) = time(|| build_component_bundle(&bytes, None).expect("build"));

        println!(
            "{:<28} {:>9} {:>10} {:>10.1?} {:>10.1?} {:>10.1?} {:>12.1?} {:>12.1?}  (build {bundle_build:.1?}, bundle {} B)",
            name,
            bytes.len(),
            sections.len(),
            compile,
            parse_sum,
            envelope,
            bundle_load,
            open_sum,
            bundle.len(),
        );
    }
}
