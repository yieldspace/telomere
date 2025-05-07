use telomere::parser::component_model::{
    ComponentParseError, ParseContext, Validator, ValidatorState,
};
use tracing::Level;

#[test]
fn test_basic_component() -> Result<(), ComponentParseError> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();

    let component = r#"
       (component
            (type (;0;)
                (component
                    (export (;0;) "add" (type (sub resource)))
                )
            )
            (import "docs:adder/add@0.1.0" (component (;0;) (type 0)))
            (component
            )
            (export "foo" (component 0))
       )
    "#;
    let binary = wat::parse_str(component).unwrap();
    // let binary = wat::parse_str(std::fs::read_to_string("foo.wat").unwrap()).unwrap();
    // std::fs::write("test.wasm", &binary).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    // let mut instrs = Vec::new();
    let mut state = ValidatorState::new();
    let arena = typed_arena::Arena::new();
    let mut validator = Validator::new(&arena);
    telomere::parser::component_model::parse_component(&mut reader, &mut state, &mut validator)?;
    let mut store = telomere::Store::new();
    let linker = telomere::runtime::component_model::Linker::new();
    println!("{:?}", validator.scope().make_component_type());
    // let instance =
    //     telomere::runtime::component_model::instantiate(&mut instrs, &mut store, &linker).unwrap();
    // println!("{:?}", instance);
    Ok(())
}

/*#[test]
fn test_with_core_wasm() {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let component = r#"
       (component
          (type (;0;)
            (instance
              (type (;0;) (func (param "a" u32) (param "b" u32) (result u32)))
              (export (;0;) "add" (func (type 0)))
            )
          )
          (import "docs:adder/add@0.1.0" (instance (;0;) (type 0)))
          (alias export 0 "add" (func (;0;)))
          (core func (;0;) (canon lower (func 0)))
          (core instance (;0;)
            (export "add" (func 0))
          )
        )
    "#;
    let binary = wat::parse_str(component).unwrap();
    std::fs::write("test.wasm", &binary).unwrap();
}
*/
