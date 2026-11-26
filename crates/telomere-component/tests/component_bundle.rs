//! Component bundle (#201) integration tests: hash-keyed AoT
//! artifacts for the core modules embedded in a component, loaded
//! through the fail-closed bundle path.
//!
//! The whole file is gated on the `aot-bundle` feature; without it
//! there is nothing to test here.

#![cfg(feature = "aot-bundle")]

use std::ops::Range;

use sha2::{Digest, Sha256};
use telomere::{AotLoadPolicy, AotSigner, AotVerifier};
use telomere_component::bundle::enumerate_core_module_sections;
use telomere_component::{
    build_component_bundle, BundleError, ComponentEngine, ComponentLinker, ComponentValue, Store,
};

// A test signer/verifier pair: HMAC-SHA256 over the payload, built on
// the `sha2` dependency (the same pattern as the core artifact
// tests). Production embedders plug in a real signature scheme
// through the same traits; the container only carries opaque
// signature bytes plus a key id.
struct HmacKey(&'static str, &'static [u8]);

fn hmac_sha256(key: &[u8], message: &[u8]) -> Vec<u8> {
    let mut key_block = [0u8; 64];
    if key.len() > 64 {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(key_block.map(|b| b ^ 0x36));
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(key_block.map(|b| b ^ 0x5c));
    outer.update(inner_hash);
    outer.finalize().to_vec()
}

impl AotSigner for HmacKey {
    fn key_id(&self) -> &str {
        self.0
    }
    fn sign(&self, payload: &[u8]) -> Vec<u8> {
        hmac_sha256(self.1, payload)
    }
}

impl AotVerifier for HmacKey {
    fn key_id(&self) -> &str {
        self.0
    }
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        hmac_sha256(self.1, payload) == signature
    }
}

fn signer() -> HmacKey {
    HmacKey("test-key-1", b"correct horse battery staple")
}

fn wrong_key_same_id() -> HmacKey {
    HmacKey("test-key-1", b"Tr0ub4dor&3")
}

fn other_key() -> HmacKey {
    HmacKey("test-key-2", b"correct horse battery staple")
}

const ADD_COMPONENT: &str = r#"
(component
  (type $add (func (param "lhs" s32) (param "rhs" s32) (result s32)))

  (core module $core-add
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add)
  )
  (core instance $core-add-instance (instantiate $core-add))

  (func (export "add") (type $add)
    (canon lift (core func $core-add-instance "add"))
  )
)
"#;

/// The command-component shape used by `tests/cli_component.rs`
/// (core module + instantiate + canon lift), with the run function
/// exported directly so it can be called through the component API.
const RUN_COMPONENT: &str = r#"
(component
  (type $run-func (func (result s32)))

  (core module $run-core
    (func (export "run") (result i32)
      i32.const 42)
  )
  (core instance $run-core-inst (instantiate $run-core))
  (func (export "run") (type $run-func)
    (canon lift (core func $run-core-inst "run")))
)
"#;

const TWO_IDENTICAL_MODULES: &str = r#"
(component
  (type $add (func (param "lhs" s32) (param "rhs" s32) (result s32)))

  (core module
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add)
  )
  (core module
    (func (export "add") (param i32 i32) (result i32)
      local.get 0
      local.get 1
      i32.add)
  )
  (core instance $first (instantiate 0))
  (core instance $second (instantiate 1))

  (func (export "add1") (type $add)
    (canon lift (core func $first "add"))
  )
  (func (export "add2") (type $add)
    (canon lift (core func $second "add"))
  )
)
"#;

fn component_bytes(wat_source: &str) -> Vec<u8> {
    wat::parse_str(wat_source).expect("component wat must assemble")
}

fn allow_unsigned() -> AotLoadPolicy {
    AotLoadPolicy {
        allow_unsigned: true,
    }
}

