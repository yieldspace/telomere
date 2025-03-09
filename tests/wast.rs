use std::path::PathBuf;

use telomere::{common::Instance, instantiate, Module, ResultValue, WasmValue};
use tracing::{error, Level};
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
                instance = Some(instantiate(&m).unwrap());
                module = Some(m);
            }
            WastDirective::AssertReturn {
                span: _,
                exec,
                results: expected,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
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
                                    assert_eq!(expected, actual)
                                }
                                (WastRetCore::I64(expected), WasmValue::I64(actual)) => {
                                    assert_eq!(expected, actual)
                                }
                                (
                                    WastRetCore::F32(NanPattern::Value(expected)),
                                    WasmValue::F32(actual),
                                ) => {
                                    assert_eq!(expected.bits, actual.to_bits())
                                }
                                (
                                    WastRetCore::F64(NanPattern::Value(expected)),
                                    WasmValue::F64(actual),
                                ) => {
                                    assert_eq!(expected.bits, actual.to_bits())
                                }
                                _ => {
                                    error!("{:?} {:?}", expected, actual);
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
                span: _,
                mut module,
                message,
            } => {
                //TODO: Is there anything that wast fails to encode that could be binary?
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let m = parser.parse_module().is_err();
                    assert_eq!(message, m.to_string())
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
            _ => {}
        }
    }
}

#[test]
fn int_literals() {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("tests/int_literals.wast");
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}
#[test]
fn block() {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("tests/block.wast");
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}
#[test]
fn call() {
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("tests/call.wast");
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}

#[test]
fn memory_grow() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    d.push("tests/memory_grow.wast");
    let wast = std::fs::read_to_string(d).unwrap();
    run_wast(&wast);
}
