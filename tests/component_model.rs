use tracing::Level;

#[test]
fn test_empty_component() {
    let component = r#"
        (component
        )
    "#;
    let binary = wat::parse_str(component).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    let k = telomere::parser::component::parse_component(&mut reader).unwrap();
    println!("{:?}", k);
}

#[test]
fn test_with_core_wasm() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::TRACE)
        .init();
    let component = r#"
       (component
          (core module
            (func (export "mod-main") (result i32)
              (i32.const 42))
          )
          (core instance (;0;) (instantiate 0))
          (type (;0;) (func (param "a" u32) (param "b" u32) (result u32)))
          (core instance (;0;)
            (export "add" (func 0))
          )
        )
    "#;
    let binary = wat::parse_str(component).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    // let mut ctx = telomere::parser::component::ParseContext::new(&mut reader);
    let k = telomere::parser::component::parse_component(&mut reader).unwrap();
    println!("{:?}", k);
}
