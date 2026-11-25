#![cfg(all(
    feature = "jit",
    any(
        all(target_os = "macos", target_arch = "aarch64"),
        all(any(target_os = "macos", target_os = "linux"), target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "riscv64", target_env = "gnu")
    )
))]

//! Container, trust, and load-path tests for the AoT artifact format
//! (#198): signed native-code bundles with an exact-build fingerprint
//! and a fail-closed loader.

use futures::executor::block_on;
use sha2::{Digest, Sha256};
use telomere::{
    build_aot_artifact, current_fingerprint, install_aot_artifact, instantiate, is_aot_artifact,
    open_aot_artifact, run_module_function, AotError, AotLoadPolicy, AotSigner, AotVerifier,
    IoReadBinaryReader, JitConfig, Module, Registry, ResultValue, RuntimeConfig, Store, VMResult,
    WasmParser, WasmValue,
};

// A test signer/verifier pair: HMAC-SHA256 over the payload, built on
// the dev-dependency `sha2`. Production embedders plug in a real
// signature scheme (ed25519 suggested) through the same traits; the
// container only carries opaque signature bytes plus a key id.
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

fn parse_module(wat_source: &str) -> Module {
    let bytes = wat::parse_str(wat_source).expect("test wat must assemble");
    let mut reader = IoReadBinaryReader::from(bytes.as_slice());
    WasmParser::new(&mut reader)
        .parse_module()
        .expect("test module must parse")
}

/// fib exercises FuncAddr relocations (direct recursive calls);
/// popcount exercises Stub relocations.
const RICH_MODULE: &str = r#"
(module
  (func $fib (export "fib") (param i32) (result i32)
    (if (result i32) (i32.lt_s (local.get 0) (i32.const 2))
      (then (local.get 0))
      (else
        (i32.add
          (call $fib (i32.sub (local.get 0) (i32.const 1)))
          (call $fib (i32.sub (local.get 0) (i32.const 2)))))))
  (func (export "pop") (param i32) (result i32)
    (i32.popcnt (local.get 0)))
  (func (export "boom") (result i32)
    (i32.div_s (i32.const 1) (i32.const 0)))
)
"#;

fn run_i32_on(store: &Store, instance: &telomere::InstanceHandle, export: &str, arg: i32) -> i32 {
    let args = ResultValue::new(vec![WasmValue::I32(arg)]);
    match block_on(run_module_function(instance, store, export, &args)) {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(v)) => *v,
            other => panic!("expected one i32 result, got {other:?}"),
        },
        result => panic!("export must run, got {result:?}"),
    }
}

/// A store with native execution enabled. `Store::new()` leaves
/// `runtime_config.jit.enabled` false, and the native entry path
/// refuses cached code in that posture — execution tests must use
/// this store or the installed blobs are never actually entered.
fn jit_store() -> Store {
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: true };
    Store::new_with_runtime_config(runtime_config)
}

fn run_export_i32(store: &Store, instance: &telomere::InstanceHandle, export: &str) -> i32 {
    let args = ResultValue::new(vec![]);
    match block_on(run_module_function(instance, store, export, &args)) {
        VMResult::Success(values) => match values.iter().next() {
            Some(WasmValue::I32(v)) => *v,
            other => panic!("expected one i32 result, got {other:?}"),
        },
        result => panic!("export must run, got {result:?}"),
    }
}

fn instantiate_on(store: &Store, module: Module) -> telomere::InstanceHandle {
    let registry = Registry::new();
    match block_on(instantiate(module, store, &registry)) {
        VMResult::Success(instance) => instance,
        result => panic!("module must instantiate, got {result:?}"),
    }
}

