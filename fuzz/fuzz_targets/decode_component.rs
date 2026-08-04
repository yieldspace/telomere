#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere_component::ComponentEngine;

const FUZZ_PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;

fuzz_target!(|input: &[u8]| {
    let input = input.to_vec();
    // A component's embedded core module section re-enters the core parser, so this target
    // reaches the same instrumented recursion as the core parser target. Sanitizer and other
    // instrumented builds use larger parser frames than the documented optimized-build stack
    // budget. Like the corpus replay harness, this larger fuzz thread is an instrumentation
    // accommodation, not a change to the parser's stack contract.
    std::thread::Builder::new()
        .stack_size(FUZZ_PARSER_STACK_BYTES)
        .spawn(move || {
            let _ = ComponentEngine::new().compile(&input);
        })
        .expect("spawning component decoder fuzz thread")
        .join()
        .expect("component decoder fuzz thread must not panic");
});