// ---------------------------------------------------------------------------
// Byte-level dissection of the bundle container, for tamper tests.
// Layout: magic(8) version(4) component blob(u64 len + bytes)
// entries: count(u32), then per entry ordinal(u32) hash(32)
// artifact blob(u64 len + bytes); signature block: key id(u32 len +
// bytes) signature blob(u64 len + bytes).
// ---------------------------------------------------------------------------

struct EntrySpan {
    span: Range<usize>,
    hash: Range<usize>,
    artifact: Range<usize>,
}

struct BundleParts {
    component: Range<usize>,
    entries: Vec<EntrySpan>,
    sig_block: Range<usize>,
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn dissect(bundle: &[u8]) -> BundleParts {
    assert_eq!(&bundle[0..8], b"TELOBND1");
    let component_len = u64_at(bundle, 12) as usize;
    let component = 20..20 + component_len;
    let mut pos = component.end;
    let count = u32_at(bundle, pos) as usize;
    pos += 4;
    let mut entries = Vec::new();
    for _ in 0..count {
        let start = pos;
        pos += 4; // ordinal
        let hash = pos..pos + 32;
        pos += 32;
        let artifact_len = u64_at(bundle, pos) as usize;
        pos += 8;
        let artifact = pos..pos + artifact_len;
        pos += artifact_len;
        entries.push(EntrySpan {
            span: start..pos,
            hash,
            artifact,
        });
    }
    let sig_block = pos..bundle.len();
    BundleParts {
        component,
        entries,
        sig_block,
    }
}

/// Rebuilds an unsigned bundle carrying only the entries at
/// `keep` (indices into the dissected entry list), for
/// missing/extraneous-artifact surgery.
fn rebuild_unsigned(bundle: &[u8], parts: &BundleParts, keep: &[usize]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&bundle[0..12]); // magic + version
    out.extend_from_slice(&bundle[12..parts.component.end]); // component blob (len + bytes)
    out.extend_from_slice(&(keep.len() as u32).to_le_bytes());
    for &index in keep {
        out.extend_from_slice(&bundle[parts.entries[index].span.clone()]);
    }
    out.extend_from_slice(&0u32.to_le_bytes()); // empty key id
    out.extend_from_slice(&0u64.to_le_bytes()); // empty signature
    out
}

#[tokio::test]
async fn signed_bundle_round_trip_substitutes_installs_and_runs() {
    let bytes = component_bytes(ADD_COMPONENT);
    let bundle = build_component_bundle(&bytes, Some(&signer())).expect("build bundle");
    assert!(telomere_component::is_component_bundle(&bundle));

    let engine = ComponentEngine::new();
    let loaded = engine
        .compile_bundle(&bundle, Some(&signer()), AotLoadPolicy::default())
        .expect("load bundle");
    let report = loaded.report();
    assert_eq!(report.core_module_sections, 1);
    assert_eq!(report.substituted_ordinals, vec![0]);
    assert_eq!(report.parsed_ordinals, Vec::<u32>::new());

    let store = Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate_bundle(&loaded, &store, &linker)
        .await
        .expect("instantiate bundle");
    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call add");
    assert_eq!(result, vec![ComponentValue::S32(42)]);

    // The one embedded module was installed exactly once, and its
    // single defined function got a native blob.
    let installs = loaded.artifacts().install_reports();
    assert_eq!(installs.len(), 1);
    let report = installs[0].1;
    assert_eq!(report.installed + report.interpreted, 1);
    assert_eq!(report.installed, 1);
}

#[test]
fn loaded_bundle_reports_its_own_signature_state() {
    let bytes = component_bytes(ADD_COMPONENT);
    let engine = ComponentEngine::new();

    let signed = build_component_bundle(&bytes, Some(&signer())).expect("build signed bundle");
    let loaded = engine
        .compile_bundle(&signed, Some(&signer()), AotLoadPolicy::default())
        .expect("load signed bundle");
    assert!(!loaded.is_unsigned());
    assert_eq!(loaded.key_id(), "test-key-1");

    let unsigned = build_component_bundle(&bytes, None).expect("build unsigned bundle");
    let loaded = engine
        .compile_bundle(&unsigned, None, allow_unsigned())
        .expect("load unsigned bundle");
    assert!(loaded.is_unsigned());
    assert_eq!(loaded.key_id(), "");

    // A verifier supplied alongside the unsigned opt-in does not
    // change what the bundle itself carries.
    let loaded = engine
        .compile_bundle(&unsigned, Some(&signer()), allow_unsigned())
        .expect("load unsigned bundle with a verifier");
    assert!(loaded.is_unsigned());
    assert_eq!(loaded.key_id(), "");
}