#[test]
fn signed_round_trip_installs_and_executes_native_code() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");
    assert!(is_aot_artifact(&bytes));

    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("signed artifact must open");
    assert!(!opened.is_unsigned());
    assert_eq!(opened.key_id(), "test-key-1");
    assert!(!opened.functions().is_empty());
    let total_relocs: usize = opened.functions().iter().map(|f| f.reloc_count()).sum();
    assert!(total_relocs > 0, "corpus module must produce relocations");
    let trap_sites: usize = opened.functions().iter().map(|f| f.trap_site_count()).sum();
    assert!(trap_sites > 0, "artifact must carry trap-site tables");

    let store = jit_store();
    assert!(
        store.runtime_config().jit.enabled,
        "execution tests must run with native execution enabled"
    );
    let instance = instantiate_on(&store, opened.module().clone());
    let report = install_aot_artifact(&store, &instance, &opened).expect("install must succeed");
    assert_eq!(report.installed, opened.functions().len());
    assert_eq!(
        store.jit_cache_stats().compiled_functions,
        report.installed,
        "installed blobs must be resident in the store cache"
    );

    assert_eq!(run_i32_on(&store, &instance, "fib", 10), 55);
    assert_eq!(run_i32_on(&store, &instance, "pop", 0b1011), 3);
    // A trap in installed native code is a trap, not a host crash, and
    // the store keeps working afterwards.
    let args = ResultValue::new(vec![]);
    let trapped = block_on(run_module_function(&instance, &store, "boom", &args));
    assert!(
        !matches!(trapped, VMResult::Success(_)),
        "division by zero must trap, got {trapped:?}"
    );
    assert_eq!(run_i32_on(&store, &instance, "fib", 6), 8);
}

#[test]
fn two_instances_get_independently_patched_copies() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");
    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("signed artifact must open");

    let store = jit_store();
    let first = instantiate_on(&store, opened.module().clone());
    let second = instantiate_on(&store, opened.module().clone());
    let first_report = install_aot_artifact(&store, &first, &opened).expect("first install");
    let second_report = install_aot_artifact(&store, &second, &opened).expect("second install");
    assert_eq!(first_report.installed, second_report.installed);
    assert_eq!(
        store.jit_cache_stats().compiled_functions,
        first_report.installed + second_report.installed,
        "each instance must hold its own patched copies"
    );
    // Recursive calls resolve through each instance's own funcaddrs.
    assert_eq!(run_i32_on(&store, &first, "fib", 10), 55);
    assert_eq!(run_i32_on(&store, &second, "fib", 10), 55);
}

#[test]
fn unsigned_artifacts_are_refused_by_default() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, None).expect("unsigned artifact must build");
    let refused = open_aot_artifact(&bytes, None, AotLoadPolicy::default());
    assert!(
        matches!(refused, Err(AotError::UnsignedRefused)),
        "default policy must refuse unsigned artifacts, got {refused:?}"
    );
    let opened = open_aot_artifact(
        &bytes,
        None,
        AotLoadPolicy {
            allow_unsigned: true,
        },
    )
    .expect("explicit opt-in must open unsigned artifacts");
    assert!(opened.is_unsigned());
}

#[test]
fn wrong_key_and_missing_verifier_are_rejected() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");

    let wrong_secret = HmacKey("test-key-1", b"wrong secret");
    let result = open_aot_artifact(&bytes, Some(&wrong_secret), AotLoadPolicy::default());
    assert!(
        matches!(result, Err(AotError::SignatureInvalid { .. })),
        "wrong secret must fail verification, got {result:?}"
    );

    let wrong_id = HmacKey("test-key-2", b"correct horse battery staple");
    let result = open_aot_artifact(&bytes, Some(&wrong_id), AotLoadPolicy::default());
    assert!(
        matches!(result, Err(AotError::KeyIdMismatch { .. })),
        "wrong key id must be rejected, got {result:?}"
    );

    let result = open_aot_artifact(&bytes, None, AotLoadPolicy::default());
    assert!(
        matches!(result, Err(AotError::NoVerifier { .. })),
        "signed artifact without a verifier must be rejected, got {result:?}"
    );
}

