use sha2::{Digest, Sha256};
use std::collections::HashSet;
#[cfg(feature = "jit")]
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    get_global, instantiate, InstanceHandle, IoReadBinaryReader, Registry, ResultValue, Store,
    VMResult, WasmParser, WasmParserError, WasmValue,
};
use tracing::error;
use wast::{
    core::{AbstractHeapType, HeapType, NanPattern, V128Pattern, WastRetCore},
    lexer::Lexer,
    parser::ParseBuffer,
    Wast, WastArg, WastRet, Wat,
};

#[cfg(feature = "jit")]
static WAST_JIT_COMPILED_FUNCTIONS: AtomicUsize = AtomicUsize::new(0);

/// Number of wast modules that went through the precompiled codec
/// round-trip (#196) in this harness process.
static WAST_PRECOMPILED_ROUND_TRIPPED: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Whether the wast harness should route every parsed module through
/// the precompiled codec (#196): parse -> serialize -> deserialize ->
/// instantiate, so the whole corpus runs on restored modules.
fn wast_precompiled_enabled() -> bool {
    std::env::var_os("TELOMERE_WAST_PRECOMPILED").is_some()
}

fn wast_precompiled_required() -> bool {
    std::env::var_os("TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT").is_some()
}

/// Round-trips one parsed module through the precompiled codec when
/// `TELOMERE_WAST_PRECOMPILED` is set. With
/// `TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT` set, any codec failure is
/// a hard test failure; without it, a failure keeps the parsed module
/// (the codec is then only exercised opportunistically).
fn precompiled_round_trip(m: telomere::Module) -> telomere::Module {
    if !wast_precompiled_enabled() {
        return m;
    }
    let required = wast_precompiled_required();
    let bytes = match telomere::serialize_module(&m) {
        Ok(bytes) => bytes,
        Err(err) => {
            assert!(
                !required,
                "TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT: serialize failed: {err}"
            );
            return m;
        }
    };
    match telomere::deserialize_module(&bytes) {
        Ok(restored) => {
            WAST_PRECOMPILED_ROUND_TRIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            restored
        }
        Err(err) => {
            assert!(
                !required,
                "TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT: deserialize failed: {err}"
            );
            m
        }
    }
}

