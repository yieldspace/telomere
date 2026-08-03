use std::sync::{Arc, Mutex};

use telomere_component::{
    ComponentEngine, ComponentError, ComponentInstance, ComponentLinker, ComponentValue,
};

const BUMP_START: u32 = 1024;
const RESULT_AREA: u32 = 512;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReallocCall {
    old_ptr: u32,
    old_len: u32,
    align: u32,
    new_len: u32,
}

fn compile_component(text: &str) -> Vec<u8> {
    wat::parse_str(text).expect("component WAT must be valid")
}

async fn instantiate(text: &str, linker: &ComponentLinker) -> (telomere::Store, ComponentInstance) {
    let engine = ComponentEngine::new();
    let program = engine
        .compile(&compile_component(text))
        .expect("component must compile");
    let store = telomere::Store::new();
    let instance = engine
        .instantiate(&program, &store, linker)
        .await
        .expect("component must instantiate");
    (store, instance)
}

async fn scalar_u32(
    instance: &ComponentInstance,
    store: &telomere::Store,
    name: &str,
    args: &[u32],
) -> u32 {
    let args = args
        .iter()
        .copied()
        .map(ComponentValue::U32)
        .collect::<Vec<_>>();
    let values = instance
        .call(store, name, &args)
        .await
        .unwrap_or_else(|error| panic!("scalar export {name} must succeed: {error}"));
    match values.as_slice() {
        [ComponentValue::U32(value)] => *value,
        other => panic!("scalar export {name} returned {other:?}, expected one u32"),
    }
}

async fn realloc_log(instance: &ComponentInstance, store: &telomere::Store) -> Vec<ReallocCall> {
    let count = scalar_u32(instance, store, "call-count", &[]).await;
    let mut calls = Vec::with_capacity(count as usize);
    for call_index in 0..count {
        calls.push(ReallocCall {
            old_ptr: scalar_u32(instance, store, "log-word", &[call_index * 4]).await,
            old_len: scalar_u32(instance, store, "log-word", &[call_index * 4 + 1]).await,
            align: scalar_u32(instance, store, "log-word", &[call_index * 4 + 2]).await,
            new_len: scalar_u32(instance, store, "log-word", &[call_index * 4 + 3]).await,
        });
    }
    assert_eq!(
        scalar_u32(instance, store, "call-count", &[]).await,
        count,
        "scalar log observers must not allocate or append realloc calls"
    );
    calls
}

async fn bytes_at(
    instance: &ComponentInstance,
    store: &telomere::Store,
    ptr: u32,
    len: usize,
) -> Vec<u8> {
    let before = scalar_u32(instance, store, "call-count", &[]).await;
    let mut bytes = Vec::with_capacity(len);
    for offset in 0..len {
        bytes.push(scalar_u32(instance, store, "byte-at", &[ptr + offset as u32]).await as u8);
    }
    assert_eq!(
        scalar_u32(instance, store, "call-count", &[]).await,
        before,
        "scalar byte observers must not allocate or append realloc calls"
    );
    bytes
}

fn guest_fixture(data: &str, misalign_realloc: bool) -> String {
    let returned_ptr = if misalign_realloc {
        "(i32.add (local.get $ptr) (i32.const 1))"
    } else {
        "(local.get $ptr)"
    };
    format!(
        r#"
  (core module $guest
    ;; [0, 511] is the immutable observation log; result area is at 512;
    ;; allocations begin at 1024, so observation never aliases lowered values.
    (memory (export "memory") 1)
    {data}
    (global $bump (mut i32) (i32.const {bump_start}))
    (global $calls (mut i32) (i32.const 0))
    (global $captured_ptr (mut i32) (i32.const 0))
    (global $captured_len (mut i32) (i32.const 0))
    (func (export "realloc")
      (param $old_ptr i32) (param $old_len i32) (param $align i32) (param $new_len i32)
      (result i32)
      (local $ptr i32) (local $copy_len i32) (local $index i32)
      (i32.store
        (i32.add (i32.mul (global.get $calls) (i32.const 16)) (i32.const 0))
        (local.get $old_ptr))
      (i32.store
        (i32.add (i32.mul (global.get $calls) (i32.const 16)) (i32.const 4))
        (local.get $old_len))
      (i32.store
        (i32.add (i32.mul (global.get $calls) (i32.const 16)) (i32.const 8))
        (local.get $align))
      (i32.store
        (i32.add (i32.mul (global.get $calls) (i32.const 16)) (i32.const 12))
        (local.get $new_len))
      (global.set $calls (i32.add (global.get $calls) (i32.const 1)))
      (local.set $ptr
        (i32.mul
          (i32.div_u
            (i32.add (global.get $bump) (i32.sub (local.get $align) (i32.const 1)))
            (local.get $align))
          (local.get $align)))
      (local.set $copy_len (local.get $old_len))
      (if (i32.lt_u (local.get $new_len) (local.get $copy_len))
        (then (local.set $copy_len (local.get $new_len))))
      (local.set $index (i32.const 0))
      (block $done
        (loop $copy
          (br_if $done (i32.ge_u (local.get $index) (local.get $copy_len)))
          (i32.store8
            (i32.add (local.get $ptr) (local.get $index))
            (i32.load8_u (i32.add (local.get $old_ptr) (local.get $index))))
          (local.set $index (i32.add (local.get $index) (i32.const 1)))
          (br $copy)))
      (global.set $bump (i32.add (local.get $ptr) (local.get $new_len)))
      {returned_ptr}
    )
    (func (export "call-count") (result i32) (global.get $calls))
    (func (export "log-word") (param $index i32) (result i32)
      (i32.load (i32.mul (local.get $index) (i32.const 4))))
    (func (export "byte-at") (param $ptr i32) (result i32)
      (i32.load8_u (local.get $ptr)))
    (func (export "word-at") (param $ptr i32) (result i32)
      (i32.load (local.get $ptr)))
    (func (export "capture-direct") (param $ptr i32) (param $len i32)
      (global.set $captured_ptr (local.get $ptr))
      (global.set $captured_len (local.get $len)))
    (func (export "capture-indirect") (param $ptr i32)
      (global.set $captured_ptr (local.get $ptr)))
    (func (export "captured-ptr") (result i32)
      (global.get $captured_ptr))
    (func (export "captured-len") (result i32)
      (global.get $captured_len))
    (func (export "last-ptr") (result i32)
      (i32.load (i32.const {result_area})))
    (func (export "tagged-len") (result i32)
      (i32.load (i32.const {result_area_plus_4})))
  )
"#,
        bump_start = BUMP_START,
        data = data,
        returned_ptr = returned_ptr,
        result_area = RESULT_AREA,
        result_area_plus_4 = RESULT_AREA + 4,
    )
}

