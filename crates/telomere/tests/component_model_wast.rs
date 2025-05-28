use std::path::PathBuf;

mod common;
async fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests/component_model_testsuite");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    common::component_model::run_component_wast(&wast).await;
}

#[tokio::test]
async fn component_basic() {
    run_test_file("basic").await;
}

#[tokio::test]
async fn component_import() {
    run_test_file("import").await;
}
#[tokio::test]
async fn component_export() {
    run_test_file("export").await;
}
#[tokio::test]
async fn component_variant() {
    run_test_file("variant").await;
}

#[tokio::test]
async fn component_valtype() {
    run_test_file("valtype").await;
}

#[tokio::test]
async fn component_resource() {
    run_test_file("resource").await;
}

#[tokio::test]
async fn component_instance_type() {
    run_test_file("instancetype").await;
}

#[tokio::test]
async fn component_instance() {
    run_test_file("instance").await;
}

#[tokio::test]
async fn component_core() {
    run_test_file("core").await;
}
#[test]
fn component_subtyping() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    run_test_file("subtyping");
}
