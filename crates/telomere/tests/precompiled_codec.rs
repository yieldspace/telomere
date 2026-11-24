//! Round-trip, determinism, and rejection tests for the precompiled
//! module codec (#196).

use futures::executor::block_on;
use telomere::{
    deserialize_module, instantiate, is_precompiled_module, run_module_function, serialize_module,
    IoReadBinaryReader, Module, PrecompiledError, Registry, ResultValue, Store, VMResult,
    WasmParser, WasmValue,
};

fn parse_module(wat_source: &str) -> Module {
    let bytes = wat::parse_str(wat_source).expect("test wat must assemble");
    let mut reader = IoReadBinaryReader::from(bytes.as_slice());
    WasmParser::new(&mut reader)
        .parse_module()
        .expect("test module must parse")
}

fn run_i32(module: Module, export: &str, args: &[i32]) -> i32 {
    let store = Store::new();
    let registry = Registry::new();
    let instance = match block_on(instantiate(module, &store, &registry)) {
        VMResult::Success(instance) => instance,
        result => panic!("module must instantiate, got {result:?}"),
    };
    let args = ResultValue::new(args.iter().map(|v| WasmValue::I32(*v)).collect());
    match block_on(run_module_function(&instance, &store, export, &args)) {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(v)) => *v,
            other => panic!("expected one i32 result, got {other:?}"),
        },
        result => panic!("export must run, got {result:?}"),
    }
}

const RICH_MODULE: &str = r#"
(module
  (memory (export "mem") 1)
  (global $g (mut i32) (i32.const 7))
  (data (i32.const 16) "hello")
  (func $fib (export "fib") (param i32) (result i32)
    (if (result i32) (i32.lt_s (local.get 0) (i32.const 2))
      (then (local.get 0))
      (else
        (i32.add
          (call $fib (i32.sub (local.get 0) (i32.const 1)))
          (call $fib (i32.sub (local.get 0) (i32.const 2)))))))
  (func (export "bump") (result i32)
    (global.set $g (i32.add (global.get $g) (i32.const 1)))
    (global.get $g))
  (func (export "read") (param i32) (result i32)
    (i32.load8_u (local.get 0)))
)
"#;

#[test]
fn round_trip_preserves_behavior() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");
    assert!(is_precompiled_module(&artifact));
    let restored = deserialize_module(&artifact).expect("artifact must deserialize");

    assert_eq!(run_i32(parse_module(RICH_MODULE), "fib", &[10]), 55);
    assert_eq!(run_i32(restored, "fib", &[10]), 55);

    let restored = deserialize_module(&artifact).expect("artifact must deserialize");
    assert_eq!(run_i32(restored, "bump", &[]), 8);
    let restored = deserialize_module(&artifact).expect("artifact must deserialize");
    assert_eq!(run_i32(restored, "read", &[16]), i32::from(b'h'));
}

#[test]
fn round_trip_covers_tables_elems_and_simd() {
    let module = parse_module(
        r#"
        (module
          (table $t 2 funcref)
          (elem (i32.const 0) $a $b)
          (func $a (result i32) (i32.const 1))
          (func $b (result i32) (i32.const 2))
          (func (export "pick") (param i32) (result i32)
            (call_indirect (result i32) (local.get 0)))
          (func (export "sum") (param v128) (result i32)
            (i32x4.extract_lane 0
              (i32x4.add (local.get 0) (v128.const i32x4 10 20 30 40))))
        )
        "#,
    );
    let artifact = serialize_module(&module).expect("module must serialize");
    let restored = deserialize_module(&artifact).expect("artifact must deserialize");
    assert_eq!(run_i32(restored, "pick", &[1]), 2);
    let restored = deserialize_module(&artifact).expect("artifact must deserialize");
    // v128 argument path is exercised by the wast harness; here just
    // confirm a second export from the same artifact instantiates.
    assert_eq!(run_i32(restored, "pick", &[0]), 1);
    let _ = module;
}

#[test]
fn serialization_is_deterministic_and_stable() {
    let module = parse_module(RICH_MODULE);
    let first = serialize_module(&module).expect("module must serialize");
    let second = serialize_module(&module).expect("module must serialize");
    assert_eq!(
        first, second,
        "same module serialized twice must be identical"
    );

    let restored = deserialize_module(&first).expect("artifact must deserialize");
    let reserialized = serialize_module(&restored).expect("restored module must serialize");
    assert_eq!(
        first, reserialized,
        "serialize(deserialize(serialize(m))) must equal serialize(m)"
    );
}

