//! Regression tests for #355: the component decoder mishandled core
//! rectype / moduletype encodings around the shared 0x50 opcode.
//!
//! In a core type definition (`core:deftype`), a bare 0x50 is a
//! `core:moduletype` and a non-final `core:subtype` is encoded with a
//! 0x00 prefix (`0x00 0x50 ...`); inside a `core:moduletype`, type
//! declarators hold a `core:rectype` in the plain Core WebAssembly
//! encoding, where a bare 0x50 is the non-final `sub` and no prefix is
//! allowed (design/mvp/Binary.md, "Type Definitions"). The decoder
//! previously rejected the first form as "unsupported core type
//! opcode: 0" and the second as "invalid core functype signature: 80".
//!
//! The byte strings mirror the cases in the vendored upstream
//! `binary/binary.wast` (the valid components at lines 892 and 949 and
//! the malformed components at lines 956 and 966), plus targeted
//! supertype-validation cases.

use telomere_component::ComponentEngine;

/// `\0asm` + version `0x0d 0x00` + layer `0x01 0x00`.
const PREAMBLE: &[u8] = b"\0asm\x0d\0\x01\0";

fn compile(bytes: &[u8]) -> Result<(), String> {
    let engine = ComponentEngine::new();
    engine.compile(bytes).map(|_| ()).map_err(|e| e.to_string())
}

fn component(sections: &[u8]) -> Vec<u8> {
    let mut bytes = PREAMBLE.to_vec();
    bytes.extend_from_slice(sections);
    bytes
}

/// One core type section (id 3) holding `entries`.
fn core_type_section(entries: &[u8]) -> Vec<u8> {
    let mut sections = vec![0x03, entries.len() as u8];
    sections.extend_from_slice(entries);
    component(&sections)
}

fn expect_rejected(bytes: &[u8], reason: &str) {
    let error = compile(bytes).expect_err("component must be rejected");
    assert!(
        error.contains(reason),
        "expected an error containing `{reason}`, got `{error}`"
    );
}

#[test]
fn upstream_core_type_sections_with_prefixed_sub_and_moduletype_are_accepted() {
    // binary.wast lines 892-914: a plain functype, a 0x00-prefixed
    // non-final subtype of a functype, and a moduletype whose import
    // refers to the first functype.
    let bytes = component(&[
        // core type section: 1 type, functype ()->()
        0x03, 0x04, 0x01, 0x60, 0x00, 0x00, //
        // core type section: 1 type, 0x00 0x50 sub, 0 supertypes,
        // functype ()->()
        0x03, 0x07, 0x01, 0x00, 0x50, 0x00, 0x60, 0x00, 0x00, //
        // core type section: 1 type, moduletype with 4 decls
        0x03, 0x18, 0x01, 0x50, 0x04, //
        0x01, 0x60, 0x00, 0x00, // type: functype ()->()
        0x00, 0x01, b'a', 0x01, b'b', 0x00, 0x00, // import a.b (func (type 0))
        0x02, 0x10, 0x01, 0x01, 0x00, // alias outer type
        0x03, 0x01, b'e', 0x00, 0x00, // export e (func (type 0))
    ]);
    compile(&bytes).expect("upstream core type sections must be accepted");
}

#[test]
fn upstream_moduletype_with_bare_sub_declarator_is_accepted() {
    // binary.wast lines 949-955: inside a moduletype, a type
    // declarator is a core rectype, so 0x50 is a non-final sub with
    // no 0x00 prefix.
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x01, // moduletype, 1 decl
        0x01, 0x50, 0x00, 0x60, 0x00, 0x00, // type: (sub (func))
    ]);
    compile(&bytes).expect("moduletype with a bare sub declarator must be accepted");
}

#[test]
fn prefixed_sub_with_supertype_is_accepted() {
    // Type 0 is a non-final subtype with no supertypes (a bare
    // functype would be final and could not be subtyped at all);
    // type 1 subtypes it with a matching signature.
    let bytes = core_type_section(&[
        0x02, // 2 core types
        0x00, 0x50, 0x00, 0x60, 0x00, 0x00, // sub, 0 supertypes, functype ()->()
        0x00, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // sub (super 0), functype ()->()
    ]);
    compile(&bytes).expect("prefixed sub with a valid supertype must be accepted");
}

