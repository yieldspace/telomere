use crate::binary::BinaryReader;
use crate::component_model::{
    ComponentExportSlot, ComponentIdx,
    CoreSortWithIdx, InlineExport, InstanceIdx, InstanceType, Reference,
    Slot, Sort, SortLike, SortWithIdx,
};
use crate::parser::component_model::{ComponentParseError, ParseContext};

#[derive(Debug)]
pub enum Instance {
    Instantiate(Instantiate),
    InlineExport(Vec<InlineExport>),
    Typed(InstanceType, Reference),
    SuperTyped(InstanceType, InstanceIdx, Reference),
}

impl Instance {
    pub fn get_export(
        &self,
        ctx: &ParseContext<impl BinaryReader>,
        self_idx: InstanceIdx,
        name: String,
        sort: Sort,
    ) -> Result<ComponentExportSlot, ComponentParseError> {
        match self {
            Instance::Instantiate(Instantiate { component_idx, .. }) => {
                let component = ctx.validator.get_component(component_idx);
                let export =
                    component.get_export(ctx.validator, component_idx.clone(), name.clone())?;
                if export.eq_sort(sort) {
                    return Ok(export);
                }
            }
            Instance::InlineExport(exports) => {
                for export in exports {
                    if export.sort.eq_sort(&sort) && export.name == name {
                        match export.sort {
                            SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
                                return Ok(ComponentExportSlot::CoreModule(Slot::Idx(idx)));
                            }
                            SortWithIdx::Func(idx) => {
                                return Ok(ComponentExportSlot::Func(Slot::Idx(idx)));
                            }
                            #[cfg(feature = "component-gated-feature-value-imports-exports")]
                            SortWithIdx::Value(_) => todo!(),
                            SortWithIdx::Type(idx) => {
                                return Ok(ComponentExportSlot::Type(Slot::Idx(idx)));
                            }
                            SortWithIdx::Component(idx) => {
                                return Ok(ComponentExportSlot::Component(Slot::Idx(idx)));
                            }
                            SortWithIdx::Instance(idx) => {
                                return Ok(ComponentExportSlot::Instance(Slot::Idx(idx)));
                            }
                            _ => {
                                return Err(ComponentParseError::InvalidSort(
                                    export.sort.clone(),
                                    "ComponentExportSlot".to_string(),
                                ));
                            }
                        }
                    }
                }
            }
            Instance::Typed(ty, _) | Instance::SuperTyped(ty, _, _) => {
                return Ok(ty.get_export(ctx.validator, self_idx, name.clone())?);
            }
        }
        Err(ComponentParseError::ExportNotFound(name))
    }
}

#[derive(Debug)]
pub struct Instantiate {
    pub component_idx: ComponentIdx,
    pub args: Vec<InstantiateArg>,
}

#[derive(Debug)]
pub struct InstantiateArg {
    pub name: String,
    pub sort: SortWithIdx,
}
