use telomere::{
    common::FunctionBody, instantiate, run_module_function, IoReadBinaryReader, Registry,
    ResultValue, Store, WasmParser, WasmValue,
};

fn parse_module(wat: &str) -> telomere::Module {
    let source = wat::parse_str(wat).expect("wat must parse");
    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module().expect("module must parse")
}

fn first_expr_len(wat: &str) -> usize {
    let module = parse_module(wat);
    let body = module.codes.0.first().expect("must have one function body");
    let FunctionBody::Wasm(func) = body else {
        panic!("expected wasm function body");
    };
    func.expr.len()
}

async fn run_wat(wat: &str, export: &str, params: Vec<WasmValue>) -> ResultValue {
    let store = Store::new();
    let registry = Registry::new();
    let instance = instantiate(parse_module(wat), &store, &registry)
        .await
        .unwrap();
    run_module_function(&instance, &store, export, &ResultValue::new(params))
        .await
        .unwrap()
}

#[test]
fn parser_reduces_cells_for_const_set_and_rmw() {
    let const_set = r#"
        (module
          (func (export "f") (local i32)
            i32.const 7
            local.set 0))
    "#;
    let rmw = r#"
        (module
          (func (export "f") (local i32)
            local.get 0
            i32.const 1
            i32.add
            local.set 0))
    "#;
    let tee_drop = r#"
        (module
          (func (export "f") (local i32)
            i32.const 1
            local.tee 0
            drop))
    "#;

    assert_eq!(first_expr_len(const_set), 5);
    assert_eq!(first_expr_len(rmw), 5);
    assert_eq!(first_expr_len(tee_drop), 5);
}

#[test]
fn parser_keeps_non_fusible_div_sequence() {
    let wat = r#"
        (module
          (func (export "f") (local i32)
            local.get 0
            i32.const 2
            i32.div_s
            local.set 0))
    "#;

    assert_eq!(first_expr_len(wat), 10);
}

#[test]
fn parser_reduces_cells_for_i64_and_float_rmw() {
    let i64_rmw = r#"
        (module
          (func (export "f") (local i64)
            local.get 0
            i64.const 1
            i64.add
            local.set 0))
    "#;
    let f32_rmw = r#"
        (module
          (func (export "f") (local f32)
            local.get 0
            f32.const 1
            f32.add
            local.set 0))
    "#;
    let f64_rmw = r#"
        (module
          (func (export "f") (local f64)
            local.get 0
            f64.const 1
            f64.add
            local.set 0))
    "#;

    assert!(first_expr_len(i64_rmw) < 9);
    assert!(first_expr_len(f32_rmw) < 9);
    assert!(first_expr_len(f64_rmw) < 9);
}

#[test]
fn parser_reduces_cells_for_compare_values() {
    let i64_cmp = r#"
        (module
          (func (export "f") (param i64) (result i32)
            local.get 0
            i64.const 7
            i64.lt_s
            return
            i32.const 0))
    "#;
    let f32_cmp = r#"
        (module
          (func (export "f") (param f32) (result i32)
            local.get 0
            f32.const 1
            f32.ge
            return
            i32.const 0))
    "#;
    let f64_cmp = r#"
        (module
          (func (export "f") (param f64) (result i32)
            local.get 0
            f64.const 3
            f64.ne
            return
            i32.const 0))
    "#;

    assert!(first_expr_len(i64_cmp) < 9);
    assert!(first_expr_len(f32_cmp) < 9);
    assert!(first_expr_len(f64_cmp) < 9);
}

#[test]
fn parser_restarts_fusion_inside_nested_blocks_after_unsupported_ops() {
    let fused = r#"
        (module
          (func (export "f") (param i32) (result i32) (local i32)
            local.get 0
            i32.const 2
            i32.div_s
            drop
            block
              local.get 0
              i32.const 1
              i32.add
              local.set 1
            end
            local.get 1))
    "#;
    let raw = r#"
        (module
          (func (export "f") (param i32) (result i32) (local i32)
            local.get 0
            i32.const 2
            i32.div_s
            drop
            block
              local.get 0
              i32.const 1
              i32.div_s
              local.set 1
            end
            local.get 1))
    "#;

    assert!(first_expr_len(fused) < first_expr_len(raw));
}

#[tokio::test]
async fn fused_sequences_preserve_runtime_results() {
    let wat = r#"
        (module
          (func (export "f") (param i32) (result i32) (local i32)
            local.get 0
            i32.const 5
            i32.add
            local.set 1
            local.get 1))
    "#;

    let result = run_wat(wat, "f", vec![WasmValue::I32(7)]).await;
    assert_eq!(result, ResultValue::new(vec![WasmValue::I32(12)]));
}

#[tokio::test]
async fn scalar_and_memory_fusion_preserve_runtime_results() {
    let scalar = r#"
        (module
          (func (export "f") (result f64) (local f64)
            f64.const 3
            local.set 0
            local.get 0
            f64.const 2
            f64.add
            return
            f64.const 0))
    "#;
    let memory = r#"
        (module
          (memory 1)
          (func (export "f") (param i32) (result i32)
            local.get 0
            i32.const 8
            i32.add
            i32.const 42
            i32.store
            local.get 0
            i32.const 8
            i32.add
            i32.load))
    "#;

    let scalar_result = run_wat(scalar, "f", vec![]).await;
    let memory_result = run_wat(memory, "f", vec![WasmValue::I32(0)]).await;

    assert_eq!(scalar_result, ResultValue::new(vec![WasmValue::F64(5.0)]));
    assert_eq!(memory_result, ResultValue::new(vec![WasmValue::I32(42)]));
}

#[tokio::test]
async fn compare_fusion_preserves_runtime_results() {
    let i64_cmp = r#"
        (module
          (func (export "f") (param i64) (result i32)
            local.get 0
            i64.const 7
            i64.lt_s
            return
            i32.const 0))
    "#;
    let f32_cmp = r#"
        (module
          (func (export "f") (param f32) (result i32)
            local.get 0
            f32.const 1
            f32.ge
            return
            i32.const 0))
    "#;
    let f64_cmp = r#"
        (module
          (func (export "f") (param f64) (result i32)
            local.get 0
            f64.const 3
            f64.ne
            return
            i32.const 0))
    "#;

    let i64_result = run_wat(i64_cmp, "f", vec![WasmValue::I64(5)]).await;
    let f32_result = run_wat(f32_cmp, "f", vec![WasmValue::F32(1.0)]).await;
    let f64_result = run_wat(f64_cmp, "f", vec![WasmValue::F64(2.0)]).await;

    assert_eq!(i64_result, ResultValue::new(vec![WasmValue::I32(1)]));
    assert_eq!(f32_result, ResultValue::new(vec![WasmValue::I32(1)]));
    assert_eq!(f64_result, ResultValue::new(vec![WasmValue::I32(1)]));
}

#[tokio::test]
async fn fusion_does_not_cross_if_boundaries() {
    let wat = r#"
        (module
          (func (export "f") (param i32 i32) (result i32)
            local.get 0
            if (result i32)
              local.get 1
              i32.const 1
              i32.add
            else
              i32.const 0
            end))
    "#;

    let taken = run_wat(wat, "f", vec![WasmValue::I32(1), WasmValue::I32(41)]).await;
    let not_taken = run_wat(wat, "f", vec![WasmValue::I32(0), WasmValue::I32(41)]).await;

    assert_eq!(taken, ResultValue::new(vec![WasmValue::I32(42)]));
    assert_eq!(not_taken, ResultValue::new(vec![WasmValue::I32(0)]));
}
