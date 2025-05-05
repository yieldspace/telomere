use crate::binary::BinaryReader;
use crate::component_model::{ComponentImport, ExternDesc, GlobalIdx, Relation};
use crate::parser::component_model::{
    parse_externdesc, ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::parse_name;

pub fn parse_import(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> ParseResult<(String, ComponentImport)> {
    let (_, name) = parse_import_name_dash(ctx)?;
    let ed = parse_externdesc(ctx)?;
    let import = match ed {
        ExternDesc::CoreModule(ty) => {
            let idx = ctx.validator.add_core_module_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_core_module(global_idx, Relation::Import(name.clone()));
            ctx.validator.register_global_core_module(idx, global_idx)?;
            // let global_idx = ctx.validator.get_global_core_module(idx)?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_import_core_module,
            // });
            ComponentImport::CoreModule(ty, global_idx)
        }
        ExternDesc::Func(ty) => {
            let idx = ctx.validator.add_func_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_func(global_idx, Relation::Import(name.clone()));
            ctx.validator.register_global_func(idx, global_idx)?;
            ComponentImport::Func(ty, global_idx)
        }
        #[cfg(feature = "component-gated-feature-value-imports-exports")]
        ExternDesc::Value(_) => todo!(),
        ExternDesc::Type(ty) => {
            // let idx = match bound {
            //     TypeBound::Eq(idx) => ctx.validator.add_type(Binding::Real(Type::Referenced(
            //         Box::new(Type::Eq(idx)),
            //         Reference::Imported(name.clone()),
            //     )))?,
            //     TypeBound::Sub => ctx.validator.add_type(Binding::Real(Type::Referenced(
            //         Box::new(Type::UniqueResource),
            //         Reference::Imported(name.clone()),
            //     )))?,
            // };
            ctx.validator.add_type(ty.clone())?;
            ComponentImport::Type(ty)
        }
        ExternDesc::Component(ty) => {
            let idx = ctx.validator.add_component_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_component(global_idx, Relation::Import(name.clone()));
            ctx.validator.register_global_component(idx, global_idx)?;
            ComponentImport::Component(ty, global_idx)
        }
        ExternDesc::Instance(ty) => {
            let idx = ctx.validator.add_instance_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_instance(global_idx, Relation::Import(name.clone()));
            ctx.validator.register_global_instance(idx, global_idx)?;
            ComponentImport::Instance(ty, global_idx)
        }
    };
    Ok((name, import))
}

pub fn parse_import_name_dash(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<String> {
    ComponentParseError::assert_magic([ctx.reader.read_exact_one()?], [0x00], "import name")?;
    // todo: check name
    let (len, name) = parse_name(ctx.reader)?;
    Ok((len + 1, name))
}
