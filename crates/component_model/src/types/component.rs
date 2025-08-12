use crate::Result;
use crate::name::{ExportName, ImportName};
use crate::types::resource::ResourcePlan;
use crate::types::{
    ComponentDefId, ComponentTypeId, FuncTypeId, InstanceTypeId, LocalTypeMap, ResourceDefId,
    TypeId, TypeIdx, TypeStore,
};
use indexmap::IndexMap;
use std::fmt::{Debug, Display, Formatter};
use thiserror::__private::AsDisplay;

#[derive(Debug)]
pub struct ComponentType {
    pub id: ComponentDefId,
    pub local_type_map: LocalTypeMap,
    pub plan: ResourcePlan,
    pub surface: ComponentSurface,
}

#[derive(Debug, Clone)]
pub enum PublicTyRef {
    Func(FuncTypeId),
    Instance(InstanceTypeId), // 型そのもの（public 化済みの TypeId 参照で十分な場面も多い）
    Component(ComponentTypeId),
    TypeEq(TypeId), // eq type
    TypeSubResource(ResourceDefId /* inner type id */),
}

#[derive(Default, Debug)]
pub struct ComponentSurface {
    pub imports: IndexMap<ImportName, PublicTyRef>,
    pub exports: IndexMap<ExportName, PublicTyRef>,
}

// impl Debug for ComponentType {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         let mut st = f.debug_struct("Component");
//         for (idx, id) in self.local_type_map.components.clone() {
//             st.field(&*format!("{:?}", idx), &self.store.components.get(&id).unwrap());
//         }
//         for (idx, id) in self.local_type_map.instances.clone() {
//             st.field(&*format!("{:?}", idx), &self.store.instances.get(&id).unwrap());
//         }
//         for (idx, id) in self.local_type_map.funcs.clone() {
//             st.field(&*format!("{:?}", idx), &self.store.funcs.get(&id).unwrap());
//         }
//         for (k, id) in self.local_type_map.types.iter().enumerate() {
//             st.field(&*format!("{:?}", k), &id);
//         }
//         st.field("resource_plan", &self.plan);
//         st.finish()
//     }
// }

impl PublicTyRef {
    pub fn ensure_sub_resource(&self) -> Result<ResourceDefId> {
        match self {
            PublicTyRef::TypeSubResource(res_id) => Ok(*res_id),
            _ => Err(crate::ComponentParseError::TypeError(format!(
                "Not a sub resource type: {:?}",
                self
            ))),
        }
    }

    pub fn ensure_component(&self) -> Result<ComponentTypeId> {
        match self {
            PublicTyRef::Component(id) => Ok(*id),
            _ => Err(crate::ComponentParseError::TypeError(format!(
                "Not a component type: {:?}",
                self
            ))),
        }
    }

    pub fn ensure_instance(&self) -> Result<InstanceTypeId> {
        match self {
            PublicTyRef::Instance(id) => Ok(*id),
            _ => Err(crate::ComponentParseError::TypeError(format!(
                "Not an instance type: {:?}",
                self
            ))),
        }
    }
}
