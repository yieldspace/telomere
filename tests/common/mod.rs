use telomere::{
    common::InstanceAddr, get_global, instantiate, IoReadBinaryReader, Registry, ResultValue,
    Store, VMResult, WasmParser, WasmValue,
};
use tracing::error;
use wast::{
    core::{AbstractHeapType, HeapType, NanPattern, WastRetCore},
    parser::ParseBuffer,
    Wast, WastArg, WastRet, Wat,
};
pub fn instantiate_wat(wat: &str, store: &mut Store, registry: &Registry) -> InstanceAddr {
    let buf = ParseBuffer::new(wat).unwrap();
    let mut wat = wast::parser::parse::<Wat>(&buf).unwrap();
    let source = wat.encode().unwrap();

    let mut reader = IoReadBinaryReader::from(&source[..]);
    let mut parser = WasmParser::new(&mut reader);

    let m = parser.parse_module().unwrap();

    instantiate(m, store, registry).unwrap()
}
fn convert_args(args: &[WastArg<'_>]) -> Vec<WasmValue> {
    args.iter()
        .map(|v| match v {
            wast::WastArg::Core(wast_arg_core) => match wast_arg_core {
                wast::core::WastArgCore::I32(v) => WasmValue::I32(*v),
                wast::core::WastArgCore::I64(v) => WasmValue::I64(*v),
                wast::core::WastArgCore::F32(f32) => WasmValue::F32(f32::from_bits(f32.bits)),
                wast::core::WastArgCore::F64(f64) => WasmValue::F64(f64::from_bits(f64.bits)),
                wast::core::WastArgCore::V128(_) => todo!(),
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
fn init_spectest(store: &mut Store, registry: &Registry) -> InstanceAddr {
    instantiate_wat(SPECTEST_WAT, store, registry)
}
#[allow(dead_code)]
pub fn run_wast(text: &str) {
    let mut store = Store::new();
    let mut registry = Registry::new();
    let st = init_spectest(&mut store, &registry);
    registry.register("spectest", st);
    run_wast_with(text, &mut store, &mut registry);
}
pub fn run_wast_with(text: &str, store: &mut Store, registry: &mut Registry) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let mut instance: Option<InstanceAddr> = None;

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
                let inst = instantiate(m, store, registry).unwrap();
                if let Some(name) = name {
                    registry.register(name.name(), inst);
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
                            instance,
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .unwrap()
                    } else {
                        telomere::run_module_function(
                            instance.unwrap(),
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
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
                        get_global(instance, store, global).unwrap();
                    } else {
                        get_global(instance.unwrap(), store, global).unwrap();
                    }
                }
                unknown => unimplemented!("{:?}", unknown),
            },
            WastDirective::AssertMalformed {
                span,
                mut module,
                message: _,
            } => {
                //TODO: Is there anything that wast fails to encode that could be binary?
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    assert!(
                        parser.parse_module().is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
            }
            WastDirective::AssertInvalid {
                span,
                mut module,
                message: _,
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                //TODO: Is there anything that wast fails to encode that could be binary?
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    // TODO: test error message
                    assert!(
                        parser.parse_module().is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
            }
            WastDirective::AssertExhaustion {
                span: _,
                call,
                message: _,
            } => {
                let result = telomere::run_module_function(
                    instance.unwrap(),
                    store,
                    call.name,
                    &ResultValue::new(convert_args(&call.args)),
                );
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
                            instance,
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        );
                        assert!(result.is_err(), "{:?}", span.linecol_in(text))
                    } else {
                        let result = telomere::run_module_function(
                            instance.unwrap(),
                            store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        );
                        assert!(result.is_err(), "{:?}", span.linecol_in(text))
                    }
                }
                wast::WastExecute::Wat(mut v) => {
                    let source = v.encode().unwrap();
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let m = parser.parse_module().unwrap();
                    assert!(
                        instantiate(m, store, registry).is_err(),
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
                    instance.unwrap(),
                    store,
                    invoke.name,
                    &ResultValue::new(convert_args(&invoke.args)),
                );
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
                registry.register(name, instance.unwrap());
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
                        instantiate(module, store, registry).is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
            }
            _ => unimplemented!(),
        }
    }
}
