use telomere::component_model::FlattenComponent;
use telomere::parser::component_model::{ComponentValidator, ParseContext};
use wast::parser::ParseBuffer;
use wast::Wast;

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
                let mut component = FlattenComponent::new();
                let mut validator = ComponentValidator::new(&mut component);
                let mut ctx = ParseContext::new(&mut reader, &mut instrs, &mut validator);
                if let Err(v) = telomere::parser::component_model::parse_component(&mut ctx) {
                    panic!("{:?} {:?}", span.linecol_in(text), v);
                }
                println!("Parsed component: {name:?}");
            }
            _ => unimplemented!(),
        }
    }
}
