//! Regression tests for #358: the locals counts declared in a function body
//! are attacker-controlled multiplicities (a single LEB u32 per group), so
//! summing them must use checked arithmetic and surface
//! `WasmParserError::TooManyLocals` instead of overflowing — a panic in
//! overflow-checked builds and a silent wrap otherwise.

use telomere::{IoReadBinaryReader, WasmParser, WasmParserError};

fn parse_module(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    WasmParser::new(&mut reader).parse_module()
}

fn assert_too_many_locals(result: Result<telomere::Module, WasmParserError>) {
    match result {
        Err(WasmParserError::TooManyLocals) => {}
        Err(e) => panic!("expected TooManyLocals, got error {e:?}"),
        Ok(_) => panic!("expected TooManyLocals, got Ok"),
    }
}

fn leb_u32(mut value: u32) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (value & 0x7f) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            return out;
        }
    }
}

/// A module with one `() -> ()` function whose body declares `groups`
/// (count, valtype byte) as its locals and immediately ends.
fn module_with_locals(groups: &[(u32, u8)]) -> Vec<u8> {
    let mut module = vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ];
    // Type section: one func type () -> ().
    module.extend_from_slice(&[0x01, 0x04, 0x01, 0x60, 0x00, 0x00]);
    // Function section: one function of type 0.
    module.extend_from_slice(&[0x03, 0x02, 0x01, 0x00]);
    // Code section with a single body.
    let mut body = Vec::new();
    body.extend_from_slice(&leb_u32(groups.len() as u32));
    for &(n, t) in groups {
        body.extend_from_slice(&leb_u32(n));
        body.push(t);
    }
    body.push(0x0b); // end
    let mut code = vec![0x01]; // one body
    code.extend_from_slice(&leb_u32(body.len() as u32));
    code.extend_from_slice(&body);
    module.push(0x0a);
    module.extend_from_slice(&leb_u32(code.len() as u32));
    module.extend_from_slice(&code);
    module
}

const I32: u8 = 0x7f;
const F32: u8 = 0x7d;

#[test]
fn fuzz_crash_input_is_rejected_not_a_panic() {
    // The exact input from #358 (CI artifact
    // fuzz-artifacts-smoke-parse_core_module-45600876940): two i32 locals
    // groups whose counts sum past u32::MAX.
    let input: [u8; 32] = [
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x03,
        0x02, 0x01, 0x00, 0x0a, 0x0c, 0x01, 0x0a, 0x02, 0xff, 0xff, 0xff, 0xff, 0x0f, 0x7f, 0x02,
        0x7f, 0x10,
    ];
    assert_too_many_locals(parse_module(&input));
}

#[test]
fn same_type_group_counts_cannot_overflow() {
    // Same shape as the crash input but with a well-formed body end.
    let module = module_with_locals(&[(u32::MAX, I32), (2, I32)]);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_f32_offset_sum_cannot_overflow() {
    // The reassignment table sums the *total* i32 count with the *running*
    // f32 count. Keeping the cumulative index small (the huge i32 group is
    // declared last) while the running f32 count jumps in one group makes
    // that sum overflow even though every checked counter stays in range.
    let module = module_with_locals(&[(1, F32), (0xc000_000f, F32), (0, F32), (0x3fff_fff0, I32)]);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_funcref_offset_sum_cannot_overflow() {
    let module = module_with_locals(&[
        (1, 0x70),
        (0xc000_000f, 0x70),
        (0, 0x70),
        (0x3fff_fff0, I32),
    ]);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_externref_offset_sum_cannot_overflow() {
    let module = module_with_locals(&[
        (1, 0x6f),
        (0xc000_000f, 0x6f),
        (0, 0x6f),
        (0x3fff_fff0, I32),
    ]);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_i64_offset_add_cannot_overflow() {
    // The i64 arm adds the 4-byte classes' byte size to the running i64
    // byte size; both products fit in u32 while their sum does not.
    let mut groups = vec![(1, 0x7e); 10];
    groups.push((0x3fff_fff0, I32));
    let module = module_with_locals(&groups);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_f64_inner_sum_cannot_overflow() {
    // The f64 arm sums the *total* i64 count with the *running* f64 count
    // before scaling by 8.
    let module = module_with_locals(&[(1, 0x7c), (0xffff_ff00, 0x7c), (1, 0x7c), (0x100, 0x7e)]);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn reassignment_v128_offset_add_cannot_overflow() {
    let mut groups = vec![(1, 0x7b); 10];
    groups.push((0x1fff_fff0, 0x7e));
    let module = module_with_locals(&groups);
    assert_too_many_locals(parse_module(&module));
}

#[test]
fn ordinary_locals_still_parse() {
    let module = module_with_locals(&[(3, I32), (2, 0x7c), (1, 0x7e)]);
    let result = parse_module(&module);
    assert!(result.is_ok(), "expected Ok, got an error");
}
