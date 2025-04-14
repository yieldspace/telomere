use crate::binary::BinaryReader;
use crate::component_model::Component;
use crate::parser::component_model::error::ComponentParseError;

mod error;

pub fn parse_component<R: BinaryReader>(reader: &mut R) -> Result<Component, ComponentParseError> {
    todo!()
}
