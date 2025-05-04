use crate::binary::BinaryReader;
use crate::component_model::section::ComponentSectionType;
use crate::component_model::{
    ComponentExport, ComponentImport, ComponentType, CoreModule, CoreModuleType, ExternDesc,
    GlobalIdx, ImportName, InlineComponent, Relation,
};
use crate::parser::component_model::canon::parse_canon;
use crate::parser::component_model::context::ParseContext;
use crate::parser::component_model::core::parse_core_instance;
use crate::parser::component_model::error::ComponentParseError;
use crate::parser::component_model::export::parse_export;
use crate::parser::component_model::import::parse_import;
use crate::parser::component_model::instance::parse_instance;
use crate::parser::component_model::types::parse_type;
use crate::parser::component_model::{
    parse_alias, parse_layer, parse_magic, parse_section_type, parse_vec_range, parse_version,
    Validator,
};
use crate::parser::core::{parse_u32, parse_vec};
use crate::runtime::component_model::instantiate::{instantiate_special_end, InstantiateInstr};
use crate::WasmParser;
use std::collections::HashMap;

pub fn parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    _parse_component(ctx)?;
    ctx.push_instr(InstantiateInstr {
        op: instantiate_special_end,
    });
    Ok(())
}

pub fn _parse_component(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    parse_magic(ctx.reader)?;
    parse_version(ctx.reader)?;
    parse_layer(ctx.reader)?;

    while let Some(st) = parse_section_type(ctx.reader)? {
        let (_, section_size) = parse_u32(ctx.reader)?;
        match st {
            ComponentSectionType::Custom => {
                parse_custom_section(ctx.reader, section_size as usize)?
            }
            ComponentSectionType::CoreModule => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let mut core_module = WasmParser::new(&mut sized_reader);
                let module = core_module.parse_module()?;
                let ty = CoreModuleType::from_module(&module);
                let idx = ctx.validator.add_core_module_type(ty)?;
                let global_idx = GlobalIdx::new();
                ctx.state
                    .register_core_module(global_idx, Relation::Defined(CoreModule::new(module)));
                ctx.validator.register_global_core_module(idx, global_idx)?
            }
            ComponentSectionType::CoreInstance => parse_core_instance_section(ctx)?,
            ComponentSectionType::CoreType => todo!(),
            ComponentSectionType::Component => {
                let mut sized_reader = ctx.reader.take(section_size as usize);
                let validator = Validator::new_child(&mut ctx.validator);
                let mut instrs = Vec::new();
                let state = &mut ctx.state;
                // 呼び出し結果を一旦保持し、`child_ctx` をスコープ外に出してから `?` を適用することで
                // `validator` への可変参照を早期に解放し、後続の不変借用と競合しないようにする。
                let validator = {
                    let mut ctx =
                        ParseContext::new(&mut sized_reader, &mut instrs, validator, state);
                    _parse_component(&mut ctx)?;
                    ctx.validator
                };

                let (import_types, imports): (
                    Vec<(ImportName, ExternDesc)>,
                    Vec<(ImportName, ComponentImport)>,
                ) = validator
                    .get_imports()
                    .into_iter()
                    .map(|(name, import)| match &import {
                        ComponentImport::CoreModule(ty, _) => (
                            (name.clone(), ExternDesc::CoreModule(ty.clone())),
                            (name, import),
                        ),
                        ComponentImport::Func(ty, _) => {
                            ((name.clone(), ExternDesc::Func(ty.clone())), (name, import))
                        }
                        ComponentImport::Type(ty) => {
                            ((name.clone(), ExternDesc::Type(ty.clone())), (name, import))
                        }
                        ComponentImport::Component(ty, _) => (
                            (name.clone(), ExternDesc::Component(ty.clone())),
                            (name, import),
                        ),
                        ComponentImport::Instance(ty, _) => (
                            (name.clone(), ExternDesc::Instance(ty.clone())),
                            (name, import),
                        ),
                    })
                    .unzip();
                let exports = validator.get_exports();
                let export_types = exports
                    .iter()
                    .map(|(name, export)| match export {
                        ComponentExport::CoreModule(ty, _) => {
                            (name.clone(), ExternDesc::CoreModule(ty.clone()))
                        }
                        ComponentExport::Func(ty, _) => {
                            (name.clone(), ExternDesc::Func(ty.clone()))
                        }
                        ComponentExport::Type(ty) => (name.clone(), ExternDesc::Type(ty.clone())),
                        ComponentExport::Component(ty, _) => {
                            (name.clone(), ExternDesc::Component(ty.clone()))
                        }
                        ComponentExport::Instance(ty, _) => {
                            (name.clone(), ExternDesc::Instance(ty.clone()))
                        }
                    })
                    .collect::<HashMap<_, _>>();
                let mut ty = ComponentType::new();
                ty.imports = HashMap::from_iter(import_types);
                ty.exports = export_types;
                let idx = ctx.validator.add_component_type(ty)?;
                let component = InlineComponent {
                    instrs,
                    imports: HashMap::from_iter(imports),
                    exports,
                };
                let global_idx = GlobalIdx::new();
                ctx.state
                    .register_component(global_idx, Relation::Defined(component));
                ctx.validator.register_global_component(idx, global_idx)?;
            }
            ComponentSectionType::Instance => parse_instance_section(ctx)?,
            ComponentSectionType::Alias => parse_alias_section(ctx)?,
            ComponentSectionType::Type => parse_type_section(ctx)?,
            ComponentSectionType::Canon => parse_canon_section(ctx)?,
            ComponentSectionType::Start => todo!(),
            ComponentSectionType::Import => parse_import_section(ctx)?,
            ComponentSectionType::Export => parse_export_section(ctx)?,
            #[cfg(feature = "component-gated-feature-value-imports-exports")]
            ComponentSectionType::Value => todo!(),
        }
    }
    Ok(())
}