fn scalar_observers() -> &'static str {
    r#"
  (func (export "call-count") (result u32)
    (canon lift (core func $guest "call-count")))
  (func (export "log-word") (param "index" u32) (result u32)
    (canon lift (core func $guest "log-word")))
  (func (export "byte-at") (param "ptr" u32) (result u32)
    (canon lift (core func $guest "byte-at")))
  (func (export "word-at") (param "ptr" u32) (result u32)
    (canon lift (core func $guest "word-at")))
  (func (export "captured-ptr") (result u32)
    (canon lift (core func $guest "captured-ptr")))
  (func (export "captured-len") (result u32)
    (canon lift (core func $guest "captured-len")))
  (func (export "last-ptr") (result u32)
    (canon lift (core func $guest "last-ptr")))
  (func (export "tagged-len") (result u32)
    (canon lift (core func $guest "tagged-len")))
"#
}

fn latin1_utf16_store_component(misalign_realloc: bool) -> String {
    format!(
        r#"
(component
  (type $host-type (func (result string)))
  (import "host" (func $host (type $host-type)))
  {guest}
  (core instance $guest (instantiate $guest))
  (core func $host-lower
    (canon lower (func $host)
      string-encoding=latin1+utf16
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  (core module $caller
    (import "" "host" (func $host (param i32)))
    (func (export "run")
      (call $host (i32.const {result_area})))
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance (export "host" (func $host-lower))))))
  (func (export "run")
    (canon lift (core func $caller "run")))
  {observers}
)
"#,
        guest = guest_fixture("", misalign_realloc),
        result_area = RESULT_AREA,
        observers = scalar_observers(),
    )
}

fn latin1_utf16_load_component(data: &str, ptr: u32, tagged_len: u32) -> String {
    format!(
        r#"
(component
  (type $host-type (func (param "value" string)))
  (import "host" (func $host (type $host-type)))
  {guest}
  (core instance $guest (instantiate $guest))
  (core func $host-lower
    (canon lower (func $host)
      string-encoding=latin1+utf16
      (memory $guest "memory")))
  (core module $caller
    (import "" "host" (func $host (param i32 i32)))
    (func (export "run")
      (call $host (i32.const {ptr}) (i32.const {tagged_len})))
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance (export "host" (func $host-lower))))))
  (func (export "run")
    (canon lift (core func $caller "run")))
)
"#,
        guest = guest_fixture(data, false),
        ptr = ptr,
        tagged_len = tagged_len as i32,
    )
}

