//! Integration tests for the component-model value index space (#241):
//! sort `0x02`, the value section (section `0x0c`), and value
//! imports/exports across decode, validation, instantiation, and runtime
//! resolution.
//!
//! The `wat`/`wast` toolchain in this workspace has no value-section
//! support, so the components here are assembled byte by byte. The builder
//! below follows the binary grammar the decoder implements (which mirrors
//! design/mvp/Binary.md for the productions it shares).

use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, ComponentValue};

// ---------- byte builder ----------

fn uleb(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if v == 0 {
            return out;
        }
    }
}

fn sleb(mut v: i64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let mut byte = (v & 0x7f) as u8;
        v >>= 7;
        let done = (v == 0 && byte & 0x40 == 0) || (v == -1 && byte & 0x40 != 0);
        if !done {
            byte |= 0x80;
        }
        out.push(byte);
        if done {
            return out;
        }
    }
}

fn name(s: &str) -> Vec<u8> {
    let mut out = uleb(s.len() as u64);
    out.extend_from_slice(s.as_bytes());
    out
}

fn section(id: u8, mut payload: Vec<u8>) -> Vec<u8> {
    let mut out = vec![id];
    out.extend(uleb(payload.len() as u64));
    out.append(&mut payload);
    out
}

fn vec_of(items: &[Vec<u8>]) -> Vec<u8> {
    let mut out = uleb(items.len() as u64);
    for item in items {
        out.extend_from_slice(item);
    }
    out
}

fn component(sections: &[Vec<u8>]) -> Vec<u8> {
    let mut out = b"\x00asm".to_vec();
    out.extend([0x0d, 0x00, 0x01, 0x00]);
    for s in sections {
        out.extend_from_slice(s);
    }
    out
}

// valtype opcodes (signed LEB type opcodes from the decoder)
const BOOL: u8 = 0x7f;
const S8: u8 = 0x7e;
const U8: u8 = 0x7d;
const S16: u8 = 0x7c;
const U16: u8 = 0x7b;
const S32: u8 = 0x7a;
const U32: u8 = 0x79;
const S64: u8 = 0x78;
const U64: u8 = 0x77;
const F32: u8 = 0x76;
const F64: u8 = 0x75;
const CHAR: u8 = 0x74;
const STRING: u8 = 0x73;

fn value_entry(valtype: &[u8], encoded: &[u8]) -> Vec<u8> {
    let mut out = valtype.to_vec();
    out.extend(uleb(encoded.len() as u64));
    out.extend_from_slice(encoded);
    out
}

/// Import/export names in sections and type declarations carry a leading
/// `0x00` byte (the plain-name discriminant of the name grammar).
fn dash_name(s: &str) -> Vec<u8> {
    let mut out = vec![0x00];
    out.extend(name(s));
    out
}

fn export_value(export_name: &str, idx: u32, desc: Option<Vec<u8>>) -> Vec<u8> {
    let mut out = dash_name(export_name);
    out.push(0x02); // sort: value
    out.extend(uleb(idx as u64));
    match desc {
        None => out.push(0x00),
        Some(desc) => {
            out.push(0x01);
            out.extend(desc);
        }
    }
    out
}

fn value_desc(valtype: &[u8]) -> Vec<u8> {
    let mut out = vec![0x02]; // externdesc: value
    out.extend_from_slice(valtype);
    out
}

// ---------- components under test ----------

