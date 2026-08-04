#![cfg(feature = "measure-switches")]

use std::{path::PathBuf, process::Command};

const OPTIMIZER_ENV: &str = "TELOMERE_OPTIMIZER";

fn cli_command(setting: Option<&str>) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_telomere-cli"));
    match setting {
        Some(value) => {
            command.env(OPTIMIZER_ENV, value);
        }
        None => {
            command.env_remove(OPTIMIZER_ENV);
        }
    }
    command
}

fn add_module_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/add.wasm")
}

#[test]
fn optimizer_switch_on_and_off_preserve_cli_results() {
    let on = cli_command(None)
        .arg(add_module_path())
        .args(["main", "20", "22"])
        .output()
        .expect("optimizer-on CLI invocation should run");
    let off = cli_command(Some("off"))
        .arg(add_module_path())
        .args(["main", "20", "22"])
        .output()
        .expect("optimizer-off CLI invocation should run");

    assert!(
        on.status.success(),
        "optimizer-on stderr={}",
        String::from_utf8_lossy(&on.stderr)
    );
    assert!(
        off.status.success(),
        "optimizer-off stderr={}",
        String::from_utf8_lossy(&off.stderr)
    );
    assert_eq!(on.stdout, off.stdout);
    assert_eq!(String::from_utf8_lossy(&on.stdout), "42\n");
}

#[test]
fn invalid_optimizer_value_exits_nonzero_before_module_execution() {
    let output = cli_command(Some("not-off"))
        .arg(add_module_path())
        .args(["main", "20", "22"])
        .output()
        .expect("CLI invocation with an invalid optimizer setting should run");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(OPTIMIZER_ENV), "stderr={stderr}");
    assert!(stderr.contains("not-off"), "stderr={stderr}");
    assert!(stderr.contains("accepted values"), "stderr={stderr}");
    assert!(stderr.contains("unset"), "stderr={stderr}");
    assert!(stderr.contains("off"), "stderr={stderr}");
}

#[test]
fn probe_reports_the_effective_optimizer_state_as_json() {
    let on = cli_command(None)
        .arg("measure-switches-probe")
        .output()
        .expect("optimizer-on probe should run");
    let off = cli_command(Some("off"))
        .arg("measure-switches-probe")
        .output()
        .expect("optimizer-off probe should run");

    assert!(
        on.status.success(),
        "optimizer-on stderr={}",
        String::from_utf8_lossy(&on.stderr)
    );
    assert!(
        off.status.success(),
        "optimizer-off stderr={}",
        String::from_utf8_lossy(&off.stderr)
    );
    assert_eq!(on.stdout, b"{\"state\":\"on\"}\n");
    assert_eq!(off.stdout, b"{\"state\":\"off\"}\n");
}
