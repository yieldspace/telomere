#![no_main]

use libfuzzer_sys::fuzz_target;
use telomere::{open_aot_artifact, AotLoadPolicy, AotVerifier};

/// Decoder-fuzzing verifier: accepts every signature so fuzz inputs
/// reach the container decoder behind the signature check. This is
/// not a trust decision — production loaders supply a real verifier;
/// here the point is that the *decoder* (envelope, fingerprint,
/// binding, function table) is total over arbitrary bytes.
struct AcceptAll;

impl AotVerifier for AcceptAll {
    fn key_id(&self) -> &str {
        // Matches nothing in particular; key-id mismatches are also
        // a valid typed rejection. Seeds carry this id (see the
        // corpus generator in the telomere replay tests) so generated
        // corpora reach the payload decoder.
        "fuzz"
    }
    fn verify(&self, _payload: &[u8], _signature: &[u8]) -> bool {
        true
    }
}

fuzz_target!(|input: &[u8]| {
    // The AoT container loader (#198) is total: every input must
    // produce either an opened artifact or a typed `AotError`, never
    // a panic. Both postures are exercised: signed-with-verifier and
    // the explicit unsigned opt-in.
    let _ = open_aot_artifact(input, Some(&AcceptAll), AotLoadPolicy::default());
    let _ = open_aot_artifact(
        input,
        None,
        AotLoadPolicy {
            allow_unsigned: true,
        },
    );
});
