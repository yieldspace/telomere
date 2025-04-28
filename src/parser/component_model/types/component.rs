use crate::binary::BinaryReader;
use crate::component_model::{
    ComponentDecl, ComponentIdx, ComponentImportType, ComponentType, CoreModule, CoreModuleIdx,
    CoreType, CoreTypeIdx, ExternDesc, Func, FuncIdx, FunctionBinding, InlineComponent, Instance,
    InstanceDecl, InstanceIdx, Type, TypeIdx,
};
use crate::parser::component_model::types::parse_import_decl;
use crate::parser::component_model::{
    parse_vec_range, ComponentParseError, ParseContext, SizedResult, Validator,
    _parse_instance_decl,
};

pub fn parse_component_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<ComponentType> {
    let start_count = ctx.reader.read_count();
    let mut new_validator = Validator::new_child(ctx.validator);
    let mut instrs = Vec::new();
    let state = &mut ctx.state;
    let mut new_ctx = ParseContext::new(ctx.reader, &mut instrs, &mut new_validator, state);

    let mut component_type = ComponentType::new();

    for _ in parse_vec_range(&mut new_ctx)? {
        let (_, decl) = parse_component_decl(&mut new_ctx)?;
        match decl {
            ComponentDecl::Import(import) => match import.ed {
                ExternDesc::CoreModule(ty) => {
                    new_ctx.validator.add_core_module_type(ty.clone())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::CoreModule(ty.clone()));
                }
                ExternDesc::Func(ty) => {
                    new_ctx.validator.add_func_type(ty.clone())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Func(ty.clone()));
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => {}
                ExternDesc::Type(ty) => {
                    new_ctx.validator.add_type(ty.clone())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Type(ty.clone()));
                }
                ExternDesc::Component(ty) => {
                    new_ctx.validator.add_component_type(ty.clone())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Component(ty.clone()));
                }
                ExternDesc::Instance(ty) => {
                    new_ctx.validator.add_instance_type(ty.clone())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Instance(ty.clone()));
                }
            },
            ComponentDecl::Instance(decl) => match decl {
                InstanceDecl::CoreModuleType(ty) => {
                    new_ctx.validator.add_core_module_type(ty.clone())?;
                }
                InstanceDecl::Type(_) => {}
                InstanceDecl::Alias(_) => {}
                InstanceDecl::ExportDecl(_) => {}
            },
        }
    }

    Ok((ctx.reader.read_count() - start_count, component_type))
}

fn parse_component_decl(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<ComponentDecl> {
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
