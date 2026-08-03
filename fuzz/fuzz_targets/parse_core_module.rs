#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere::{IoReadBinaryReader, WasmParser};

fuzz_target!(|input: &[u8]| {
    let mut reader = IoReadBinaryReader::from(input);
    let mut parser = WasmParser::new(&mut reader);
    let _ = parser.parse_module();
});
