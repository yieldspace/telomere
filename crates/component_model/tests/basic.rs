use binary_reader::IoReadBinaryReader;
use component_model::{ComponentParser, CoreInstanceIndex, CoreModuleIndex, Dependency};
use std::collections::HashMap;

#[tokio::test]
async fn test_basic() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    let component = r#"
(component
   (core module)
   (component)
   (instance (instantiate 0))
)
    "#;
    let binary = wat::parse_str(component)?;
    let mut reader = IoReadBinaryReader::from(binary.as_slice());

    let parser = ComponentParser::new(&mut reader);
    let component = parser.parse()?;
    Ok(())
}
