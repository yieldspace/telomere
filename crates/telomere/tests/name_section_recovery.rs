use std::io::{ErrorKind, Read};

use futures::executor::block_on;
use telomere::{
    instantiate, run_module_function, IoReadBinaryReader, Registry, ResultValue, Store, VMResult,
    WasmParser, WasmParserError, WasmValue,
};

fn parse_module(bytes: &[u8]) -> Result<telomere::Module, WasmParserError> {
    let mut reader = IoReadBinaryReader::from(bytes);
    WasmParser::new(&mut reader).parse_module()
}

fn module_header() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version
    ]
}

fn append_section(module: &mut Vec<u8>, id: u8, contents: &[u8]) {
    assert!(
        contents.len() < 128,
        "fixture section must use a one-byte LEB size"
    );
    module.push(id);
    module.push(contents.len() as u8);
    module.extend_from_slice(contents);
}

fn append_u32_leb(bytes: &mut Vec<u8>, mut value: u32) {
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

struct ShortRead<'a> {
    bytes: &'a [u8],
    max_read: usize,
}

impl Read for ShortRead<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let len = self.bytes.len().min(self.max_read).min(buf.len());
        buf[..len].copy_from_slice(&self.bytes[..len]);
        self.bytes = &self.bytes[len..];
        Ok(len)
    }
}

fn append_answer_sections(module: &mut Vec<u8>) {
    append_section(module, 1, &[0x01, 0x60, 0x00, 0x01, 0x7f]); // () -> i32
    append_section(module, 3, &[0x01, 0x00]); // one function of type 0
    append_section(
        module,
        7,
        &[0x01, 0x06, b'a', b'n', b's', b'w', b'e', b'r', 0x00, 0x00],
    );
    append_section(module, 10, &[0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b]);
}

fn module_with_malformed_name_body() -> Vec<u8> {
    let mut module = module_header();
    append_section(
        &mut module,
        0,
        &[
            0x04, b'n', b'a', b'm', b'e', // custom section name
            0x00, 0x00, 0x00, // module-name subsection with an invalid declared size
            0xde, 0xad, // bytes that recovery must skip before the type section
        ],
    );
    append_answer_sections(&mut module);
    module
}

fn append_valid_name_section(module: &mut Vec<u8>) {
    append_section(
        module,
        0,
        &[
            0x04, b'n', b'a', b'm', b'e', // custom section name
            0x00, 0x04, 0x03, b'f', b'o', b'o', // module-name subsection
        ],
    );
}

fn append_malformed_name_section_with_recovery_bytes(
    module: &mut Vec<u8>,
    skip_size: usize,
    available_recovery_bytes: usize,
) {
    assert!(available_recovery_bytes <= skip_size);
    let declared_size = 5 + 3 + skip_size;
    module.push(0);
    append_u32_leb(
        module,
        u32::try_from(declared_size).expect("fixture section size must fit in u32"),
    );
    module.extend_from_slice(&[
        0x04, b'n', b'a', b'm', b'e', // custom section name
        0x00, 0x00, 0x00, // malformed module-name subsection
    ]);
    module.resize(module.len() + available_recovery_bytes, 0);
}

fn module_with_truncated_name_section_after_recovery_skip(
    available_recovery_bytes: usize,
) -> Vec<u8> {
    let declared_skip_size = available_recovery_bytes + 1;

    let mut module = module_header();
    append_malformed_name_section_with_recovery_bytes(
        &mut module,
        declared_skip_size,
        available_recovery_bytes,
    );
    module
}

#[test]
fn malformed_name_body_is_ignored_and_resynchronizes_at_the_section_boundary() {
    let module = parse_module(&module_with_malformed_name_body())
        .expect("a malformed name body must not reject the core module");

    assert!(module.name.is_none());
    assert_eq!(module.fts.0.len(), 1);
    assert_eq!(module.functions.len(), 1);
    assert_eq!(module.exs.0.len(), 1);

    let store = Store::new();
    let registry = Registry::new();
    let instance = match block_on(instantiate(module, &store, &registry)) {
        VMResult::Success(instance) => instance,
        result => panic!("the re-synchronized module must instantiate, got {result:?}"),
    };
    match block_on(run_module_function(
        &instance,
        &store,
        "answer",
        &ResultValue::new(vec![]),
    )) {
        VMResult::Success(values) => {
            assert_eq!(values, ResultValue::new(vec![WasmValue::I32(42)]));
        }
        result => panic!("the subsequent export must run, got {result:?}"),
    }
}

