#![cfg(feature = "threads")]

mod common;

use common::instantiate_wat;
use telomere::{
    component_support::common::module_memories, instantiate, run_module_function,
    IoReadBinaryReader, Registry, ResultValue, Store, VMResult, WasmParser, WasmParserError,
    WasmValue,
};

fn parse_module_bytes(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module()
}

fn parse_module(wat: &str) -> telomere::Module {
    let source = wat::parse_str(wat).expect("wat must parse");
    parse_module_bytes(&source).expect("module must parse")
}

#[test]
fn parser_rejects_shared_memory_without_maximum() {
    let bytes = [
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
        0x05, // memory section
        0x03, // section size
        0x01, // memory count
        0x02, // shared flag without max
        0x01, // min
    ];
    let err = match parse_module_bytes(&bytes) {
        Ok(_) => panic!("shared memory without max must fail"),
        Err(err) => err,
    };
    assert!(matches!(err, WasmParserError::InvalidLimit));
}

#[test]
fn parser_accepts_shared_memory_with_maximum() {
    let module = parse_module("(module (memory 1 2 shared))");
    let memories = module_memories(&module);
    assert_eq!(memories.len(), 1);
    assert!(memories[0].shared);
    assert_eq!(memories[0].limits.min, 1);
    assert_eq!(memories[0].limits.max, Some(2));
}

#[tokio::test]
async fn shared_memory_import_mismatch_is_unlinkable() {
    let store = Store::new();
    let mut registry = Registry::new();
    let exporter = instantiate_wat(
        r#"
        (module
          (memory (export "mem") 1 2 shared))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", exporter);

    let result = instantiate(
        parse_module(
            r#"
            (module
              (import "host" "mem" (memory 1 2)))
            "#,
        ),
        &store,
        &registry,
    )
    .await;
    assert!(matches!(result, VMResult::Unlinkable));
}

#[tokio::test]
async fn shared_memory_import_export_store_load_and_grow_roundtrip() {
    let store = Store::new();
    let mut registry = Registry::new();

    let exporter = instantiate_wat(
        r#"
        (module
          (memory (export "mem") 1 2 shared)
          (func (export "size") (result i32)
            memory.size)
          (func (export "load0") (result i32)
            i32.const 0
            i32.load))
        "#,
        &store,
        &registry,
    )
    .await;
    registry.register("host", exporter.clone());

    let importer = instantiate_wat(
        r#"
        (module
          (import "host" "mem" (memory 1 2 shared))
          (func (export "size") (result i32)
            memory.size)
          (func (export "grow") (param i32) (result i32)
            local.get 0
            memory.grow)
          (func (export "store0") (param i32)
            i32.const 0
            local.get 0
            i32.store))
        "#,
        &store,
        &registry,
    )
    .await;

    assert_eq!(
        run_module_function(&exporter, &store, "size", &ResultValue::new(vec![]))
            .await
            .unwrap(),
        ResultValue::new(vec![WasmValue::I32(1)])
    );
    assert_eq!(
        run_module_function(
            &importer,
            &store,
            "store0",
            &ResultValue::new(vec![WasmValue::I32(42)])
        )
        .await
        .unwrap(),
        ResultValue::new(vec![])
    );
    assert_eq!(
        run_module_function(&exporter, &store, "load0", &ResultValue::new(vec![]))
            .await
            .unwrap(),
        ResultValue::new(vec![WasmValue::I32(42)])
    );
    assert_eq!(
        run_module_function(
            &importer,
            &store,
            "grow",
            &ResultValue::new(vec![WasmValue::I32(1)])
        )
        .await
        .unwrap(),
        ResultValue::new(vec![WasmValue::I32(1)])
    );
    assert_eq!(
        run_module_function(&exporter, &store, "size", &ResultValue::new(vec![]))
            .await
            .unwrap(),
        ResultValue::new(vec![WasmValue::I32(2)])
    );
    assert_eq!(
        run_module_function(&importer, &store, "size", &ResultValue::new(vec![]))
            .await
            .unwrap(),
        ResultValue::new(vec![WasmValue::I32(2)])
    );
}
