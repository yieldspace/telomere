#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere::deserialize_module;

fuzz_target!(|input: &[u8]| {
    // The precompiled-module decoder (#196) is total: every input must
    // produce either a module or a typed `PrecompiledError`, never a
    // panic. `materialize` runs inside deserialization, so this also
    // covers reconstruction of the lowered instruction streams.
    let _ = deserialize_module(input);
});
