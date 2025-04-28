use crate::binary::BinaryReader;
use crate::component_model::{ComponentImport, ExternDesc, GlobalIdx, Relation};
use crate::parser::component_model::{
    parse_externdesc, ComponentParseError, ParseContext, ParseResult, SizedResult,
};
use crate::parser::core::parse_name;

// fn parse_externdesc_import(
//     ctx: &mut ParseContext<impl BinaryReader>,
// ) -> SizedResult<ExternDesc> {
//     let start_count = ctx.reader.read_count();
//     let start_count = ctx.reader.read_count();
//     let desc = match ctx.reader.read_exact_one()? {
//         0x00 => {
//             ComponentParseError::assert_magic(
//                 [ctx.reader.read_exact_one()?],
//                 [0x00],
//                 "extern desc",
//             )?;
//             let idx = parse_core_type_idx(ctx)?;
//             let ty = ctx.validator.get_core_type(idx)?;
//             ExternDesc::CoreModule(ty.try_into()?)
//         }
//         0x01 => {
//             let idx = parse_type_idx(ctx)?;
//             let ty = ctx.validator.get_type(idx)?;
//             ExternDesc::Func(ty.try_into()?)
//         }
//         #[cfg(feature = "component-gated-feature-value-imports-exports")]
//         0x02 => {
//             let (_, b) = crate::parser::component_model::types::parse_valuebound(ctx)?;
//             ExternDesc::Value(b)
//         }
//         0x03 => {
//             // let (_, b) = crate::parser::component_model::types::parse_typebound(ctx)?;
//             // ExternDesc::Type(b)
//             todo!()
//         }
//         0x04 => {
//             let idx = parse_type_idx(ctx)?;
//             let ty = ctx.validator.get_type(idx)?;
//             ExternDesc::Component(ty.try_into()?)
//         }
//         0x05 => {
//             let idx = parse_type_idx(ctx)?;
//             let ty = ctx.validator.get_type(idx)?;
//             ExternDesc::Instance(ty.try_into()?)
//         }
//         _ => todo!(),
//     };
//     Ok((ctx.reader.read_count() - start_count, desc))
// }

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
                .register_core_module(global_idx.clone(), Relation::Import(name.clone()));
            ctx.validator.register_global_core_module(idx, global_idx)?;
            // let global_idx = ctx.validator.get_global_core_module(idx)?;
            // ctx.push_instr(InstantiateInstr {
            //     op: instantiate_import_core_module,
            // });
            ComponentImport::CoreModule(global_idx)
        }
        ExternDesc::Func(ty) => {
            let idx = ctx.validator.add_func_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_func(global_idx.clone(), Relation::Import(name.clone()));
            ctx.validator.register_global_func(idx, global_idx)?;
            ComponentImport::Func(global_idx)
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
                .register_component(global_idx.clone(), Relation::Import(name.clone()));
            ctx.validator.register_global_component(idx, global_idx)?;
            ComponentImport::Component(global_idx)
        }
        ExternDesc::Instance(ty) => {
            let idx = ctx.validator.add_instance_type(ty.clone())?;
            let global_idx = GlobalIdx::new();
            ctx.state
                .register_instance(global_idx.clone(), Relation::Import(name.clone()));
            ctx.validator.register_global_instance(idx, global_idx)?;
            ComponentImport::Instance(global_idx)
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