fn string_roundtrip_component(encoding: &str) -> String {
    format!(
        r#"
(component
  (type $host-type (func (param "value" string) (result string)))
  (import "host" (func $host (type $host-type)))
  {guest}
  (core instance $guest (instantiate $guest))
  (core func $host-lower
    (canon lower (func $host)
      string-encoding={encoding}
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  (core module $caller
    (import "" "host" (func $host (param i32 i32 i32)))
    (func (export "run") (param i32 i32) (result i32)
      (call $host (local.get 0) (local.get 1) (i32.const {result_area}))
      (i32.const {result_area}))
  )
  (core instance $caller
    (instantiate $caller
      (with "" (instance (export "host" (func $host-lower))))))
  (func (export "run") (param "value" string) (result string)
    (canon lift
      (core func $caller "run")
      string-encoding={encoding}
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
)
"#,
        guest = guest_fixture("", false),
        encoding = encoding,
        result_area = RESULT_AREA,
    )
}

fn direct_list_component(type_defs: &str, misalign_realloc: bool) -> String {
    format!(
        r#"
(component
  {type_defs}
  {guest}
  (core instance $guest (instantiate $guest))
  (func (export "run") (param "values" $value)
    (canon lift
      (core func $guest "capture-direct")
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  {observers}
)
"#,
        type_defs = type_defs,
        guest = guest_fixture("", misalign_realloc),
        observers = scalar_observers(),
    )
}

fn indirect_list_component(type_defs: &str, params: &str, misalign_realloc: bool) -> String {
    format!(
        r#"
(component
  {type_defs}
  {guest}
  (core instance $guest (instantiate $guest))
  (func (export "run")
    {params}
    (canon lift
      (core func $guest "capture-indirect")
      (memory $guest "memory")
      (realloc (func $guest "realloc"))))
  {observers}
)
"#,
        type_defs = type_defs,
        params = params,
        guest = guest_fixture("", misalign_realloc),
        observers = scalar_observers(),
    )
}

fn u64_list_and_15_u32_params() -> String {
    let trailing = (0..15)
        .map(|index| format!(r#"(param "tail-{index}" u32)"#))
        .collect::<Vec<_>>()
        .join("\n    ");
    format!(
        r#"
    (param "head" u64)
    (param "values" $value)
    {trailing}
"#,
        trailing = trailing,
    )
}

fn list_and_16_u32_params() -> String {
    let trailing = (0..16)
        .map(|index| format!(r#"(param "tail-{index}" u32)"#))
        .collect::<Vec<_>>()
        .join("\n    ");
    format!(
        r#"
    (param "values" $value)
    {trailing}
"#,
        trailing = trailing,
    )
}

#[tokio::test]
async fn latin1_utf16_store_empty_records_the_exact_zero_length_allocation() {
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert!(args.is_empty());
        Ok(vec![ComponentValue::String(String::new())])
    });
    let (store, instance) = instantiate(&latin1_utf16_store_component(false), &linker).await;

    instance
        .call(&store, "run", &[])
        .await
        .expect("store call must succeed");

    // Canonical ABI `store_string_to_latin1_or_utf16`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        realloc_log(&instance, &store).await,
        vec![ReallocCall {
            old_ptr: 0,
            old_len: 0,
            align: 2,
            new_len: 0,
        }]
    );
    assert_eq!(
        scalar_u32(&instance, &store, "last-ptr", &[]).await,
        BUMP_START
    );
    assert_eq!(scalar_u32(&instance, &store, "tagged-len", &[]).await, 0);
    // Canonical ABI `store_string_to_latin1_or_utf16` emits no payload bytes for empty input:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        bytes_at(&instance, &store, BUMP_START, 0).await,
        Vec::<u8>::new()
    );
    assert_eq!(
        scalar_u32(&instance, &store, "call-count", &[]).await,
        1,
        "all scalar observers, including tagged length, must leave the realloc log unchanged"
    );
}

#[tokio::test]
async fn latin1_utf16_store_records_all_compact_and_utf16_reallocations() {
    struct StoreCase {
        name: &'static str,
        value: &'static str,
        expected_log: Vec<ReallocCall>,
        expected_ptr: u32,
        expected_bytes: Vec<u8>,
        expected_tagged_len: u32,
    }

    let cases = vec![
        StoreCase {
            name: "hello",
            value: "hello",
            expected_log: vec![ReallocCall {
                old_ptr: 0,
                old_len: 0,
                align: 2,
                new_len: 5,
            }],
            expected_ptr: BUMP_START,
            expected_bytes: b"hello".to_vec(),
            expected_tagged_len: 5,
        },
        StoreCase {
            name: "h-e-acute",
            value: "hé",
            expected_log: vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 2,
                    new_len: 3,
                },
                ReallocCall {
                    old_ptr: BUMP_START,
                    old_len: 3,
                    align: 2,
                    new_len: 2,
                },
            ],
            expected_ptr: BUMP_START + 4,
            expected_bytes: vec![0x68, 0xe9],
            expected_tagged_len: 2,
        },
        StoreCase {
            name: "u00ff",
            value: "ÿ",
            expected_log: vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 2,
                    new_len: 2,
                },
                ReallocCall {
                    old_ptr: BUMP_START,
                    old_len: 2,
                    align: 2,
                    new_len: 1,
                },
            ],
            expected_ptr: BUMP_START + 2,
            expected_bytes: vec![0xff],
            expected_tagged_len: 1,
        },
        StoreCase {
            name: "h-snowman",
            value: "h☃",
            expected_log: vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 2,
                    new_len: 4,
                },
                ReallocCall {
                    old_ptr: BUMP_START,
                    old_len: 4,
                    align: 2,
                    new_len: 8,
                },
                ReallocCall {
                    old_ptr: BUMP_START + 4,
                    old_len: 8,
                    align: 2,
                    new_len: 4,
                },
            ],
            expected_ptr: BUMP_START + 12,
            expected_bytes: vec![0x68, 0x00, 0x03, 0x26],
            expected_tagged_len: 0x8000_0002,
        },
        StoreCase {
            name: "u0100",
            value: "Ā",
            expected_log: vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 2,
                    new_len: 2,
                },
                ReallocCall {
                    old_ptr: BUMP_START,
                    old_len: 2,
                    align: 2,
                    new_len: 4,
                },
                ReallocCall {
                    old_ptr: BUMP_START + 2,
                    old_len: 4,
                    align: 2,
                    new_len: 2,
                },
            ],
            expected_ptr: BUMP_START + 6,
            expected_bytes: vec![0x00, 0x01],
            expected_tagged_len: 0x8000_0001,
        },
        StoreCase {
            name: "grinning-face",
            value: "😀",
            expected_log: vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 2,
                    new_len: 4,
                },
                ReallocCall {
                    old_ptr: BUMP_START,
                    old_len: 4,
                    align: 2,
                    new_len: 8,
                },
                ReallocCall {
                    old_ptr: BUMP_START + 4,
                    old_len: 8,
                    align: 2,
                    new_len: 4,
                },
            ],
            expected_ptr: BUMP_START + 12,
            expected_bytes: vec![0x3d, 0xd8, 0x00, 0xde],
            expected_tagged_len: 0x8000_0002,
        },
    ];

    for case in cases {
        let name = case.name;
        let value = case.value.to_owned();
        let mut linker = ComponentLinker::new();
        linker.register_import("host", move |_store, args| {
            assert!(args.is_empty(), "{name}: host must receive no arguments");
            Ok(vec![ComponentValue::String(value.clone())])
        });
        let (store, instance) = instantiate(&latin1_utf16_store_component(false), &linker).await;
        instance
            .call(&store, "run", &[])
            .await
            .unwrap_or_else(|error| panic!("{name}: store call must succeed: {error}"));

        // Canonical ABI `store_string_to_latin1_or_utf16`:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            realloc_log(&instance, &store).await,
            case.expected_log,
            "{}: complete realloc sequence",
            name
        );
        // Canonical ABI `store_string_to_latin1_or_utf16` returns a pointer plus tagged length:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            scalar_u32(&instance, &store, "last-ptr", &[]).await,
            case.expected_ptr,
            "{}: final allocation pointer",
            name
        );
        assert_eq!(
            scalar_u32(&instance, &store, "tagged-len", &[]).await,
            case.expected_tagged_len,
            "{}: tagged length",
            name
        );
        // Canonical ABI `store_string_to_latin1_or_utf16` byte layout:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            bytes_at(
                &instance,
                &store,
                case.expected_ptr,
                case.expected_bytes.len(),
            )
            .await,
            case.expected_bytes,
            "{}: stored bytes",
            name
        );
        assert_eq!(
            scalar_u32(&instance, &store, "call-count", &[]).await,
            case.expected_log.len() as u32,
            "{name}: scalar observers must not append realloc calls"
        );
    }
}