#[tokio::test]
async fn cli_style_command_component_runs_via_bundle() {
    let bytes = component_bytes(RUN_COMPONENT);
    let bundle = build_component_bundle(&bytes, None).expect("build bundle");

    let engine = ComponentEngine::new();
    let loaded = engine
        .compile_bundle(&bundle, None, allow_unsigned())
        .expect("load bundle");
    assert_eq!(loaded.report().substituted_ordinals, vec![0]);

    let store = Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate_bundle(&loaded, &store, &linker)
        .await
        .expect("instantiate bundle");
    let result = instance.call(&store, "run", &[]).await.expect("call run");
    assert_eq!(result, vec![ComponentValue::S32(42)]);
    assert_eq!(loaded.artifacts().install_reports().len(), 1);
}

#[tokio::test]
async fn parse_is_demonstrably_skipped_for_substituted_sections() {
    let bytes = component_bytes(ADD_COMPONENT);
    let mut bundle = build_component_bundle(&bytes, None).expect("build bundle");
    let parts = dissect(&bundle);
    let sections = enumerate_core_module_sections(&bytes).expect("walk component");
    assert_eq!(sections.len(), 1);

    // Replace the section payload inside the bundle's embedded
    // component with zero bytes (deterministically unparseable: the
    // core module magic is gone) and re-key the entry to the hash of
    // the corrupted bytes. If the load path parsed the section, this
    // load would fail; substitution must carry it.
    let section_range = sections[0].range.clone();
    let abs =
        parts.component.start + section_range.start..parts.component.start + section_range.end;
    for byte in &mut bundle[abs.clone()] {
        *byte = 0;
    }
    let corrupted: Vec<u8> = bundle[abs.clone()].to_vec();
    let new_hash = Sha256::digest(&corrupted);
    bundle[parts.entries[0].hash.clone()].copy_from_slice(&new_hash);

    // Control: the corrupted component really is unparseable through
    // the plain path.
    let corrupted_component = bundle[parts.component.clone()].to_vec();
    let engine = ComponentEngine::new();
    assert!(
        engine.compile(&corrupted_component).is_err(),
        "corrupted section bytes must not parse"
    );

    let loaded = engine
        .compile_bundle(&bundle, None, allow_unsigned())
        .expect("bundle load must substitute, not parse");
    assert_eq!(loaded.report().substituted_ordinals, vec![0]);
    assert_eq!(loaded.report().parsed_ordinals, Vec::<u32>::new());

    let store = Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate_bundle(&loaded, &store, &linker)
        .await
        .expect("instantiate bundle");
    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(19), ComponentValue::I32(23)],
        )
        .await
        .expect("call add");
    assert_eq!(result, vec![ComponentValue::S32(42)]);
}

