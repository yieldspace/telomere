use std::path::PathBuf;

use common::run_wast;

mod common;
async fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests/core-effect-test");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast).await;
}
#[tokio::test]
async fn effect_bulk() {
    run_test_file("effect-bulk").await;
}
