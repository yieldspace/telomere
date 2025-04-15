use tracing::Level;

#[test]
fn test_basic_component() {
    tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let component = r#"
       (component)
    "#;
    let binary = wat::parse_str(component).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    let k = telomere::parser::component_model::parse_component(&mut reader).unwrap();
    let mut store = telomere::Store::new();
    let linker = telomere::runtime::component_model::Linker::new();
    let instance = telomere::runtime::component_model::instantiate(k, &mut store, &linker).unwrap();
    todo!()
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
