use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_component_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "telomere-cli-{name}-{}-{nanos}.component.wasm",
        std::process::id()
    ))
}

fn write_command_component(path: &PathBuf, discriminant: i32) {
    write_component(
        path,
        format!(
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
        ),
    );
}

fn write_exit_component(path: &PathBuf, discriminant: i32) {
    write_component(
        path,
        format!(
            r#"
(component
  (type $status (result))
  (type $exit-func (func (param "status" $status)))
  (type $exit-instance (instance (export "exit" (func (type $exit-func)))))
  (import "wasi:cli/exit@0.2.6" (instance $exit (type $exit-instance)))
  (alias export $exit "exit" (func $exit-func))
  (core func $exit-lower (canon lower (func $exit-func)))

  (type $run-func (func (result $status)))
  (type $run-instance (instance (export "run" (func (type $run-func)))))
  (core module $run-core
    (import "" "exit" (func $exit (param i32)))
    (func (export "run") (result i32)
      i32.const {discriminant}
      call $exit
      unreachable
    )
  )
  (core instance $run-core-inst
    (instantiate $run-core
      (with "" (instance
        (export "exit" (func $exit-lower))
      ))
    )
  )
  (func $run (type $run-func)
    (canon lift (core func $run-core-inst "run")))

  (instance $run-export
    (export "run" (func $run))
  )
  (export "wasi:cli/run@0.2.6" (instance $run-export))
)
"#
        ),
    );
}

fn write_component(path: &PathBuf, wat: String) {
    let bytes = wat::parse_str(wat).expect("component wat must parse");
    fs::write(path, bytes).expect("component file should be written");
}

#[test]
fn legacy_core_wasm_invocation_still_works() {
    let output = Command::new(env!("CARGO_BIN_EXE_telomere-cli"))
        .args(["examples/add.wasm", "main", "1", "2"])
        .output()
        .expect("binary should run");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "3");
}

#[test]
fn component_subcommand_runs_wasi_cli_command() {
    let path = temp_component_path("ok");
    write_command_component(&path, 0);

    let output = Command::new(env!("CARGO_BIN_EXE_telomere-cli"))
        .args(["component", path.to_str().unwrap(), "--", "one", "two"])
        .output()
        .expect("binary should run");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());

    fs::remove_file(path).expect("temp component should be removed");
}

#[test]
fn component_subcommand_propagates_guest_failure_exit_code() {
    let path = temp_component_path("err");
    write_command_component(&path, 1);

    let output = Command::new(env!("CARGO_BIN_EXE_telomere-cli"))
        .args(["component", path.to_str().unwrap()])
        .output()
        .expect("binary should run");

    assert_eq!(output.status.code(), Some(1));

    fs::remove_file(path).expect("temp component should be removed");
}

#[test]
fn component_subcommand_maps_wasi_cli_exit_success_code() {
    let path = temp_component_path("exit-ok");
    write_exit_component(&path, 0);

    let output = Command::new(env!("CARGO_BIN_EXE_telomere-cli"))
        .args(["component", path.to_str().unwrap()])
        .output()
        .expect("binary should run");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    fs::remove_file(path).expect("temp component should be removed");
}

#[test]
fn component_subcommand_maps_wasi_cli_exit_failure_code() {
    let path = temp_component_path("exit-err");
    write_exit_component(&path, 1);

    let output = Command::new(env!("CARGO_BIN_EXE_telomere-cli"))
        .args(["component", path.to_str().unwrap()])
        .output()
        .expect("binary should run");

    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    fs::remove_file(path).expect("temp component should be removed");
}
