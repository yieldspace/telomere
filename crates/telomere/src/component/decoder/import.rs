use crate::binary::BinaryReader;
use crate::component::decoder::name::parse_import_name_dash;
use crate::component::decoder::types::parse_externdesc;
use crate::component::decoder::types::validate_annotated_import;
use crate::component::decoder::{ParseContext, ParseResult};
use crate::component::ir::types::{ComponentImportType, Generic, GenericBound, Type};
use crate::component::ir::CoreRelation;
use crate::component::ir::{
    ComponentImport, ExternDesc, ParsedImportName, PlainName, Relation, StrongUnique,
};

use super::ComponentParseError;

pub fn parse_import(ctx: &mut ParseContext<impl BinaryReader>) -> ParseResult<()> {
    let name = parse_import_name_dash(ctx)?;
    let focus = ctx.validator.scope_mut();
    for existing in &focus.import_names {
        if existing.weak_eq(&name) {
            Err(ComponentParseError::InvalidImportName(format!(
                "import name `{}` conflicts with previous name `{}`",
                name.original, existing.original
            )))?;
        }
    }
    focus.import_names.push(name.clone());
    let desc = parse_externdesc(ctx)?;
    validate_annotated_import(ctx, &name, &desc)?;
    if matches!(desc, ExternDesc::Instance(_)) {
        ensure_concrete_surface_name(&name)?;
    }
    match desc {
        ExternDesc::Module(module_ty) => {
            let global_idx = ctx
                .state
                .core_module_store
                .register(CoreRelation::ImportModule(name.original.clone()));
            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Module);
            focus.core_modules.register(global_idx);

            let focus = ctx.validator.scope_mut();
            focus.imports.insert(
                name.original,
                ComponentImportType::CoreModule(module_ty.clone()),
            );
            focus.core_modules.add(module_ty);
        }
        ExternDesc::Component(type_id) => {
            let global_idx = ctx
                .state
                .component_store
                .register(Relation::Import(name.original.clone()));
            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Component);
            focus.components.register(global_idx);

            let focus = ctx.validator.scope_mut();
            focus.component_indexes.add(type_id);
            focus.imports.insert(
                name.original,
                ComponentImportType::Type {
                    type_id,
                    generic: Generic::new(GenericBound::Eq(type_id)),
                },
            );
        }
        ExternDesc::Instance(type_id) => {
            let type_id = ctx.validator.freshen_import_type_id(type_id)?;
            let ty = ctx.validator.get_type(type_id);
            tracing::trace!("ExternDesc::Instance: {:?}", ty);
            let global = ctx
                .state
                .instance_store
                .register(Relation::Import(name.original.clone()));
            let focus = ctx.state.scope_mut();

            focus.add_import(&name, ComponentImport::Instance);
            focus.instances.register(global);

            let focus = ctx.validator.scope_mut();

            focus.imports.insert(
                name.original,
                ComponentImportType::Type {
                    type_id,
                    generic: Generic::new(GenericBound::Eq(type_id)),
                },
            );
            focus.instance_indexes.add(type_id);
        }
        ExternDesc::Eq(type_id) => {
            let focus = ctx.validator.scope_mut();

            focus.imports.insert(
                name.original,
                ComponentImportType::Type {
                    type_id,
                    generic: Generic::new(GenericBound::Eq(type_id)),
                },
            );
            focus.type_indexes.add(type_id);
        }
        ExternDesc::Sub => {
            let generic = Generic::new(GenericBound::Sub);
            let type_id = ctx.validator.new_type(Type::Generic(generic.clone()));
            let focus = ctx.validator.scope_mut();
            focus.imports.insert(
                name.original,
                ComponentImportType::Type { type_id, generic },
            );
            focus.type_indexes.add(type_id);
        }
        ExternDesc::Func(type_id) => {
            let global_idx = ctx
                .state
                .func_store
                .register(Relation::Import(name.original.clone()));

            let focus = ctx.state.scope_mut();
            focus.add_import(&name, ComponentImport::Func);
            focus.funcs.register(global_idx);

            let focus = ctx.validator.scope_mut();
            focus.func_indexes.add(type_id);
            focus.imports.insert(
                name.original,
                ComponentImportType::Type {
                    type_id,
                    generic: Generic::new(GenericBound::Eq(type_id)),
                },
            );
        }
        ExternDesc::Value(_) => Err(ComponentParseError::Unsupported(
            "value imports are not supported".to_owned(),
        ))?,
    }
    Ok(())
}

fn ensure_concrete_surface_name(name: &crate::component::ir::ImportName) -> ParseResult<()> {
    let ParsedImportName::Plain(plain) = &name.parsed else {
        return Ok(());
    };
    for label in plain_labels(plain) {
        if !crate::component::decoder::name::is_kebab_label(label) {
            return Err(ComponentParseError::InvalidImportName(format!(
                "`{}` is not in kebab case",
                name.original
            )));
        }
    }
    Ok(())
}

fn plain_labels(plain: &PlainName) -> Vec<&str> {
    match plain {
        PlainName::Plain(label) | PlainName::Constructor(label) => vec![label.0.as_str()],
        PlainName::Method(resource, method) | PlainName::Static(resource, method) => {
            vec![resource.0.as_str(), method.0.as_str()]
        }
    }
}
