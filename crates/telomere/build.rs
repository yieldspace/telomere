//! Captures build-identity facts for the precompiled-module codec (#196).
//!
//! The codec's artifact fingerprint pins artifacts to the exact build that
//! produced them: crate version, rustc version, target triple, feature set,
//! and the op-registry hash. Crate version and the feature set are visible
//! to the crate itself, but the rustc version and the target triple are
//! only available to the build script, so they are forwarded here as
//! `cargo:rustc-env` values.

use std::env;
use std::process::Command;

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

    // The values above depend on the toolchain, not on any file input;
    // rerun only when this script itself changes.
    println!("cargo:rerun-if-changed=build.rs");
}
