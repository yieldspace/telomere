use std::path::PathBuf;

use telomere::{
    runtime::vm::{ResultValue, WasmValue},
    Module,
};
use wast::{core::WastRetCore, parser::ParseBuffer, Wast, WastRet};

fn run_wast(wast: &str) {
    let buf = ParseBuffer::new(wast).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let mut module: Option<Module> = None;
    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let source = m.encode().unwrap();
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let mut parser = telomere::WasmParser::new(&mut reader);
                let m = parser.parse_module().unwrap();
                module = Some(m)
            }
            WastDirective::AssertReturn {
                span,
                exec,
                results: expected,
            } => match exec {
                wast::WastExecute::Invoke(v) => {
                    let actual = telomere::run_module_function(
                        module.as_ref().unwrap(),
                        v.name,
                        &ResultValue::new(vec![]),
                    );
                    for (expected, actual) in expected.iter().zip(actual.iter()) {
                        if let WastRet::Core(expected) = expected {
                            match (expected, actual) {
                                (WastRetCore::I32(expected), WasmValue::I32(actual)) => {
                                    assert_eq!(expected, actual)
                                }
                                (WastRetCore::I64(expected), WasmValue::I64(actual)) => {
                                    assert_eq!(expected, actual)
                                }
                                _ => unimplemented!(),
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
                message,
            } => {
                //TODO: Is there anything that wast fails to encode that could be binary?
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let mut parser = telomere::WasmParser::new(&mut reader);
                    let m = parser.parse_module().unwrap_err();
                    assert_eq!(message, m.to_string())
                }
            }
            _ => unimplemented!(),
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
