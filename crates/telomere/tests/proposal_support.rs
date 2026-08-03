use telomere::{IoReadBinaryReader, ProposalFeature, WasmParser, WasmParserError};

const SUPPORT_MATRIX: &str = include_str!("../../../docs/support-matrix.md");

fn parse_module(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    WasmParser::new(&mut reader).parse_module()
}

fn parse_error(bytes: &[u8]) -> WasmParserError {
    match parse_module(bytes) {
        Ok(_) => panic!("fixture must be rejected"),
        Err(error) => error,
    }
}

fn module_with_section(section_id: u8, contents: &[u8]) -> Vec<u8> {
    assert!(
        contents.len() < 128,
        "fixture section must use one-byte LEB size"
    );
    let mut module = vec![
        0x00,
        0x61,
        0x73,
        0x6d, // magic
        0x01,
        0x00,
        0x00,
        0x00, // version
        section_id,
        contents.len() as u8,
    ];
    module.extend_from_slice(contents);
    module
}

fn function_module(instructions: &[u8]) -> Vec<u8> {
    let mut body = vec![0x00]; // no locals
    body.extend_from_slice(instructions);
    body.push(0x0b); // end
    assert!(body.len() < 128, "fixture body must use one-byte LEB size");

    let mut module = vec![
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
        0x00, // one () -> () type
        0x03,
        0x02,
        0x01,
        0x00, // one function using type 0
        0x0a,
        (2 + body.len()) as u8,
        0x01,
        body.len() as u8,
    ];
    module.extend_from_slice(&body);
    module
}

fn encode_u32(mut value: u32) -> Vec<u8> {
    let mut encoded = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        encoded.push(byte);
        if value == 0 {
            return encoded;
        }
    }
}

fn prefixed_instruction_module(prefix: u8, subopcode: u32) -> Vec<u8> {
    let mut instruction = vec![prefix];
    instruction.extend(encode_u32(subopcode));
    function_module(&instruction)
}

fn assert_feature(bytes: &[u8], expected: ProposalFeature) {
    match parse_error(bytes) {
        WasmParserError::UnsupportedFeature { feature, .. } => assert_eq!(feature, expected),
        error => panic!("expected {expected:?}, got {error:?}"),
    }
}

#[test]
fn relaxed_simd_range_is_named_without_claiming_its_neighbors() {
    for subopcode in 0x100..=0x113 {
        assert_feature(
            &prefixed_instruction_module(0xfd, subopcode),
            ProposalFeature::RelaxedSimd,
        );
    }

    assert!(matches!(
        parse_error(&prefixed_instruction_module(0xfd, 0x114)),
        WasmParserError::InvalidInstruction(_)
    ));
}

#[test]
fn relaxed_simd_diagnostic_preserves_leb128_subopcode() {
    assert!(matches!(
        parse_error(&prefixed_instruction_module(0xfd, 0x100)),
        WasmParserError::UnsupportedFeature {
            feature: ProposalFeature::RelaxedSimd,
            opcode: [0xfd, 0x80, 0x02, 0x00],
        }
    ));
}

#[cfg(not(feature = "simd"))]
#[test]
fn no_default_standard_simd_mapping_preserves_wast_boundaries_and_holes() {
    for subopcode in [0x99, 0x9b, 0xe3, 0xef] {
        assert_feature(
            &prefixed_instruction_module(0xfd, subopcode),
            ProposalFeature::Simd,
        );
    }

    for subopcode in [0x9a, 0xe2, 0xee] {
        assert!(matches!(
            parse_error(&prefixed_instruction_module(0xfd, subopcode)),
            WasmParserError::InvalidInstruction(_)
        ));
    }
}

#[cfg(not(feature = "simd"))]
#[test]
fn no_default_unknown_simd_diagnostic_preserves_leb128_subopcode() {
    assert!(matches!(
        parse_error(&prefixed_instruction_module(0xfd, 0x114)),
        WasmParserError::InvalidInstruction([0xfd, 0x94, 0x02, 0x00])
    ));
}

#[test]
fn garbage_collection_mapping_is_limited_to_wast_assignments() {
    // wast 243.0.0 assigns struct, array, concrete-casting, conversion, and
    // i31 operations contiguously through 0x1e.
    for subopcode in 0x00..=0x1e {
        assert_feature(
            &prefixed_instruction_module(0xfb, subopcode),
            ProposalFeature::GarbageCollection,
        );
    }

    for subopcode in [0x1f, 0x20, 0x21, 0x22] {
        assert!(matches!(
            parse_error(&prefixed_instruction_module(0xfb, subopcode)),
            WasmParserError::InvalidInstruction(_)
        ));
    }
}

