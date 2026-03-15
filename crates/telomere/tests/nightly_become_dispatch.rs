#![cfg(telomere_nightly)]

mod common;

use common::instantiate_wat;
use telomere::{run_module_function, Registry, ResultValue, Store, WasmValue};

const ITERATIONS: usize = 100_000;

fn loop_dispatch_module(iterations: usize) -> String {
    format!(
        r#"(module
  (func (export "run") (result i32) (local i32)
    i32.const {iterations}
    local.set 0
    (loop
      local.get 0
      i32.const 1
      i32.sub
      local.tee 0
      br_if 0
    )
    local.get 0
  )
)"#
    )
}

#[tokio::test]
async fn nightly_become_runs_long_dispatch_chain() {
    let mut store = Store::new();
    let registry = Registry::new();
    let wat = loop_dispatch_module(ITERATIONS);
    let instance = instantiate_wat(&wat, &mut store, &registry).await;

    let result = run_module_function(&instance, &mut store, "run", &ResultValue::new(vec![]))
        .await
        .unwrap();

    assert_eq!(
        result.iter().copied().collect::<Vec<_>>(),
        vec![WasmValue::I32(0)]
    );
}