#[test]
fn final_sub_and_rec_group_are_accepted() {
    // Type 0: sub final with no supertypes. Types 1-2: a rec group
    // whose second member subtypes the first (a non-final sub).
    let bytes = core_type_section(&[
        0x02, // 2 core type entries
        0x4f, 0x00, 0x60, 0x00, 0x00, // sub final, functype ()->()
        0x4e, 0x02, // rec, 2 subtypes
        0x50, 0x00, 0x60, 0x00, 0x00, // sub, functype ()->()
        0x50, 0x01, 0x01, 0x60, 0x00, 0x00, // sub (super 1), functype ()->()
    ]);
    compile(&bytes).expect("final sub and rec group must be accepted");
}

#[test]
fn zero_prefix_not_followed_by_sub_is_rejected() {
    // In a core type definition, 0x00 is only the non-final-sub
    // prefix; anything but 0x50 after it is malformed.
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x00, 0x60, 0x00, 0x00, // 0x00 then a functype, not 0x50
    ]);
    expect_rejected(&bytes, "0x00 0x50");
}

#[test]
fn prefixed_sub_with_unknown_supertype_is_rejected() {
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x00, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // sub (super 0), but no type 0 exists
    ]);
    expect_rejected(&bytes, "type idx not found");
}

#[test]
fn prefixed_sub_with_module_supertype_is_rejected() {
    // Type 0 is a moduletype; a func subtype cannot subtype it.
    let bytes = core_type_section(&[
        0x02, // 2 core types
        0x50, 0x00, // empty moduletype
        0x00, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // sub (super 0) (func)
    ]);
    expect_rejected(&bytes, "is not a func type");
}

#[test]
fn sub_of_final_group_member_is_rejected() {
    // Within one rec group, a bare comptype is final, so the second
    // member cannot subtype it.
    let bytes = core_type_section(&[
        0x01, // 1 core type entry
        0x4e, 0x02, // rec, 2 subtypes
        0x60, 0x00, 0x00, // functype ()->() (final)
        0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // sub (super 0), functype ()->()
    ]);
    expect_rejected(&bytes, "is final");
}

#[test]
fn moduletype_declarator_with_zero_prefix_is_rejected() {
    // binary.wast lines 956-965: inside a moduletype, 0x00 is not a
    // core rectype leading byte.
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x01, // moduletype, 1 decl
        0x01, 0x00, 0x50, 0x00, 0x60, 0x00, 0x00, // type: 0x00 0x50 ...
    ]);
    expect_rejected(&bytes, "invalid leading byte");
}

#[test]
fn moduletype_sub_with_non_comptype_is_rejected() {
    // binary.wast lines 966-976: after `sub` with no supertypes, the
    // next byte must be a comptype; 0x01 is not one.
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x01, // moduletype, 1 decl
        0x01, 0x50, 0x00, 0x01, // type: sub, 0 supertypes, then 0x01
    ]);
    expect_rejected(&bytes, "invalid leading byte");
}

#[test]
fn moduletype_sub_with_unknown_supertype_is_rejected() {
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x01, // moduletype, 1 decl
        0x01, 0x50, 0x01, 0x05, 0x60, 0x00, 0x00, // type: sub (super 5) (func)
    ]);
    expect_rejected(&bytes, "type idx not found");
}

// ---------------------------------------------------------------------------
// Supertype validation completeness (Codex review of PR #371): a declared
// supertype must be non-final and signature-matching no matter where it was
// defined — the same rectype, an earlier definition, or a module type's
// local type space (including aliases into it) — and a subtype declares at
// most one supertype. wasmparser rejects the negative shapes with "sub type
// cannot have a final super type", "sub type must match super type" and
// "multiple supertypes".
// ---------------------------------------------------------------------------

#[test]
fn sub_of_bare_functype_from_earlier_definition_is_rejected() {
    // Type 0 is a bare functype, which is a final subtype; type 1
    // cannot subtype it even though the two are separate definitions.
    let bytes = core_type_section(&[
        0x02, // 2 core types
        0x60, 0x00, 0x00, // functype ()->() (final)
        0x00, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // sub (super 0), functype ()->()
    ]);
    expect_rejected(&bytes, "is final");
}

#[test]
fn moduletype_sub_of_final_local_type_is_rejected() {
    // Local type 0 is a bare functype declarator (final); local
    // type 1 cannot subtype it.
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x02, // moduletype, 2 decls
        0x01, 0x60, 0x00, 0x00, // type: functype ()->() (final)
        0x01, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // type: sub (super 0), functype ()->()
    ]);
    expect_rejected(&bytes, "is final");
}