#[tokio::test]
async fn latin1_utf16_load_uses_the_tag_to_choose_latin1_or_utf16() {
    struct LoadCase {
        name: &'static str,
        data: &'static str,
        ptr: u32,
        tagged_len: u32,
        expected: &'static str,
    }

    let cases = [
        LoadCase {
            name: "latin1-e9",
            data: r#"(data (i32.const 64) "\e9")"#,
            ptr: 64,
            tagged_len: 1,
            expected: "é",
        },
        LoadCase {
            name: "latin1-h-e9",
            data: r#"(data (i32.const 64) "\68\e9")"#,
            ptr: 64,
            tagged_len: 2,
            expected: "hé",
        },
        LoadCase {
            name: "tagged-h-snowman",
            data: r#"(data (i32.const 64) "\68\00\03\26")"#,
            ptr: 64,
            tagged_len: 0x8000_0002,
            expected: "h☃",
        },
        LoadCase {
            name: "tagged-grinning-face",
            data: r#"(data (i32.const 64) "\3d\d8\00\de")"#,
            ptr: 64,
            tagged_len: 0x8000_0002,
            expected: "😀",
        },
    ];

    for case in cases {
        let name = case.name;
        let received = Arc::new(Mutex::new(Vec::<String>::new()));
        let received_by_host = Arc::clone(&received);
        let mut linker = ComponentLinker::new();
        linker.register_import("host", move |_store, args| {
            let [ComponentValue::String(value)] = args else {
                return Err(ComponentError::InvalidArgument(format!(
                    "{name}: expected one string, got {args:?}",
                )));
            };
            received_by_host
                .lock()
                .expect("host observation lock must not be poisoned")
                .push(value.clone());
            Ok(Vec::new())
        });
        let (store, instance) = instantiate(
            &latin1_utf16_load_component(case.data, case.ptr, case.tagged_len),
            &linker,
        )
        .await;
        instance
            .call(&store, "run", &[])
            .await
            .unwrap_or_else(|error| panic!("{name}: load call must succeed: {error}"));

        let values = received
            .lock()
            .expect("host observation lock must not be poisoned")
            .clone();
        // Canonical ABI `load_string_from_range`:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            values,
            vec![case.expected.to_owned()],
            "{name}: decoded host string"
        );
        if name == "latin1-h-e9" {
            assert_ne!(
                values[0], "\u{e968}",
                "Latin-1 h/e9 must not be decoded as one U+E968 UTF-16 code unit"
            );
        }
    }
}