#[test]
fn valid_name_section_is_retained() {
    let mut module = module_header();
    append_valid_name_section(&mut module);
    append_answer_sections(&mut module);

    let module = parse_module(&module).expect("a valid name section must parse");
    assert_eq!(
        module
            .name
            .as_ref()
            .and_then(|names| names.module_name.as_ref())
            .map(|name| name.0.as_str()),
        Some("foo")
    );
}

#[test]
fn malformed_name_section_does_not_clear_a_previous_valid_name_section() {
    let mut module = module_header();
    append_valid_name_section(&mut module);
    append_section(
        &mut module,
        0,
        &[
            0x04, b'n', b'a', b'm', b'e', // custom section name
            0x00, 0x00, 0x00, // malformed module-name subsection
            0xde, 0xad,
        ],
    );
    append_answer_sections(&mut module);

    let module = parse_module(&module).expect("the malformed section must be ignored");
    assert_eq!(
        module
            .name
            .as_ref()
            .and_then(|names| names.module_name.as_ref())
            .map(|name| name.0.as_str()),
        Some("foo")
    );
}

#[test]
fn oversized_outer_name_section_is_a_hard_error() {
    let mut module = module_header();
    append_answer_sections(&mut module);
    module.extend_from_slice(&[
        0x00, 0x06, // custom section declares six payload bytes
        0x04, b'n', b'a', b'm', b'e', // but provides only the five-byte name
    ]);

    match parse_module(&module) {
        Err(WasmParserError::IoError(error)) => {
            assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        }
        Err(error) => panic!("expected a truncated outer-section error, got {error:?}"),
        Ok(_) => panic!("an oversized custom section must be rejected"),
    }
}

#[test]
fn truncated_name_sections_across_skip_chunk_boundary_are_hard_errors() {
    for available_recovery_bytes in [4 * 1024 - 1, 4 * 1024 + 1] {
        match parse_module(&module_with_truncated_name_section_after_recovery_skip(
            available_recovery_bytes,
        )) {
            Err(WasmParserError::IoError(error)) => {
                assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
            }
            Err(error) => {
                panic!(
                    "expected a truncated outer-section error after {available_recovery_bytes} recovery bytes, got {error:?}"
                )
            }
            Ok(_) => {
                panic!(
                    "a name section truncated after {available_recovery_bytes} recovery bytes must be rejected"
                )
            }
        }
    }
}

#[test]
fn malformed_name_recovery_fills_skip_chunks_across_short_reads() {
    let mut bytes = module_header();
    let skip_size = 4 * 1024 + 1;
    append_malformed_name_section_with_recovery_bytes(&mut bytes, skip_size, skip_size);
    append_answer_sections(&mut bytes);

    let mut reader = IoReadBinaryReader::from(ShortRead {
        bytes: &bytes,
        max_read: 127,
    });
    let module = WasmParser::new(&mut reader)
        .parse_module()
        .expect("short reads must not make an intact custom section look truncated");

    assert!(module.name.is_none());
    assert_eq!(module.functions.len(), 1);
    assert_eq!(module.exs.0.len(), 1);
}

#[test]
fn malformed_custom_section_name_is_a_hard_error() {
    let mut module = module_header();
    append_section(&mut module, 0, &[0x01]); // name length without its byte

    match parse_module(&module) {
        Err(WasmParserError::IoError(error)) => {
            assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        }
        Err(error) => panic!("expected a malformed custom-name error, got {error:?}"),
        Ok(_) => panic!("a malformed custom section name must be rejected"),
    }
}
