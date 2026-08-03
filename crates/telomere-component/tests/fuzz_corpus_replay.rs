use std::{
    collections::BTreeMap,
    ffi::OsStr,
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};
use wast::{parser::ParseBuffer, QuoteWat, Wast, WastDirective, WastExecute, Wat};

const TARGET: &str = "decode_component";
const WASM_MAGIC: &[u8; 4] = b"\0asm";
const COMPONENT_VERSION: [u8; 4] = [0x0d, 0, 1, 0];

#[derive(Default)]
struct Report {
    source_files: usize,
    extractable_payloads: usize,
    generated: usize,
    replayed: usize,
    replayed_seeds: usize,
    replayed_regressions: usize,
    emitted: usize,
    existing: usize,
    skipped: BTreeMap<&'static str, usize>,
}

impl Report {
    fn skip(&mut self, reason: &'static str) {
        *self.skipped.entry(reason).or_default() += 1;
    }
}

#[test]
fn replays_decode_component_corpus() {
    let output_root = corpus_output_root();
    let repository_root = repository_root();
    let mut files = testsuite_roots(&repository_root)
        .into_iter()
        .flat_map(|root| collect_wast_files(&root))
        .collect::<Vec<_>>();
    files.sort();
    assert!(
        !files.is_empty(),
        "expected at least one .wast file in the corpus source suites"
    );

    let mut report = Report {
        source_files: files.len(),
        ..Report::default()
    };
    let mut corpus = BTreeMap::new();

    for path in files {
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(_) => {
                report.skip("source_read");
                continue;
            }
        };
        let buffer = match ParseBuffer::new(&text) {
            Ok(buffer) => buffer,
            Err(_) => {
                report.skip("wast_parse");
                continue;
            }
        };
        let mut wast = match wast::parser::parse::<Wast<'_>>(&buffer) {
            Ok(wast) => wast,
            Err(_) => {
                report.skip("wast_parse");
                continue;
            }
        };
        extract_directives(&mut wast.directives, &mut corpus, &mut report);
    }

    report.generated = corpus.len();
    let output_dir = output_root.map(|root| {
        let output_dir = root.join(TARGET);
        fs::create_dir_all(&output_dir).unwrap_or_else(|error| {
            panic!(
                "failed to create fuzz corpus output directory {}: {error}",
                output_dir.display()
            )
        });
        output_dir
    });

    for (hash, bytes) in corpus {
        if let Some(output_dir) = &output_dir {
            if emit_corpus_case(output_dir, &hash, &bytes) {
                report.emitted += 1;
            } else {
                report.existing += 1;
            }
        }

        replay_component_case(&bytes);
        report.replayed += 1;
    }

    replay_committed_cases(&repository_root, CommittedCorpus::Seeds, &mut report);
    replay_committed_cases(&repository_root, CommittedCorpus::Regressions, &mut report);

    let skipped = report.skipped.values().sum::<usize>();
    let reasons = report
        .skipped
        .iter()
        .map(|(reason, count)| format!("{reason}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "fuzz corpus replay target={TARGET} source_files={} extractable={} generated={} replayed={} replayed_seeds={} replayed_regressions={} emitted={} existing={} skipped={} reasons={}",
        report.source_files,
        report.extractable_payloads,
        report.generated,
        report.replayed,
        report.replayed_seeds,
        report.replayed_regressions,
        report.emitted,
        report.existing,
        skipped,
        if reasons.is_empty() { "none" } else { &reasons },
    );
}

fn corpus_output_root() -> Option<PathBuf> {
    let path = std::env::var_os("TELOMERE_FUZZ_CORPUS_OUT")?;
    let path = PathBuf::from(path);
    assert!(
        path.is_absolute(),
        "TELOMERE_FUZZ_CORPUS_OUT must be an absolute path, got {}",
        path.display()
    );
    Some(path)
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap_or_else(|| panic!("failed to resolve repository root from CARGO_MANIFEST_DIR"))
        .to_owned()
}

fn testsuite_roots(repository_root: &Path) -> [PathBuf; 2] {
    let core = repository_root.join("crates/telomere/tests/wasm-testsuite");
    assert!(
        core.is_dir(),
        "missing wasm testsuite submodule at {}. Run `git submodule update --init --recursive` from the repository root.",
        core.display()
    );

    let component =
        repository_root.join("crates/telomere-component/tests/component_model_testsuite");
    assert!(
        component.is_dir(),
        "missing component model testsuite at {}",
        component.display()
    );

    [core, component]
}

