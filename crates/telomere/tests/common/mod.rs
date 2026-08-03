use sha2::{Digest, Sha256};
use std::collections::HashSet;
#[cfg(feature = "jit")]
use std::sync::atomic::{AtomicUsize, Ordering};
use telomere::{
    common::InstanceHandle, get_global, instantiate, IoReadBinaryReader, Registry, ResultValue,
    Store, VMResult, WasmParser, WasmValue,
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

pub struct SpecDivergence {
    pub module_sha256: &'static str,
    pub why: &'static str,
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

    let m = parser.parse_module().unwrap();

    instantiate(m, store, registry).await.unwrap()
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

#[cfg(feature = "jit")]
fn wast_store() -> Store {
    let store = if std::env::var_os("TELOMERE_WAST_JIT").is_some() && telomere::jit_supported() {
        let code_cache_max_bytes = std::env::var("TELOMERE_WAST_JIT_CACHE_MAX_BYTES")
            .ok()
            .and_then(|value| value.parse::<u32>().ok())
            .unwrap_or(64 * 1024 * 1024);
        let mut runtime_config = telomere::RuntimeConfig::default();
        runtime_config.jit = telomere::JitConfig {
            enabled: true,
            code_cache_max_bytes,
        };
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

    Store::new()
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
                tracing::trace!("{:?}", m.elems);
                let inst = instantiate(m, store, registry).await.unwrap();
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
                                    // TODO: is canonical nan?
                                    assert!(actual.is_nan(), "{:?}", span.linecol_in(text));
                                }
                                (
                                    WastRetCore::F32(NanPattern::ArithmeticNan),
                                    WasmValue::F32(actual),
                                ) => {
                                    // TODO: is arithmetic nan?
                                    assert!(actual.is_nan(), "{:?}", span.linecol_in(text));
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
                                    // TODO: is canonical nan?
                                    assert!(actual.is_nan());
                                }
                                (
                                    WastRetCore::F64(NanPattern::ArithmeticNan),
                                    WasmValue::F64(actual),
                                ) => {
                                    // TODO: is arithmetic nan?
                                    assert!(actual.is_nan());
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
                                            NanPattern::ArithmeticNan
                                            | NanPattern::CanonicalNan => assert!(
                                                b.is_nan(),
                                                "expected: NaN, actual: {b} @ {:?}",
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
                                            NanPattern::ArithmeticNan
                                            | NanPattern::CanonicalNan => assert!(b.is_nan()),
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
                // Message fidelity is outside #130's scope and tracked by #120.
                if message != "alignment must not be larger than natural" {
                    let multiple_tables = message == "multiple tables";
                    let multiple_memories = message == "multiple memories";
                    // Multi-table/multi-memory support supersedes these pre-3.0 assertions.
                    if let Ok(source) = module.encode() {
                        let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                        let mut parser = telomere::WasmParser::new(&mut reader);
                        // TODO: test error message
                        let res = parser.parse_module();

                        match res {
                            Err(_err) => {
                                // FIXME:
                                //assert!(err.to_string().starts_with(message),"{} == {},message validation failed@{:?}",err.to_string(),message,span.linecol_in(text));
                            }
                            Ok(_) if multiple_tables || multiple_memories => {}
                            Ok(_) => panic!("AssertInvalid failed@{:?}", span.linecol_in(text)),
                        }
                    }
                } else {
                    tracing::warn!("now we ignoring alignment error")
                }
            }
            WastDirective::AssertExhaustion {
                span: _,
                call,
                message: _,
            } => {
                let result = telomere::run_module_function(
                    instance.as_ref().unwrap(),
                    store,
                    call.name,
                    &ResultValue::new(convert_args(&call.args)),
                )
                .await;
                assert!(result.is_err());
            }
            WastDirective::AssertTrap {
                span,
                exec,
                message: _,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    tracing::trace!(
                        "executing(trap) {} {:?} @ {:?}",
                        v.name,
                        v.args,
                        span.linecol_in(text)
                    );

                    if let Some(id) = v.module {
                        let instance = registry.get(id.name()).unwrap();
                        let result = telomere::run_module_function(
                            &instance,
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await;
                        assert!(result.is_err(), "{:?}", span.linecol_in(text))
                    } else {
                        let result = telomere::run_module_function(
                            instance.as_ref().unwrap(),
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .await;
                        assert!(result.is_err(), "{:?}", span.linecol_in(text))
                    }
                }
                wast::WastExecute::Wat(mut v) => {
                    let source = v.encode().unwrap();
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let m = parser.parse_module().unwrap();
                    assert!(
                        instantiate(m, store, registry).await.is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
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
                    let module = parser.parse_module().unwrap();

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
