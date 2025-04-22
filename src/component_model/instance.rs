use crate::binary::BinaryReader;
use crate::component_model::{
    ComponentExport, ComponentExportSlot, ComponentIdx, CoreSortWithIdx, InlineExport,
    InstanceExportType, InstanceIdx, InstanceType, Reference, Slot, Sort, SortLike, SortWithIdx,
};
use crate::parser::component_model::{ComponentParseError, ParseContext};
use std::collections::HashMap;

#[derive(Debug)]
pub enum LazyValue<V> {
    Value(V),
    Lazy,
}

impl<V> LazyValue<V> {
    pub fn unwrap(self) -> V {
        match self {
            LazyValue::Value(value) => value,
            LazyValue::Lazy => panic!("Tried to unwrap a lazy value"),
        }
    }

    pub fn is_value(&self) -> bool {
        matches!(self, LazyValue::Value(_))
    }

    pub fn is_lazy(&self) -> bool {
        matches!(self, LazyValue::Lazy)
    }
}

#[derive(Debug, Clone)]
pub struct Instance {
    pub(crate) value: Option<InstanceValue>,
    pub(crate) ty: InstanceType,
}

#[derive(Debug, Clone)]
pub struct InstanceValue {
    pub component_idx: Option<ComponentIdx>,
    pub args: HashMap<String, SortWithIdx>,
    pub exports: HashMap<String, SortWithIdx>,
}

impl InstanceValue {
    pub fn get_type(&self) -> InstanceType {
        InstanceType {
            core_types: vec![],
            types: vec![],
            instances: vec![],
            exports: Default::default(),
        }
    }
}

impl Instance {
    pub fn new(value: Option<InstanceValue>, ty: InstanceType) -> Self {
        Self { value, ty }
    }
    pub fn get_export(&self, name: &String) -> Result<Option<SortWithIdx>, ComponentParseError> {
        match &self.value {
            Some(value) => value
                .exports
                .get(name)
                .cloned()
                .map(|x| Some(x))
                .ok_or_else(|| ComponentParseError::ExportNotFound(name.clone())),
            None => Ok(None),
        }
    }

    pub fn get_export_type(
        &self,
        name: &String,
    ) -> Result<InstanceExportType, ComponentParseError> {
        self.ty.get_export(name)
    }
    // pub fn get_export(
    //     &self,
    //     ctx: &ParseContext<impl BinaryReader>,
    //     self_idx: InstanceIdx,
    //     name: String,
    //     sort: Sort,
    // ) -> Result<ComponentExportSlot, ComponentParseError> {
    //     match self {
    //         Instance::Instantiate(Instantiate { component_idx, .. }) => {
    //             let component = ctx.validator.get_component(component_idx);
    //             let export =
    //                 component.get_export(ctx.validator, component_idx.clone(), name.clone())?;
    //             if export.eq_sort(sort) {
    //                 return Ok(export);
    //             }
    //         }
    //         Instance::InlineExport(exports) => {
    //             for export in exports {
    //                 if export.sort.eq_sort(&sort) && export.name == name {
    //                     match export.sort {
    //                         SortWithIdx::Core(CoreSortWithIdx::Module(idx)) => {
    //                             return Ok(ComponentExportSlot::CoreModule(Slot::Idx(idx)));
    //                         }
    //                         SortWithIdx::Func(idx) => {
    //                             return Ok(ComponentExportSlot::Func(Slot::Idx(idx)));
    //                         }
    //                         #[cfg(feature = "component-gated-feature-value-imports-exports")]
    //                         SortWithIdx::Value(_) => todo!(),
    //                         SortWithIdx::Type(idx) => {
    //                             return Ok(ComponentExportSlot::Type(Slot::Idx(idx)));
    //                         }
    //                         SortWithIdx::Component(idx) => {
    //                             return Ok(ComponentExportSlot::Component(Slot::Idx(idx)));
    //                         }
    //                         SortWithIdx::Instance(idx) => {
    //                             return Ok(ComponentExportSlot::Instance(Slot::Idx(idx)));
    //                         }
    //                         _ => {
    //                             return Err(ComponentParseError::InvalidSort(
    //                                 export.sort.clone(),
    //                                 "ComponentExportSlot".to_string(),
    //                             ));
    //                         }
    //                     }
    //                 }
    //             }
    //         }
    //         Instance::Typed(ty, _) | Instance::SuperTyped(ty, _, _) => {
    //             return Ok(ty.get_export(ctx.validator, self_idx, name.clone())?);
    //         }
    //     }
    //     Err(ComponentParseError::ExportNotFound(name))
    // }
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
