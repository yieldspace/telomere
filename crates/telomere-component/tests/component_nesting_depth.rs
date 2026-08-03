#![cfg(not(debug_assertions))]

use telomere::{IoReadBinaryReader, WasmParserError, MAX_CONTROL_NESTING_DEPTH};
use telomere_component::{
    decoder::{parse_component, ComponentParseError, ParseState, Validator},
    ComponentEngine, ComponentError, MAX_COMPONENT_NESTING_DEPTH,
};

const RELEASE_STACK_BUDGET_BYTES: usize = 512 * 1024;
const COMPONENT_TYPE_OPCODE: u8 = 0x41;
const INSTANCE_TYPE_OPCODE: u8 = 0x42;

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

fn nested_type(depth: u32, opcode: u8) -> Vec<u8> {
    assert!(depth > 0, "a nesting fixture must contain a type");

    let mut ty = vec![opcode, 0x00]; // empty component or instance type
    for _ in 1..depth {
        let mut outer = vec![opcode, 0x01, 0x01]; // one nested type declaration
        outer.extend(ty);
        ty = outer;
    }
    ty
}

fn mixed_type(depth: u32) -> Vec<u8> {
    assert!(depth > 0, "a nesting fixture must contain a type");

    let mut opcodes = (0..depth)
        .map(|index| {
            if index % 2 == 0 {
                COMPONENT_TYPE_OPCODE
            } else {
                INSTANCE_TYPE_OPCODE
            }
        })
        .collect::<Vec<_>>();
    let innermost = opcodes.pop().expect("the fixture has at least one type");
    let mut ty = vec![innermost, 0x00];
    for opcode in opcodes.into_iter().rev() {
        let mut outer = vec![opcode, 0x01, 0x01]; // one nested type declaration
        outer.extend(ty);
        ty = outer;
    }
    ty
}

fn component_with_type(ty: Vec<u8>) -> Vec<u8> {
    let mut component = empty_component();
    let mut type_section = vec![0x01]; // one component type definition
    type_section.extend(ty);
    append_section(&mut component, 0x07, &type_section);
    component
}

fn component_with_mixed_nesting(section_depth: u32, type_depth: u32) -> Vec<u8> {
    let mut component = component_with_type(mixed_type(type_depth));
    for _ in 0..section_depth {
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

fn expect_public_nesting_error(error: ComponentError, limit: u32) {
    assert!(
        matches!(error, ComponentError::NestingTooDeep { limit: actual } if actual == limit),
        "unexpected error: {error}"
    );
}

#[test]
fn engine_accepts_and_rejects_nested_component_sections_at_the_boundary() {
    ComponentEngine::new()
        .compile(&nested_component_sections(MAX_COMPONENT_NESTING_DEPTH))
        .expect("100 nested component sections must compile");

    let bytes = nested_component_sections(MAX_COMPONENT_NESTING_DEPTH + 1);
    let internal = decode_component(&bytes).expect_err("101 nested sections must be rejected");
    assert!(matches!(
        internal,
        ComponentParseError::NestingTooDeep {
            limit: MAX_COMPONENT_NESTING_DEPTH
        }
    ));

    let public = ComponentEngine::new()
        .compile(&bytes)
        .expect_err("101 nested component sections must fail through the public API");
    expect_public_nesting_error(public, MAX_COMPONENT_NESTING_DEPTH);
}

#[test]
fn engine_enforces_component_type_nesting_boundary() {
    ComponentEngine::new()
        .compile(&component_with_type(nested_type(
            MAX_COMPONENT_NESTING_DEPTH,
            COMPONENT_TYPE_OPCODE,
        )))
        .expect("100 nested component types must compile");

    let error = ComponentEngine::new()
        .compile(&component_with_type(nested_type(
            MAX_COMPONENT_NESTING_DEPTH + 1,
            COMPONENT_TYPE_OPCODE,
        )))
        .expect_err("101 nested component types must fail");
    expect_public_nesting_error(error, MAX_COMPONENT_NESTING_DEPTH);
}

#[test]
fn engine_enforces_instance_type_nesting_boundary() {
    ComponentEngine::new()
        .compile(&component_with_type(nested_type(
            MAX_COMPONENT_NESTING_DEPTH,
            INSTANCE_TYPE_OPCODE,
        )))
        .expect("100 nested instance types must compile");

    let error = ComponentEngine::new()
        .compile(&component_with_type(nested_type(
            MAX_COMPONENT_NESTING_DEPTH + 1,
            INSTANCE_TYPE_OPCODE,
        )))
        .expect_err("101 nested instance types must fail");
    expect_public_nesting_error(error, MAX_COMPONENT_NESTING_DEPTH);
}

#[test]
fn engine_accepts_component_and_instance_type_limits_within_the_release_stack_budget() {
    let component_type = component_with_type(nested_type(
        MAX_COMPONENT_NESTING_DEPTH,
        COMPONENT_TYPE_OPCODE,
    ));
    let instance_type = component_with_type(nested_type(
        MAX_COMPONENT_NESTING_DEPTH,
        INSTANCE_TYPE_OPCODE,
    ));

    std::thread::Builder::new()
        .stack_size(RELEASE_STACK_BUDGET_BYTES)
        .spawn(move || {
            ComponentEngine::new()
                .compile(&component_type)
                .expect("100 nested component types must fit the release stack budget");
            ComponentEngine::new()
                .compile(&instance_type)
                .expect("100 nested instance types must fit the release stack budget");
        })
        .expect("spawning component type compiler thread")
        .join()
        .expect("component type compiler thread must not panic");
}

#[test]
fn engine_counts_component_sections_and_mixed_types_against_one_budget() {
    ComponentEngine::new()
        .compile(&component_with_mixed_nesting(50, 50))
        .expect("50 nested sections plus 50 mixed types must compile");

    let error = ComponentEngine::new()
        .compile(&component_with_mixed_nesting(50, 51))
        .expect_err("50 nested sections plus 51 mixed types must fail");
    expect_public_nesting_error(error, MAX_COMPONENT_NESTING_DEPTH);
}

#[test]
fn engine_composes_component_and_core_limits_within_the_release_stack_budget() {
    let accepted =
        component_with_core_module(MAX_COMPONENT_NESTING_DEPTH, MAX_CONTROL_NESTING_DEPTH);
    std::thread::Builder::new()
        .stack_size(RELEASE_STACK_BUDGET_BYTES)
        // #154 leaves debug `parse_inst` frames around 130 KiB, so this release-only
        // test verifies the documented optimized-build combined stack budget instead.
        .spawn(move || ComponentEngine::new().compile(&accepted))
        .expect("spawning component compiler thread")
        .join()
        .expect("component compiler thread must not panic")
        .expect("100 component sections plus 512 core blocks must fit the release stack budget");

    let error = ComponentEngine::new()
        .compile(&component_with_core_module(
            MAX_COMPONENT_NESTING_DEPTH,
            MAX_CONTROL_NESTING_DEPTH + 1,
        ))
        .expect_err("513 nested core blocks must fail inside a component");
    expect_public_nesting_error(error, MAX_CONTROL_NESTING_DEPTH);
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