#[inline]
fn parse_custom_section<R: BinaryReader>(
    reader: &mut R,
    size: usize,
) -> Result<(), ComponentParseError> {
    // Custom section parsing logic
    for _ in 0..size {
        reader.read_exact_one()?;
    }
    Ok(())
}

#[inline]
fn parse_core_instance_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    for _ in parse_vec_range(ctx)? {
        let (_, (inst, ty)) = parse_core_instance(ctx)?;
        let idx = ctx.validator.add_core_instance_type(ty)?;
        let global_idx = GlobalIdx::new();
        ctx.state
            .register_core_instance(global_idx, Relation::Defined(inst));
        ctx.validator
            .register_global_core_instance(idx, global_idx)?;
    }
    Ok(())
}

#[inline]
fn parse_instance_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Core instance parsing logic
    parse_vec(ctx, |v| v.reader, parse_instance)?;
    Ok(())
}

#[inline]
fn parse_alias_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Alias parsing logic
    parse_vec(ctx, |v| v.reader, parse_alias)?;
    Ok(())
}

#[inline]
fn parse_type_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Type parsing logic
    for _ in parse_vec_range(ctx)? {
        let (_, ty) = parse_type(ctx)?;
        ctx.validator.add_type(ty)?;
    }
    Ok(())
}

#[inline]
fn parse_canon_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Canon parsing logic
    parse_vec(ctx, |v| v.reader, parse_canon)?;
    Ok(())
}

#[inline]
fn parse_import_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Import parsing logic
    for _ in parse_vec_range(ctx)? {
        let (name, import) = parse_import(ctx)?;
        ctx.validator.add_import(name, import);
    }
    Ok(())
}

#[inline]
fn parse_export_section(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> Result<(), ComponentParseError> {
    // Export parsing logic
    for _ in parse_vec_range(ctx)? {
        let (name, export) = parse_export(ctx)?;
        match export.clone() {
            ComponentExport::CoreModule(ty, idx) => {
                let local = ctx.validator.add_core_module_type(ty)?;
                ctx.validator.register_global_core_module(local, idx)?;
            }
            ComponentExport::Func(ty, idx) => {
                let local = ctx.validator.add_func_type(ty)?;
                ctx.validator.register_global_func(local, idx)?;
            }
            ComponentExport::Type(ty) => {
                ctx.validator.add_type(ty)?;
            }
            ComponentExport::Component(ty, idx) => {
                let local = ctx.validator.add_component_type(ty)?;
                ctx.validator.register_global_component(local, idx)?;
            }
            ComponentExport::Instance(ty, idx) => {
                let local = ctx.validator.add_instance_type(ty)?;
                ctx.validator.register_global_instance(local, idx)?;
            }
        }
        ctx.validator.add_export(name, export);
    }
    Ok(())
}