#[tokio::test]
async fn existing_utf8_and_utf16_string_arms_still_roundtrip() {
    for (encoding, value) in [("utf8", "hé"), ("utf16", "h☃")] {
        let expected = value.to_owned();
        let mut linker = ComponentLinker::new();
        linker.register_import("host", move |_store, args| {
            assert_eq!(args, &[ComponentValue::String(expected.clone())]);
            Ok(args.to_vec())
        });
        let (store, instance) = instantiate(&string_roundtrip_component(encoding), &linker).await;
        let result = instance
            .call(&store, "run", &[ComponentValue::String(value.to_owned())])
            .await
            .unwrap_or_else(|error| panic!("{encoding}: roundtrip must succeed: {error}"));
        assert_eq!(result, vec![ComponentValue::String(value.to_owned())]);
    }
}

#[tokio::test]
async fn latin1_utf16_load_rejects_an_odd_pointer() {
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let received_by_host = Arc::clone(&received);
    let mut linker = ComponentLinker::new();
    linker.register_import("host", move |_store, args| {
        received_by_host
            .lock()
            .expect("host observation lock must not be poisoned")
            .push(format!("{args:?}"));
        Ok(Vec::new())
    });
    let (store, instance) = instantiate(
        &latin1_utf16_load_component(r#"(data (i32.const 65) "\00\01")"#, 65, 0x8000_0001),
        &linker,
    )
    .await;

    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("odd latin1+utf16 pointer must trap");
    // Canonical ABI `load_string_from_range` requires `ptr == align_to(ptr, 2)`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert!(
        matches!(error, ComponentError::Trap(_)),
        "unexpected error: {error}"
    );
    assert!(
        received
            .lock()
            .expect("host observation lock must not be poisoned")
            .is_empty(),
        "trap must occur before host import invocation"
    );
}

#[tokio::test]
async fn latin1_utf16_store_rejects_a_misaligned_realloc_result() {
    let mut linker = ComponentLinker::new();
    linker.register_import("host", |_store, args| {
        assert!(args.is_empty());
        Ok(vec![ComponentValue::String("hello".to_owned())])
    });
    let (store, instance) = instantiate(&latin1_utf16_store_component(true), &linker).await;

    let error = instance
        .call(&store, "run", &[])
        .await
        .expect_err("misaligned latin1+utf16 realloc result must trap");
    // Canonical ABI `store_string_to_latin1_or_utf16` checks `ptr == align_to(ptr, 2)`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert!(
        matches!(error, ComponentError::Trap(_)),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn direct_list_u64_uses_8_byte_alignment_and_stride() {
    let values = [0x0102_0304_0506_0708u64, 0x8877_6655_4433_2211];
    let expected_bytes = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let (store, instance) = instantiate(
        &direct_list_component("(type $value (list u64))", false),
        &ComponentLinker::new(),
    )
    .await;

    instance
        .call(
            &store,
            "run",
            &[ComponentValue::List(
                values.iter().copied().map(ComponentValue::U64).collect(),
            )],
        )
        .await
        .expect("direct list<u64> lowering must succeed");

    // Canonical ABI `lower_flat_list` / `store_list_into_range`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        realloc_log(&instance, &store).await,
        vec![ReallocCall {
            old_ptr: 0,
            old_len: 0,
            align: 8,
            new_len: 16,
        }],
        "list<u64> must allocate one 8-byte-stride body"
    );
    assert_eq!(
        scalar_u32(&instance, &store, "captured-ptr", &[]).await,
        BUMP_START
    );
    assert_eq!(
        scalar_u32(&instance, &store, "captured-len", &[]).await,
        values.len() as u32
    );
    // Canonical ABI `store_list_into_range` uses `elem_size(u64) == 8` and alignment 8:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        bytes_at(&instance, &store, BUMP_START, expected_bytes.len()).await,
        expected_bytes
    );
    assert_eq!(
        scalar_u32(&instance, &store, "call-count", &[]).await,
        1,
        "direct scalar observers must not append realloc calls"
    );
}

