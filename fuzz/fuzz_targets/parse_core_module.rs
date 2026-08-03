#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere::{IoReadBinaryReader, WasmParser};

const FUZZ_PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

fuzz_target!(|input: &[u8]| {
    let input = input.to_vec();
    // Sanitizer and other instrumented builds use larger parser frames than the documented
    // optimized-build stack budget. Like the corpus replay harness, this larger fuzz thread is
    // an instrumentation accommodation, not a change to the parser's stack contract.
    std::thread::Builder::new()
        .stack_size(FUZZ_PARSER_STACK_BYTES)
        .spawn(move || {
            let mut reader = IoReadBinaryReader::from(&input[..]);
            let mut parser = WasmParser::new(&mut reader);
            let _ = parser.parse_module();
        })
        .expect("spawning core parser fuzz thread")
        .join()
        .expect("core parser fuzz thread must not panic");
});
