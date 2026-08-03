#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere_component::fuzz_canonical_lift_args_from_bytes;

const FIXTURE_BYTES: &[u8] = include_bytes!("../fixtures/canon_abi.wasm");

// The input byte layout lives in `telomere_component`'s feature-gated fuzz
// adapter rather than here, so that this target and the stable corpus replay
// decode a committed input identically. A reproducer that decoded differently
// in the two places would silently stop reproducing.
fuzz_target!(|input: &[u8]| {
    let _ = fuzz_canonical_lift_args_from_bytes(FIXTURE_BYTES, input);
});