#[test]
fn moduletype_sub_of_aliased_final_type_is_rejected() {
    // The moduletype aliases the component's bare functype (final)
    // as its local type 0; a subtype of the alias is still a subtype
    // of a final type.
    let bytes = core_type_section(&[
        0x02, // 2 core types
        0x60, 0x00, 0x00, // functype ()->() (final)
        0x50, 0x02, // moduletype, 2 decls
        0x02, 0x10, 0x01, 0x01, 0x00, // alias outer 1 0 (core type)
        0x01, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // type: sub (super 0), functype ()->()
    ]);
    expect_rejected(&bytes, "is final");
}

#[test]
fn sub_with_mismatched_signature_in_rec_group_is_rejected() {
    // (sub (func)) followed by (sub 0 (func (param i32))): the
    // subtype's signature must match its supertype's.
    let bytes = core_type_section(&[
        0x01, // 1 core type entry
        0x4e, 0x02, // rec, 2 subtypes
        0x50, 0x00, 0x60, 0x00, 0x00, // sub, functype ()->()
        0x50, 0x01, 0x00, 0x60, 0x01, 0x7f, 0x00, // sub (super 0), functype (i32)->()
    ]);
    expect_rejected(&bytes, "sub type must match super type");
}

#[test]
fn sub_with_mismatched_signature_from_earlier_definition_is_rejected() {
    // The supertype is a non-final subtype from an earlier
    // definition; the signature check applies across definitions too.
    let bytes = core_type_section(&[
        0x02, // 2 core types
        0x00, 0x50, 0x00, 0x60, 0x00, 0x00, // sub, 0 supertypes, functype ()->()
        0x00, 0x50, 0x01, 0x00, 0x60, 0x01, 0x7f, 0x00, // sub (super 0), functype (i32)->()
    ]);
    expect_rejected(&bytes, "sub type must match super type");
}

#[test]
fn moduletype_sub_with_mismatched_signature_is_rejected() {
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x02, // moduletype, 2 decls
        0x01, 0x50, 0x00, 0x60, 0x00, 0x00, // type: sub, functype ()->()
        0x01, 0x50, 0x01, 0x00, 0x60, 0x01, 0x7f,
        0x00, // type: sub (super 0), functype (i32)->()
    ]);
    expect_rejected(&bytes, "sub type must match super type");
}

#[test]
fn moduletype_sub_with_valid_supertype_is_accepted() {
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x02, // moduletype, 2 decls
        0x01, 0x50, 0x00, 0x60, 0x00, 0x00, // type: sub, functype ()->()
        0x01, 0x50, 0x01, 0x00, 0x60, 0x00, 0x00, // type: sub (super 0), functype ()->()
    ]);
    compile(&bytes).expect("moduletype sub with a valid supertype must be accepted");
}

#[test]
fn sub_with_multiple_supertypes_in_rec_group_is_rejected() {
    let bytes = core_type_section(&[
        0x01, // 1 core type entry
        0x4e, 0x03, // rec, 3 subtypes
        0x50, 0x00, 0x60, 0x00, 0x00, // sub, functype ()->()
        0x50, 0x00, 0x60, 0x00, 0x00, // sub, functype ()->()
        0x50, 0x02, 0x00, 0x01, 0x60, 0x00, 0x00, // sub (super 0 1), functype ()->()
    ]);
    expect_rejected(&bytes, "multiple supertypes");
}

#[test]
fn prefixed_sub_with_multiple_supertypes_is_rejected() {
    let bytes = core_type_section(&[
        0x03, // 3 core types
        0x00, 0x50, 0x00, 0x60, 0x00, 0x00, // sub, 0 supertypes, functype ()->()
        0x00, 0x50, 0x00, 0x60, 0x00, 0x00, // sub, 0 supertypes, functype ()->()
        0x00, 0x50, 0x02, 0x00, 0x01, 0x60, 0x00, 0x00, // sub (super 0 1), functype ()->()
    ]);
    expect_rejected(&bytes, "multiple supertypes");
}

#[test]
fn moduletype_sub_with_multiple_supertypes_is_rejected() {
    let bytes = core_type_section(&[
        0x01, // 1 core type
        0x50, 0x03, // moduletype, 3 decls
        0x01, 0x50, 0x00, 0x60, 0x00, 0x00, // type: sub, functype ()->()
        0x01, 0x50, 0x00, 0x60, 0x00, 0x00, // type: sub, functype ()->()
        0x01, 0x50, 0x02, 0x00, 0x01, 0x60, 0x00,
        0x00, // type: sub (super 0 1), functype ()->()
    ]);
    expect_rejected(&bytes, "multiple supertypes");
}