#[test]
fn hash_mismatch_is_a_typed_error() {
    let bytes = component_bytes(ADD_COMPONENT);
    let engine = ComponentEngine::new();

    // Flip a byte of the recorded key hash.
    let mut bundle = build_component_bundle(&bytes, None).expect("build bundle");
    let parts = dissect(&bundle);
    bundle[parts.entries[0].hash.start] ^= 0xff;
    let err = engine
        .compile_bundle(&bundle, None, allow_unsigned())
        .err()
        .expect("tampered hash must fail");
    assert!(
        matches!(err, BundleError::HashMismatch { ordinal: 0, .. }),
        "expected HashMismatch, got {err:?}"
    );

    // Flip a byte of the section bytes inside the embedded component
    // (the recorded hash no longer matches the section).
    let mut bundle = build_component_bundle(&bytes, None).expect("build bundle");
    let parts = dissect(&bundle);
    let sections = enumerate_core_module_sections(&bytes).expect("walk component");
    let abs = parts.component.start + sections[0].range.start;
    bundle[abs] ^= 0xff;
    let err = engine
        .compile_bundle(&bundle, None, allow_unsigned())
        .err()
        .expect("tampered section must fail");
    assert!(
        matches!(err, BundleError::HashMismatch { ordinal: 0, .. }),
        "expected HashMismatch, got {err:?}"
    );
}

#[test]
fn missing_artifact_is_a_typed_error() {
    let bytes = component_bytes(TWO_IDENTICAL_MODULES);
    let bundle = build_component_bundle(&bytes, None).expect("build bundle");
    let parts = dissect(&bundle);
    assert_eq!(parts.entries.len(), 2);

    // Drop the second entry entirely.
    let cut = rebuild_unsigned(&bundle, &parts, &[0]);
    let engine = ComponentEngine::new();
    let err = engine
        .compile_bundle(&cut, None, allow_unsigned())
        .err()
        .expect("missing artifact must fail");
    assert!(
        matches!(err, BundleError::MissingArtifact { ordinal: 1 }),
        "expected MissingArtifact(1), got {err:?}"
    );
}

#[test]
fn extraneous_artifact_is_a_typed_error() {
    let bytes = component_bytes(ADD_COMPONENT);
    let bundle = build_component_bundle(&bytes, None).expect("build bundle");
    let parts = dissect(&bundle);

    // Append a fabricated entry for ordinal 7 (reusing entry 0's
    // artifact bytes) ahead of the empty signature block.
    let mut out = Vec::new();
    out.extend_from_slice(&bundle[0..12]);
    out.extend_from_slice(&bundle[12..parts.component.end]);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&bundle[parts.entries[0].span.clone()]);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.extend_from_slice(&[0u8; 32]);
    let artifact = &bundle[parts.entries[0].artifact.clone()];
    out.extend_from_slice(&(artifact.len() as u64).to_le_bytes());
    out.extend_from_slice(artifact);
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes());

    let engine = ComponentEngine::new();
    let err = engine
        .compile_bundle(&out, None, allow_unsigned())
        .err()
        .expect("extraneous artifact must fail");
    assert!(
        matches!(err, BundleError::ExtraneousArtifact { ordinal: 7 }),
        "expected ExtraneousArtifact(7), got {err:?}"
    );
}

#[test]
fn signature_failures_are_typed_errors() {
    let bytes = component_bytes(ADD_COMPONENT);
    let engine = ComponentEngine::new();
    let signed = build_component_bundle(&bytes, Some(&signer())).expect("build signed bundle");

    // Signed bundle, no verifier.
    let err = engine
        .compile_bundle(&signed, None, AotLoadPolicy::default())
        .err()
        .expect("signed bundle without verifier must fail");
    assert!(
        matches!(err, BundleError::NoVerifier { .. }),
        "expected NoVerifier, got {err:?}"
    );

    // Verifier under a different key id.
    let err = engine
        .compile_bundle(&signed, Some(&other_key()), AotLoadPolicy::default())
        .err()
        .expect("key id mismatch must fail");
    assert!(
        matches!(err, BundleError::KeyIdMismatch { .. }),
        "expected KeyIdMismatch, got {err:?}"
    );

    // Right key id, wrong key material.
    let err = engine
        .compile_bundle(
            &signed,
            Some(&wrong_key_same_id()),
            AotLoadPolicy::default(),
        )
        .err()
        .expect("bad signature must fail");
    assert!(
        matches!(err, BundleError::SignatureInvalid { .. }),
        "expected SignatureInvalid, got {err:?}"
    );

    // Unsigned bundle under the default policy.
    let unsigned = build_component_bundle(&bytes, None).expect("build unsigned bundle");
    let err = engine
        .compile_bundle(&unsigned, None, AotLoadPolicy::default())
        .err()
        .expect("unsigned bundle must be refused by default");
    assert!(
        matches!(err, BundleError::UnsignedRefused),
        "expected UnsignedRefused, got {err:?}"
    );

    // Tampering with the component region or the artifact region of
    // a signed bundle breaks the one signature over the whole set.
    for region in ["component", "artifact"] {
        let mut tampered = signed.clone();
        let parts = dissect(&tampered);
        let at = match region {
            "component" => parts.component.start,
            _ => parts.entries[0].artifact.start,
        };
        tampered[at] ^= 0xff;
        let err = engine
            .compile_bundle(&tampered, Some(&signer()), AotLoadPolicy::default())
            .err()
            .expect("tampered signed bundle must fail");
        assert!(
            matches!(err, BundleError::SignatureInvalid { .. }),
            "tampered {region}: expected SignatureInvalid, got {err:?}"
        );
    }
}

