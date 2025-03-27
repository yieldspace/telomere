#[test]
fn test_empty_component() {
    let component = r#"
        (component
        )
    "#;
    let binary = wat::parse_str(component).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    telomere::parser::component::parse_component(&mut reader, None).unwrap();
}

#[test]
fn test_with_core_wasm() {
    let component = r#"
        (component
          (core module
            (func (export "mod-main") (result i32)
              (i32.const 42))
          )
        )
    "#;
    let binary = wat::parse_str(component).unwrap();
    let mut reader = telomere::IoReadBinaryReader::from(&binary[..]);
    telomere::parser::component::parse_component(&mut reader, None).unwrap();
}