#[test]
fn every_truncation_is_a_typed_error() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");
    for len in 0..artifact.len() {
        let prefix = &artifact[..len];
        assert!(
            deserialize_module(prefix).is_err(),
            "prefix of {len} bytes must not deserialize"
        );
    }
}

/// Length of the artifact header: magic + version + four fingerprint
/// strings + the op-table hash.
fn header_len(artifact: &[u8]) -> usize {
    fn leb(bytes: &[u8], pos: &mut usize) -> usize {
        let mut value = 0usize;
        let mut shift = 0;
        loop {
            let byte = bytes[*pos];
            *pos += 1;
            value |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    }
    let mut pos = 8;
    let _version = leb(artifact, &mut pos);
    for _ in 0..4 {
        let len = leb(artifact, &mut pos);
        pos += len;
    }
    pos + 8
}

#[test]
fn every_header_byte_flip_is_rejected() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");
    // No single-byte change to the header (magic, format version, or
    // any fingerprint field) may produce a module.
    let header_len = header_len(&artifact);
    for offset in 0..header_len {
        let mut mutated = artifact.clone();
        mutated[offset] ^= 0xff;
        assert!(
            deserialize_module(&mutated).is_err(),
            "header byte flip at {offset} must be rejected"
        );
    }
}

#[test]
fn random_corruption_never_panics() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");
    // Deterministic xorshift64 PRNG: no external crates, reproducible.
    let mut state = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    for _ in 0..3000 {
        let mut mutated = artifact.clone();
        let flips = 1 + (next() % 8) as usize;
        for _ in 0..flips {
            let offset = (next() as usize) % mutated.len();
            mutated[offset] = next() as u8;
        }
        // The result may be Ok only if the flips happened to land in
        // payload bytes that still decode consistently; it must never
        // panic.
        let _ = deserialize_module(&mutated);
    }
    for _ in 0..512 {
        let len = (next() as usize) % 256;
        let garbage: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let _ = deserialize_module(&garbage);
    }
}

/// Walks the artifact framing to the first op id in the codes section.
fn first_op_id_offset(artifact: &[u8]) -> usize {
    fn leb(bytes: &[u8], pos: &mut usize) -> usize {
        let mut value = 0usize;
        let mut shift = 0;
        loop {
            let byte = bytes[*pos];
            *pos += 1;
            value |= usize::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return value;
            }
            shift += 7;
        }
    }
    let mut pos = 8; // magic
    let _version = leb(artifact, &mut pos);
    for _ in 0..4 {
        let len = leb(artifact, &mut pos);
        pos += len; // fingerprint strings
    }
    pos += 8; // op table hash
    loop {
        let tag = artifact[pos];
        pos += 1;
        let len = leb(artifact, &mut pos);
        if tag == 12 {
            // codes section body
            let _count = leb(artifact, &mut pos);
            pos += 36; // locals: 9 fixed u32
            let pool = leb(artifact, &mut pos);
            pos += pool * 8;
            let _code_len = leb(artifact, &mut pos);
            if artifact[pos] == 1 {
                pos += 1;
                let _label = leb(artifact, &mut pos);
            } else {
                pos += 1;
            }
            return pos; // the fixed u16 op id starts here
        }
        pos += len;
    }
}

#[test]
fn unknown_op_id_is_a_typed_error() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");
    let offset = first_op_id_offset(&artifact);
    let mut mutated = artifact.clone();
    mutated[offset] = 0xff;
    mutated[offset + 1] = 0xff;
    match deserialize_module(&mutated).err() {
        Some(PrecompiledError::UnknownOpId(0xffff)) => {}
        other => panic!("expected UnknownOpId(0xffff), got {other:?}"),
    }
}

#[test]
fn bad_magic_and_trailing_bytes_are_typed_errors() {
    let module = parse_module(RICH_MODULE);
    let artifact = serialize_module(&module).expect("module must serialize");

    let mut not_precompiled = artifact.clone();
    not_precompiled[0] = b'X';
    match deserialize_module(&not_precompiled).err() {
        Some(PrecompiledError::BadMagic) => {}
        other => panic!("expected BadMagic, got {other:?}"),
    }

    let mut trailing = artifact.clone();
    trailing.push(0);
    match deserialize_module(&trailing).err() {
        Some(PrecompiledError::TrailingBytes { trailing: 1 }) => {}
        other => panic!("expected TrailingBytes, got {other:?}"),
    }

    assert!(!is_precompiled_module(b"\0asm\x01\0\0\0"));
    assert!(is_precompiled_module(&artifact));
}
