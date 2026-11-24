//! Corpus cold-start comparison for the AoT artifact container
//! (#198): aggregate `WasmParser::parse_module` time versus aggregate
//! `open_aot_artifact` time (signature verification + fingerprint
//! check + module binding + precompiled deserialize + function-table
//! validation) over a directory of `.wasm` corpus files. Artifact
//! production time and size are reported separately: production is
//! the offline step whose cost the load path exists to avoid paying
//! at startup.
//!
//! The corpus directory can be produced by the fuzz corpus generator:
//!
//! ```sh
//! TELOMERE_FUZZ_CORPUS_OUT=/tmp/fuzz-corpus \
//!   cargo test -p telomere --test fuzz_corpus_replay --release --features jit
//! cargo bench -p telomere --bench aot_cold_start --features jit -- \
//!   /tmp/fuzz-corpus/parse_core_module
//! ```
//!
//! The signer/verifier here is a demo MAC (FNV-1a based) so the
//! example needs no crypto dependency; it is *not* a signature
//! scheme and must not be copied into a production embedder.

#[cfg(not(feature = "jit"))]
fn main() {
    eprintln!("aot_cold_start requires the `jit` feature");
}

#[cfg(feature = "jit")]
fn main() {
    inner::real_main();
}

#[cfg(feature = "jit")]
mod inner {
    use std::fs;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use telomere::{
        build_aot_artifact, open_aot_artifact, AotLoadPolicy, AotSigner, AotVerifier,
        IoReadBinaryReader, WasmParser,
    };

    struct DemoKey;

    fn demo_mac(payload: &[u8]) -> Vec<u8> {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in payload {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash.to_le_bytes().to_vec()
    }

    impl AotSigner for DemoKey {
        fn key_id(&self) -> &str {
            "demo"
        }
        fn sign(&self, payload: &[u8]) -> Vec<u8> {
            demo_mac(payload)
        }
    }

    impl AotVerifier for DemoKey {
        fn key_id(&self) -> &str {
            "demo"
        }
        fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
            demo_mac(payload) == signature
        }
    }

    pub fn real_main() {
        let dir = std::env::args()
            .nth(1)
            .expect("usage: aot_cold_start <corpus-dir>");
        let mut files: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap_or_else(|error| panic!("cannot read corpus dir {dir}: {error}"))
            .map(|entry| entry.expect("corpus dir entry").path())
            .filter(|path| path.is_file())
            .collect();
        files.sort();
        assert!(!files.is_empty(), "corpus dir {dir} is empty");

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
        let mut functions = 0usize;
        let mut parse_total = Duration::ZERO;
        let mut build_total = Duration::ZERO;
        let mut open_total = Duration::ZERO;

        for path in &files {
            let bytes = fs::read(path).expect("corpus file must read");
            let start = Instant::now();
            let mut reader = IoReadBinaryReader::from(bytes.as_slice());
            let parsed = WasmParser::new(&mut reader).parse_module();
            parse_total += start.elapsed();
            let module = match parsed {
                Ok(module) => module,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            let start = Instant::now();
            let artifact = match build_aot_artifact(&module, Some(&DemoKey)) {
                Ok(artifact) => artifact,
                Err(_) => {
                    skipped += 1;
                    continue;
                }
            };
            build_total += start.elapsed();
            let start = Instant::now();
            let opened = open_aot_artifact(&artifact, Some(&DemoKey), AotLoadPolicy::default())
                .expect("a built artifact must open");
            open_total += start.elapsed();
            functions += opened.functions().len();
            modules += 1;
            wasm_bytes += bytes.len() as u64;
            artifact_bytes += artifact.len() as u64;
        }

        println!(
            "modules={modules} skipped={skipped} functions={functions} \
             wasm_bytes={wasm_bytes} artifact_bytes={artifact_bytes}"
        );
        println!("parse_total={parse_total:?}");
        println!("build_total={build_total:?} (offline production, not load path)");
        println!(
            "open_total={open_total:?} (verify + fingerprint + binding + deserialize + validate)"
        );
    }
}