#[tokio::test]
async fn direct_primitive_lists_use_their_element_alignments_and_strides() {
    struct DirectCase {
        name: &'static str,
        type_defs: &'static str,
        values: Vec<ComponentValue>,
        align: u32,
        stride: u32,
        elements: Vec<Vec<u8>>,
    }

    let f64_values = [1.5f64, -2.25];
    let u32_values = [0x0102_0304u32, 0xaabb_ccdd];
    let cases = vec![
        DirectCase {
            name: "list<f64>",
            type_defs: "(type $value (list float64))",
            values: f64_values
                .iter()
                .copied()
                .map(ComponentValue::F64)
                .collect(),
            align: 8,
            stride: 8,
            elements: f64_values
                .iter()
                .map(|value| value.to_le_bytes().to_vec())
                .collect(),
        },
        DirectCase {
            name: "list<u8>",
            type_defs: "(type $value (list u8))",
            values: vec![
                ComponentValue::U8(0x00),
                ComponentValue::U8(0xaa),
                ComponentValue::U8(0xff),
            ],
            align: 1,
            stride: 1,
            elements: vec![vec![0x00], vec![0xaa], vec![0xff]],
        },
        DirectCase {
            name: "list<u32>",
            type_defs: "(type $value (list u32))",
            values: u32_values
                .iter()
                .copied()
                .map(ComponentValue::U32)
                .collect(),
            align: 4,
            stride: 4,
            elements: u32_values
                .iter()
                .map(|value| value.to_le_bytes().to_vec())
                .collect(),
        },
    ];

    for case in cases {
        let DirectCase {
            name,
            type_defs,
            values,
            align,
            stride,
            elements,
        } = case;
        let expected_body = elements.concat();
        let len = values.len() as u32;
        let (store, instance) = instantiate(
            &direct_list_component(type_defs, false),
            &ComponentLinker::new(),
        )
        .await;
        instance
            .call(&store, "run", &[ComponentValue::List(values)])
            .await
            .unwrap_or_else(|error| panic!("{name}: direct lowering must succeed: {error}"));

        // Canonical ABI `lower_flat_list` / `store_list_into_range`:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            realloc_log(&instance, &store).await,
            vec![ReallocCall {
                old_ptr: 0,
                old_len: 0,
                align,
                new_len: expected_body.len() as u32,
            }],
            "{name}: full realloc call"
        );
        assert_eq!(
            scalar_u32(&instance, &store, "captured-ptr", &[]).await,
            BUMP_START,
            "{name}: list body pointer"
        );
        assert_eq!(
            scalar_u32(&instance, &store, "captured-len", &[]).await,
            len,
            "{name}: list length"
        );
        // Canonical ABI `store_list_into_range` uses each element's `elem_size` as the stride:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            bytes_at(&instance, &store, BUMP_START, expected_body.len()).await,
            expected_body,
            "{name}: complete list body bytes"
        );
        for (index, expected_element) in elements.iter().enumerate() {
            // Canonical ABI `store_list_into_range` / `elem_size` stride:
            // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
            assert_eq!(
                bytes_at(
                    &instance,
                    &store,
                    BUMP_START + stride * index as u32,
                    expected_element.len(),
                )
                .await,
                *expected_element,
                "{name}: element {index} at stride {stride}"
            );
        }
        assert_eq!(
            scalar_u32(&instance, &store, "call-count", &[]).await,
            1,
            "{name}: scalar observers must not append realloc calls"
        );
    }
}

#[tokio::test]
async fn direct_list_record_uses_16_byte_stride_with_b_at_offset_8() {
    let records = [
        (0x11u8, 0x0102_0304_0506_0708u64),
        (0x22, 0x8877_6655_4433_2211),
    ];
    let values = records
        .iter()
        .map(|(a, b)| {
            ComponentValue::Record(vec![
                ("a".to_owned(), ComponentValue::U8(*a)),
                ("b".to_owned(), ComponentValue::U64(*b)),
            ])
        })
        .collect::<Vec<_>>();
    let expected_bytes = records
        .iter()
        .flat_map(|(a, b)| {
            let mut bytes = vec![*a];
            bytes.extend([0; 7]);
            bytes.extend(b.to_le_bytes());
            bytes
        })
        .collect::<Vec<_>>();
    let (store, instance) = instantiate(
        &direct_list_component(
            r#"
  (type $entry (record (field "a" u8) (field "b" u64)))
  (export "entry" (type $entry))
  (type $value (list $entry))
  (export "value" (type $value))
"#,
            false,
        ),
        &ComponentLinker::new(),
    )
    .await;

    instance
        .call(&store, "run", &[ComponentValue::List(values)])
        .await
        .expect("direct list<record{a:u8,b:u64}> lowering must succeed");

    // Canonical ABI `lower_flat_list` / `store_list_into_range`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        realloc_log(&instance, &store).await,
        vec![ReallocCall {
            old_ptr: 0,
            old_len: 0,
            align: 8,
            new_len: 32,
        }]
    );
    assert_eq!(
        scalar_u32(&instance, &store, "captured-ptr", &[]).await,
        BUMP_START
    );
    assert_eq!(scalar_u32(&instance, &store, "captured-len", &[]).await, 2);
    // Canonical ABI `store_list_into_range` / `store_record` gives `{a:u8,b:u64}` size 16,
    // alignment 8, and b at offset 8:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        bytes_at(&instance, &store, BUMP_START, expected_bytes.len()).await,
        expected_bytes
    );
    for (index, (_, b)) in records.iter().enumerate() {
        // Canonical ABI `store_record` field alignment for b at offset 8:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            bytes_at(&instance, &store, BUMP_START + 16 * index as u32 + 8, 8).await,
            b.to_le_bytes(),
            "record {index}: b must be at byte offset 8 of its 16-byte stride"
        );
    }
    assert_eq!(
        scalar_u32(&instance, &store, "call-count", &[]).await,
        1,
        "scalar observers must not append realloc calls"
    );
}