/// A component defining one value of many shapes and exporting all of them.
fn defined_values_component() -> Vec<u8> {
    // Type section:
    //   0: record { a: u32, b: string }
    //   1: list u8
    //   2: flags { read, write, exec }
    //   3: variant { off, on(u16) }
    //   4: option u32
    //   5: tuple (u32, bool)
    //   6: enum { red, green }
    //   7: result (u32, string)
    //   8: list u8 (len 3)
    let record = {
        let mut v = vec![0x72];
        v.extend(vec_of(&[
            [name("a"), vec![U32]].concat(),
            [name("b"), vec![STRING]].concat(),
        ]));
        v
    };
    let list_u8 = vec![0x70, U8];
    let flags = {
        let mut v = vec![0x6e];
        v.extend(vec_of(&[name("read"), name("write"), name("exec")]));
        v
    };
    let variant = {
        let mut v = vec![0x71];
        v.extend(vec_of(&[
            [name("off"), vec![0x00, 0x00]].concat(),
            [name("on"), vec![0x01, U16, 0x00]].concat(),
        ]));
        v
    };
    let option_u32 = vec![0x6b, U32];
    let tuple = {
        let mut v = vec![0x6f];
        v.extend(vec_of(&[vec![U32], vec![BOOL]]));
        v
    };
    let enum_ty = {
        let mut v = vec![0x6d];
        v.extend(vec_of(&[name("red"), name("green")]));
        v
    };
    let result_ty = vec![0x6a, 0x01, U32, 0x01, STRING];
    let fixed_list = {
        let mut v = vec![0x67, U8];
        v.extend(uleb(3));
        v
    };
    let types = section(
        0x07,
        vec_of(&[
            record, list_u8, flags, variant, option_u32, tuple, enum_ty, result_ty, fixed_list,
        ]),
    );

    let char_bytes = "\u{1f980}".as_bytes().to_vec();
    let values = section(
        0x0c,
        vec_of(&[
            value_entry(&[U32], &uleb(300)),
            value_entry(&[STRING], &name("hello")),
            value_entry(&[0x00], &[vec![7u8], name("x")].concat()),
            value_entry(&[0x01], &[vec![3u8, 1, 2, 3]].concat()),
            value_entry(&[0x02], &[0x05]),
            value_entry(&[0x03], &[vec![1u8], uleb(1000)].concat()),
            value_entry(&[0x04], &[vec![1u8], uleb(42)].concat()),
            value_entry(&[0x05], &[vec![9u8, 1]].concat()),
            value_entry(&[0x06], &[1]),
            value_entry(&[0x07], &[vec![0u8], uleb(5)].concat()),
            value_entry(&[0x08], &[9, 8, 7]),
            value_entry(&[BOOL], &[1]),
            value_entry(&[CHAR], &char_bytes),
            value_entry(&[F64], &1.5f64.to_le_bytes()),
            value_entry(&[S64], &sleb(-2)),
            value_entry(&[U64], &uleb(4_294_967_295)),
            value_entry(&[S16], &sleb(-300)),
            value_entry(&[S32], &sleb(-1)),
            value_entry(&[S8], &[0xfb]),
            value_entry(&[F32], &2.5f32.to_le_bytes()),
        ]),
    );

    let export_names = [
        "v-u32",
        "v-string",
        "v-record",
        "v-list",
        "v-flags",
        "v-variant",
        "v-option",
        "v-tuple",
        "v-enum",
        "v-result",
        "v-fixed-list",
        "v-bool",
        "v-char",
        "v-f64",
        "v-s64",
        "v-u64",
        "v-s16",
        "v-s32",
        "v-s8",
        "v-f32",
    ];
    let mut export_items: Vec<Vec<u8>> = export_names
        .iter()
        .enumerate()
        .map(|(i, n)| export_value(n, i as u32, None))
        .collect();
    // One export carrying an explicit externtype description.
    export_items.push(export_value("v-u32-desc", 0, Some(value_desc(&[U32]))));
    let exports = section(0x0b, vec_of(&export_items));

    component(&[types, values, exports])
}

/// A component importing a `u32` value and re-exporting it.
fn import_reexport_component() -> Vec<u8> {
    let import = {
        let mut v = dash_name("v");
        v.extend(value_desc(&[U32]));
        v
    };
    let imports = section(0x0a, vec_of(&[import]));
    let exports = section(0x0b, vec_of(&[export_value("v", 0, None)]));
    component(&[imports, exports])
}

