use crate::component::decoder::ParseResult;
use crate::component::ir::types::CoreModuleType;
use crate::component::ir::TypeId;
use std::collections::HashMap;

use super::{InstanceExportType, Validator};

#[derive(Debug, Clone)]
pub enum GenericsReplaceDSL {
    ExportCoreModule(String, CoreModuleType),
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
fn resolve_generics(
    ty: TypeId,
    validator: &mut Validator,
    unified: &mut HashMap<TypeId, TypeId>,
) -> ParseResult<TypeId> {
    validator.instantiate_type_id(ty, unified)
}
impl GenericsReplaceDSL {
    pub fn evaluate_one(
        instr: &GenericsReplaceDSL,
        validator: &mut Validator,
        env: &mut GenericsReplaceDSLEnvironment,
    ) -> ParseResult<()> {
        let GenericsReplaceDSLEnvironment { result, unified } = env;

        match instr {
            GenericsReplaceDSL::ExportCoreModule(name, ty) => {
                result.insert(name.clone(), InstanceExportType::CoreModule(ty.clone()));
            }
            GenericsReplaceDSL::ExportComponent(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Component(
                        validator.resolve_surface_type_id(*type_id, unified)?,
                    ),
                );
            }
            GenericsReplaceDSL::ExportInstance(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Instance(
                        validator.resolve_surface_type_id(*type_id, unified)?,
                    ),
                );
            }
            GenericsReplaceDSL::ExportFunc(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Func(resolve_generics(*type_id, validator, unified)?),
                );
            }
            GenericsReplaceDSL::ExportTypeEq(name, type_id) => {
                result.insert(
                    name.clone(),
                    InstanceExportType::Type(resolve_generics(*type_id, validator, unified)?),
                );
            }
            GenericsReplaceDSL::ExportTypeSub(name, type_id) => {
                let new_type_id = validator.instantiate_sub_resource_type(*type_id, unified)?;
                result.insert(name.clone(), InstanceExportType::Type(new_type_id));
            }
        }
        Ok(())
    }
    pub fn evaluate(
        program: &[GenericsReplaceDSL],
        validator: &mut Validator,
        initial_unified: HashMap<TypeId, TypeId>,
    ) -> ParseResult<HashMap<String, InstanceExportType>> {
        let mut env = GenericsReplaceDSLEnvironment {
            result: HashMap::new(),
            unified: initial_unified,
        };
        tracing::trace!("{program:?}");
        for instr in program {
            Self::evaluate_one(instr, validator, &mut env)?
        }
        Ok(env.result)
    }
}
