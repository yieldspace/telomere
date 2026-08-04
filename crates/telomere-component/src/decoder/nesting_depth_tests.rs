#![cfg(not(debug_assertions))]

use super::{parse_component, ComponentParseError, ParseState, Validator};
use crate::support::binary::IoReadBinaryReader;
use crate::MAX_COMPONENT_NESTING_DEPTH;
use telomere::{WasmParserError, MAX_CONTROL_NESTING_DEPTH};

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

fn append_section(component: &mut Vec<u8>, id: u8, payload: &[u8]) {
    component.push(id);
    encode_u32_leb128(payload.len() as u32, component);
    component.extend_from_slice(payload);
}

fn empty_component() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x0d, 0x00, 0x01, 0x00, // component version and layer
    ]
}

fn wrap_component(inner: &[u8]) -> Vec<u8> {
    let mut outer = empty_component();
    append_section(&mut outer, 0x04, inner);
    outer
}

fn nested_component_sections(depth: u32) -> Vec<u8> {
    let mut component = empty_component();
    for _ in 0..depth {
        component = wrap_component(&component);
    }
    component
}

fn nested_core_module(depth: u32) -> Vec<u8> {
    let mut body = Vec::with_capacity(2 + depth as usize * 3);
    body.push(0x00); // no local declarations
    for _ in 0..depth {
        body.extend([0x02, 0x40]); // block with an empty block type
    }
    body.extend(std::iter::repeat_n(0x0b, depth as usize + 1)); // nested blocks and function body

    let mut code_section = vec![0x01]; // one function body
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

fn component_with_core_module(component_depth: u32, core_depth: u32) -> Vec<u8> {
    let core_module = nested_core_module(core_depth);
    let mut component = empty_component();
    append_section(&mut component, 0x01, &core_module);
    for _ in 0..component_depth {
        component = wrap_component(&component);
    }
    component
}

fn decode_component(bytes: &[u8]) -> Result<(), ComponentParseError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    let state_arena = typed_arena::Arena::new();
    let mut state = ParseState::new(&state_arena);
    let validator_arena = typed_arena::Arena::new();
    let mut validator = Validator::new(&validator_arena);
    parse_component(&mut reader, &mut state, &mut validator)
}

#[test]
fn decoder_rejects_nested_component_sections_at_the_boundary() {
    let error = decode_component(&nested_component_sections(MAX_COMPONENT_NESTING_DEPTH + 1))
        .expect_err("101 nested sections must be rejected");

    assert!(matches!(
        error,
        ComponentParseError::NestingTooDeep {
            limit: MAX_COMPONENT_NESTING_DEPTH
        }
    ));
}

#[test]
fn core_nesting_error_remains_distinguishable_before_public_conversion() {
    let error = decode_component(&component_with_core_module(
        0,
        MAX_CONTROL_NESTING_DEPTH + 1,
    ))
    .expect_err("513 nested core blocks must fail inside a component");

    assert!(matches!(
        error,
        ComponentParseError::CoreWasmError(WasmParserError::NestingTooDeep {
            limit: MAX_CONTROL_NESTING_DEPTH
        })
    ));
}
