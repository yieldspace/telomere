//! Captures build-identity facts for the precompiled-module codec (#196).
//!
//! The codec's artifact fingerprint pins artifacts to the exact build that
//! produced them: crate version, rustc version, target triple, feature set,
//! the op-registry hash, and a hash of this crate's own source tree. Crate
//! version and the feature set are visible to the crate itself, but the
//! rustc version, the target triple, and the source hash are only
//! available to the build script, so they are forwarded here as
//! `cargo:rustc-env` values.
//!
//! The source hash is what makes "exact build" mean the actual code: two
//! builds that differ only in a handler's implementation (same crate
//! version, rustc, target, features, and handler names) previously
//! produced identical fingerprints, so an artifact written by one could
//! be loaded and executed by the other even if a handler's operand
//! interpretation had changed. Hashing `src/` closes that gap: any
//! source change produces a different fingerprint and old artifacts are
//! rejected with a typed mismatch instead of being silently executed.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// FNV-1a 64-bit hash of the crate's `src/` tree: relative paths and file
/// contents, visited in sorted order so the result is deterministic
/// across filesystems and checkouts.
fn source_tree_hash(src_dir: &Path) -> u64 {
    fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
            .map(|read| {
                read.filter_map(|entry| entry.ok().map(|entry| entry.path()))
                    .collect()
            })
            .unwrap_or_default();
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect(&entry, out);
            } else if entry.extension().is_some_and(|ext| ext == "rs") {
                out.push(entry);
            }
        }
    }
    let mut files = Vec::new();
    collect(src_dir, &mut files);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |bytes: &[u8]| {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for file in &files {
        let relative = file.strip_prefix(src_dir).unwrap_or(file);
        mix(relative.to_string_lossy().as_bytes());
        mix(&[0]);
        if let Ok(contents) = std::fs::read(file) {
            mix(&contents);
        }
        mix(&[0xff]);
    }
    hash
}

fn main() {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    let version = Command::new(&rustc)
        .arg("--version")
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=TELOMERE_BUILD_RUSTC_VERSION={version}");

    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_string());
    println!("cargo:rustc-env=TELOMERE_BUILD_TARGET={target}");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let source_hash = source_tree_hash(&Path::new(&manifest_dir).join("src"));
    println!("cargo:rustc-env=TELOMERE_BUILD_SOURCE_HASH={source_hash:016x}");

    // The toolchain values depend on the toolchain, not on any file
    // input; the source hash depends on this script and on `src/`.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src");
}
