use std::path::PathBuf;
mod common;
fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests/component_model_testsuite");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    common::component_model::run_component_wast(&wast);
}

#[test]
fn component_basic() {
    run_test_file("basic");
}

#[test]
fn component_import() {
    run_test_file("import");
}
