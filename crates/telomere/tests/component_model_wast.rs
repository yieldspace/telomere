use std::path::PathBuf;

use tracing::Level;
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
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    run_test_file("import");
}

#[test]
fn component_variant() {
    run_test_file("variant");
}

#[test]
fn component_valtype() {
    run_test_file("valtype");
}

#[test]
fn component_resource() {
    run_test_file("resource");
}

#[test]
fn component_instance_type() {
    run_test_file("instancetype");
}

#[test]
fn component_instance() {
    run_test_file("instance");
}

#[test]
fn component_core() {
    run_test_file("core");
}