fn collect_wast_files(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
            .map(|entry| {
                entry.unwrap_or_else(|error| {
                    panic!("failed to read an entry below {}: {error}", dir.display())
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.is_file() && path.extension() == Some(OsStr::new("wast")) {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort();
    files
}

fn extract_directives(
    directives: &mut [WastDirective<'_>],
    corpus: &mut BTreeMap<String, Vec<u8>>,
    report: &mut Report,
) {
    for directive in directives {
        match directive {
            WastDirective::Module(module)
            | WastDirective::ModuleDefinition(module)
            | WastDirective::AssertMalformed { module, .. }
            | WastDirective::AssertInvalid { module, .. } => {
                extract_quote_wat(module, corpus, report);
            }
            WastDirective::AssertUnlinkable { module, .. } => {
                extract_wat(module, corpus, report);
            }
            WastDirective::AssertTrap { exec, .. }
            | WastDirective::AssertReturn { exec, .. }
            | WastDirective::AssertException { exec, .. }
            | WastDirective::AssertSuspension { exec, .. } => {
                extract_execute(exec, corpus, report);
            }
            WastDirective::Thread(thread) => {
                extract_directives(&mut thread.directives, corpus, report);
            }
            WastDirective::ModuleInstance { .. }
            | WastDirective::Register { .. }
            | WastDirective::Invoke(_)
            | WastDirective::AssertExhaustion { .. }
            | WastDirective::Wait { .. } => {}
        }
    }
}

fn extract_execute(
    execute: &mut WastExecute<'_>,
    corpus: &mut BTreeMap<String, Vec<u8>>,
    report: &mut Report,
) {
    if let WastExecute::Wat(wat) = execute {
        extract_wat(wat, corpus, report);
    }
}

fn extract_quote_wat(
    module: &mut QuoteWat<'_>,
    corpus: &mut BTreeMap<String, Vec<u8>>,
    report: &mut Report,
) {
    report.extractable_payloads += 1;
    match module.encode() {
        Ok(bytes) => insert_component_case(bytes, corpus, report),
        Err(_) => report.skip("encode"),
    }
}

fn extract_wat(module: &mut Wat<'_>, corpus: &mut BTreeMap<String, Vec<u8>>, report: &mut Report) {
    report.extractable_payloads += 1;
    match module.encode() {
        Ok(bytes) => insert_component_case(bytes, corpus, report),
        Err(_) => report.skip("encode"),
    }
}

fn insert_component_case(
    bytes: Vec<u8>,
    corpus: &mut BTreeMap<String, Vec<u8>>,
    report: &mut Report,
) {
    if !is_component(&bytes) {
        report.skip("non_component_header");
        return;
    }
    corpus.entry(sha256_hex(&bytes)).or_insert(bytes);
}

fn is_component(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[..4] == WASM_MAGIC && bytes[4..8] == COMPONENT_VERSION
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";

    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        hex.push(HEX[(byte >> 4) as usize] as char);
        hex.push(HEX[(byte & 0x0f) as usize] as char);
    }
    hex
}

fn emit_corpus_case(output_dir: &Path, hash: &str, bytes: &[u8]) -> bool {
    let path = output_dir.join(hash);
    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(bytes).unwrap_or_else(|error| {
                panic!(
                    "failed to write fuzz corpus case {}: {error}",
                    path.display()
                )
            });
            true
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let existing = fs::read(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            assert_eq!(
                existing,
                bytes,
                "SHA-256 corpus filename collision at {}",
                path.display()
            );
            false
        }
        Err(error) => panic!(
            "failed to create fuzz corpus case {}: {error}",
            path.display()
        ),
    }
}

/// The stack a replayed decode is given; see the note on the core parser's
/// equivalent in `crates/telomere/tests/fuzz_corpus_replay.rs`. The component
/// decoder recurses on nested types and nested components with no depth limit
/// either, so it is given the same accommodation rather than being left to
/// abort the test process.
const REPLAY_STACK_BYTES: usize = 64 * 1024 * 1024;

fn replay_component_case(bytes: &[u8]) {
    let bytes = bytes.to_vec();
    std::thread::Builder::new()
        .stack_size(REPLAY_STACK_BYTES)
        .spawn(move || {
            let _ = telomere_component::ComponentEngine::new().compile(&bytes);
        })
        .expect("spawning a replay thread")
        .join()
        .expect("a replayed decode must not panic");
}

#[derive(Clone, Copy)]
enum CommittedCorpus {
    Seeds,
    Regressions,
}

impl CommittedCorpus {
    fn directory_name(self) -> &'static str {
        match self {
            Self::Seeds => "seeds",
            Self::Regressions => "regressions",
        }
    }
}

fn replay_committed_cases(repository_root: &Path, category: CommittedCorpus, report: &mut Report) {
    let root = repository_root
        .join("fuzz")
        .join(category.directory_name())
        .join(TARGET);
    if !root.exists() {
        return;
    }
    if !root.is_dir() {
        report.skip("committed_not_directory");
        return;
    }

    for path in collect_all_files(&root) {
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                report.skip("committed_read");
                continue;
            }
        };
        replay_component_case(&bytes);
        report.replayed += 1;
        match category {
            CommittedCorpus::Seeds => report.replayed_seeds += 1,
            CommittedCorpus::Regressions => report.replayed_regressions += 1,
        }
    }
}

/// Accepts only files that are corpus inputs.
///
/// This is a positive filter rather than a list of names to skip. The committed
/// corpus directories carry `.gitkeep` placeholders, and a directory opened in a
/// file browser on macOS gains a `.DS_Store`; either would otherwise be replayed
/// as a malformed input and counted in the corpus totals.
fn is_corpus_candidate(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| !name.starts_with('.'))
}

fn collect_all_files(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>) {
        let mut entries = fs::read_dir(dir)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", dir.display()))
            .map(|entry| {
                entry.unwrap_or_else(|error| {
                    panic!("failed to read an entry below {}: {error}", dir.display())
                })
            })
            .collect::<Vec<_>>();
        entries.sort_by_key(|entry| entry.path());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, files);
            } else if path.is_file() && is_corpus_candidate(&path) {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files.sort();
    files
}
