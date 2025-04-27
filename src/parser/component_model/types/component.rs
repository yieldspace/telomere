use crate::binary::BinaryReader;
use crate::component_model::{ComponentDecl, ComponentImportType, ComponentType, ExternDesc, Instance, InstanceDecl, InstanceIdx, Type, TypeIdx};
use crate::parser::component_model::types::{parse_import_decl, TypeValidator};
use crate::parser::component_model::{
    DefaultValidator,
    parse_vec_range, ParseContext, SizedResult, Validator, _parse_instance_decl,
};
use crate::parser::component_model::validator::DefaultParent;

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ComponentType> {
    let start_count = ctx.reader.read_count();
    let mut new_validator = TypeValidator::new(DefaultParent::new(ctx.validator));
    let mut instrs = Vec::new();
    let mut new_ctx = ParseContext::new(ctx.reader, &mut instrs, &mut new_validator);

    let mut component_type = ComponentType::new();

    for _ in parse_vec_range(&mut new_ctx)? {
        let (_, decl) = parse_component_decl(&mut new_ctx)?;
        match decl {
            ComponentDecl::Import(import) => {
                match import.ed {
                    ExternDesc::CoreModule(ty) => {
                        new_ctx.validator.add_core_module_type(ty.clone());
                        component_type.imports.insert(import.name, ComponentImportType::CoreModule(ty.clone()));
                    }
                    ExternDesc::Func(ty) => {
                        new_ctx.validator.add_func_type(ty.clone());
                        component_type.imports.insert(import.name, ComponentImportType::Func(ty.clone()));
                    }
                    #[cfg(feature = "component-gated-feature-value-imports-exports")]
                    ExternDesc::Value(_) => {}
                    ExternDesc::Type(ty) => {
                        new_ctx.validator.add_type(ty.clone());
                        component_type.imports.insert(import.name, ComponentImportType::Type(ty.clone()));

                    }
                    ExternDesc::Component(ty) => {
                        new_ctx.validator.add_component_type(ty.clone());
                        component_type.imports.insert(import.name, ComponentImportType::Component(ty.clone()));
                    }
                    ExternDesc::Instance(ty) => {
                        new_ctx.validator.add_instance_type(ty.clone());
                        component_type.imports.insert(import.name, ComponentImportType::Instance(ty.clone()));
                    }
                }
            }
            ComponentDecl::Instance(decl) => match decl {
                InstanceDecl::CoreModuleType(ty) => {
                    new_ctx.validator.add_core_module_type(ty);
                }
                InstanceDecl::Type(_) => {}
                InstanceDecl::Alias(_) => {}
                InstanceDecl::ExportDecl(_) => {}
            }
        }
    }

    Ok((ctx.reader.read_count() - start_count, component_type))
}

fn parse_component_decl(
    ctx: &mut ParseContext<impl BinaryReader, impl DefaultValidator>,
) -> SizedResult<ComponentDecl> {
    let start_count = ctx.reader.read_count();
    let decl = match ctx.reader.read_exact_one()? {
        0x03 => {
            let (_, decl) = parse_import_decl(ctx)?;
            ComponentDecl::Import(decl)
        }
        x => {
            let (_, decl) = _parse_instance_decl(ctx, Some(x))?;
            ComponentDecl::Instance(decl)
        }
    };
    Ok((ctx.reader.read_count() - start_count, decl))
}