#[test]
fn tamper_matrix_produces_typed_errors_and_never_panics() {
    let bytes = component_bytes(ADD_COMPONENT);
    let engine = ComponentEngine::new();
    let unsigned = build_component_bundle(&bytes, None).expect("build unsigned bundle");

    // Every strict prefix of a valid bundle is rejected.
    for len in 0..unsigned.len() {
        let result = engine.compile_bundle(&unsigned[..len], None, allow_unsigned());
        assert!(result.is_err(), "prefix of {len} bytes must be rejected");
    }

    let parts = dissect(&unsigned);
    let sections = enumerate_core_module_sections(&bytes).expect("walk component");
    let cases: Vec<(&str, usize, BundleError)> = vec![
        ("magic", 0, BundleError::BadMagic),
        (
            "version",
            8,
            BundleError::UnsupportedFormatVersion {
                found: u32::from_le_bytes([0xff, 1, 0, 0]),
                supported: 1,
            },
        ),
        (
            "core module section payload",
            parts.component.start + sections[0].range.start,
            BundleError::HashMismatch {
                ordinal: 0,
                expected: String::new(),
                actual: String::new(),
            },
        ),
    ];
    for (name, at, expected) in cases {
        let mut tampered = unsigned.clone();
        tampered[at] ^= 0xff;
        let err = engine
            .compile_bundle(&tampered, None, allow_unsigned())
            .err()
            .unwrap_or_else(|| panic!("tampered {name} must fail"));
        assert_eq!(
            std::mem::discriminant(&err),
            std::mem::discriminant(&expected),
            "tampered {name}: expected {expected:?}, got {err:?}"
        );
    }

    // Corrupting an inner artifact's bytes (unsigned dev bundle) is
    // caught by the #198 loader as a typed artifact error.
    let mut tampered = unsigned.clone();
    let parts = dissect(&tampered);
    let at = parts.entries[0].artifact.start + 16;
    tampered[at] ^= 0xff;
    let err = engine
        .compile_bundle(&tampered, None, allow_unsigned())
        .err()
        .expect("corrupted artifact must fail");
    assert!(
        matches!(err, BundleError::Artifact { ordinal: 0, .. }),
        "expected Artifact error, got {err:?}"
    );

    // Trailing garbage after the signature block.
    let mut tampered = unsigned.clone();
    tampered.extend_from_slice(b"garbage");
    let err = engine
        .compile_bundle(&tampered, None, allow_unsigned())
        .err()
        .expect("trailing bytes must fail");
    assert!(
        matches!(err, BundleError::TrailingBytes { extra: 7 }),
        "expected TrailingBytes, got {err:?}"
    );
    let _ = parts.sig_block;
}