#[tokio::test]
async fn indirect_u64_list_and_15_u32_params_use_an_80_byte_outer_area() {
    struct IndirectCase {
        name: &'static str,
        type_defs: &'static str,
        values: Vec<ComponentValue>,
        elements: Vec<Vec<u8>>,
        stride: u32,
        record_b: Option<Vec<u64>>,
    }

    let u64_values = [
        0x0102_0304_0506_0708u64,
        0x8877_6655_4433_2211,
        0x1020_3040_5060_7080,
    ];
    let f64_values = [1.5f64, -2.25, 3.75];
    let records = [
        (0x11u8, 0x0102_0304_0506_0708u64),
        (0x22, 0x8877_6655_4433_2211),
        (0x33, 0x1020_3040_5060_7080),
    ];
    let cases = vec![
        IndirectCase {
            name: "list<u64>",
            type_defs: "(type $value (list u64))",
            values: u64_values
                .iter()
                .copied()
                .map(ComponentValue::U64)
                .collect(),
            elements: u64_values
                .iter()
                .map(|value| value.to_le_bytes().to_vec())
                .collect(),
            stride: 8,
            record_b: None,
        },
        IndirectCase {
            name: "list<f64>",
            type_defs: "(type $value (list float64))",
            values: f64_values
                .iter()
                .copied()
                .map(ComponentValue::F64)
                .collect(),
            elements: f64_values
                .iter()
                .map(|value| value.to_le_bytes().to_vec())
                .collect(),
            stride: 8,
            record_b: None,
        },
        IndirectCase {
            name: "list<record{a:u8,b:u64}>",
            type_defs: r#"
  (type $entry (record (field "a" u8) (field "b" u64)))
  (export "entry" (type $entry))
  (type $value (list $entry))
  (export "value" (type $value))
"#,
            values: records
                .iter()
                .map(|(a, b)| {
                    ComponentValue::Record(vec![
                        ("a".to_owned(), ComponentValue::U8(*a)),
                        ("b".to_owned(), ComponentValue::U64(*b)),
                    ])
                })
                .collect(),
            elements: records
                .iter()
                .map(|(a, b)| {
                    let mut bytes = vec![*a];
                    bytes.extend([0; 7]);
                    bytes.extend(b.to_le_bytes());
                    bytes
                })
                .collect(),
            stride: 16,
            record_b: Some(records.iter().map(|(_, b)| *b).collect()),
        },
    ];

    for case in cases {
        let IndirectCase {
            name,
            type_defs,
            values,
            elements,
            stride,
            record_b,
        } = case;
        let expected_body = elements.concat();
        let list_len = values.len() as u32;
        let head = 0x1112_1314_1516_1718u64;
        let mut args = vec![ComponentValue::U64(head), ComponentValue::List(values)];
        args.extend((0..15).map(|index| ComponentValue::U32(0x80 + index)));
        let (store, instance) = instantiate(
            &indirect_list_component(type_defs, &u64_list_and_15_u32_params(), false),
            &ComponentLinker::new(),
        )
        .await;
        instance
            .call(&store, "run", &args)
            .await
            .unwrap_or_else(|error| panic!("{name}: indirect lowering must succeed: {error}"));

        let outer = scalar_u32(&instance, &store, "captured-ptr", &[]).await;
        let inner = scalar_u32(&instance, &store, "word-at", &[outer + 8]).await;
        // Canonical ABI `lower_flat_values` allocates the indirect tuple before `store_list_into_range`:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            realloc_log(&instance, &store).await,
            vec![
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 8,
                    new_len: 80,
                },
                ReallocCall {
                    old_ptr: 0,
                    old_len: 0,
                    align: 8,
                    new_len: expected_body.len() as u32,
                },
            ],
            "{name}: outer 80-byte area followed by list body"
        );
        // Canonical ABI `lower_flat_values` tuple layout: u64 at +0, list handle at +8:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(outer, BUMP_START, "{name}: outer area pointer");
        assert_eq!(
            bytes_at(&instance, &store, outer, 8).await,
            head.to_le_bytes(),
            "{name}: u64 is at outer + 0"
        );
        assert_eq!(
            inner,
            BUMP_START + 80,
            "{name}: list pointer loaded from outer + 8"
        );
        assert_eq!(
            scalar_u32(&instance, &store, "word-at", &[outer + 12]).await,
            list_len,
            "{name}: list length loaded from outer + 12"
        );
        assert_eq!(
            scalar_u32(&instance, &store, "word-at", &[outer + 16]).await,
            0x80,
            "{name}: first trailing u32 at outer + 16"
        );
        assert_eq!(
            scalar_u32(&instance, &store, "word-at", &[outer + 72]).await,
            0x8e,
            "{name}: fifteenth trailing u32 at outer + 72"
        );
        // Canonical ABI `store_list_into_range` / `elem_size` byte layout:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            bytes_at(&instance, &store, inner, expected_body.len()).await,
            expected_body,
            "{name}: list body bytes"
        );
        for (index, expected_element) in elements.iter().enumerate() {
            // Canonical ABI `store_list_into_range` uses the aligned element stride:
            // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
            assert_eq!(
                bytes_at(
                    &instance,
                    &store,
                    inner + stride * index as u32,
                    expected_element.len(),
                )
                .await,
                *expected_element,
                "{name}: element {index} at stride {stride}"
            );
        }
        if let Some(record_b) = record_b {
            for (index, b) in record_b.iter().enumerate() {
                // Canonical ABI `store_record` aligns u64 field b to byte offset 8:
                // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
                assert_eq!(
                    bytes_at(&instance, &store, inner + 16 * index as u32 + 8, 8).await,
                    b.to_le_bytes(),
                    "{name}: record {index} b field at offset 8"
                );
            }
        }
        assert_eq!(
            scalar_u32(&instance, &store, "call-count", &[]).await,
            2,
            "{name}: scalar observers must not append realloc calls"
        );
    }
}

