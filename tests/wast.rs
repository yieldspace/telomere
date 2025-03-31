use std::path::PathBuf;

use telomere::{
    common::InstanceAddr, get_global, instantiate, Registry, ResultValue, Store, WasmValue,
};
use tracing::{error, Level};
use wast::{
    core::{AbstractHeapType, HeapType, NanPattern, WastRetCore},
    parser::ParseBuffer,
    Wast, WastArg, WastRet, Wat,
};
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
const SPECTEST_WAST: &str = r#"
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
    let buf = ParseBuffer::new(SPECTEST_WAST).unwrap();
    let mut wat = wast::parser::parse::<Wat>(&buf).unwrap();
    let source = wat.encode().unwrap();

    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);

    let m = parser.parse_module().unwrap();

    instantiate(m, store, registry).unwrap()
}
fn run_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let mut instance: Option<InstanceAddr> = None;
    let mut store = Store::new();
    let mut registry = Registry::new();
    let st = init_spectest(&mut store, &registry);
    registry.register("spectest", st);
    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let name = m.name();

                let source = m.encode().unwrap();
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let mut parser = telomere::WasmParser::new(&mut reader);
                let m = parser.parse_module().unwrap();
                tracing::trace!("{:?}", m.elems);
                let inst = instantiate(m, &mut store, &registry).unwrap();
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
                            &mut store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        )
                        .unwrap()
                    } else {
                        telomere::run_module_function(
                            instance.unwrap(),
                            &mut store,
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
                        get_global(instance, &mut store, global).unwrap();
                    } else {
                        get_global(instance.unwrap(), &mut store, global).unwrap();
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
                    &mut store,
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
                            &mut store,
                            v.name,
                            &ResultValue::new(convert_args(&v.args)),
                        );
                        assert!(result.is_err(), "{:?}", span.linecol_in(text))
                    } else {
                        let result = telomere::run_module_function(
                            instance.unwrap(),
                            &mut store,
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
                        instantiate(m, &mut store, &registry).is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
                v => {
                    todo!("{v:?}")
                }
            },
            WastDirective::Invoke(invoke) => {
                let result = telomere::run_module_function(
                    instance.unwrap(),
                    &mut store,
                    invoke.name,
                    &ResultValue::new(convert_args(&invoke.args)),
                );
                assert!(!result.is_err())
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
                        instantiate(module, &mut store, &registry).is_err(),
                        "{:?}",
                        span.linecol_in(text)
                    )
                }
            }
            _ => unimplemented!(),
        }
    }
}
fn run_test_file(name: &str) {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    d.push("tests");
    d.push(format!("{name}.wast"));
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}
#[test]
fn int_literals() {
    run_test_file("int_literals");
}
#[test]
fn block() {
    run_test_file("block");
}
#[test]
fn call() {
    run_test_file("call");
}

#[test]
fn memory_grow() {
    run_test_file("memory_grow");
}

#[test]
fn call_indirect() {
    run_test_file("call_indirect");
}
#[test]
fn loop_() {
    run_test_file("loop");
}

#[test]
fn br_if() {
    run_test_file("br_if");
}

#[test]
fn const_() {
    run_test_file("const");
}

#[test]
fn nop() {
    run_test_file("nop");
}

#[test]
fn func() {
    run_test_file("func");
}

#[test]
fn br_table() {
    run_test_file("br_table");
}

#[test]
fn memory() {
    run_test_file("memory");
}

#[test]
fn if_() {
    run_test_file("if");
}
#[test]
fn address() {
    run_test_file("address");
}
#[test]
fn align() {
    run_test_file("align");
}
#[test]
fn memory_copy() {
    run_test_file("memory_copy");
}
#[test]
fn memory_fill() {
    run_test_file("memory_fill");
}
#[test]
fn memory_trap() {
    run_test_file("memory_trap");
}

#[test]
fn memory_redundancy() {
    run_test_file("memory_redundancy");
}

#[test]
fn memory_size() {
    run_test_file("memory_size");
}
#[test]
fn memory_init() {
    run_test_file("memory_init");
}
#[test]
fn imports() {
    run_test_file("imports");
}

#[test]
fn comments() {
    run_test_file("comments");
}
#[test]
fn conversions() {
    run_test_file("conversions");
}

#[test]
fn custom() {
    run_test_file("custom");
}
#[test]
fn data() {
    run_test_file("data");
}
/*
#[test]
fn bulk() {
    run_test_file("bulk");
}
#[test]
fn elem() {
    run_test_file("bulk");
}
*/
#[test]
fn endianness() {
    run_test_file("endianness");
}
#[test]
fn exports() {
    run_test_file("exports");
}
#[test]
fn f32() {
    run_test_file("f32");
}
#[test]
fn f32_bitwise() {
    run_test_file("f32_bitwise");
}
#[test]
fn f32_cmp() {
    run_test_file("f32_cmp");
}
#[test]
fn f64() {
    run_test_file("f64");
}
#[test]
fn f64_bitwise() {
    run_test_file("f64_bitwise");
}
#[test]
fn f64_cmp() {
    run_test_file("f64_cmp");
}
#[test]
fn fac() {
    run_test_file("fac");
}

#[test]
fn float_exprs() {
    run_test_file("float_exprs");
}
#[test]
fn float_literals() {
    run_test_file("float_literals");
}
#[test]
fn float_memory() {
    run_test_file("float_memory");
}
#[test]
fn float_misc() {
    run_test_file("float_misc");
}
#[test]
fn forward() {
    run_test_file("forward");
}
#[test]
fn func_ptrs() {
    run_test_file("func_ptrs");
}
#[test]
fn global() {
    run_test_file("global");
}
#[test]
fn i32() {
    run_test_file("i32");
}
#[test]
fn i64() {
    run_test_file("i64");
}
#[test]
fn inline_module() {
    run_test_file("inline_module");
}
#[test]
fn int_exprs() {
    run_test_file("int_exprs");
}
/*
TODO: library bug?
#[test]
fn labels() {
    run_test_file("labels");
}*/
#[test]
fn left_to_right() {
    run_test_file("left-to-right");
}

#[test]
fn linking() {
    run_test_file("linking");
}

#[test]
fn load() {
    run_test_file("load");
}
#[test]
fn local_get() {
    run_test_file("local_get");
}
#[test]
fn local_set() {
    run_test_file("local_set");
}
#[test]
fn local_tee() {
    run_test_file("local_tee");
}
/*
library limitation
#[test]
fn names() {
    run_test_file("names");
}
*/
#[test]
fn obsolete_keywords() {
    run_test_file("obsolete-keywords");
}
#[test]
fn ref_func() {
    run_test_file("ref_func");
}
#[test]
fn ref_is_null() {
    run_test_file("ref_is_null");
}
#[test]
fn ref_null() {
    run_test_file("ref_null");
}
#[test]
fn return_() {
    run_test_file("return");
}
#[test]
fn select() {
    run_test_file("select");
}
#[test]
fn skip_stack_guard_page() {
    run_test_file("skip-stack-guard-page");
}
#[test]
fn stack() {
    run_test_file("stack");
}
#[test]
fn start() {
    run_test_file("start");
}
#[test]
fn store() {
    run_test_file("store");
}