#[tokio::test]
async fn identical_modules_embedded_twice_are_keyed_by_ordinal() {
    let bytes = component_bytes(TWO_IDENTICAL_MODULES);
    let sections = enumerate_core_module_sections(&bytes).expect("walk component");
    assert_eq!(sections.len(), 2);
    let first = &bytes[sections[0].range.clone()];
    let second = &bytes[sections[1].range.clone()];
    assert_eq!(first, second, "fixture must embed byte-identical modules");

    let bundle = build_component_bundle(&bytes, Some(&signer())).expect("build bundle");
    let parts = dissect(&bundle);
    assert_eq!(parts.entries.len(), 2);
    assert_eq!(
        bundle[parts.entries[0].hash.clone()],
        bundle[parts.entries[1].hash.clone()],
        "identical modules share a hash; the ordinal is the key"
    );

    let engine = ComponentEngine::new();
    let loaded = engine
        .compile_bundle(&bundle, Some(&signer()), AotLoadPolicy::default())
        .expect("load bundle");
    assert_eq!(loaded.report().substituted_ordinals, vec![0, 1]);
    assert_eq!(loaded.report().parsed_ordinals, Vec::<u32>::new());

    let store = Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate_bundle(&loaded, &store, &linker)
        .await
        .expect("instantiate bundle");
    for export in ["add1", "add2"] {
        let result = instance
            .call(
                &store,
                export,
                &[ComponentValue::I32(30), ComponentValue::I32(12)],
            )
            .await
            .expect("call export");
        assert_eq!(result, vec![ComponentValue::S32(42)], "export {export}");
    }
    assert_eq!(loaded.artifacts().install_reports().len(), 2);
}

/// A core module defined inside a nested component, exported by that
/// component, and aliased + instantiated by the parent. The alias
/// occupies a different core-module index than the definition, but the
/// bundle artifact keyed to the definition must still be installed
/// (Codex P2 on PR #315: artifact identity must survive core module
/// aliases).
const ALIASED_NESTED_MODULE: &str = r#"
(component
  (type $add (func (param "lhs" s32) (param "rhs" s32) (result s32)))

  (component $inner
    (core module $inner-core
      (func (export "add") (param i32 i32) (result i32)
        local.get 0
        local.get 1
        i32.add)
    )
    (export "core" (core module $inner-core))
  )
  (instance $inner-instance (instantiate $inner))
  (alias export $inner-instance "core" (core module $aliased-core))
  (core instance $aliased-instance (instantiate $aliased-core))

  (func (export "add") (type $add)
    (canon lift (core func $aliased-instance "add"))
  )
)
"#;

#[tokio::test]
async fn bundle_installs_artifact_through_core_module_alias() {
    let bytes = component_bytes(ALIASED_NESTED_MODULE);
    let sections = enumerate_core_module_sections(&bytes).expect("walk component");
    assert_eq!(sections.len(), 1);

    let bundle = build_component_bundle(&bytes, Some(&signer())).expect("build bundle");
    let engine = ComponentEngine::new();
    let loaded = engine
        .compile_bundle(&bundle, Some(&signer()), AotLoadPolicy::default())
        .expect("load bundle");
    assert_eq!(loaded.report().substituted_ordinals, vec![0]);
    assert_eq!(loaded.report().parsed_ordinals, Vec::<u32>::new());

    let store = Store::new();
    let linker = ComponentLinker::new();
    let instance = engine
        .instantiate_bundle(&loaded, &store, &linker)
        .await
        .expect("instantiate bundle");
    let result = instance
        .call(
            &store,
            "add",
            &[ComponentValue::I32(20), ComponentValue::I32(22)],
        )
        .await
        .expect("call add");
    assert_eq!(result, vec![ComponentValue::S32(42)]);

    // The module reaches instantiation through an alias, but its
    // artifact must still be installed exactly once.
    let installs = loaded.artifacts().install_reports();
    assert_eq!(
        installs.len(),
        1,
        "artifact must be installed through the core module alias, got {installs:?}"
    );
    let report = installs[0].1;
    assert_eq!(report.installed + report.interpreted, 1);
    assert_eq!(report.installed, 1);
}
