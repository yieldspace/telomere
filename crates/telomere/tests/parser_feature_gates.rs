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

#[cfg(not(feature = "threads"))]
fn parse_unshared_memory_atomic_module(
    instructions: &[u8],
) -> Result<telomere::Module, WasmParserError> {
    let body_len = 1 + instructions.len(); // local declaration count plus instructions
    let code_section_len = 2 + body_len; // function count and body length, both one-byte LEBs
    assert!(body_len < 128, "fixture body must use one-byte LEB lengths");

    let mut bytes = vec![
        0x00,
        0x61,
        0x73,
        0x6d, // magic
        0x01,
        0x00,
        0x00,
        0x00, // version
        0x01,
        0x04,
        0x01,
        0x60,
        0x00,
        0x00, // type: () -> ()
        0x03,
        0x02,
        0x01,
        0x00, // one function of type 0
        0x05,
        0x03,
        0x01,
        0x00,
        0x01, // one unshared memory with min 1
        0x0a,
        code_section_len as u8,
        0x01,
        body_len as u8,
        0x00, // no local declarations
    ];
    bytes.extend_from_slice(instructions);
    parse_module_bytes(&bytes)
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

#[cfg(not(feature = "threads"))]
#[test]
fn parser_reports_unsupported_threads_for_memory_atomic_notify() {
    let err = match parse_unshared_memory_atomic_module(&[
        0x41, 0x00, // i32.const 0 (address)
        0x41, 0x00, // i32.const 0 (wake count)
        0xfe, 0x00, 0x02, 0x00, // memory.atomic.notify, align=4, offset=0
        0x1a, // drop result
        0x0b, // end
    ]) {
        Ok(_) => panic!("threads-off parser must reject memory.atomic.notify"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        WasmParserError::UnsupportedFeature {
            feature: telomere::ProposalFeature::Threads,
            opcode: [0xfe, 0, 0, 0],
        }
    ));
}

#[cfg(not(feature = "threads"))]
#[test]
fn parser_reports_unsupported_threads_for_memory_atomic_wait32() {
    let err = match parse_unshared_memory_atomic_module(&[
        0x41, 0x00, // i32.const 0 (address)
        0x41, 0x00, // i32.const 0 (expected value)
        0x42, 0x00, // i64.const 0 (timeout)
        0xfe, 0x01, 0x02, 0x00, // memory.atomic.wait32, align=4, offset=0
        0x1a, // drop result
        0x0b, // end
    ]) {
        Ok(_) => panic!("threads-off parser must reject memory.atomic.wait32"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        WasmParserError::UnsupportedFeature {
            feature: telomere::ProposalFeature::Threads,
            opcode: [0xfe, 0, 0, 0],
        }
    ));
}
