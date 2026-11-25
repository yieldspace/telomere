# Component bundles: hash-keyed AoT artifacts for embedded core modules

Contract for the component bundle container (#201), implemented in
`crates/telomere-component/src/bundle.rs` behind the `aot-bundle`
feature of `telomere-component`. Measurements live in
[benchmarks/component-bundle.md](../benchmarks/component-bundle.md);
the core artifact contract this builds on is
[aot-artifacts.md](aot-artifacts.md).

## Shape

A bundle is a component binary plus a set of core-module AoT
artifacts:

- The component binary is embedded verbatim and remains the source
  of truth for everything component-level. No component structure
  (types, canonical options, the instance graph) is serialized.
- Each artifact is a standalone #198 core artifact, byte-identical
  to what `build_aot_artifact` produces for the same core module —
  there is no bundle-specific artifact format.
- Each artifact is keyed by `(ordinal, SHA-256)`: the CoreModule
  section's depth-first encounter ordinal within the component
  (nested components included, matching the decoder's registration
  order) and the SHA-256 of that section's exact byte range. The
  ordinal is part of the key because two byte-identical modules
  embedded twice are distinct instances in the component index
  space; the hash binds the artifact to the exact bytes it was
  produced from.

## Container

`TELOBND1`, format version 1. All integers are little-endian.

| Field | Form |
| --- | --- |
| magic | 8 bytes `TELOBND1` |
| format version | u32 |
| component binary | u64 length + bytes |
| entry count | u32 (≤ 65536, ordinals strictly increasing) |
| per entry | ordinal u32, section SHA-256 (32 bytes), artifact (u64 length + bytes) |
| signature block | key id (u32 length + UTF-8 bytes), signature (u64 length + bytes) |

The signature covers every byte before the signature block — the
component binary and the whole keyed set — so component bytes and
native code cannot be mixed and matched across bundles. Signing and
verification use the #198 `AotSigner` / `AotVerifier` traits
(embedder-pluggable; ed25519 is the suggested default there).

Inner artifacts are unsigned by construction: the bundle signature
is the single trust root for the set. The loader opens inner
artifacts only after the bundle-level posture has passed, and the
#198 fingerprint and module-binding checks still run per artifact.

## Load path

Fail-closed, in this order (mirrors the #198 posture):

1. Parse only the outer envelope (magic, version, field lengths,
   signature-block lengths). Nothing is interpreted and no
   executable memory exists yet; opening a bundle takes no store.
2. Verify the bundle signature. A signed bundle requires a verifier
   with a matching key id and a valid signature
   (`NoVerifier` / `KeyIdMismatch` / `SignatureInvalid`). An
   unsigned bundle requires the embedder's explicit
   `AotLoadPolicy::allow_unsigned` opt-in (`UnsignedRefused`
   otherwise) — the dev-mode posture, never a default.
3. Pre-check the keyed set against the embedded component's
   CoreModule sections, before any decoding: a section with no
   artifact is `MissingArtifact`, a section whose bytes hash
   differently than its key is `HashMismatch`, an artifact keyed to
   a nonexistent section is `ExtraneousArtifact`. A hash mismatch
   is a typed error, never a silent re-parse: whether running
   unsigned, re-parsed code is acceptable is the embedder's
   decision, made by loading the bare component instead.
4. Open each artifact through the #198 loader.
5. Decode the component exactly as `ComponentEngine::compile`
   does, except each CoreModule section substitutes the artifact's
   deserialized module for `WasmParser::parse_module`. The
   substituted module passes the same decoder contract validation
   as a parsed one. Substitution is total: the decoder fails
   closed (`ComponentParseError::BundleSubstitutionMissing`) rather
   than falling back to parsing if the pre-check invariant is ever
   broken internally.
6. At instantiation (`ComponentEngine::instantiate_bundle`), each
   defined core instance created from a substituted module gets
   its artifact installed with `install_aot_artifact`, exactly as
   in the core path; install reports accumulate on the loaded
   bundle's artifact set.

The load report (`BundleLoadReport`) accounts for every CoreModule
section as substituted or parsed; a successful bundle load
substitutes all of them.

## Producer

`build_component_bundle(component, signer)` enumerates the
component's CoreModule sections, parses each embedded module (it
never instantiates, so no guest code runs), runs the core AoT
pipeline per section, and emits the keyed set, signed as a whole
when a signer is given.

## Non-goals and limits

- No serialization of component-level structures (above).
- No canonical-ABI native codegen; lift/lower is unchanged.
- No change to component semantics under #117/#118/#167; the
  substituted module is the same `Module` value the parser would
  have produced.
- The section walker mirrors the decoder's nesting limit (100):
  sections deeper than the limit get no ordinal, and such
  components fail at decode with the decoder's own error.
- CLI wiring (`telomere compile` growing a bundle mode) is a
  follow-up: the producer and loader here are the seam it needs,
  and the CLI surface is being reworked concurrently (#199).
