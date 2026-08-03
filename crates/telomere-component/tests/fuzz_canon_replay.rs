//! Stable-toolchain replay for the `canon_lift_args` fuzz target.
//!
//! The other two targets consume raw module and component bytes, so their
//! corpora are generated from the vendored `.wast` suites (see
//! `fuzz_corpus_replay.rs`). This target's input is a structured byte layout
//! that no suite produces, so its corpus is entirely committed: hand-written
//! seeds plus a minimized reproducer for every failure found during bring-up.
//!
//! Replaying them here is what keeps those reproducers honest. The fuzz target
//! itself only runs under a nightly toolchain, so without this test a committed
//! reproducer would never be exercised by `cargo test --workspace`, and a
//! regression could land unnoticed on the toolchain the project ships.

use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
};

use telomere_component::fuzz_canonical_lift_args_from_bytes;

const TARGET: &str = "canon_lift_args";

#[test]
fn replays_canon_lift_args_corpus() {
    let repository_root = repository_root();
    let fixture = fs::read(repository_root.join("fuzz/fixtures/canon_abi.wasm"))
        .expect("the canonical ABI fixture is committed");

    let mut replayed_seeds = 0usize;
    let mut replayed_regressions = 0usize;

    for (category, counter) in [
        ("seeds", &mut replayed_seeds),
        ("regressions", &mut replayed_regressions),
    ] {
        let root = repository_root.join("fuzz").join(category).join(TARGET);
        if !root.is_dir() {
            continue;
        }
        for path in collect_corpus_files(&root) {
            let bytes = fs::read(&path)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
            // A returned error is a normal outcome: these inputs are mostly
            // invalid. The assertion this test makes is that the adapter
            // returns rather than aborting the process.
            let _ = fuzz_canonical_lift_args_from_bytes(&fixture, &bytes);
            *counter += 1;
        }
    }

    println!(
        "fuzz corpus replay target={TARGET} replayed_seeds={replayed_seeds} \
         replayed_regressions={replayed_regressions}"
    );

    assert!(
        replayed_seeds + replayed_regressions > 0,
        "expected at least one committed {TARGET} corpus entry to replay"
    );
}

/// The empty input must still reach the adapter rather than being rejected by
/// the decoder, otherwise short corpus entries would exercise nothing.
#[test]
fn empty_input_reaches_the_adapter() {
    let repository_root = repository_root();
    let fixture = fs::read(repository_root.join("fuzz/fixtures/canon_abi.wasm"))
        .expect("the canonical ABI fixture is committed");
    let error = fuzz_canonical_lift_args_from_bytes(&fixture, &[])
        .expect_err("an empty input still invokes the adapter");
    assert!(
        error.to_string().contains("canonical ABI value underflow"),
        "unexpected adapter error: {error}"
    );
}

/// Accepts only files that are corpus inputs; see the note in
/// `fuzz_corpus_replay.rs` on why this is a positive filter.
fn is_corpus_candidate(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| !name.starts_with('.'))
}

fn collect_corpus_files(root: &Path) -> Vec<PathBuf> {
    let mut files = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", root.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| {
                    panic!("failed to read an entry below {}: {error}", root.display())
                })
                .path()
        })
        .filter(|path| path.is_file() && is_corpus_candidate(path))
        .collect::<Vec<_>>();
    files.sort();
    files
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate manifest lives two levels below the repository root")
        .to_path_buf()
}