/// Flips one byte and requires a typed rejection (never a panic,
/// never an opening). Returns the error for variant assertions.
fn expect_rejected(bytes: &[u8], offset: usize) -> AotError {
    let mut tampered = bytes.to_vec();
    tampered[offset] ^= 0x01;
    match open_aot_artifact(&tampered, Some(&signer()), AotLoadPolicy::default()) {
        Err(error) => error,
        Ok(_) => panic!("tampered artifact at offset {offset} must be rejected"),
    }
}

#[test]
fn tamper_matrix_is_rejected_with_typed_errors() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");

    // Magic.
    assert!(matches!(expect_rejected(&bytes, 0), AotError::BadMagic));
    // A bit flip anywhere in the signed payload or the signature must
    // fail verification (the signature covers the whole container).
    for offset in [16, bytes.len() / 3, bytes.len() / 2, bytes.len() - 1] {
        let error = expect_rejected(&bytes, offset);
        assert!(
            matches!(error, AotError::SignatureInvalid { .. }),
            "flip at {offset} must fail signature verification, got {error:?}"
        );
    }
    // Truncation at every structural boundary is a typed error.
    for cut in [8, 12, bytes.len() / 2, bytes.len() - 1] {
        let result = open_aot_artifact(&bytes[..cut], Some(&signer()), AotLoadPolicy::default());
        assert!(result.is_err(), "truncation at {cut} must be rejected");
    }
    // Trailing garbage after the signature block is rejected.
    let mut trailed = bytes.clone();
    trailed.push(0);
    let result = open_aot_artifact(&trailed, Some(&signer()), AotLoadPolicy::default());
    assert!(
        matches!(result, Err(AotError::TrailingBytes { .. })),
        "trailing bytes must be rejected, got {result:?}"
    );
    // Random garbage never panics the decoder.
    let mut state: u64 = 0x853c_49e6_748f_ea9b;
    for _ in 0..512 {
        let len = (state % 256) as usize;
        let mut garbage = vec![0u8; len];
        for byte in &mut garbage {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            *byte = (state >> 33) as u8;
        }
        let _ = open_aot_artifact(&garbage, Some(&signer()), AotLoadPolicy::default());
        let _ = open_aot_artifact(
            &garbage,
            None,
            AotLoadPolicy {
                allow_unsigned: true,
            },
        );
    }
}

#[test]
fn fingerprint_mismatch_names_both_fingerprints() {
    // Rebuild an artifact whose fingerprint block was rewritten to a
    // different build's values, then re-sign it: signature passes,
    // fingerprint check must fire and name both fingerprints.
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");
    let mut tampered = bytes.clone();
    // The fingerprint's crate-version string lives early in the
    // payload; flipping a byte there changes a fingerprint field.
    let needle = env!("CARGO_PKG_VERSION").as_bytes();
    let pos = tampered
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("fingerprint must embed the crate version");
    tampered[pos] ^= 0x01;
    // Re-sign the tampered container so the signature check passes
    // and the fingerprint check is what fires. The signed region ends
    // where the signature block's key-id string begins.
    let key_id = b"test-key-1";
    let key_pos = tampered
        .windows(key_id.len())
        .rposition(|window| window == key_id)
        .expect("signature block must embed the key id");
    let signed_region_end = key_pos - 1; // key-id length byte
    let new_signature = signer().sign(&tampered[..signed_region_end]);
    let sig_start = key_pos + key_id.len() + 1; // + signature length byte
    tampered[sig_start..sig_start + 32].copy_from_slice(&new_signature);

    let result = open_aot_artifact(&tampered, Some(&signer()), AotLoadPolicy::default());
    match result {
        Err(AotError::FingerprintMismatch { artifact, current }) => {
            assert_ne!(*artifact, *current);
            assert_eq!(*current, current_fingerprint());
        }
        other => panic!("expected fingerprint mismatch, got {other:?}"),
    }
}

