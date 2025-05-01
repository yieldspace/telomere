use crate::component_model::{InstanceType, Type};

#[derive(Debug, Clone)]
pub enum AliasType {
    Type(Type),
    Instance(InstanceType),
}
