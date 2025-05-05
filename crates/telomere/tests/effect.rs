use std::path::PathBuf;

use common::run_wast;

mod common;
fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests/core-effect-test");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}
#[test]
fn effect_bulk() {
    run_test_file("effect-bulk");
}
