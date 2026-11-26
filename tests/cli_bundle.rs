//! End-to-end tests for the component bundle CLI surface (#327):
//! `telomere-cli compile --bundle` produces a #201 bundle from a
//! component, `telomere-cli component` loads it under the same
//! key/trust posture as core AoT artifacts, and
//! `telomere-cli compile --check` validates it. These drive the
//! real binary so the clap surface, the file output, and the run
//! path are all exercised the way a user hits them.
//!
//! The bundle producer and loader live behind the CLI's `jit`
//! feature (which enables `telomere-component/aot-bundle`); the
//! whole file compiles away without it.
#![cfg(feature = "jit")]

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_telomere-cli")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(test: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "telomere-cli-bundle-{test}-{}-{nonce}",
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

/// The `wasi:cli/run` command component shape used by
/// `tests/cli_component.rs`: the run export reports the
/// discriminant as its result, so 0 exits successfully and 1
/// exits with status 1.
fn write_command_component(dir: &TempDir, discriminant: i32) -> PathBuf {
    let path = dir.path("guest.wasm");
    let bytes = wat::parse_str(format!(
        r#"
(component
  (type $run-result (result))
  (type $run-func (func (result $run-result)))
  (type $run-instance (instance (export "run" (func (type $run-func)))))

  (core module $run-core
    (func (export "run") (result i32)
      i32.const {discriminant})
  )
  (core instance $run-core-inst (instantiate $run-core))
  (func $run (type $run-func)
    (canon lift (core func $run-core-inst "run")))

  (instance $run-export
    (export "run" (func $run))
  )
  (export "wasi:cli/run@0.2.6" (instance $run-export))
)
"#
    ))
    .expect("the command component wat must assemble");
    fs::write(&path, bytes).expect("the component must be writable");
    path
}

fn write_key(dir: &TempDir, name: &str, seed: u8) -> PathBuf {
    let path = dir.path(name);
    fs::write(&path, [seed; 32]).expect("the key file must be writable");
    path
}

fn compile_bundle(dir: &TempDir, component: &Path, extra: &[&str]) -> PathBuf {
    let bundle = dir.path("guest.tbnd");
    let component_arg = arg(component);
    let bundle_arg = arg(&bundle);
    let mut args = vec![
        "compile",
        "--bundle",
        component_arg.as_str(),
        "-o",
        bundle_arg.as_str(),
    ];
    args.extend_from_slice(extra);
    let compile = run(&args);
    assert!(
        compile.status.success(),
        "compile --bundle must succeed: {}",
        stderr(&compile)
    );
    bundle
}

#[test]
fn bundle_round_trip_runs_like_the_plain_component() {
    for (discriminant, expected_code) in [(0, 0), (1, 1)] {
        let dir = TempDir::new("round-trip");
        let component = write_command_component(&dir, discriminant);
        let bundle = compile_bundle(&dir, &component, &[]);

        let direct = run(&["component", &arg(&component)]);
        assert_eq!(
            direct.status.code(),
            Some(expected_code),
            "plain component run: {}",
            stderr(&direct)
        );

        let via_bundle = run(&["component", "--trust-unverified-artifact", &arg(&bundle)]);
        assert_eq!(
            via_bundle.status.code(),
            Some(expected_code),
            "bundle run must match the plain run: {}",
            stderr(&via_bundle)
        );
        assert_eq!(stdout(&via_bundle), stdout(&direct));
    }
}

#[test]
fn unsigned_bundle_needs_explicit_trust() {
    let dir = TempDir::new("unsigned");
    let component = write_command_component(&dir, 0);
    let bundle = compile_bundle(&dir, &component, &[]);

    let refused = run(&["component", &arg(&bundle)]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("unsigned component bundle refused"),
        "the refusal must name the trust posture: {}",
        stderr(&refused)
    );
}

