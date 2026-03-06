use telomere::component::ComponentEngine;
use wast::parser::ParseBuffer;
use wast::Wast;
#[allow(dead_code)]
pub fn run_component_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();
    let engine = ComponentEngine::new();

    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let name = m.name();
                let span = m.span();
                let source = m.encode().unwrap();
                if let Err(v) = engine.compile(&source) {
                    panic!("{:?} {:?}", span.linecol_in(text), v);
                }
                println!("Parsed component: {name:?}");
            }
            WastDirective::AssertInvalid {
                span, mut module, ..
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                if let Ok(source) = module.encode() {
                    let res = engine.compile(&source);

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
