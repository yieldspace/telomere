use crate::{
    component_model::{ResourceId, TypeId},
    parser::component_model::ParseResult,
};
use std::collections::HashMap;

use super::{InstanceExportType, Type, Validator};

#[derive(Debug, Clone)]
pub enum GenericsReplaceDSL {
    ExportComponent(String, TypeId),
    ExportInstance(String, TypeId),
    ExportFunc(String, TypeId),
    ExportTypeEq(String, TypeId),
    ExportTypeSub(String, TypeId),
}
pub struct GenericsReplaceDSLEnvironment {
    result: HashMap<String, InstanceExportType>,
    unified: HashMap<TypeId, TypeId>,
}
fn resolve_generics(ty: TypeId, unified: &HashMap<TypeId, TypeId>) -> TypeId {
    if let Some(ty) = unified.get(&ty) {
        resolve_generics(*ty, unified)
    } else {
        // FIXME: should resolve inner type
        ty
    }
}
impl GenericsReplaceDSL {
    pub fn evaluate_one(
        instr: &GenericsReplaceDSL,
        validator: &mut Validator,
        env: &mut GenericsReplaceDSLEnvironment,
    ) -> ParseResult<()> {
        let GenericsReplaceDSLEnvironment { result, unified } = env;

        match instr {
            GenericsReplaceDSL::ExportComponent(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Component(resolve_generics(*type_id, unified)),
                );
            }
            GenericsReplaceDSL::ExportInstance(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Instance(resolve_generics(*type_id, unified)),
                );
            }
            GenericsReplaceDSL::ExportFunc(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Func(resolve_generics(*type_id, unified)),
                );
            }
            GenericsReplaceDSL::ExportTypeEq(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Type(resolve_generics(*type_id, unified)),
                );
            }
            GenericsReplaceDSL::ExportTypeSub(name, type_id) => {
                let new_type_id = validator.new_type(Type::Resource(ResourceId::new()));
                unified.insert(*type_id, new_type_id);
                result.insert(name.clone(), InstanceExportType::Type(new_type_id));
            }
        }
        Ok(())
    }
    pub fn evaluate(
        program: &[GenericsReplaceDSL],
        validator: &mut Validator,
    ) -> ParseResult<HashMap<String, InstanceExportType>> {
        let mut env = GenericsReplaceDSLEnvironment {
            result: HashMap::new(),
            unified: HashMap::new(),
        };
        tracing::trace!("{program:?}");
        for instr in program {
            Self::evaluate_one(instr, validator, &mut env)?
        }
        Ok(env.result)
    }
}