#[test]
fn mismatched_instance_is_rejected_without_touching_cache() {
    let module = parse_module(RICH_MODULE);
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");
    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("signed artifact must open");

    let other = parse_module("(module (func (export \"x\") (result i32) (i32.const 1)))");
    let store = jit_store();
    let instance = instantiate_on(&store, other);
    let before = store.jit_cache_stats().compiled_functions;
    let result = install_aot_artifact(&store, &instance, &opened);
    assert!(
        matches!(result, Err(AotError::InstanceMismatch)),
        "installing into a different module's instance must fail, got {result:?}"
    );
    assert_eq!(
        store.jit_cache_stats().compiled_functions,
        before,
        "a failed install must leave the cache untouched"
    );
}

#[test]
fn install_into_importing_instance_is_rejected_and_provider_is_untouched() {
    // The provider's exported `main` calls its own funcidx 1 and
    // returns 99. The importer resolves funcidx 0 to that provider
    // function and defines a funcidx 1 returning 42. The artifact
    // module has the same function count and the same lowered body at
    // every index as the importer's index space — but its funcidx 0
    // is a function it defines itself. Installing it into the
    // importer must fail: the blob for funcidx 0 would otherwise be
    // installed under the provider function's address, patched
    // against the importer's function table, and the provider's
    // `main` would start returning 42.
    let provider = parse_module(
        "(module (func (export \"main\") (result i32) (call 1)) \
         (func (result i32) (i32.const 99)))",
    );
    let importer = parse_module(
        "(module (import \"p\" \"main\" (func (result i32))) \
         (func (export \"run\") (result i32) (i32.const 42)))",
    );
    let artifact_module =
        parse_module("(module (func (result i32) (call 1)) (func (result i32) (i32.const 42)))");
    let bytes = build_aot_artifact(&artifact_module, Some(&signer())).expect("artifact must build");
    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("signed artifact must open");

    let store = jit_store();
    let provider_instance = instantiate_on(&store, provider);
    let mut registry = Registry::new();
    registry.register("p", provider_instance.clone());
    let importer_instance = match block_on(instantiate(importer, &store, &registry)) {
        VMResult::Success(instance) => instance,
        result => panic!("importer must instantiate, got {result:?}"),
    };

    let before = store.jit_cache_stats().compiled_functions;
    let result = install_aot_artifact(&store, &importer_instance, &opened);
    assert!(
        matches!(result, Err(AotError::InstanceMismatch)),
        "installing into an importing instance must fail, got {result:?}"
    );
    assert_eq!(
        store.jit_cache_stats().compiled_functions,
        before,
        "a failed install must leave the cache untouched"
    );
    assert_eq!(
        run_export_i32(&store, &provider_instance, "main"),
        99,
        "the provider function's behavior must be unchanged"
    );
}

#[test]
fn install_succeeds_after_start_function_populated_the_cache() {
    // With native execution enabled, instantiation runs the start
    // function, and lazy compilation caches the functions it reaches
    // before `install_aot_artifact` can be called. Installation must
    // accept entries that are already cached instead of failing the
    // whole batch as a budget error.
    let module = parse_module(
        "(module (func $get (result i32) (i32.const 7)) \
         (func $start (drop (call $get))) (start $start) \
         (func (export \"get\") (result i32) (call $get)))",
    );
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("artifact must build");
    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("signed artifact must open");

    let store = jit_store();
    let instance = instantiate_on(&store, opened.module().clone());
    let cached_before = store.jit_cache_stats().compiled_functions;
    assert!(
        cached_before > 0,
        "the start function must have populated the cache, got {cached_before}"
    );
    let report = install_aot_artifact(&store, &instance, &opened)
        .expect("install after the start function must succeed");
    assert_eq!(report.installed, opened.functions().len());
    assert_eq!(run_export_i32(&store, &instance, "get"), 7);
}