/// Outer component: defines 42, instantiates the inner component with it,
/// aliases the instance's export, and re-exports it.
fn composed_component() -> Vec<u8> {
    let inner = import_reexport_component();
    let components = section(0x04, inner);

    let values = section(0x0c, vec_of(&[value_entry(&[U32], &uleb(42))]));

    // instance 0 := instantiate component 0 with ("v" = value 0)
    let instantiate = {
        let mut v = vec![0x00];
        v.extend(uleb(0)); // component idx
        let mut arg = name("v");
        arg.push(0x02);
        arg.extend(uleb(0));
        v.extend(vec_of(&[arg]));
        v
    };
    let instances = section(0x05, vec_of(&[instantiate]));

    // value 1 := alias export "v" of instance 0
    let alias = {
        let mut v = vec![0x02, 0x00]; // sort value, alias kind export
        v.extend(uleb(0)); // instance idx
        v.extend(name("v"));
        v
    };
    let aliases = section(0x06, vec_of(&[alias]));

    let exports = section(0x0b, vec_of(&[export_value("v", 1, None)]));
    component(&[components, values, instances, aliases, exports])
}

/// A component whose inline-export instance carries a defined value.
fn inline_instance_component() -> Vec<u8> {
    let values = section(0x0c, vec_of(&[value_entry(&[U32], &uleb(7))]));
    // instance 0 := (instance (export "v" (value 0)))
    let inline = {
        let mut v = vec![0x01];
        let mut item = vec![0x00];
        item.extend(name("v"));
        item.push(0x02);
        item.extend(uleb(0));
        v.extend(vec_of(&[item]));
        v
    };
    let instances = section(0x05, vec_of(&[inline]));
    // export instance 0 as "i"
    let export = {
        let mut v = dash_name("i");
        v.push(0x05); // sort: instance
        v.extend(uleb(0));
        v.push(0x00); // no externdesc
        v
    };
    let exports = section(0x0b, vec_of(&[export]));
    component(&[values, instances, exports])
}

/// A component type with a value import and an instance type with a value
/// export; both types are only declared, which still exercises the
/// type-level declaration paths.
fn type_level_component() -> Vec<u8> {
    // component type: (component (import "v" (value u32)))
    let component_ty = {
        let mut decl = vec![0x03]; // importdecl
        decl.extend(dash_name("v"));
        decl.extend(value_desc(&[U32]));
        let mut v = vec![0x41];
        v.extend(vec_of(&[decl]));
        v
    };
    // instance type: (instance (export "w" (value string)))
    let instance_ty = {
        let mut decl = vec![0x04]; // exportdecl
        decl.extend(dash_name("w"));
        decl.extend(value_desc(&[STRING]));
        let mut v = vec![0x42];
        v.extend(vec_of(&[decl]));
        v
    };
    let types = section(0x07, vec_of(&[component_ty, instance_ty]));
    component(&[types])
}

// ---------- tests ----------

#[tokio::test]
async fn defined_values_are_exported_and_readable() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&defined_values_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");

    let get = |n: &str| {
        instance
            .get_value(&store, n)
            .expect("value export resolves")
    };
    assert_eq!(get("v-u32"), ComponentValue::U32(300));
    assert_eq!(get("v-string"), ComponentValue::String("hello".to_owned()));
    assert_eq!(
        get("v-record"),
        ComponentValue::Record(vec![
            ("a".to_owned(), ComponentValue::U32(7)),
            ("b".to_owned(), ComponentValue::String("x".to_owned())),
        ])
    );
    assert_eq!(
        get("v-list"),
        ComponentValue::List(vec![
            ComponentValue::U8(1),
            ComponentValue::U8(2),
            ComponentValue::U8(3)
        ])
    );
    assert_eq!(
        get("v-flags"),
        ComponentValue::Flags(vec!["read".to_owned(), "exec".to_owned()])
    );
    assert_eq!(
        get("v-variant"),
        ComponentValue::Variant {
            case: "on".to_owned(),
            value: Some(Box::new(ComponentValue::U16(1000))),
        }
    );
    assert_eq!(
        get("v-option"),
        ComponentValue::Variant {
            case: "some".to_owned(),
            value: Some(Box::new(ComponentValue::U32(42))),
        }
    );
    assert_eq!(
        get("v-tuple"),
        ComponentValue::Record(vec![
            ("0".to_owned(), ComponentValue::U32(9)),
            ("1".to_owned(), ComponentValue::Bool(true)),
        ])
    );
    assert_eq!(
        get("v-enum"),
        ComponentValue::Variant {
            case: "green".to_owned(),
            value: None,
        }
    );
    assert_eq!(
        get("v-result"),
        ComponentValue::Variant {
            case: "ok".to_owned(),
            value: Some(Box::new(ComponentValue::U32(5))),
        }
    );
    assert_eq!(
        get("v-fixed-list"),
        ComponentValue::List(vec![
            ComponentValue::U8(9),
            ComponentValue::U8(8),
            ComponentValue::U8(7)
        ])
    );
    assert_eq!(get("v-bool"), ComponentValue::Bool(true));
    assert_eq!(get("v-char"), ComponentValue::Char('\u{1f980}'));
    assert_eq!(get("v-f64"), ComponentValue::F64(1.5));
    assert_eq!(get("v-s64"), ComponentValue::S64(-2));
    assert_eq!(get("v-u64"), ComponentValue::U64(4_294_967_295));
    assert_eq!(get("v-s16"), ComponentValue::S16(-300));
    assert_eq!(get("v-s32"), ComponentValue::S32(-1));
    assert_eq!(get("v-s8"), ComponentValue::S8(-5));
    assert_eq!(get("v-f32"), ComponentValue::F32(2.5));
    assert_eq!(get("v-u32-desc"), ComponentValue::U32(300));
}

