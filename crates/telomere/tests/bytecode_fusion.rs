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