#[test]
fn ref_eq_is_gc_while_neighboring_function_reference_opcodes_are_not() {
    let ref_eq = parse_error(&function_module(&[0xd3]));
    assert!(matches!(
        ref_eq,
        WasmParserError::UnsupportedFeature {
            feature: ProposalFeature::GarbageCollection,
            ..
        }
    ));
    assert!(ref_eq
        .to_string()
        .starts_with("unsupported proposal feature 'gc' for opcode "));
    assert!(matches!(
        parse_error(&function_module(&[0xd4])),
        WasmParserError::InvalidInstruction(_)
    ));

    let ref_func = wat::parse_str(
        r#"
        (module
          (func $target)
          (elem declare func $target)
          (func (drop (ref.func $target)))
        )
        "#,
    )
    .expect("ref.func fixture must encode");
    parse_module(&ref_func).expect("ref.func remains supported and is not GC");
}

#[test]
fn exception_handling_opcodes_and_tag_section_are_named() {
    for opcode in [0x06, 0x07, 0x08, 0x09, 0x0a, 0x18, 0x19, 0x1f] {
        assert_feature(
            &function_module(&[opcode]),
            ProposalFeature::ExceptionHandling,
        );
    }
    assert_feature(
        &module_with_section(13, &[]),
        ProposalFeature::ExceptionHandling,
    );

    assert!(matches!(
        parse_error(&function_module(&[0x1e])),
        WasmParserError::InvalidInstruction(_)
    ));
}

#[test]
fn memory_proposal_flags_are_named_without_reclassifying_table64() {
    assert_feature(
        &module_with_section(5, &[0x01, 0x04, 0x01]),
        ProposalFeature::Memory64,
    );
    assert!(matches!(
        parse_error(&module_with_section(5, &[0x01, 0x06, 0x01])),
        WasmParserError::UnsupportedFeature {
            feature: ProposalFeature::Memory64,
            opcode: [0x06, 0x00, 0x00, 0x00],
        }
    ));
    assert_feature(
        &module_with_section(5, &[0x01, 0x08, 0x01, 0x10]),
        ProposalFeature::CustomPageSizes,
    );
    // The combined flag contains both proposals; page sizes deliberately win
    // the single-feature diagnostic so this result is deterministic.
    assert_feature(
        &module_with_section(5, &[0x01, 0x0c, 0x01, 0x10]),
        ProposalFeature::CustomPageSizes,
    );

    assert!(matches!(
        parse_error(&module_with_section(4, &[0x01, 0x70, 0x04, 0x01])),
        WasmParserError::InvalidLimit
    ));
    assert!(matches!(
        parse_error(&module_with_section(5, &[0x01, 0x14, 0x01])),
        WasmParserError::InvalidLimit
    ));
}

#[test]
fn wide_arithmetic_mapping_starts_at_subopcode_nineteen() {
    assert!(matches!(
        parse_error(&prefixed_instruction_module(0xfc, 18)),
        WasmParserError::InvalidInstruction(_)
    ));
    for subopcode in 19..=22 {
        assert_feature(
            &prefixed_instruction_module(0xfc, subopcode),
            ProposalFeature::WideArithmetic,
        );
    }
    assert!(matches!(
        parse_error(&prefixed_instruction_module(0xfc, 23)),
        WasmParserError::InvalidInstruction(_)
    ));
}

#[test]
fn extended_const_arithmetic_is_named_without_claiming_other_const_opcodes() {
    for opcode in [0x6a, 0x6b, 0x6c] {
        assert_feature(
            &global_with_const_opcode(opcode),
            ProposalFeature::ExtendedConst,
        );
    }
    for opcode in [0x7c, 0x7d, 0x7e] {
        assert_feature(
            &global_with_const_opcode(opcode),
            ProposalFeature::ExtendedConst,
        );
    }

    assert!(matches!(
        parse_error(&global_with_const_opcode(0x6d)),
        WasmParserError::InvalidConstInstruction(0x6d)
    ));
}

#[test]
fn support_matrix_lists_every_parser_proposal_identity() {
    let core_proposal_section = SUPPORT_MATRIX
        .split_once("## Core WebAssembly proposals")
        .expect("support matrix must have a core proposal section")
        .1
        .split_once("## Component Model and canonical ABI")
        .expect("core proposal section must end before component support")
        .0;

    for feature in [
        ProposalFeature::Simd,
        ProposalFeature::Threads,
        ProposalFeature::RelaxedSimd,
        ProposalFeature::GarbageCollection,
        ProposalFeature::ExceptionHandling,
        ProposalFeature::Memory64,
        ProposalFeature::CustomPageSizes,
        ProposalFeature::WideArithmetic,
        ProposalFeature::ExtendedConst,
    ] {
        let identity = feature.to_string();
        let token = format!("`{identity}`");
        assert!(
            core_proposal_section
                .lines()
                .filter(|line| line.trim_start().starts_with('|'))
                .any(|row| row.contains(&token)),
            "support matrix must include {token} in a core proposal table row"
        );
    }
}

fn global_with_const_opcode(opcode: u8) -> Vec<u8> {
    let (value_type, constant) = if opcode >= 0x7c {
        (0x7e, 0x42)
    } else {
        (0x7f, 0x41)
    };
    module_with_section(
        6,
        &[
            0x01, value_type, 0x00, constant, 0x00, constant, 0x00, opcode, 0x0b,
        ],
    )
}
