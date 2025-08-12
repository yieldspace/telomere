use binary_reader::IoReadBinaryReader;
use component_model::{ComponentParser, CoreInstanceIndex, CoreModuleIndex, Dependency, TypeStore};
use std::collections::HashMap;

#[tokio::test]
async fn test_basic() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    let component = r#"
(component
   (core module)
   (component
     (import "key" (type (sub resource)))
   )
   (type (resource (rep i32)))
   (instance (instantiate 0 (with "key" (type 0))))
)
    "#;
    let binary = wat::parse_str(component)?;
    let mut reader = IoReadBinaryReader::from(binary.as_slice());

    let mut store = TypeStore::default();
    let parser = ComponentParser::new(&mut reader, &mut store);
    let component = parser.parse()?;
    Ok(())
}
