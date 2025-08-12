use binary_reader::IoReadBinaryReader;
use component_model::{ComponentParser, TypeStore};
use telomere_wasm::Store;
use wast::Wast;
use wast::parser::ParseBuffer;

#[allow(dead_code)]
pub async fn run_component_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();

    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let name = m.name();
                let span = m.span();
                let source = m.encode().unwrap();
                let mut reader = IoReadBinaryReader::from(&source[..]);
                let mut store = TypeStore::default();
                let parser = ComponentParser::new(&mut reader, None);
                let component = parser.parse().unwrap();
            }
            WastDirective::AssertInvalid {
                span, mut module, ..
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                if let Ok(source) = module.encode() {
                    let mut reader = IoReadBinaryReader::from(&source[..]);
                    let mut store = TypeStore::default();
                    let parser = ComponentParser::new(&mut reader, None);
                    let res = parser.parse();

                    match res {
                        Err(_err) => {
                            // TODO:
                        }
                        Ok(_) => panic!("Expected panic but succeed@{:?}", span.linecol_in(text)),
                    }
                }
            }
            _ => unimplemented!(),
        }
    }
}
