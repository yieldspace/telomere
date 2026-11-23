//! Regression tests for issue #279: an active element segment of kind 6
//! (explicit table index, expression items) must be validated against the
//! table it declares, not against table 0.

use telomere::{IoReadBinaryReader, WasmParser, WasmParserError};

const FUNCREF: u8 = 0x70;
const EXTERNREF: u8 = 0x6f;

/// Builds a module with two one-element tables — table 0 of reftype
/// `table0_rt`, table 1 of reftype `table1_rt` — and a single kind-6 active
/// element segment targeting `tableidx` with element type `seg_rt` and one
/// `ref.null` item of that type.
fn two_table_kind6_module(table0_rt: u8, table1_rt: u8, tableidx: u8, seg_rt: u8) -> Vec<u8> {
    let mut bytes = vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ];
    // Table section (id 4): two tables, each limits {flags 0, min 1}.
    bytes.extend_from_slice(&[
        0x04, 0x07, 0x02, table0_rt, 0x00, 0x01, table1_rt, 0x00, 0x01,
    ]);
    // Element section (id 9): one kind-6 segment.
    bytes.extend_from_slice(&[
        0x09, 0x0b, 0x01,     // section id, size, segment count
        0x06,     // kind 6: active, explicit table index, expression items
        tableidx, // declared table index
        0x41, 0x00, 0x0b,   // offset: i32.const 0, end
        seg_rt, // segment element type
        0x01,   // one item
        0xd0, seg_rt, 0x0b, // ref.null <seg_rt>, end
    ]);
    bytes
}

fn parse_module_bytes(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    let mut parser = WasmParser::new(&mut reader);
    parser.parse_module()
}

/// The segment declares externref items for table 1, which is a funcref
/// table: validation must reject it. Checking table 0 (externref) instead
/// wrongly accepts it.
#[test]
fn kind6_elem_validated_against_declared_table_rejects_type_mismatch() {
    let bytes = two_table_kind6_module(EXTERNREF, FUNCREF, 1, EXTERNREF);
    let err = match parse_module_bytes(&bytes) {
        Ok(_) => panic!("segment type does not match the declared table and must be rejected"),
        Err(err) => err,
    };
    assert!(
        matches!(err, WasmParserError::InvalidStackValType(..)),
        "expected a reftype mismatch error, got {err:?}"
    );
}

/// The segment declares funcref items for table 1, which is a funcref
/// table: validation must accept it. Checking table 0 (externref) instead
/// wrongly rejects it.
#[test]
fn kind6_elem_validated_against_declared_table_accepts_matching_type() {
    let bytes = two_table_kind6_module(EXTERNREF, FUNCREF, 1, FUNCREF);
    parse_module_bytes(&bytes)
        .expect("segment type matches the declared table and must be accepted");
}

/// Control: a kind-6 segment targeting table 0 is validated against
/// table 0, as before.
#[test]
fn kind6_elem_targeting_table_zero_still_validated_against_table_zero() {
    let ok = two_table_kind6_module(EXTERNREF, FUNCREF, 0, EXTERNREF);
    parse_module_bytes(&ok).expect("table 0 segment with matching type must be accepted");

    let bad = two_table_kind6_module(EXTERNREF, FUNCREF, 0, FUNCREF);
    assert!(
        parse_module_bytes(&bad).is_err(),
        "table 0 segment with mismatched type must be rejected"
    );
}
