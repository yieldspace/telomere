use telomere::parser::component_model::{ParseContext, ParseState, Validator};
use wast::parser::ParseBuffer;
use wast::Wast;
#[allow(dead_code)]
pub fn run_component_wast(text: &str) {
    let buf = ParseBuffer::new(text).unwrap();
    let wast = wast::parser::parse::<Wast>(&buf).unwrap();

    for directive in wast.directives {
        use wast::WastDirective;
        match directive {
            WastDirective::Module(mut m) => {
                let name = m.name();
                let span = m.span();
                let source = m.encode().unwrap();
                let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                let state_arena = typed_arena::Arena::new();
                let mut state = ParseState::new(&state_arena);
                let arena = typed_arena::Arena::new();
                let mut validator = Validator::new(&arena);
                if let Err(v) = telomere::parser::component_model::parse_component(
                    &mut reader,
                    &mut state,
                    &mut validator,
                ) {
                    panic!("{:?} {:?}", span.linecol_in(text), v);
                }
                println!("Parsed component: {name:?}");
            }
            WastDirective::AssertInvalid {
                span, mut module, ..
            } => {
                tracing::trace!("AssertInvalid @ {:?}", span.linecol_in(text));
                if let Ok(source) = module.encode() {
                    let mut reader = telomere::IoReadBinaryReader::from(&source[..]);
                    let state_arena = typed_arena::Arena::new();
                    let mut state = ParseState::new(&state_arena);
                    let arena = typed_arena::Arena::new();
                    let mut validator = Validator::new(&arena);
                    let res = telomere::parser::component_model::parse_component(
                        &mut reader,
                        &mut state,
                        &mut validator,
                    );

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
