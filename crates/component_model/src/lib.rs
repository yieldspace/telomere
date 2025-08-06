mod canon;
mod error;
mod name;
mod parser;

pub use error::ComponentParseError;
pub use parser::ComponentParser;
use std::collections::HashMap;
use std::pin::Pin;
use telomere_wasm::common::{Import, InstanceHandle};
use telomere_wasm::{instantiate as core_instantiate, Module, Registry, Store};

pub type Result<T> = std::result::Result<T, ComponentParseError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ComponentIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreModuleIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CoreInstanceIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FunctionIndex(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstanceIndex(pub u32);

pub trait ImportRegistryImpl {
    fn register_to_context(
        &self,
        imports: &HashMap<String, ComponentImport>,
        context: &mut InstantiateContext,
    ) {
        todo!()
    }
}

pub struct ContextImportRegistry<'a> {
    pub context: &'a InstantiateContext,
}

impl<'a> ContextImportRegistry<'a> {
    pub fn new(context: &'a InstantiateContext) -> Self {
        Self { context }
    }
}

impl ImportRegistryImpl for ContextImportRegistry<'_> {
    fn register_to_context(
        &self,
        imports: &HashMap<String, ComponentImport>,
        context: &mut InstantiateContext,
    ) {
        todo!()
    }
}

#[derive(Clone)]
pub struct InstantiateContext {}

impl InstantiateContext {
    pub fn get_core_module(&self, index: CoreModuleIndex) -> Option<&Module> {
        todo!()
    }

    pub fn get_component(&self, index: ComponentIndex) -> Option<&Component> {
        todo!()
    }

    pub fn register_instance(&mut self, index: InstanceIndex, instance: Instance) {
        todo!()
    }
    pub fn register_core_instance(&mut self, index: CoreInstanceIndex, instance: InstanceHandle) {
        todo!()
    }
}

pub struct CoreInstance {
    pub module_index: CoreModuleIndex,
}

#[derive(Clone)]
pub struct Component {
    pub imports: HashMap<String, ComponentImport>,
    pub exports: HashMap<String, ComponentExport>,
    pub context: InstantiateContext,
    pub dependencies: Vec<Dependency>,
}

#[derive(Clone)]
/// bind to
pub enum ComponentImport {
    Component(ComponentIndex), // note: あとで型をつける
    Instance(InstanceIndex),
    Function(FunctionIndex),
    Resource,
    CoreModule(CoreModuleIndex),
}

#[derive(Clone)]
pub enum ComponentExport {
    Component(ComponentIndex),
    Instance(InstanceIndex),
    Function(FunctionIndex),
    Type,
    Resource,
    CoreModule(CoreModuleIndex),
}

pub struct Instance {
    pub exports: HashMap<String, InstanceExport>,
    functions: HashMap<FunctionIndex, ComponentFunction>,
    instances: HashMap<InstanceIndex, Instance>,
    /// only exports core modules
    core_modules: HashMap<CoreModuleIndex, CoreModuleIndex>,
    core_instances: HashMap<CoreInstanceIndex, InstanceHandle>,
}

pub enum InstanceExport {
    Function(FunctionIndex),
    Instance(InstanceIndex),
    CoreModule(CoreModuleIndex),
}

pub struct ComponentFunction {}

pub enum ComponentFuncDef {
    Lift(ComponentFunction),
    ResourceNew,
    ResourceDrop,
    ResourceRep,
}

impl Component {
    /// componentをインタンス化する．
    /// componentの中での依存関係であってもこの関数を利用する
    pub async fn instantiate<R>(self, store: &mut Store, registry: R) -> Instance
    where
        R: ImportRegistryImpl,
    {
        let Self {
            imports,
            dependencies,
            mut context,
            ..
        } = self;
        // todo: importを処理してcontextに登録する．
        registry.register_to_context(&imports, &mut context);

        for dependency in dependencies {
            match dependency {
                Dependency::CoreInstantiate(idx, module_idx) => {
                    let module = context
                        .get_core_module(module_idx)
                        .expect("Core module not found");

                    let registry = Registry::new();
                    let instance = core_instantiate(module.clone(), store, &registry)
                        .await
                        .unwrap();
                    println!("Instantiated core instance: {:?}", instance);
                    context.register_core_instance(idx, instance);
                }
                Dependency::Lower(_) => {
                    // やりたいこととしては，lowerする関数としてcore wasmの関数を作るんだけど，
                    // その前に，liftについて考える
                    // liftって結局wrapper関数を作るだけなので、マジで簡単
                    // lowerはliftの逆をすればいいんだけど，lower/liftをどっちも動的にやるのは普通に処理に時間がかかるから，うまくやる必要がある
                    todo!()
                }
                Dependency::Start => {
                    todo!()
                }
                Dependency::Instantiate(idx, component_idx) => {
                    let component = context
                        .get_component(component_idx)
                        .expect("Component not found");
                    {
                        let registry = ContextImportRegistry::new(&context);
                        let instance = component.clone().instantiate(store, registry).await;
                        context.register_instance(idx, instance);
                    }
                }
            }
        }
        Instance {
            exports: HashMap::new(), // Populate with actual exports if needed
            functions: Default::default(),
            instances: Default::default(),
            core_modules: Default::default(),
            core_instances: Default::default(),
        }
    }
}

#[derive(Clone)]
pub enum Dependency {
    CoreInstantiate(CoreInstanceIndex, CoreModuleIndex),
    Instantiate(InstanceIndex, ComponentIndex),
    Lower(LowerAdaptor),
    Start,
}

#[derive(Clone)]
/// Component Function -> Core Function
pub struct LowerAdaptor {
    pub func: FunctionIndex,
    // options: canon options
}
