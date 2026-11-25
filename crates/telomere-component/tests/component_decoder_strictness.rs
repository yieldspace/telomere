//! Regression tests for #319: the component decoder accepted malformed
//! binaries (truncated sections, section contents read past their
//! declared size, invalid UTF-8 in a custom-section name) and wrapped
//! core-layer decode errors without the specific reason, surfacing only
//! `error from underlying layer` / `invalid leb128 encoding` where the
//! spec's error text names `unexpected end-of-file` / `integer too
//! large`.
//!
//! The byte strings mirror the `assert_malformed` cases in the vendored
//! upstream `binary/binary.wast` (lines 43, 51, 84, 91, 98, 105, 149,
//! 157, 166 and the import-name overrun at line 1445).

use telomere_component::ComponentEngine;

/// `\0asm` + version `0x0d 0x00` + layer `0x01 0x00`.
const PREAMBLE: &[u8] = b"\0asm\x0d\0\x01\0";
/// Core module preamble: `\0asm` + version `0x01 0x00 0x00 0x00`.
const CORE_PREAMBLE: &[u8] = b"\0asm\x01\0\0\0";

fn compile(bytes: &[u8]) -> Result<(), String> {
    let engine = ComponentEngine::new();
    engine.compile(bytes).map(|_| ()).map_err(|e| e.to_string())
}

fn component(sections: &[u8]) -> Vec<u8> {
    let mut bytes = PREAMBLE.to_vec();
    bytes.extend_from_slice(sections);
    bytes
}

fn expect_malformed(bytes: &[u8], reason: &str) {
    let error = compile(bytes).expect_err("malformed component must be rejected");
    assert!(
        error.contains(reason),
        "expected an error containing `{reason}`, got `{error}`"
    );
}

#[test]
fn truncated_custom_section_name_is_rejected() {
    // Custom section (3 bytes): name length 5, only 2 bytes present.
    expect_malformed(
        &component(&[0x00, 0x03, 0x05, 0x61, 0x62]),
        "unexpected end-of-file",
    );
}

#[test]
fn invalid_utf8_custom_section_name_is_rejected() {
    // Custom section (3 bytes): name is not valid UTF-8.
    expect_malformed(
        &component(&[0x00, 0x03, 0x02, 0xff, 0xfe]),
        "malformed UTF-8 encoding",
    );
}

#[test]
fn custom_section_with_garbage_body_is_accepted() {
    // binary.wast's leading component: custom-section bodies are opaque
    // bytes; only the name must be well-formed, and a repeated name is
    // fine.
    let mut sections = vec![0x00, 0x12, 0x0e];
    sections.extend_from_slice(b"component-name");
    sections.extend_from_slice(&[0xff, 0xfe, 0x01]);
    sections.extend_from_slice(&[0x00, 0x10, 0x0e]);
    sections.extend_from_slice(b"component-name");
    sections.push(0x99);
    compile(&component(&sections)).expect("opaque custom-section bodies must be accepted");
}

#[test]
fn type_section_size_beyond_input_is_rejected() {
    // Type section declares 3 bytes; only 1 byte of contents follows.
    expect_malformed(&component(&[0x07, 0x03, 0x00]), "unexpected end-of-file");
}

#[test]
fn type_section_content_beyond_declared_size_is_rejected() {
    // Type section declares 1 byte (the count); the type byte itself
    // sits outside the declared section and must not be read.
    expect_malformed(
        &component(&[0x07, 0x01, 0x01, 0x73]),
        "unexpected end-of-file",
    );
}

#[test]
fn section_with_unconsumed_bytes_is_rejected() {
    // Type section declares 3 bytes and the input holds all 3, but the
    // count consumes only 1: the leftover bytes are not a valid section
    // continuation, so the component is malformed.
    let error = compile(&component(&[0x07, 0x03, 0x00, 0x00, 0x00]))
        .expect_err("a section leaving bytes unconsumed must be rejected");
    assert!(
        error.contains("invalid section size"),
        "unexpected error: {error}"
    );
    // The framing itself is malformed, so the public category is a
    // decode error, not a validation error (PR #354 Codex P2).
    assert!(
        error.starts_with("decode error:"),
        "expected a decode error, got: {error}"
    );
}

#[test]
fn section_size_leb_running_past_eof_is_rejected() {
    // Custom section: the size LEB itself runs past the end of input.
    expect_malformed(&component(&[0x00, 0x80, 0x80]), "unexpected end-of-file");
}

#[test]
fn section_id_without_size_is_rejected() {
    // A lone section id with no size byte at all.
    expect_malformed(&component(&[0x00]), "unexpected end-of-file");
}

#[test]
fn section_size_leb_with_bits_beyond_u32_is_rejected() {
    // Type section: the size LEB sets bits past the 32nd.
    expect_malformed(
        &component(&[0x07, 0x81, 0x80, 0x80, 0x80, 0x70, 0x00]),
        "integer too large",
    );
}

#[test]
fn vec_count_larger_than_input_is_rejected() {
    // Type section (4 bytes): vec count 999999, one type byte present.
    expect_malformed(
        &component(&[0x07, 0x04, 0xbf, 0x84, 0x3d, 0x73]),
        "unexpected end-of-file",
    );
}

#[test]
fn vec_count_exceeding_entries_present_is_rejected() {
    // Type section (2 bytes): count 2, only one type given.
    expect_malformed(
        &component(&[0x07, 0x02, 0x02, 0x73]),
        "unexpected end-of-file",
    );
}

#[test]
fn import_name_length_overrunning_input_is_rejected() {
    // Import section (5 bytes): one import, plain externname, name
    // length 127 with only 2 bytes following in the whole input.
    expect_malformed(
        &component(&[0x0a, 0x05, 0x01, 0x00, 0x7f, 0x61, 0x62]),
        "unexpected end-of-file",
    );
}

#[test]
fn core_module_eof_reason_is_preserved() {
    // Core module section (12 bytes): a core type section whose func
    // type is cut off after the 0x60 marker. The component error must
    // keep the core layer's reason instead of flattening it to
    // `error from underlying layer`.
    let mut sections = vec![0x01, 0x0c];
    sections.extend_from_slice(CORE_PREAMBLE);
    sections.extend_from_slice(&[0x01, 0x02, 0x01, 0x60]);
    expect_malformed(&component(&sections), "unexpected end-of-file");
}

#[test]
fn core_module_integer_too_large_reason_is_preserved() {
    // Core module section (14 bytes): a core type section whose size
    // LEB sets bits past the 32nd.
    let mut sections = vec![0x01, 0x0e];
    sections.extend_from_slice(CORE_PREAMBLE);
    sections.extend_from_slice(&[0x01, 0x81, 0x80, 0x80, 0x80, 0x70, 0x00]);
    expect_malformed(&component(&sections), "integer too large");
}

#[test]
fn empty_component_is_accepted() {
    compile(&component(&[])).expect("an empty component must be accepted");
}

#[test]
fn empty_type_section_is_accepted() {
    compile(&component(&[0x07, 0x01, 0x00])).expect("an empty type section must be accepted");
}

#[test]
fn valid_core_module_section_is_accepted() {
    // Core module section (8 bytes): an empty core module.
    let mut sections = vec![0x01, 0x08];
    sections.extend_from_slice(CORE_PREAMBLE);
    compile(&component(&sections)).expect("an empty core module must be accepted");
}