#[tokio::test]
async fn indirect_u32_list_and_16_u32_params_use_a_72_byte_outer_area() {
    let values = [0x0102_0304u32, 0xaabb_ccdd, 0x1020_3040];
    let expected_body = values
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect::<Vec<_>>();
    let mut args = vec![ComponentValue::List(
        values.iter().copied().map(ComponentValue::U32).collect(),
    )];
    args.extend((0..16).map(|index| ComponentValue::U32(0x90 + index)));
    let (store, instance) = instantiate(
        &indirect_list_component("(type $value (list u32))", &list_and_16_u32_params(), false),
        &ComponentLinker::new(),
    )
    .await;

    instance
        .call(&store, "run", &args)
        .await
        .expect("indirect list<u32> lowering must succeed");

    let outer = scalar_u32(&instance, &store, "captured-ptr", &[]).await;
    let inner = scalar_u32(&instance, &store, "word-at", &[outer]).await;
    // Canonical ABI `lower_flat_values` allocates an indirect 72-byte tuple before lowering
    // its list field with `lower_flat_list` / `store_list_into_range`:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        realloc_log(&instance, &store).await,
        vec![
            ReallocCall {
                old_ptr: 0,
                old_len: 0,
                align: 4,
                new_len: 72,
            },
            ReallocCall {
                old_ptr: 0,
                old_len: 0,
                align: 4,
                new_len: expected_body.len() as u32,
            },
        ],
        "hand-calculated 72-byte outer area followed by the 4n list body"
    );
    // Canonical ABI `lower_flat_values` places the list handle at outer + 0:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(outer, BUMP_START, "outer area pointer");
    assert_eq!(inner, BUMP_START + 72, "list pointer loaded from outer + 0");
    assert_eq!(
        scalar_u32(&instance, &store, "word-at", &[outer + 4]).await,
        values.len() as u32,
        "list length loaded from outer + 4"
    );
    assert_eq!(
        scalar_u32(&instance, &store, "word-at", &[outer + 8]).await,
        0x90,
        "first trailing u32 at outer + 8"
    );
    assert_eq!(
        scalar_u32(&instance, &store, "word-at", &[outer + 68]).await,
        0x9f,
        "sixteenth trailing u32 at outer + 68"
    );
    // Canonical ABI `store_list_into_range` uses `elem_size(u32) == 4` as the stride:
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert_eq!(
        bytes_at(&instance, &store, inner, expected_body.len()).await,
        expected_body
    );
    for (index, value) in values.iter().enumerate() {
        // Canonical ABI `store_list_into_range` increments by the 4-byte u32 stride:
        // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
        assert_eq!(
            bytes_at(&instance, &store, inner + 4 * index as u32, 4).await,
            value.to_le_bytes(),
            "element {index} must start at inner + 4 * {index}"
        );
    }
    assert_eq!(
        scalar_u32(&instance, &store, "call-count", &[]).await,
        2,
        "scalar observers must not append realloc calls"
    );
}

#[tokio::test]
async fn direct_list_lowering_rejects_a_misaligned_realloc_result() {
    let (store, instance) = instantiate(
        &direct_list_component("(type $value (list u64))", true),
        &ComponentLinker::new(),
    )
    .await;

    let error = instance
        .call(
            &store,
            "run",
            &[ComponentValue::List(vec![ComponentValue::U64(
                0x0102_0304_0506_0708,
            )])],
        )
        .await
        .expect_err("misaligned list realloc result must trap");
    // Canonical ABI `lower_flat_list` / `store_list_into_range` require a list body pointer
    // returned by realloc to be aligned to the u64 element alignment (8):
    // https://github.com/WebAssembly/component-model/blob/73b7ad51d3b5d6f1ef53c923d8c585e28b242bcc/design/mvp/CanonicalABI.md
    assert!(
        matches!(error, ComponentError::Trap(_)),
        "unexpected error: {error}"
    );
}