#[test]
fn producer_enforces_the_loader_function_limit() {
    // One more defined function than the loader's MAX_FUNCTIONS: the
    // producer must fail instead of returning an artifact this same
    // build cannot open.
    let mut wat = String::from("(module (type (func))");
    for _ in 0..=65_536 {
        wat.push_str(" (func (type 0) nop)");
    }
    wat.push(')');
    let module = parse_module(&wat);
    let result = build_aot_artifact(&module, None);
    assert!(
        matches!(
            result,
            Err(AotError::LimitExceeded {
                what: "function count",
                ..
            })
        ),
        "production over the container limit must fail, got {result:?}"
    );
}

// ---------------------------------------------------------------------------
// #199: the offline producer must never execute guest code, and the
// load path must provably skip the front-end phases it replaces.

#[test]
fn building_an_artifact_never_executes_guest_code() {
    // The start function traps unconditionally. If the producer
    // instantiated the module the way a naive "instantiate in a
    // scratch store, then emit" compiler would, building the
    // artifact would run the start function and fail; the producer
    // instead works from module-level facts only, so the build
    // succeeds and the trap never fires.
    let module = parse_module(
        "(module \
           (func (export \"answer\") (result i32) i32.const 42) \
           (func unreachable) \
           (start 1))",
    );

    // Sanity: the probe is live — instantiating the module really
    // does run the start function and fail.
    let store = Store::new();
    let registry = Registry::new();
    let outcome = block_on(instantiate(module.clone(), &store, &registry));
    assert!(
        !matches!(outcome, VMResult::Success(_)),
        "instantiating a module whose start traps must fail, got {outcome:?}"
    );

    let unsigned = build_aot_artifact(&module, None).expect("unsigned build must succeed");
    assert!(is_aot_artifact(&unsigned));
    let signed = build_aot_artifact(&module, Some(&signer())).expect("signed build must succeed");
    assert!(is_aot_artifact(&signed));
}

#[test]
fn artifact_load_install_and_call_skip_frontend_phases() {
    let module = parse_module(
        "(module \
           (func (export \"add\") (param i32 i32) (result i32) \
             local.get 0 local.get 1 i32.add) \
           (func (export \"seven\") (result i32) i32.const 7))",
    );
    let bytes = build_aot_artifact(&module, Some(&signer())).expect("build must succeed");

    // Open: the module must come from the artifact front-end
    // (deserialize), never from parse+optimize.
    let before = telomere::frontend_phase_counters();
    let opened = open_aot_artifact(&bytes, Some(&signer()), AotLoadPolicy::default())
        .expect("open must succeed");
    let after_open = telomere::frontend_phase_counters();
    assert_eq!(after_open.parsed_modules, before.parsed_modules);
    assert_eq!(after_open.optimized_functions, before.optimized_functions);
    assert_eq!(
        after_open.deserialized_modules,
        before.deserialized_modules + 1
    );

    // Install + call: the blobs are mapped as-is; the on-demand
    // emitter must not run for functions the artifact carries.
    let store = jit_store();
    let instance = instantiate_on(&store, opened.module().clone());
    let before_install = telomere::frontend_phase_counters();
    let report = install_aot_artifact(&store, &instance, &opened).expect("install must succeed");
    assert_eq!(
        report.installed, 2,
        "both functions must carry blobs for this test to prove anything"
    );
    let args = ResultValue::new(vec![WasmValue::I32(19), WasmValue::I32(23)]);
    let result = match block_on(run_module_function(&instance, &store, "add", &args)) {
        VMResult::Success(values) => values,
        other => panic!("installed add must run, got {other:?}"),
    };
    assert_eq!(result.iter().next(), Some(&WasmValue::I32(42)));
    let after_call = telomere::frontend_phase_counters();
    assert_eq!(
        after_call.emitted_functions, before_install.emitted_functions,
        "installing and calling AoT code must not emit on demand"
    );
    assert_eq!(after_call.parsed_modules, before_install.parsed_modules);
}
