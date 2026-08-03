#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere_component::ComponentEngine;

fuzz_target!(|input: &[u8]| {
    let _ = ComponentEngine::new().compile(input);
});
