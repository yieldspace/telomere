use std::collections::HashMap;
use crate::parser::component::{ComponentOp, RawComponent, RawData};
use crate::{Component, Dependency, Result, ComponentIndex, InstanceIndex};
use crate::name::ExportName;
use crate::parser::idx::{RawComponentIdx, RawImportId, RawInstanceIdx};
use crate::parser::instance::RawInstanceDef;


pub struct Inliner {
    raw_component: RawComponent,
}

pub enum InlineInstanceExport {
    Component(RawComponent)
}

pub struct InlineInstance {
    exports: HashMap<ExportName, InlineInstanceExport>
}

pub struct LocalContext {
    imports: HashMap<RawImportId, RawComponentIdx>,
    instances: HashMap<RawInstanceIdx, InlineInstance>,
}

pub struct InlineContext {
    dependencies: Vec<Dependency>,
}

impl InlineContext {
    fn get_component_from_data(&self, data: &RawData<RawComponent>) -> Result<&RawComponent> {
        match data {
            RawData::Defined(component) => Ok(component),
            RawData::Imported(import_id) => Err("Cannot inline imported component".into()),
            RawData::ReExported(_, _) => Err("Cannot inline re-exported component".into()),
        }
    }
}

impl Inliner {
    pub fn new(raw_component: RawComponent) -> Self {
        Self { raw_component }
    }

    pub fn run(self, ctx: &mut InlineContext) -> Result<Component> {
        let Self { raw_component } = self;
        for op in &raw_component.ops {
            match op {
                ComponentOp::Instantiate(idx) => {
                    let instance = raw_component.get_instance(idx)?;
                    match instance {
                        RawData::Defined(instance_def) => match instance_def {
                            RawInstanceDef::Instantiate(instance) => {
                                let component = raw_component.get_component(&instance.component_idx)?;
                            }
                            RawInstanceDef::InlineExport(inline_export) => {}
                        }
                        RawData::Imported(import_id) => {}
                        RawData::ReExported(name, idx) => {}
                    }
                }
                ComponentOp::CoreInstantiate(_) => {}
                ComponentOp::DefineCoreModule(_) => {}
                ComponentOp::DefineComponent(_) => {}
            }
        }
        Ok(())
    }
}

impl InlineContext {
    pub fn new() -> Self {
        Self {
            dependencies: Vec::new(),
        }
    }

    pub fn push_instantiate(&mut self,) {

    }
}