#[allow(dead_code)]
pub fn reset_wast_precompiled_round_tripped() {
    WAST_PRECOMPILED_ROUND_TRIPPED.store(0, std::sync::atomic::Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn assert_wast_precompiled_round_tripped() {
    if wast_precompiled_required() {
        assert!(
            wast_precompiled_enabled(),
            "TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT requires TELOMERE_WAST_PRECOMPILED=1"
        );
        let round_tripped =
            WAST_PRECOMPILED_ROUND_TRIPPED.load(std::sync::atomic::Ordering::Relaxed);
        assert!(
            round_tripped > 0,
            "TELOMERE_WAST_PRECOMPILED_REQUIRE_ACCEPT completed the wast suite without round-tripping any module"
        );
    }
}

/// Number of wast functions installed from AoT artifacts (#198) in
/// this harness process.
#[cfg(feature = "jit")]
static WAST_AOT_INSTALLED: AtomicUsize = AtomicUsize::new(0);

/// Whether the wast harness should route every parsed module through
/// the AoT artifact container (#198): parse -> build artifact -> open
/// (verify) -> instantiate the embedded module -> install the native
/// blobs, so the whole corpus runs on AoT-loaded code.
#[cfg(feature = "jit")]
fn wast_aot_enabled() -> bool {
    std::env::var_os("TELOMERE_WAST_AOT").is_some()
}

#[cfg(feature = "jit")]
fn wast_aot_required() -> bool {
    std::env::var_os("TELOMERE_WAST_AOT_REQUIRE_ACCEPT").is_some()
}

/// Harness signer/verifier for the wast AoT lane: HMAC-SHA256 with a
/// fixed test key (the same construction the container's integration
/// tests use). The lane exercises the container mechanics, not key
/// management.
#[cfg(feature = "jit")]
struct WastAotKey;

#[cfg(feature = "jit")]
fn wast_aot_hmac(message: &[u8]) -> Vec<u8> {
    let key: &[u8] = b"telomere-wast-aot-harness-key";
    let mut key_block = [0u8; 64];
    key_block[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(key_block.map(|b| b ^ 0x36));
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(key_block.map(|b| b ^ 0x5c));
    outer.update(inner_hash);
    outer.finalize().to_vec()
}

#[cfg(feature = "jit")]
impl telomere::AotSigner for WastAotKey {
    fn key_id(&self) -> &str {
        "wast-harness"
    }
    fn sign(&self, payload: &[u8]) -> Vec<u8> {
        wast_aot_hmac(payload)
    }
}

#[cfg(feature = "jit")]
impl telomere::AotVerifier for WastAotKey {
    fn key_id(&self) -> &str {
        "wast-harness"
    }
    fn verify(&self, payload: &[u8], signature: &[u8]) -> bool {
        wast_aot_hmac(payload) == signature
    }
}

/// Builds and opens an AoT artifact for `m` when the lane is enabled,
/// returning the module to instantiate (the artifact's embedded
/// module) and the opened artifact to install afterwards. Failures
/// are hard failures under `TELOMERE_WAST_AOT_REQUIRE_ACCEPT` and
/// keep the parsed module otherwise.
#[cfg(feature = "jit")]
fn aot_prepare(m: telomere::Module) -> (telomere::Module, Option<WastAotArtifact>) {
    if !wast_aot_enabled() {
        return (m, None);
    }
    let required = wast_aot_required();
    let key = WastAotKey;
    let bytes = match telomere::build_aot_artifact(&m, Some(&key)) {
        Ok(bytes) => bytes,
        Err(err) => {
            assert!(
                !required,
                "TELOMERE_WAST_AOT_REQUIRE_ACCEPT: build failed: {err}"
            );
            return (m, None);
        }
    };
    match telomere::open_aot_artifact(&bytes, Some(&key), telomere::AotLoadPolicy::default()) {
        Ok(opened) => {
            let module = opened.module().clone();
            (module, Some(opened))
        }
        Err(err) => {
            assert!(
                !required,
                "TELOMERE_WAST_AOT_REQUIRE_ACCEPT: open failed: {err}"
            );
            (m, None)
        }
    }
}

/// The artifact type the wast AoT lane carries between prepare and
/// install; Infallible in non-jit builds, where the lane is off and
/// no artifact can exist (the container API itself needs aot-load,
/// which a plain build does not enable).
#[cfg(feature = "jit")]
type WastAotArtifact = telomere::AotArtifact;
#[cfg(not(feature = "jit"))]
type WastAotArtifact = std::convert::Infallible;

#[cfg(not(feature = "jit"))]
fn aot_prepare(m: telomere::Module) -> (telomere::Module, Option<WastAotArtifact>) {
    (m, None)
}

/// Installs a prepared AoT artifact into the instance just created
/// from its module. See [`aot_prepare`] for the failure posture.
#[cfg(feature = "jit")]
fn aot_install(store: &Store, instance: &InstanceHandle, artifact: Option<WastAotArtifact>) {
    let Some(artifact) = artifact else {
        return;
    };
    match telomere::install_aot_artifact(store, instance, &artifact) {
        Ok(report) => {
            WAST_AOT_INSTALLED.fetch_add(report.installed, Ordering::Relaxed);
        }
        Err(err) => {
            assert!(
                !wast_aot_required(),
                "TELOMERE_WAST_AOT_REQUIRE_ACCEPT: install failed: {err}"
            );
        }
    }
}

#[cfg(not(feature = "jit"))]
fn aot_install(_store: &Store, _instance: &InstanceHandle, _artifact: Option<WastAotArtifact>) {}

#[allow(dead_code)]
pub fn reset_wast_aot_installed() {
    #[cfg(feature = "jit")]
    WAST_AOT_INSTALLED.store(0, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn assert_wast_aot_installed() {
    #[cfg(feature = "jit")]
    if wast_aot_required() {
        assert!(
            wast_aot_enabled(),
            "TELOMERE_WAST_AOT_REQUIRE_ACCEPT requires TELOMERE_WAST_AOT=1"
        );
        let installed = WAST_AOT_INSTALLED.load(Ordering::Relaxed);
        assert!(
            installed > 0,
            "TELOMERE_WAST_AOT_REQUIRE_ACCEPT completed the wast suite without installing any AoT function"
        );
    }
}

pub struct SpecDivergence {
    pub module_sha256: &'static str,
    pub why: &'static str,
}

// ---------------------------------------------------------------------------
// Spec-fidelity checks for the wast harness (#131).
//
// The spec suite states *why* a module is invalid or *why* execution traps,
// not just that it fails. The tables below map those spec messages onto
// telomere's distinguishable error values (VMResult variants for traps,
// WasmParserError variants for validation). A spec message with no table
// entry fails the suite loudly instead of being silently discarded, so the
// tables stay complete for the suite this harness actually runs.
// ---------------------------------------------------------------------------

/// The spec's canonical NaN: all-ones exponent, the quiet (most significant)
/// payload bit set, and every remaining payload bit clear.
fn is_canonical_nan_f32(value: f32) -> bool {
    matches!(value.to_bits(), 0x7fc0_0000 | 0xffc0_0000)
}

fn is_canonical_nan_f64(value: f64) -> bool {
    matches!(
        value.to_bits(),
        0x7ff8_0000_0000_0000 | 0xfff8_0000_0000_0000
    )
}

/// The spec's arithmetic NaN: any NaN whose quiet payload bit is set.
/// Canonical NaNs are a subset of arithmetic NaNs.
fn is_arithmetic_nan_f32(value: f32) -> bool {
    value.is_nan() && value.to_bits() & 0x0040_0000 != 0
}

fn is_arithmetic_nan_f64(value: f64) -> bool {
    value.is_nan() && value.to_bits() & 0x0008_0000_0000_0000 != 0
}

fn vm_result_name<V>(result: &VMResult<V>) -> &'static str {
    match result {
        VMResult::Success(_) => "success",
        VMResult::Unreachable => "Unreachable",
        VMResult::StackOverflow => "StackOverflow",
        VMResult::MemoryIndexOutOfRange => "MemoryIndexOutOfRange",
        VMResult::UnalignedAtomic => "UnalignedAtomic",
        VMResult::TableIndexOutOfRange => "TableIndexOutOfRange",
        VMResult::CallIndirectInvalidType => "CallIndirectInvalidType",
        VMResult::TableUninitialized => "TableUninitialized",
        VMResult::Unlinkable => "Unlinkable",
        VMResult::MemoryAllocationFailed => "MemoryAllocationFailed",
        VMResult::InvalidOperand => "InvalidOperand",
        VMResult::DivideByZero => "DivideByZero",
        VMResult::IntegerOverflow => "IntegerOverflow",
        VMResult::InvalidConversion => "InvalidConversion",
        VMResult::Unimplemented => "Unimplemented",
        VMResult::FuelExhausted => "FuelExhausted",
        VMResult::Cancelled => "Cancelled",
        VMResult::ResourceLimitExceeded => "ResourceLimitExceeded",
    }
}

/// Drops a trailing " <index>" from a spec message ("unknown memory 0",
/// "uninitialized element 2") so the table can key on the message family.
fn spec_message_base(message: &str) -> &str {
    let base = match message.split_once(':') {
        // Detailed forms such as "type mismatch: instruction requires ..."
        // share the base message's mapping.
        Some((head, _)) if head == "type mismatch" => head,
        _ => message,
    };
    match base.rfind(' ') {
        Some(idx)
            if !base[idx + 1..].is_empty()
                && base[idx + 1..].bytes().all(|b| b.is_ascii_digit()) =>
        {
            &base[..idx]
        }
        _ => base,
    }
}

/// Asserts that an execution result is the trap the spec message mandates.
fn assert_trap_message<V>(
    message: &str,
    result: &VMResult<V>,
    span: wast::token::Span,
    text: &str,
) {
    let accepted = match spec_message_base(message) {
        "unreachable" => matches!(result, VMResult::Unreachable),
        "out of bounds memory access" => matches!(result, VMResult::MemoryIndexOutOfRange),
        "out of bounds table access" | "undefined element" => {
            matches!(result, VMResult::TableIndexOutOfRange)
        }
        "uninitialized element" => matches!(result, VMResult::TableUninitialized),
        "indirect call type mismatch" => matches!(result, VMResult::CallIndirectInvalidType),
        "integer divide by zero" => matches!(result, VMResult::DivideByZero),
        "integer overflow" => matches!(result, VMResult::IntegerOverflow),
        "invalid conversion to integer" => matches!(result, VMResult::InvalidConversion),
        "unaligned atomic" => matches!(result, VMResult::UnalignedAtomic),
        "call stack exhausted" => matches!(result, VMResult::StackOverflow),
        _ => panic!(
            "no trap-message mapping for {message:?} (#131); actual result: {} @ {:?}",
            vm_result_name(result),
            span.linecol_in(text)
        ),
    };
    assert!(
        accepted,
        "trap reason mismatch: expected {message:?}, got {} @ {:?}",
        vm_result_name(result),
        span.linecol_in(text)
    );
}

/// Asserts that a parser rejection is the validation error the spec message
/// mandates. Variant matching is the primary check; a parser error whose
/// Display text equals the spec message verbatim is also accepted, because
/// several WasmParserError displays are spec-worded (including the index in
/// e.g. "unknown memory 0").
fn assert_invalid_message(
    message: &str,
    err: &WasmParserError,
    span: wast::token::Span,
    text: &str,
) {
    use WasmParserError as E;
    let variant_accepted = match spec_message_base(message) {
        "type mismatch" => matches!(
            err,
            E::InvalidStackValType(..)
                | E::InvalidStackValTypeAny
                | E::InvalidElementSectionType(..)
                // call_indirect / return_call_indirect against a non-funcref
                // table is a typing failure reported with the table index.
                | E::InvalidTableType(_)
        ),
        "alignment must not be larger than natural" => matches!(err, E::InvalidAlignment(_)),
        "unknown memory" => matches!(err, E::InvalidMemIdx(_)),
        "unknown table" => matches!(err, E::InvalidTableIndex(_)),
        "unknown function" => matches!(err, E::InvalidFuncIdx(_)),
        "unknown type" => matches!(err, E::InvalidTypeIdx(_)),
        "unknown local" => matches!(err, E::InvalidLocalIndex(_)),
        "unknown global" => matches!(err, E::InvalidGlobalIndex(_) | E::UnknownGlobal),
        "unknown label" => matches!(err, E::UnknownLabel),
        "unknown data segment" => matches!(err, E::InvalidDataIdx(_)),
        "unknown elem segment" => matches!(err, E::UnknownElement(_)),
        "duplicate export name" => matches!(err, E::DuplicatedExport(_)),
        "multiple memories" => matches!(err, E::MultipleMemory),
        "constant expression required" => matches!(err, E::InvalidConstInstruction(_)),
        "global is immutable" | "immutable global" => matches!(err, E::InvalidGlobalAccess),
        "invalid result arity" => matches!(err, E::InvalidResultArity),
        "size minimum must not be greater than maximum" => matches!(err, E::InvalidLimit),
        // parse_memtype deliberately rejects a shared memory without a
        // maximum with the generic limits error.
        "shared memory must have maximum" => matches!(err, E::InvalidLimit),
        "memory size must be at most 65536 pages (4GiB)" => matches!(err, E::InvalidMemorySize(_)),
        "start function" => matches!(err, E::StartFunction),
        "undeclared function reference" => matches!(err, E::UndeclaredFunctionReference),
        "invalid lane index" => {
            matches!(err, E::InvalidInstruction(bytes) if bytes[0] == 0xFD)
        }
        _ => panic!(
            "no invalid-message mapping for {message:?} (#131); actual error: {err:?} ({err}) @ {:?}",
            span.linecol_in(text)
        ),
    };
    assert!(
        variant_accepted || err.to_string() == message,
        "invalid reason mismatch: expected {message:?}, got {err:?} ({err}) @ {:?}",
        span.linecol_in(text)
    );
}

fn wast_parse_buffer(text: &str) -> ParseBuffer<'_> {
    let mut lexer = Lexer::new(text);
    // `names.wast` intentionally contains confusing Unicode in upstream identifiers.
    lexer.allow_confusing_unicode(true);
    ParseBuffer::new_with_lexer(lexer).unwrap()
}

pub async fn instantiate_wat(wat: &str, store: &Store, registry: &Registry) -> InstanceHandle {
    let buf = ParseBuffer::new(wat).unwrap();
    let mut wat = wast::parser::parse::<Wat>(&buf).unwrap();
    let source = wat.encode().unwrap();

    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);

    let m = precompiled_round_trip(parser.parse_module().unwrap());
    let (m, aot_artifact) = aot_prepare(m);

    let instance = instantiate(m, store, registry).await.unwrap();
    aot_install(store, &instance, aot_artifact);
    instance
}
fn convert_args(args: &[WastArg<'_>]) -> Vec<WasmValue> {
    args.iter()
        .map(|v| match v {
            wast::WastArg::Core(wast_arg_core) => match wast_arg_core {
                wast::core::WastArgCore::I32(v) => WasmValue::I32(*v),
                wast::core::WastArgCore::I64(v) => WasmValue::I64(*v),
                wast::core::WastArgCore::F32(f32) => WasmValue::F32(f32::from_bits(f32.bits)),
                wast::core::WastArgCore::F64(f64) => WasmValue::F64(f64::from_bits(f64.bits)),
                wast::core::WastArgCore::V128(v) => {
                    WasmValue::V128(u128::from_le_bytes(v.to_le_bytes()))
                }
                wast::core::WastArgCore::RefNull(rt) => match rt {
                    HeapType::Abstract {
                        shared: _,
                        ty: AbstractHeapType::Func,
                    } => WasmValue::FuncRef(0),
                    HeapType::Abstract {
                        shared: _,
                        ty: AbstractHeapType::Extern,
                    } => WasmValue::ExternRef(0),
                    unknown => todo!("{unknown:?}"),
                },
                wast::core::WastArgCore::RefExtern(v) => WasmValue::ExternRef(*v + 0x40000000),
                wast::core::WastArgCore::RefHost(_) => todo!(),
            },
            wast::WastArg::Component(_) => todo!(),
            _ => todo!(),
        })
        .collect()
}

#[cfg(feature = "threads")]
const SPECTEST_WAT: &str = r#"
(module
    (global (export "global_i32") i32 (i32.const 666))
    (global (export "global_i64") i64 (i64.const 666))
    (global (export "global_f32") f32 (f32.const 666.6))
    (global (export "global_f64") f64 (f64.const 666.6))

    (table (export "table") 10 20 funcref)

    (memory (export "memory") 1 2)
    (memory (export "shared_memory") 1 2 shared)

    (func (export "print"))
    (func (export "print_i32") (param i32))
    (func (export "print_i64") (param i64))
    (func (export "print_f32") (param f32))
    (func (export "print_f64") (param f64))
    (func (export "print_i32_f32") (param i32 f32))
    (func (export "print_f64_f64") (param f64 f64))
)
"#;

#[cfg(not(feature = "threads"))]
const SPECTEST_WAT: &str = r#"
(module
    (global (export "global_i32") i32 (i32.const 666))
    (global (export "global_i64") i64 (i64.const 666))
    (global (export "global_f32") f32 (f32.const 666.6))
    (global (export "global_f64") f64 (f64.const 666.6))

    (table (export "table") 10 20 funcref)

    (memory (export "memory") 1 2)

    (func (export "print"))
    (func (export "print_i32") (param i32))
    (func (export "print_i64") (param i64))
    (func (export "print_f32") (param f32))
    (func (export "print_f64") (param f64))
    (func (export "print_i32_f32") (param i32 f32))
    (func (export "print_f64_f64") (param f64 f64))
)
"#;
async fn init_spectest(store: &Store, registry: &Registry) -> InstanceHandle {
    instantiate_wat(SPECTEST_WAT, store, registry).await
}
#[allow(dead_code)]
pub async fn run_wast(text: &str) {
    let mut divergence_hits = HashSet::new();
    run_wast_with_spec_divergences(text, &[], &mut divergence_hits).await;
}

pub async fn run_wast_with_spec_divergences(
    text: &str,
    divergences: &[SpecDivergence],
    divergence_hits: &mut HashSet<&'static str>,
) {
    let store = wast_store();
    let mut registry = Registry::new();
    let st = init_spectest(&store, &registry).await;
    registry.register("spectest", st.clone());
    run_wast_with_spec_divergences_in_context(
        text,
        &store,
        &mut registry,
        divergences,
        divergence_hits,
    )
    .await;
    assert_wast_jit_acceptance(&store);
}

/// Whether the suite should run with the quarantined benchmark-shaped
/// recognizers (#188) reconnected. The default run keeps them off; the
/// `TELOMERE_WAST_BENCHMARK_RECOGNIZERS=1` run proves the suite also passes
/// with them on.
fn wast_benchmark_recognizers() -> bool {
    std::env::var_os("TELOMERE_WAST_BENCHMARK_RECOGNIZERS").is_some()
}

#[cfg(feature = "jit")]
fn wast_store() -> Store {
    let benchmark_recognizers = wast_benchmark_recognizers();
    let store = if (std::env::var_os("TELOMERE_WAST_JIT").is_some() && telomere::jit_supported())
        || benchmark_recognizers
    {
        let code_cache_max_bytes = std::env::var("TELOMERE_WAST_JIT_CACHE_MAX_BYTES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(64 * 1024 * 1024);
        let mut runtime_config = telomere::RuntimeConfig::default();
        if std::env::var_os("TELOMERE_WAST_JIT").is_some() && telomere::jit_supported() {
            runtime_config.jit = telomere::JitConfig { enabled: true };
            runtime_config.limits.max_code_cache_bytes = code_cache_max_bytes;
        }
        runtime_config.recognizers.benchmark_shaped = benchmark_recognizers;
        Store::new_with_runtime_config(runtime_config)
    } else {
        Store::new()
    };

    if std::env::var_os("TELOMERE_WAST_JIT_REQUIRE_ACCEPT").is_some() {
        assert!(
            telomere::jit_supported(),
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires JIT support on this target"
        );
        assert!(
            store.runtime_config().jit.enabled,
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires TELOMERE_WAST_JIT=1"
        );
    }

    store
}

#[cfg(not(feature = "jit"))]
fn wast_store() -> Store {
    if std::env::var_os("TELOMERE_WAST_JIT_REQUIRE_ACCEPT").is_some() {
        panic!(
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires the telomere `jit` feature to be enabled"
        );
    }

    if wast_benchmark_recognizers() {
        let mut runtime_config = telomere::RuntimeConfig::default();
        runtime_config.recognizers.benchmark_shaped = true;
        Store::new_with_runtime_config(runtime_config)
    } else {
        Store::new()
    }
}

fn assert_wast_jit_acceptance(store: &Store) {
    #[cfg(feature = "jit")]
    if std::env::var_os("TELOMERE_WAST_JIT_REQUIRE_ACCEPT").is_some() {
        assert!(
            store.runtime_config().jit.enabled,
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires a JIT-enabled store"
        );
        let stats = store.jit_cache_stats();
        assert_eq!(
            stats.rejected_functions, 0,
            "wast fixture produced JIT compile rejection: {stats:?}"
        );
        WAST_JIT_COMPILED_FUNCTIONS.fetch_add(stats.compiled_functions, Ordering::Relaxed);
    }

    #[cfg(not(feature = "jit"))]
    {
        let _ = store;
    }
}

#[allow(dead_code)]
pub fn reset_wast_jit_compiled_functions() {
    #[cfg(feature = "jit")]
    WAST_JIT_COMPILED_FUNCTIONS.store(0, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn assert_wast_jit_compiled_functions() {
    #[cfg(feature = "jit")]
    if std::env::var_os("TELOMERE_WAST_JIT_REQUIRE_ACCEPT").is_some() {
        assert!(
            telomere::jit_supported(),
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires JIT support on this target"
        );
        assert!(
            std::env::var_os("TELOMERE_WAST_JIT").is_some(),
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires TELOMERE_WAST_JIT=1"
        );
        let compiled_functions = WAST_JIT_COMPILED_FUNCTIONS.load(Ordering::Relaxed);
        assert!(
            compiled_functions > 0,
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT completed the wast suite without compiling any functions"
        );
    }

    #[cfg(not(feature = "jit"))]
    if std::env::var_os("TELOMERE_WAST_JIT_REQUIRE_ACCEPT").is_some() {
        panic!(
            "TELOMERE_WAST_JIT_REQUIRE_ACCEPT requires the telomere `jit` feature to be enabled"
        );
    }
}

#[allow(dead_code)]
pub async fn run_wast_with(text: &str, store: &Store, registry: &mut Registry) {
    let mut divergence_hits = HashSet::new();
    run_wast_with_spec_divergences_in_context(text, store, registry, &[], &mut divergence_hits)
        .await;
}

async fn run_wast_with_spec_divergences_in_context(
    text: &str,
    store: &Store,
    registry: &mut Registry,
    divergences: &[SpecDivergence],
    divergence_hits: &mut HashSet<&'static str>,
) {
    let buf = wast_parse_buffer(text);
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let mut instance: Option<InstanceHandle> = None;

    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let name = m.name();
                let span = m.span();
                let source = m.encode().unwrap();
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let mut parser = telomere::WasmParser::new(&mut reader);
                let m = match parser.parse_module() {
                    Ok(v) => v,
                    Err(v) => {
                        panic!("{:?} {:?}", span.linecol_in(text), v);
                    }
                };
                let m = precompiled_round_trip(m);
                let (m, aot_artifact) = aot_prepare(m);
                tracing::trace!("instantiating parsed module");
                let inst = instantiate(m, store, registry).await.unwrap();
                aot_install(store, &inst, aot_artifact);
                if let Some(name) = name {
                    registry.register(name.name(), inst.clone());
                }
                instance = Some(inst);
            }
            WastDirective::AssertReturn {
                span,
                exec,
                results: expected,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    tracing::trace!(
                        "executing {} {:?} @ {:?}",
                        v.name,
                        v.args,
                        v.span.linecol_in(text)
                    );
                    let actual = if let Some(id) = v.module {
                        let instance = registry.get(id.name()).unwrap();
                        telomere::run_module_function(
                            &instance,
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await
                        .unwrap()
                    } else {
                        telomere::run_module_function(
                            instance.as_ref().unwrap(),
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await
                        .unwrap()
                    };
                    assert_eq!(
                        expected.len(),
                        actual.len(),
                        "result arity mismatch: expected {} value(s), got {} @ {:?}",
                        expected.len(),
                        actual.len(),
                        span.linecol_in(text)
                    );
                    for (expected, actual) in expected.iter().zip(actual.iter()) {
                        if let WastRet::Core(expected) = expected {
                            match (expected, actual) {
                                (WastRetCore::I32(expected), WasmValue::I32(actual)) => {
                                    assert_eq!(expected, actual, "{:?}", span.linecol_in(text))
                                }
                                (WastRetCore::I64(expected), WasmValue::I64(actual)) => {
                                    assert_eq!(expected, actual, "{:?}", span.linecol_in(text))
                                }
                                (
                                    WastRetCore::F32(NanPattern::Value(expected)),
                                    WasmValue::F32(actual),
                                ) => {
                                    assert_eq!(
                                        expected.bits,
                                        actual.to_bits(),
                                        "{:?}",
                                        span.linecol_in(text)
                                    )
                                }
                                (
                                    WastRetCore::F32(NanPattern::CanonicalNan),
                                    WasmValue::F32(actual),
                                ) => {
                                    assert!(
                                        is_canonical_nan_f32(*actual),
                                        "expected canonical NaN, got {actual:?} @ {:?}",
                                        span.linecol_in(text)
                                    );
                                }
                                (
                                    WastRetCore::F32(NanPattern::ArithmeticNan),
                                    WasmValue::F32(actual),
                                ) => {
                                    assert!(
                                        is_arithmetic_nan_f32(*actual),
                                        "expected arithmetic NaN, got {actual:?} @ {:?}",
                                        span.linecol_in(text)
                                    );
                                }
                                (
                                    WastRetCore::F64(NanPattern::Value(expected)),
                                    WasmValue::F64(actual),
                                ) => {
                                    assert_eq!(
                                        expected.bits,
                                        actual.to_bits(),
                                        "{:?}",
                                        span.linecol_in(text)
                                    )
                                }
                                (
                                    WastRetCore::F64(NanPattern::CanonicalNan),
                                    WasmValue::F64(actual),
                                ) => {
                                    assert!(
                                        is_canonical_nan_f64(*actual),
                                        "expected canonical NaN, got {actual:?} @ {:?}",
                                        span.linecol_in(text)
                                    );
                                }
                                (
                                    WastRetCore::F64(NanPattern::ArithmeticNan),
                                    WasmValue::F64(actual),
                                ) => {
                                    assert!(
                                        is_arithmetic_nan_f64(*actual),
                                        "expected arithmetic NaN, got {actual:?} @ {:?}",
                                        span.linecol_in(text)
                                    );
                                }
                                (WastRetCore::V128(V128Pattern::I64x2(x)), WasmValue::V128(b)) => {
                                    let x = x.as_ptr() as *const u8;
                                    let mut buf = [0; 16];
                                    unsafe {
                                        std::ptr::copy_nonoverlapping(x, buf.as_mut_ptr(), 16)
                                    };
                                    assert_eq!(b.to_le_bytes(), buf, "{:?}", span.linecol_in(text))
                                }
                                (WastRetCore::V128(V128Pattern::I32x4(x)), WasmValue::V128(b)) => {
                                    let x = x.as_ptr() as *const u8;
                                    let mut buf = [0; 16];
                                    unsafe {
                                        std::ptr::copy_nonoverlapping(x, buf.as_mut_ptr(), 16)
                                    };
                                    assert_eq!(b.to_le_bytes(), buf, "{:?}", span.linecol_in(text))
                                }
                                (WastRetCore::V128(V128Pattern::I16x8(x)), WasmValue::V128(b)) => {
                                    let x = x.as_ptr() as *const u8;
                                    let mut buf = [0; 16];
                                    unsafe {
                                        std::ptr::copy_nonoverlapping(x, buf.as_mut_ptr(), 16)
                                    };
                                    assert_eq!(b.to_le_bytes(), buf, "{:?}", span.linecol_in(text))
                                }
                                (WastRetCore::V128(V128Pattern::I8x16(x)), WasmValue::V128(b)) => {
                                    let x = x.as_ptr() as *const u8;
                                    let mut buf = [0; 16];
                                    unsafe {
                                        std::ptr::copy_nonoverlapping(x, buf.as_mut_ptr(), 16)
                                    };
                                    assert_eq!(b.to_le_bytes(), buf, "{:?}", span.linecol_in(text))
                                }
                                (WastRetCore::V128(V128Pattern::F32x4(x)), WasmValue::V128(b)) => {
                                    let b = b.to_le_bytes();
                                    let mut b_a = [0u8; 4];
                                    b_a.copy_from_slice(&b[0..4]);
                                    let mut b_b = [0u8; 4];
                                    b_b.copy_from_slice(&b[4..8]);
                                    let mut b_c = [0u8; 4];
                                    b_c.copy_from_slice(&b[8..12]);
                                    let mut b_d = [0u8; 4];
                                    b_d.copy_from_slice(&b[12..16]);
                                    let actuals = [
                                        f32::from_le_bytes(b_a),
                                        f32::from_le_bytes(b_b),
                                        f32::from_le_bytes(b_c),
                                        f32::from_le_bytes(b_d),
                                    ];
                                    for (a, b) in x.iter().zip(actuals) {
                                        match a {
                                            NanPattern::Value(a) => {
                                                assert_eq!(
                                                    a.bits,
                                                    b.to_bits(),
                                                    "expected: {} actual: {b} @ {:?}",
                                                    f32::from_bits(a.bits),
                                                    span.linecol_in(text)
                                                )
                                            }
                                            NanPattern::CanonicalNan => assert!(
                                                is_canonical_nan_f32(b),
                                                "expected: canonical NaN, actual: {b} @ {:?}",
                                                span.linecol_in(text)
                                            ),
                                            NanPattern::ArithmeticNan => assert!(
                                                is_arithmetic_nan_f32(b),
                                                "expected: arithmetic NaN, actual: {b} @ {:?}",
                                                span.linecol_in(text)
                                            ),
                                        }
                                    }
                                }
                                (WastRetCore::V128(V128Pattern::F64x2(x)), WasmValue::V128(b)) => {
                                    let b = b.to_le_bytes();
                                    let mut b_a = [0u8; 8];
                                    b_a.copy_from_slice(&b[0..8]);
                                    let mut b_b = [0u8; 8];
                                    b_b.copy_from_slice(&b[8..16]);
                                    let actuals =
                                        [f64::from_le_bytes(b_a), f64::from_le_bytes(b_b)];
                                    for (a, b) in x.iter().zip(actuals) {
                                        match a {
                                            NanPattern::Value(a) => {
                                                assert_eq!(
                                                    a.bits,
                                                    b.to_bits(),
                                                    "expected: {},actual: {b}@{:?}",
                                                    f64::from_bits(a.bits),
                                                    span.linecol_in(text)
                                                )
                                            }
                                            NanPattern::CanonicalNan => assert!(
                                                is_canonical_nan_f64(b),
                                                "expected: canonical NaN, actual: {b} @ {:?}",
                                                span.linecol_in(text)
                                            ),
                                            NanPattern::ArithmeticNan => assert!(
                                                is_arithmetic_nan_f64(b),
                                                "expected: arithmetic NaN, actual: {b} @ {:?}",
                                                span.linecol_in(text)
                                            ),
                                        }
                                    }
                                }
                                (WastRetCore::RefNull(_), WasmValue::ExternRef(0)) => {
                                    // ok
                                }
                                (WastRetCore::RefExtern(Some(v)), WasmValue::ExternRef(vv)) => {
                                    // ok
                                    assert_eq!(v + 0x40000000, *vv)
                                }
                                (WastRetCore::RefNull(_), WasmValue::FuncRef(0)) => {
                                    // ok
                                }
                                _ => {
                                    error!(
                                        "{:?} {:?} {:?}",
                                        expected,
                                        actual,
                                        span.linecol_in(text)
                                    );
                                    unimplemented!()
                                }
                            }
                        } else {
                            unimplemented!()
                        }
                    }
                }
                wast::WastExecute::Get {
                    span: _,
                    module: id,
                    global,
                } => {
                    if let Some(id) = id {
                        let instance = registry.get(id.name()).unwrap();
                        get_global(&instance, store, global).unwrap();
                    } else {
                        get_global(instance.as_ref().unwrap(), store, global).unwrap();
                    }
                }
                unknown => unimplemented!("{:?}", unknown),
            },
            WastDirective::AssertMalformed {
                span,
                mut module,
                message: _,
            } => {
                let test = module.to_test().unwrap();
                match test {
                    wast::QuoteWatTest::Text(source) | wast::QuoteWatTest::Binary(source) => {
                        assert_malformed_module(&source, span, text, divergences, divergence_hits)
                    }
                }
            }
            WastDirective::AssertInvalid {
                span,
                mut module,
                message,
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                // Multi-table/multi-memory support supersedes these pre-3.0
                // assertions: a module the parser accepts is fine, but a
                // rejection must still be for the stated reason (#131).
                let superseded = matches!(message, "multiple tables" | "multiple memories");
                let source = module.encode().unwrap_or_else(|err| {
                    panic!(
                        "AssertInvalid module failed to encode: {err} @ {:?}",
                        span.linecol_in(text)
                    )
                });
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let mut parser = telomere::WasmParser::new(&mut reader);
                match parser.parse_module() {
                    Err(err) if message == "multiple tables" => {
                        // The parser implements multiple tables, so it has no
                        // error variant for this superseded assertion; any
                        // rejection here is a defect in its own right.
                        panic!(
                            "AssertInvalid (multiple tables) rejected unexpectedly: {err} @ {:?}",
                            span.linecol_in(text)
                        )
                    }
                    Err(err) => assert_invalid_message(message, &err, span, text),
                    Ok(_) if superseded => {}
                    Ok(_) => panic!("AssertInvalid failed@{:?}", span.linecol_in(text)),
                }
            }
            WastDirective::AssertExhaustion {
                span,
                call,
                message,
            } => {
                let result = telomere::run_module_function(
                    instance.as_ref().unwrap(),
                    store,
                    call.name,
                    &ResultValue::new(convert_args(&call.args)),
                )
                .await;
                assert_trap_message(message, &result, span, text);
            }
            WastDirective::AssertTrap {
                span,
                exec,
                message,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    tracing::trace!(
                        "executing(trap) {} {:?} @ {:?}",
                        v.name,
                        v.args,
                        span.linecol_in(text)
                    );

                    let result = if let Some(id) = v.module {
                        let instance = registry.get(id.name()).unwrap();
                        telomere::run_module_function(
                            &instance,
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await
                    } else {
                        telomere::run_module_function(
                            instance.as_ref().unwrap(),
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await
                    };
                    assert_trap_message(message, &result, span, text);
                }
                wast::WastExecute::Wat(mut v) => {
                    let source = v.encode().unwrap();
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let m = precompiled_round_trip(parser.parse_module().unwrap());
                    let result = instantiate(m, store, registry).await;
                    assert_trap_message(message, &result, span, text);
                }
                v => {
                    todo!("{v:?}")
                }
            },
            WastDirective::Invoke(invoke) => {
                tracing::trace!(
                    "Invoke {} @ {:?}",
                    invoke.name,
                    invoke.span.linecol_in(text)
                );
                let result = telomere::run_module_function(
                    instance.as_ref().unwrap(),
                    store,
                    invoke.name,
                    &ResultValue::new(convert_args(&invoke.args)),
                )
                .await;
                match result {
                    VMResult::Success(_) => {} // ok
                    other => panic!("{:?}", other),
                }
            }
            WastDirective::Register {
                span: _,
                name,
                module: _id,
            } => {
                //assert!(id.is_none());
                registry.register(name, instance.clone().unwrap());
            }
            WastDirective::AssertUnlinkable {
                span,
                mut module,
                message: _,
            } => {
                //TODO: Is there anything that wast fails to encode that could be binary?
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let module = precompiled_round_trip(parser.parse_module().unwrap());

                    // TODO: test error message
                    assert!(
                        instantiate(module, store, registry).await.is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
            }
            _ => unimplemented!(),
        }
    }
}

fn assert_malformed_module(
    source: &[u8],
    span: wast::token::Span,
    text: &str,
    divergences: &[SpecDivergence],
    divergence_hits: &mut HashSet<&'static str>,
) {
    let module_sha256 = format!("{:x}", Sha256::digest(source));
    let divergence = divergences
        .iter()
        .find(|divergence| divergence.module_sha256 == module_sha256);
    let mut reader = telomere::IoReadBinaryReader::from(source);
    let mut parser = telomere::WasmParser::new(&mut reader);

    if let Some(divergence) = divergence {
        assert!(
            parser.parse_module().is_ok(),
            "stale spec divergence {}: {} @ {:?}",
            divergence.module_sha256,
            divergence.why,
            span.linecol_in(text),
        );
        divergence_hits.insert(divergence.module_sha256);
    } else {
        assert!(
            parser.parse_module().is_err(),
            "AssertMalformed unexpectedly parsed SHA-256 {module_sha256} @ {:?}",
            span.linecol_in(text),
        );
    }
}