#[tokio::test]
async fn linker_value_import_is_reexported() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&import_reexport_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();

    let mut linker = ComponentLinker::new();
    linker.register_import_value("v", ComponentValue::U32(42));
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    assert_eq!(
        instance.get_value(&store, "v").expect("value resolves"),
        ComponentValue::U32(42)
    );
}

#[tokio::test]
async fn linker_value_with_wrong_type_is_rejected_at_instantiate() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&import_reexport_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();

    let mut linker = ComponentLinker::new();
    linker.register_import_value("v", ComponentValue::String("nope".to_owned()));
    let err = match engine.instantiate(&program, &store, &linker).await {
        Ok(_) => panic!("instantiate must reject a mistyped value import"),
        Err(err) => err,
    };
    assert!(
        err.to_string().contains("does not match its declared type"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn missing_linker_value_fails_at_first_use() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&import_reexport_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate is lazy about value imports");
    let err = instance
        .get_value(&store, "v")
        .expect_err("resolving an unprovided value import must fail");
    assert!(
        matches!(err, ComponentError::Link(_)),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn value_flows_through_instantiate_and_alias() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&composed_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    assert_eq!(
        instance.get_value(&store, "v").expect("value resolves"),
        ComponentValue::U32(42)
    );
}

#[tokio::test]
async fn inline_instance_value_export_is_readable() {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&inline_instance_component())
        .expect("compile should succeed");
    let store = telomere::Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate(&program, &store, &linker)
        .await
        .expect("instantiate should succeed");
    assert_eq!(
        instance
            .exports()
            .get_instance("i")
            .get_value(&store, "v")
            .expect("value resolves"),
        ComponentValue::U32(7)
    );
}

#[test]
fn type_level_value_declarations_compile() {
    let engine = ComponentEngine::new();
    engine
        .compile(&type_level_component())
        .expect("component/instance types with value decls should compile");
}

// ---------- invalid binaries ----------

fn compile_err(bytes: &[u8]) -> String {
    let engine = ComponentEngine::new();
    engine
        .compile(bytes)
        .expect_err("compile must fail")
        .to_string()
}

#[test]
fn value_length_mismatch_is_rejected() {
    // u32 value 42 with two trailing bytes the encoding does not consume.
    let values = section(0x0c, vec_of(&[value_entry(&[U32], &[42, 0, 0])]));
    let err = compile_err(&component(&[values]));
    assert!(err.contains("does not match its declared length"), "{err}");
}

#[test]
fn invalid_bool_byte_is_rejected() {
    let values = section(0x0c, vec_of(&[value_entry(&[BOOL], &[2])]));
    let err = compile_err(&component(&[values]));
    assert!(err.contains("invalid bool value byte: 2"), "{err}");
}

#[test]
fn variant_case_out_of_range_is_rejected() {
    let variant = {
        let mut v = vec![0x71];
        v.extend(vec_of(&[[name("off"), vec![0x00, 0x00]].concat()]));
        v
    };
    let types = section(0x07, vec_of(&[variant]));
    let values = section(0x0c, vec_of(&[value_entry(&[0x00], &[3])]));
    let err = compile_err(&component(&[types, values]));
    assert!(
        err.contains("variant case index 3 is out of range"),
        "{err}"
    );
}

#[test]
fn flags_bit_beyond_declared_labels_is_rejected() {
    let flags = {
        let mut v = vec![0x6e];
        v.extend(vec_of(&[name("a"), name("b")]));
        v
    };
    let types = section(0x07, vec_of(&[flags]));
    let values = section(0x0c, vec_of(&[value_entry(&[0x00], &[0x04])]));
    let err = compile_err(&component(&[types, values]));
    assert!(
        err.contains("flags value sets a bit beyond the declared flags"),
        "{err}"
    );
}

#[test]
fn overlong_integer_encoding_is_rejected() {
    // u16 in three LEB bytes whose final byte carries bits past bit 15.
    let values = section(0x0c, vec_of(&[value_entry(&[U16], &[0xff, 0xff, 0x7f])]));
    let err = compile_err(&component(&[values]));
    assert!(err.contains("integer representation is too long"), "{err}");
}

#[test]
fn defined_value_with_resource_handle_is_rejected() {
    // type 0: resource; type 1: own 0
    let resource = vec![0x3f, 0x7f, 0x00];
    let own = vec![0x69, 0x00];
    let types = section(0x07, vec_of(&[resource, own]));
    let values = section(0x0c, vec_of(&[value_entry(&[0x01], &[0])]));
    let err = compile_err(&component(&[types, values]));
    assert!(
        err.contains("defined values cannot contain resource handles"),
        "{err}"
    );
}

#[test]
fn borrowed_value_import_is_rejected() {
    // type 0: resource; type 1: borrow 0; import "v" (value (type 1))
    let resource = vec![0x3f, 0x7f, 0x00];
    let borrow = vec![0x68, 0x00];
    let types = section(0x07, vec_of(&[resource, borrow]));
    let import = {
        let mut v = dash_name("v");
        v.extend(value_desc(&[0x01]));
        v
    };
    let imports = section(0x0a, vec_of(&[import]));
    let err = compile_err(&component(&[types, imports]));
    assert!(
        err.contains("value types cannot contain borrowed resource handles"),
        "{err}"
    );
}

#[test]
fn export_of_unknown_value_index_is_rejected() {
    let exports = section(0x0b, vec_of(&[export_value("v", 5, None)]));
    let err = compile_err(&component(&[exports]));
    assert!(err.contains("type idx not found 5"), "{err}");
}

#[test]
fn export_with_mismatched_declared_type_is_rejected() {
    let values = section(0x0c, vec_of(&[value_entry(&[U32], &uleb(1))]));
    let exports = section(
        0x0b,
        vec_of(&[export_value("v", 0, Some(value_desc(&[STRING])))]),
    );
    let err = compile_err(&component(&[values, exports]));
    assert!(err.contains("prim valtype mismatch"), "{err}");
}

#[test]
fn instantiate_with_mismatched_value_arg_is_rejected() {
    // Inner imports a string value; the outer defines a u32 and passes it.
    let inner = {
        let import = {
            let mut v = dash_name("v");
            v.extend(value_desc(&[STRING]));
            v
        };
        let imports = section(0x0a, vec_of(&[import]));
        let exports = section(0x0b, vec_of(&[export_value("v", 0, None)]));
        component(&[imports, exports])
    };
    let components = section(0x04, inner);
    let values = section(0x0c, vec_of(&[value_entry(&[U32], &uleb(42))]));
    let instantiate = {
        let mut v = vec![0x00];
        v.extend(uleb(0));
        let mut arg = name("v");
        arg.push(0x02);
        arg.extend(uleb(0));
        v.extend(vec_of(&[arg]));
        v
    };
    let instances = section(0x05, vec_of(&[instantiate]));
    let err = compile_err(&component(&[components, values, instances]));
    assert!(err.contains("prim valtype mismatch"), "{err}");
}
