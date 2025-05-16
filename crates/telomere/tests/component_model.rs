use telomere::parser::component_model::{ComponentParseError, ParseContext, ParseState, Validator};
use tracing::Level;

#[test]
fn test_basic_component() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();

    let component = r#"
(component
  (import "Libc" (core module $Libc ...))
  (core instance $libc (instantiate $Libc))
  (type $R (resource (rep i32) (dtor (func $libc "free"))))
  (core func $R_new (param i32) (result i32)
    (canon resource.new $R)
  )
  (core module $Main
    (import "canon" "R_new" (func $R_new (param i32) (result i32)))
    (func (export "make_R") (param ...) (result i32)
      (return (call $R_new ...))
    )
  )
  (core instance $main (instantiate $Main
    (with "canon" (instance (export "R_new" (func $R_new))))
  ))
  (export $R' "r" (type $R))
  (func (export "make-r") (param ...) (result (own $R))
    (canon lift (core func $main "make_R"))
  )
)
    "#;
    let binary = wat::parse_str(component)?;
    // let binary = wat::parse_str(std::fs::read_to_string("foo.wat").unwrap()).unwrap();
    // std::fs::write("test.wasm", &binary).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    // let mut instrs = Vec::new();
    let state_arena = typed_arena::Arena::new();
    let mut state = ParseState::new(&state_arena);
    let arena = typed_arena::Arena::new();
    let mut validator = Validator::new(&arena);
    telomere::parser::component_model::parse_component(&mut reader, &mut state, &mut validator)?;
    let mut store = telomere::Store::new();
    let linker = telomere::runtime::component_model::Linker::new();
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
