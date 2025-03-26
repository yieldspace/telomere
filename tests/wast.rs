use std::path::PathBuf;

use telomere::{common::Instance, instantiate, Module, ResultValue, WasmValue};
use tracing::error;
use wast::{
    core::{NanPattern, WastRetCore},
    parser::ParseBuffer,
    Wast, WastArg, WastRet,
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
                wast::core::WastArgCore::RefNull(_) => todo!(),
                wast::core::WastArgCore::RefExtern(_) => todo!(),
                wast::core::WastArgCore::RefHost(_) => todo!(),
            },
            wast::WastArg::Component(_) => todo!(),
            _ => todo!(),
        })
        .collect()
}
fn run_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let mut module: Option<Module> = None;
    let mut instance: Option<Instance> = None;
    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let source = m.encode().unwrap();
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let mut parser = telomere::WasmParser::new(&mut reader);
                let m = parser.parse_module().unwrap();
                tracing::trace!("{:?}", m.elems);
                instance = Some(instantiate(&m).unwrap());
                module = Some(m);
            }
            WastDirective::AssertReturn {
                span,
                exec,
                results: expected,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    tracing::trace!("executing {}", v.name);
                    let actual = telomere::run_module_function(
                        module.as_ref().unwrap(),
                        instance.as_mut().unwrap(),
                        v.name,
                        &ResultValue::new(convert_args(&v.args)),
                    )
                    .unwrap();
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
                _ => unimplemented!(),
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
                    module.as_ref().unwrap(),
                    instance.as_mut().unwrap(),
                    call.name,
                    &ResultValue::new(convert_args(&call.args)),
                );
                assert!(result.is_err());
            }
            WastDirective::AssertTrap {
                span: _,
                exec,
                message: _,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    let result = telomere::run_module_function(
                        module.as_ref().unwrap(),
                        instance.as_mut().unwrap(),
                        v.name,
                        &ResultValue::new(convert_args(&v.args)),
                    );
                    assert!(result.is_err())
                }
                _ => {
                    todo!()
                }
            },
            WastDirective::Invoke(invoke) => {
                let result = telomere::run_module_function(
                    module.as_ref().unwrap(),
                    instance.as_mut().unwrap(),
                    invoke.name,
                    &ResultValue::new(convert_args(&invoke.args)),
                );
                assert!(!result.is_err())
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