#[test]
fn signed_bundle_verifies_with_its_key() {
    let dir = TempDir::new("signed");
    let component = write_command_component(&dir, 0);
    let key = write_key(&dir, "key.bin", 7);
    let key_arg = arg(&key);
    let bundle = compile_bundle(&dir, &component, &["--sign-key", key_arg.as_str()]);

    let bundle_arg = arg(&bundle);
    let no_key = run(&["component", bundle_arg.as_str()]);
    assert!(!no_key.status.success());
    assert!(
        stderr(&no_key).contains("no verifier was supplied"),
        "a signed bundle without its key must be refused: {}",
        stderr(&no_key)
    );

    let wrong_key = write_key(&dir, "wrong.bin", 9);
    let wrong_arg = arg(&wrong_key);
    let refused = run(&[
        "component",
        "--verify-key",
        wrong_arg.as_str(),
        bundle_arg.as_str(),
    ]);
    assert!(!refused.status.success());

    let verified = run(&[
        "component",
        "--verify-key",
        key_arg.as_str(),
        bundle_arg.as_str(),
    ]);
    assert!(
        verified.status.success(),
        "the signing key must verify: {}",
        stderr(&verified)
    );
}

#[test]
fn check_accepts_fresh_bundle_and_reports_signature_state() {
    let dir = TempDir::new("check");
    let component = write_command_component(&dir, 0);

    let unsigned = compile_bundle(&dir, &component, &[]);
    let check = run(&["compile", "--check", &arg(&unsigned)]);
    assert!(
        check.status.success(),
        "a fresh unsigned bundle must check out: {}",
        stderr(&check)
    );
    assert!(stdout(&check).contains("unsigned"));
    assert!(
        stdout(&check).contains("1 substituted"),
        "the check must report the substitution accounting: {}",
        stdout(&check)
    );

    let key = write_key(&dir, "key.bin", 3);
    let key_arg = arg(&key);
    let signed = compile_bundle(&dir, &component, &["--sign-key", key_arg.as_str()]);

    let check = run(&["compile", "--check", &arg(&signed)]);
    assert!(!check.status.success());
    assert!(
        stderr(&check).contains("no verifier was supplied"),
        "an unverifiable signature is a rejection: {}",
        stderr(&check)
    );

    let signed_arg = arg(&signed);
    let check = run(&[
        "compile",
        "--check",
        signed_arg.as_str(),
        "--verify-key",
        key_arg.as_str(),
    ]);
    assert!(
        check.status.success(),
        "the signing key must check out: {}",
        stderr(&check)
    );
    assert!(stdout(&check).contains("signed with hmac-sha256:"));
}

#[test]
fn tampered_signed_bundle_is_rejected() {
    let dir = TempDir::new("tampered");
    let component = write_command_component(&dir, 0);
    let key = write_key(&dir, "key.bin", 5);
    let key_arg = arg(&key);
    let bundle = compile_bundle(&dir, &component, &["--sign-key", key_arg.as_str()]);

    // Flip a bit in the trailing signature block.
    let mut bytes = fs::read(&bundle).expect("the bundle must be readable");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&bundle, bytes).expect("the bundle must be writable");

    let bundle_arg = arg(&bundle);
    let refused = run(&[
        "component",
        "--verify-key",
        key_arg.as_str(),
        bundle_arg.as_str(),
    ]);
    assert!(!refused.status.success());
    assert!(
        stderr(&refused).contains("signature verification failed"),
        "the rejection must name the signature failure: {}",
        stderr(&refused)
    );

    let check = run(&[
        "compile",
        "--check",
        bundle_arg.as_str(),
        "--verify-key",
        key_arg.as_str(),
    ]);
    assert!(!check.status.success());
    assert!(stderr(&check).contains("signature verification failed"));
}

#[test]
fn compile_bundle_rejects_core_modules_bundles_and_plain_compile_still_rejects_components() {
    let dir = TempDir::new("rejects");
    let component = write_command_component(&dir, 0);
    let bundle = compile_bundle(&dir, &component, &[]);

    let core = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/add.wasm");
    let output = run(&["compile", "--bundle", &arg(&core)]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("not a Wasm component"),
        "bundling a core module must name the mismatch: {}",
        stderr(&output)
    );

    let output = run(&["compile", "--bundle", &arg(&bundle)]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("already a component bundle"),
        "bundling a bundle must be refused: {}",
        stderr(&output)
    );

    let output = run(&["compile", &arg(&component)]);
    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("compile --bundle"),
        "plain compile must point at the bundle mode: {}",
        stderr(&output)
    );
}
