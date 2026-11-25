//! End-to-end tests for `telomere-cli compile` (#199): the offline
//! artifact producer, the fail-closed artifact load path, and
//! `compile --check`. These drive the real binary so the clap
//! surface, the file output, and the run path are all exercised the
//! way a user hits them.
//!
//! The AoT cases need the artifact producer and loader, which live
//! behind the CLI's `jit` feature; they compile away without it.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_telomere-cli")
}

fn example(file: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(file)
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "telomere-cli-compile-{test}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp dir must be creatable");
        Self(path)
    }

    fn path(&self, file: &str) -> PathBuf {
        self.0.join(file)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(binary())
        .args(args)
        .output()
        .expect("the telomere-cli binary must run")
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout must be utf-8")
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr must be utf-8")
}

fn arg(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A module whose start function traps: any compile path that
/// instantiated it would fail, so a successful compile proves the
/// producer never executes guest code.
fn trapping_start_wasm(dir: &TempDir) -> PathBuf {
    let path = dir.path("trapping-start.wasm");
    let bytes = wat::parse_str(
        "(module \
           (func (export \"answer\") (result i32) i32.const 42) \
           (func unreachable) \
           (start 1))",
    )
    .expect("the probe wat must assemble");
    fs::write(&path, bytes).expect("the probe module must be writable");
    path
}

/// Flips one byte of the crate-version string embedded in an
/// artifact's build fingerprint, producing a "stale" artifact: one
/// that a different build would have written.
fn corrupt_fingerprint(artifact: &Path) {
    let version = env!("CARGO_PKG_VERSION").as_bytes();
    let mut bytes = fs::read(artifact).expect("the artifact must be readable");
    let position = bytes
        .windows(version.len())
        .position(|window| window == version)
        .expect("the artifact fingerprint must embed the crate version");
    bytes[position] = if bytes[position] == b'0' { b'1' } else { b'0' };
    fs::write(artifact, bytes).expect("the artifact must be writable");
}

#[test]
fn precompiled_artifact_runs_identically_to_the_module() {
    let dir = TempDir::new("precompiled-add");
    let artifact = dir.path("add.tpre");
    let compile = run(&["compile", &arg(&example("add.wasm")), "-o", &arg(&artifact)]);
    assert!(
        compile.status.success(),
        "compile must succeed: {}",
        stderr(&compile)
    );

    let direct = run(&[&arg(&example("add.wasm")), "main", "1", "2"]);
    let via_artifact = run(&[&arg(&artifact), "main", "1", "2"]);
    assert!(direct.status.success(), "direct run: {}", stderr(&direct));
    assert!(
        via_artifact.status.success(),
        "artifact run: {}",
        stderr(&via_artifact)
    );
    assert_eq!(stdout(&via_artifact), stdout(&direct));
    assert_eq!(stdout(&via_artifact), "3\n");
}

#[test]
fn precompiled_preview1_artifact_matches_direct_run() {
    let dir = TempDir::new("precompiled-preview1");
    // Same basename as the example so the guest's argv[0] matches.
    let artifact = dir.path("wasi-preview1-hello.wasm");
    let compile = run(&[
        "compile",
        &arg(&example("wasi-preview1-hello.wasm")),
        "-o",
        &arg(&artifact),
    ]);
    assert!(
        compile.status.success(),
        "compile must succeed: {}",
        stderr(&compile)
    );

    let direct = run(&[
        &arg(&example("wasi-preview1-hello.wasm")),
        "--",
        "one",
        "two",
    ]);
    let via_artifact = run(&[&arg(&artifact), "--", "one", "two"]);
    assert!(direct.status.success(), "direct run: {}", stderr(&direct));
    assert!(
        via_artifact.status.success(),
        "artifact run: {}",
        stderr(&via_artifact)
    );
    assert_eq!(stdout(&via_artifact), stdout(&direct));
    assert!(stdout(&via_artifact).contains("one\ntwo\n"));
}

#[test]
fn compile_rejects_component_binaries() {
    let dir = TempDir::new("component");
    let output = run(&[
        "compile",
        &arg(&example("component-add.wasm")),
        "-o",
        &arg(&dir.path("component.tpre")),
    ]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("component"),
        "the rejection must name the component workflow: {}",
        stderr(&output)
    );
}

#[test]
fn compile_never_executes_guest_code() {
    let dir = TempDir::new("no-guest-exec");
    let module = trapping_start_wasm(&dir);

    // Running the module fails: instantiation executes the start
    // function, which traps.
    let direct = run(&[&arg(&module), "answer"]);
    assert!(
        !direct.status.success(),
        "the trapping start function must make a direct run fail"
    );

    // Compiling it succeeds, for both artifact kinds available in
    // this build, because the producer only parses.
    let artifact = dir.path("trapping.tpre");
    let compile = run(&["compile", &arg(&module), "-o", &arg(&artifact)]);
    assert!(
        compile.status.success(),
        "compile must not run the start function: {}",
        stderr(&compile)
    );
}

#[test]
fn check_accepts_fresh_precompiled_and_rejects_stale() {
    let dir = TempDir::new("check-precompiled");
    let artifact = dir.path("add.tpre");
    let compile = run(&["compile", &arg(&example("add.wasm")), "-o", &arg(&artifact)]);
    assert!(compile.status.success(), "compile: {}", stderr(&compile));

    let check = run(&["compile", "--check", &arg(&artifact)]);
    assert!(
        check.status.success(),
        "a fresh artifact must check out: {}",
        stderr(&check)
    );
    assert!(stdout(&check).contains("ok:"));

    corrupt_fingerprint(&artifact);
    let check = run(&["compile", "--check", &arg(&artifact)]);
    assert!(!check.status.success(), "a stale artifact must be rejected");
    assert!(
        stderr(&check).contains("different build"),
        "the rejection must name the typed reason: {}",
        stderr(&check)
    );

    let check = run(&["compile", "--check", &arg(&example("add.wasm"))]);
    assert!(!check.status.success());
    assert!(
        stderr(&check).contains("not a telomere artifact"),
        "a plain module is not an artifact: {}",
        stderr(&check)
    );
}

#[cfg(feature = "jit")]
mod aot {
    use super::*;

    fn compile_aot(dir: &TempDir, module: &Path, extra: &[&str]) -> PathBuf {
        let artifact = dir.path("module.taot");
        let module_arg = arg(module);
        let artifact_arg = arg(&artifact);
        let mut args = vec![
            "compile",
            module_arg.as_str(),
            "--aot",
            "-o",
            artifact_arg.as_str(),
        ];
        args.extend_from_slice(extra);
        let compile = run(&args);
        assert!(
            compile.status.success(),
            "compile --aot must succeed: {}",
            stderr(&compile)
        );
        artifact
    }

    fn write_key(dir: &TempDir, name: &str, seed: u8) -> PathBuf {
        let path = dir.path(name);
        fs::write(&path, [seed; 32]).expect("the key file must be writable");
        path
    }

    #[test]
    fn aot_artifact_runs_identically_to_the_module() {
        let dir = TempDir::new("aot-add");
        let artifact = compile_aot(&dir, &example("add.wasm"), &[]);

        let direct = run(&[&arg(&example("add.wasm")), "main", "1", "2"]);
        let via_artifact = run(&[
            "--trust-unverified-artifact",
            &arg(&artifact),
            "main",
            "1",
            "2",
        ]);
        assert!(direct.status.success());
        assert!(
            via_artifact.status.success(),
            "artifact run: {}",
            stderr(&via_artifact)
        );
        assert_eq!(stdout(&via_artifact), stdout(&direct));
        assert_eq!(stdout(&via_artifact), "3\n");
    }

    #[test]
    fn aot_unsigned_artifact_needs_explicit_trust() {
        let dir = TempDir::new("aot-unsigned");
        let artifact = compile_aot(&dir, &example("add.wasm"), &[]);

        let refused = run(&[&arg(&artifact), "main", "1", "2"]);
        assert!(!refused.status.success());
        assert!(
            stderr(&refused).contains("unsigned AoT artifact refused"),
            "the refusal must name the trust posture: {}",
            stderr(&refused)
        );
    }

    #[test]
    fn aot_signed_artifact_verifies_with_its_key() {
        let dir = TempDir::new("aot-signed");
        let key = write_key(&dir, "key.bin", 7);
        let key_arg = arg(&key);
        let artifact = compile_aot(
            &dir,
            &example("add.wasm"),
            &["--sign-key", key_arg.as_str()],
        );

        let artifact_arg = arg(&artifact);
        let no_key = run(&[artifact_arg.as_str(), "main", "1", "2"]);
        assert!(!no_key.status.success());
        assert!(
            stderr(&no_key).contains("no verifier was supplied"),
            "a signed artifact without its key must be refused: {}",
            stderr(&no_key)
        );

        let wrong_key = write_key(&dir, "wrong.bin", 9);
        let wrong_arg = arg(&wrong_key);
        let artifact_arg = arg(&artifact);
        let refused = run(&[
            "--verify-key",
            wrong_arg.as_str(),
            artifact_arg.as_str(),
            "main",
            "1",
            "2",
        ]);
        assert!(!refused.status.success());

        let key_arg = arg(&key);
        let verified = run(&[
            "--verify-key",
            key_arg.as_str(),
            artifact_arg.as_str(),
            "main",
            "1",
            "2",
        ]);
        assert!(
            verified.status.success(),
            "the signing key must verify: {}",
            stderr(&verified)
        );
        assert_eq!(stdout(&verified), "3\n");
    }

    #[test]
    fn aot_preview1_artifact_matches_direct_run() {
        let dir = TempDir::new("aot-preview1");
        // Same basename as the example so the guest's argv[0] matches.
        let artifact = dir.path("wasi-preview1-hello.wasm");
        let compile = run(&[
            "compile",
            &arg(&example("wasi-preview1-hello.wasm")),
            "--aot",
            "-o",
            &arg(&artifact),
        ]);
        assert!(
            compile.status.success(),
            "compile --aot must succeed: {}",
            stderr(&compile)
        );

        let direct = run(&[
            &arg(&example("wasi-preview1-hello.wasm")),
            "--",
            "one",
            "two",
        ]);
        let via_artifact = run(&[
            "--trust-unverified-artifact",
            &arg(&artifact),
            "--",
            "one",
            "two",
        ]);
        assert!(direct.status.success(), "direct run: {}", stderr(&direct));
        assert!(
            via_artifact.status.success(),
            "artifact run: {}",
            stderr(&via_artifact)
        );
        assert_eq!(stdout(&via_artifact), stdout(&direct));
    }

    #[test]
    fn aot_compile_never_executes_guest_code() {
        let dir = TempDir::new("aot-no-guest-exec");
        let module = trapping_start_wasm(&dir);
        let artifact = compile_aot(&dir, &module, &[]);
        assert!(artifact.exists());
    }

    #[test]
    fn check_accepts_fresh_aot_and_rejects_stale() {
        let dir = TempDir::new("check-aot");
        let artifact = compile_aot(&dir, &example("add.wasm"), &[]);

        let check = run(&["compile", "--check", &arg(&artifact)]);
        assert!(
            check.status.success(),
            "a fresh artifact must check out: {}",
            stderr(&check)
        );
        assert!(stdout(&check).contains("unsigned"));

        corrupt_fingerprint(&artifact);
        let check = run(&["compile", "--check", &arg(&artifact)]);
        assert!(!check.status.success(), "a stale artifact must be rejected");
        assert!(
            stderr(&check).contains("different build"),
            "the rejection must name the typed reason: {}",
            stderr(&check)
        );
    }

    #[test]
    fn check_signed_aot_requires_its_key() {
        let dir = TempDir::new("check-aot-signed");
        let key = write_key(&dir, "key.bin", 3);
        let key_arg = arg(&key);
        let artifact = compile_aot(
            &dir,
            &example("add.wasm"),
            &["--sign-key", key_arg.as_str()],
        );

        let check = run(&["compile", "--check", &arg(&artifact)]);
        assert!(!check.status.success());
        assert!(
            stderr(&check).contains("no verifier was supplied"),
            "an unverifiable signature is a rejection: {}",
            stderr(&check)
        );

        let artifact_arg = arg(&artifact);
        let check = run(&[
            "compile",
            "--check",
            artifact_arg.as_str(),
            "--verify-key",
            key_arg.as_str(),
        ]);
        assert!(
            check.status.success(),
            "the signing key must check out: {}",
            stderr(&check)
        );
    }
}
