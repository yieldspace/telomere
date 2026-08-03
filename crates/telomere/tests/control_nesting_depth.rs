#![cfg(not(debug_assertions))]

use telomere::{IoReadBinaryReader, WasmParser, WasmParserError, MAX_CONTROL_NESTING_DEPTH};

const RELEASE_STACK_BUDGET_BYTES: usize = 512 * 1024;

fn encode_u32_leb128(mut value: u32, bytes: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        bytes.push(byte);
        if value == 0 {
            return;
        }
    }
}

fn nested_control_module(depth: u32, controls: &[u8]) -> Vec<u8> {
    assert!(
        !controls.is_empty(),
        "a nesting fixture must contain a control opcode"
    );

    let mut body = Vec::with_capacity(2 + depth as usize * 3);
    body.push(0x00); // no local declarations
    for index in 0..depth as usize {
        let control = controls[index % controls.len()];
        if control == 0x04 {
            body.extend([0x41, 0x00]); // i32.const 0, the if condition
        }
        body.extend([control, 0x40]); // control instruction with an empty block type
    }
    body.extend(std::iter::repeat_n(0x0b, depth as usize + 1)); // nested blocks and function body

    let mut code_section = Vec::with_capacity(body.len() + 6);
    code_section.push(0x01); // one function body
    encode_u32_leb128(body.len() as u32, &mut code_section);
    code_section.extend(body);

    let mut module = vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // type: () -> ()
        0x03, 0x02, 0x01, 0x00, // one function of type 0
        0x0a, // code section
    ];
    encode_u32_leb128(code_section.len() as u32, &mut module);
    module.extend(code_section);
    module
}

fn nested_block_module(depth: u32) -> Vec<u8> {
    nested_control_module(depth, &[0x02])
}

fn parse_module(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module()
}

#[test]
fn parser_accepts_control_nesting_at_the_limit() {
    let module = nested_block_module(MAX_CONTROL_NESTING_DEPTH);
    parse_module(&module).expect("512 nested blocks must parse");
}

#[test]
fn parser_rejects_control_nesting_above_the_limit() {
    let module = nested_block_module(MAX_CONTROL_NESTING_DEPTH + 1);
    let err = match parse_module(&module) {
        Ok(_) => panic!("513 nested blocks must be rejected"),
        Err(err) => err,
    };

    assert!(matches!(
        err,
        WasmParserError::NestingTooDeep {
            limit: MAX_CONTROL_NESTING_DEPTH
        }
    ));
}

#[test]
fn parser_enforces_the_limit_for_mixed_block_loop_and_if_nesting() {
    let controls = [0x02, 0x03, 0x04]; // block, loop, if
    let accepted = nested_control_module(MAX_CONTROL_NESTING_DEPTH, &controls);
    parse_module(&accepted).expect("512 mixed control constructs must parse");

    let rejected = nested_control_module(MAX_CONTROL_NESTING_DEPTH + 1, &controls);
    let err = match parse_module(&rejected) {
        Ok(_) => panic!("513 mixed control constructs must be rejected"),
        Err(err) => err,
    };
    assert!(matches!(
        err,
        WasmParserError::NestingTooDeep {
            limit: MAX_CONTROL_NESTING_DEPTH
        }
    ));
}

#[test]
fn parser_accepts_control_nesting_at_the_limit_on_the_release_stack_budget() {
    let module = nested_block_module(MAX_CONTROL_NESTING_DEPTH);
    std::thread::Builder::new()
        .stack_size(RELEASE_STACK_BUDGET_BYTES)
        // #154 leaves debug `parse_inst` frames around 130 KiB, so this release-only
        // test verifies the documented optimized-build stack budget instead.
        .spawn(move || parse_module(&module))
        .expect("spawning parser thread")
        .join()
        .expect("parser thread must not panic")
        .expect("512 nested blocks must fit the release stack budget");
}
