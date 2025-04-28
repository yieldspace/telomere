use telomere::parser::component_model::{ParseContext, Validator};
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
                let mut instrs = Vec::new();
                let mut validator = Validator::new();
                let mut state = telomere::component_model::CompiledState::new();
                let mut ctx = ParseContext::new(&mut reader, &mut instrs, &mut validator, &mut state);
                if let Err(v) = telomere::parser::component_model::parse_component(&mut ctx) {
                    panic!("{:?} {:?}", span.linecol_in(text), v);
                }
                println!("Parsed component: {name:?}");
            }
            _ => unimplemented!(),
        }
    }
}
