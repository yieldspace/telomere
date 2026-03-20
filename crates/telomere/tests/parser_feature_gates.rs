#![cfg(any(not(feature = "simd"), not(feature = "threads")))]

use telomere::{IoReadBinaryReader, WasmParser, WasmParserError};

fn parse_module_bytes(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module()
}

fn parse_module_err(wat: &str) -> WasmParserError {
    let source = wat::parse_str(wat).expect("wat must parse");
    match parse_module_bytes(&source) {
        Ok(_) => panic!("module must fail to parse"),
        Err(err) => err,
    }
}

#[cfg(not(feature = "simd"))]
#[test]
fn parser_reports_unsupported_simd_opcode() {
    let err = parse_module_err(
        r#"
        (module
          (func
            (drop (v128.const i32x4 0 0 0 0))))
        "#,
    );
    assert!(matches!(
        err,
        WasmParserError::UnsupportedFeature {
            feature: telomere::ProposalFeature::Simd,
            opcode: [0xFD, 0, 0, 0],
        }
    ));
}

#[cfg(not(feature = "threads"))]
#[test]
fn parser_reports_unsupported_threads_for_shared_memory() {
    let err = parse_module_err("(module (memory 1 2 shared))");
    assert!(matches!(
        err,
        WasmParserError::UnsupportedFeature {
            feature: telomere::ProposalFeature::Threads,
            ..
        }
    ));
}
