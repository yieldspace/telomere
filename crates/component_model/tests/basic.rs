use binary_reader::IoReadBinaryReader;
use component_model::{CoreInstanceIndex, CoreModuleIndex, Dependency};
use std::collections::HashMap;

#[tokio::test]
async fn test_basic() -> anyhow::Result<()> {
    let component = r#"
(module)
    "#;
    let binary = wat::parse_str(component)?;
    let mut reader = IoReadBinaryReader::from(binary.as_slice());
    let module = telomere_wasm::WasmParser::new(&mut reader).parse_module()?;

    let mut store = telomere_wasm::Store::new();
    let c = component_model::Component {
        core_modules: HashMap::from([(CoreModuleIndex(0), module)]),
        core_instances: HashMap::from([(
            CoreInstanceIndex(0),
            component_model::CoreInstance {
                module_index: CoreModuleIndex(0),
            },
        )]),
        dependencies: vec![Dependency::CoreInstantiate(CoreInstanceIndex(0))],
    };
    c.instantiate(&mut store).await;
    Ok(())
}
