use crate::binary::BinaryReader;
use crate::component_model::{
    ComponentDecl, ComponentFunction, ComponentIdx, ComponentImportType, ComponentType, CoreModule,
    CoreModuleIdx, CoreType, CoreTypeIdx, ExternDesc, FuncIdx, FunctionBinding, InlineComponent,
    Instance, InstanceDecl, InstanceIdx, Resolver, Type, TypeIdx,
};
use crate::parser::component_model::types::{parse_import_decl, TypeValidatorState};
use crate::parser::component_model::validator::{
    DefaultValidatorState, IdxValidator, ValidatorStateImpl,
};
use crate::parser::component_model::{
    parse_vec_range, ComponentParseError, ParseContext, SizedResult, Validator,
    _parse_instance_decl,
};
use crate::parser::component_model::types::validator::TypeSuperValidator;

pub fn parse_component_type(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<FuncIdx, Resolved = ComponentFunction>
            + IdxValidator<TypeIdx, Resolved = Type>
            + IdxValidator<CoreModuleIdx, Resolved = CoreModule>
            + IdxValidator<InstanceIdx, Resolved = Instance>
            + IdxValidator<ComponentIdx, Resolved = InlineComponent>
            + IdxValidator<CoreTypeIdx, Resolved = CoreType> + TypeSuperValidator,
    >,
) -> SizedResult<ComponentType> {
    let start_count = ctx.reader.read_count();
    let mut new_validator = Validator::new(TypeValidatorState::new(&mut ctx.validator.state));
    let mut instrs = Vec::new();
    let mut new_ctx = ParseContext::new(ctx.reader, &mut instrs, &mut new_validator);

    let mut component_type = ComponentType::new();

    for _ in parse_vec_range(&mut new_ctx)? {
        let (_, decl) = parse_component_decl(&mut new_ctx)?;
        match decl {
            ComponentDecl::Import(import) => match import.ed {
                ExternDesc::CoreModule(ty) => {
                    new_ctx
                        .validator
                        .state
                        .add_core_module(CoreModule::new(None, ty.clone()).into())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::CoreModule(ty.clone()));
                }
                ExternDesc::Func(ty) => {
                    new_ctx.validator.state.add_func(FunctionBinding::Real(
                        ComponentFunction::new(None, ty.clone()),
                    ))?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Func(ty.clone()));
                }
                #[cfg(feature = "component-gated-feature-value-imports-exports")]
                ExternDesc::Value(_) => {}
                ExternDesc::Type(ty) => {
                    new_ctx.validator.state.add_type(ty.clone().into());
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Type(ty.clone()));
                }
                ExternDesc::Component(ty) => {
                    new_ctx
                        .validator
                        .state
                        .add_component(InlineComponent::new(None, ty.clone()).into())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Component(ty.clone()));
                }
                ExternDesc::Instance(ty) => {
                    new_ctx
                        .validator
                        .state
                        .add_instance(Instance::new(None, ty.clone()).into())?;
                    component_type
                        .imports
                        .insert(import.name, ComponentImportType::Instance(ty.clone()));
                }
            },
            ComponentDecl::Instance(decl) => match decl {
                InstanceDecl::CoreModuleType(ty) => {
                    new_ctx
                        .validator
                        .state
                        .add_core_module(CoreModule::new(None, ty.clone()).into())?;
                }
                InstanceDecl::Type(_) => {}
                InstanceDecl::Alias(_) => {}
                InstanceDecl::ExportDecl(_) => {}
            },
        }
    }

    Ok((ctx.reader.read_count() - start_count, component_type))
}

fn parse_component_decl(
    ctx: &mut ParseContext<
        impl BinaryReader,
        impl ValidatorStateImpl
            + IdxValidator<InstanceIdx, Resolved = Instance>
            + IdxValidator<TypeIdx, Resolved = Type>
            + Resolver<Type, Error = ComponentParseError>
            + Resolver<Instance, Error = ComponentParseError>
            + IdxValidator<FuncIdx, Resolved = ComponentFunction>
            + IdxValidator<CoreModuleIdx, Resolved = CoreModule>
            + IdxValidator<ComponentIdx, Resolved = InlineComponent>
            + Resolver<ComponentFunction, Error = ComponentParseError>
            + IdxValidator<CoreTypeIdx, Resolved = CoreType>
            + Resolver<CoreType, Error = ComponentParseError>
            + TypeSuperValidator,
    >,
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
