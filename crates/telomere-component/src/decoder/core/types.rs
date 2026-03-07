use crate::decoder::{ComponentParseError, ParseContext, SizedResult};
use crate::ir::types::{
    CoreFuncType, CoreModuleExportType, CoreModuleImportType, CoreModuleType, CoreType,
};
use crate::ir::LocalIdx;
use crate::support::binary::BinaryReader;
use crate::support::common::{
    FuncType as WasmFuncType, GlobalType as WasmGlobalType, Limits, MemType as WasmMemType, Mut,
    RefType, ResultType, TableType as WasmTableType, ValType as WasmValType,
};
use crate::support::parser::core::{parse_name, parse_u32, parse_vec};
use std::collections::HashMap;

enum LocalCoreType {
    Func(CoreFuncType),
    Module,
}

fn parse_wasm_valtype(reader: &mut impl BinaryReader) -> SizedResult<WasmValType> {
    let ty = match reader.read_exact_one()? {
        0x7f => WasmValType::I32,
        0x7e => WasmValType::I64,
        0x7d => WasmValType::F32,
        0x7c => WasmValType::F64,
        0x7b => WasmValType::V128,
        0x70 => WasmValType::FuncRef,
        0x6f => WasmValType::ExternRef,
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid core valtype: {x}"
            )));
        }
    };
    Ok((1, ty))
}

fn parse_wasm_result_type(reader: &mut impl BinaryReader) -> SizedResult<ResultType> {
    let (len, types) = parse_vec(reader, |v| v, parse_wasm_valtype)?;
    Ok((len, ResultType(types)))
}

fn parse_wasm_functype(reader: &mut impl BinaryReader) -> SizedResult<WasmFuncType> {
    let start = reader.read_count();
    let signature = reader.read_exact_one()?;
    if signature != 0x60 {
        return Err(ComponentParseError::InvalidSignature(format!(
            "invalid core functype signature: {signature}"
        )));
    }
    parse_wasm_functype_body(reader, start)
}

fn parse_wasm_functype_body(
    reader: &mut impl BinaryReader,
    start: usize,
) -> SizedResult<WasmFuncType> {
    let (_, params) = parse_wasm_result_type(reader)?;
    let (_, results) = parse_wasm_result_type(reader)?;
    Ok((reader.read_count() - start, WasmFuncType(params, results)))
}

fn parse_wasm_reftype(reader: &mut impl BinaryReader) -> SizedResult<RefType> {
    let ty = match reader.read_exact_one()? {
        0x70 => RefType::FuncRef,
        0x6f => RefType::ExternRef,
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid core reftype: {x}"
            )));
        }
    };
    Ok((1, ty))
}

fn parse_wasm_limits(reader: &mut impl BinaryReader) -> SizedResult<Limits> {
    let start = reader.read_count();
    let limits = match reader.read_exact_one()? {
        0x00 => {
            let (_, min) = parse_u32(reader)?;
            Limits { min, max: None }
        }
        0x01 => {
            let (_, min) = parse_u32(reader)?;
            let (_, max) = parse_u32(reader)?;
            if min > max {
                return Err(ComponentParseError::InvalidType(
                    "invalid core limits".to_owned(),
                ));
            }
            Limits {
                min,
                max: Some(max),
            }
        }
        0x02 => {
            let (_, min) = parse_u32(reader)?;
            Limits { min, max: None }
        }
        0x03 => {
            let (_, min) = parse_u32(reader)?;
            let (_, max) = parse_u32(reader)?;
            if min > max {
                return Err(ComponentParseError::InvalidType(
                    "invalid core limits".to_owned(),
                ));
            }
            Limits {
                min,
                max: Some(max),
            }
        }
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid core limits tag: {x}"
            )));
        }
    };
    Ok((reader.read_count() - start, limits))
}

fn parse_wasm_table_type(reader: &mut impl BinaryReader) -> SizedResult<WasmTableType> {
    let start = reader.read_count();
    let (_, reftype) = parse_wasm_reftype(reader)?;
    let (_, limits) = parse_wasm_limits(reader)?;
    Ok((
        reader.read_count() - start,
        WasmTableType { reftype, limits },
    ))
}

fn parse_wasm_global_type(reader: &mut impl BinaryReader) -> SizedResult<WasmGlobalType> {
    let start = reader.read_count();
    let (_, valtype) = parse_wasm_valtype(reader)?;
    let mutable = match reader.read_exact_one()? {
        0x00 => Mut::Const,
        0x01 => Mut::Var,
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid core mutability: {x}"
            )));
        }
    };
    Ok((
        reader.read_count() - start,
        WasmGlobalType(valtype, mutable),
    ))
}

fn parse_wasm_memtype(reader: &mut impl BinaryReader) -> SizedResult<WasmMemType> {
    let start = reader.read_count();
    let (_, limits) = parse_wasm_limits(reader)?;
    const MAX_WASM32_PAGES: u32 = 65536;
    if limits.min > MAX_WASM32_PAGES || limits.max.is_some_and(|max| max > MAX_WASM32_PAGES) {
        return Err(ComponentParseError::InvalidType(
            "memory size must be at most 65536 pages (4GiB)".to_owned(),
        ));
    }
    Ok((reader.read_count() - start, WasmMemType(limits)))
}

fn resolve_core_func_type(local_types: &[LocalCoreType], idx: u32) -> SizedResult<CoreFuncType> {
    let ty = local_types
        .get(idx as usize)
        .ok_or(ComponentParseError::TypeIdxNotFound(idx))?;
    match ty {
        LocalCoreType::Func(func_ty) => Ok((0, func_ty.clone())),
        LocalCoreType::Module => Err(ComponentParseError::InvalidType(
            "core typeidx must refer to func type".to_owned(),
        )),
    }
}

fn parse_core_import_desc(
    ctx: &mut ParseContext<impl BinaryReader>,
    local_types: &[LocalCoreType],
) -> SizedResult<CoreModuleImportType> {
    let start = ctx.reader.read_count();
    let ty = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, idx) = parse_u32(ctx.reader)?;
            CoreModuleImportType::Func(resolve_core_func_type(local_types, idx)?.1)
        }
        0x01 => CoreModuleImportType::Table(parse_wasm_table_type(ctx.reader)?.1),
        0x02 => CoreModuleImportType::Memory(parse_wasm_memtype(ctx.reader)?.1),
        0x03 => CoreModuleImportType::Global(parse_wasm_global_type(ctx.reader)?.1),
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid core import desc: {x}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start, ty))
}

fn parse_core_export_desc(
    ctx: &mut ParseContext<impl BinaryReader>,
    local_types: &[LocalCoreType],
) -> SizedResult<CoreModuleExportType> {
    let start = ctx.reader.read_count();
    let ty = match ctx.reader.read_exact_one()? {
        0x00 => {
            let (_, idx) = parse_u32(ctx.reader)?;
            CoreModuleExportType::Func(resolve_core_func_type(local_types, idx)?.1)
        }
        0x01 => CoreModuleExportType::Table(parse_wasm_table_type(ctx.reader)?.1),
        0x02 => CoreModuleExportType::Memory(parse_wasm_memtype(ctx.reader)?.1),
        0x03 => CoreModuleExportType::Global(parse_wasm_global_type(ctx.reader)?.1),
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid core export desc: {x}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start, ty))
}

fn parse_core_module_alias(
    ctx: &mut ParseContext<impl BinaryReader>,
    local_types: &mut Vec<LocalCoreType>,
) -> SizedResult<()> {
    let start = ctx.reader.read_count();
    let sort = ctx.reader.read_exact_one()?;
    if sort != 0x10 {
        return Err(ComponentParseError::Unsupported(format!(
            "unsupported core module alias sort: {sort}"
        )));
    }
    match ctx.reader.read_exact_one()? {
        0x01 => {
            let (_, count) = parse_u32(ctx.reader)?;
            let (_, index) = parse_u32(ctx.reader)?;
            let ty = ctx
                .validator
                .outer_type_scope(count)?
                .core_types
                .get(LocalIdx::new(index))?
                .clone();
            match ty {
                CoreType::Func(func_ty) => local_types.push(LocalCoreType::Func(func_ty)),
                CoreType::Module(_) => local_types.push(LocalCoreType::Module),
            }
        }
        x => {
            return Err(ComponentParseError::InvalidSignature(format!(
                "invalid core module alias kind: {x}"
            )));
        }
    }
    Ok((ctx.reader.read_count() - start, ()))
}

fn parse_core_module_type(
    ctx: &mut ParseContext<impl BinaryReader>,
) -> SizedResult<CoreModuleType> {
    let start = ctx.reader.read_count();
    let (_, len) = parse_u32(ctx.reader)?;
    let mut local_types = Vec::new();
    let mut imports = HashMap::<String, HashMap<String, CoreModuleImportType>>::new();
    let mut exports = HashMap::<String, CoreModuleExportType>::new();

    for _ in 0..len {
        match ctx.reader.read_exact_one()? {
            0x00 => {
                let (_, module) = parse_name(ctx.reader)?;
                let (_, name) = parse_name(ctx.reader)?;
                let (_, desc) = parse_core_import_desc(ctx, &local_types)?;
                if imports
                    .entry(module)
                    .or_default()
                    .insert(name, desc)
                    .is_some()
                {
                    return Err(ComponentParseError::TypeMismatch(
                        "duplicate import name".to_owned(),
                    ));
                }
            }
            0x01 => {
                let (_, ty) = parse_wasm_functype(ctx.reader)?;
                local_types.push(LocalCoreType::Func(ty));
            }
            0x02 => {
                parse_core_module_alias(ctx, &mut local_types)?;
            }
            0x03 => {
                let (_, name) = parse_name(ctx.reader)?;
                let (_, desc) = parse_core_export_desc(ctx, &local_types)?;
                if exports.insert(name.clone(), desc).is_some() {
                    return Err(ComponentParseError::TypeMismatch(format!(
                        "export name `{name}` already defined"
                    )));
                }
            }
            x => {
                return Err(ComponentParseError::Unsupported(format!(
                    "unsupported core module type decl: {x}"
                )));
            }
        }
    }

    Ok((
        ctx.reader.read_count() - start,
        CoreModuleType { imports, exports },
    ))
}

pub fn parse_core_type(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<CoreType> {
    let start = ctx.reader.read_count();
    let ty = match ctx.reader.read_exact_one()? {
        0x60 => CoreType::Func(parse_wasm_functype_body(ctx.reader, start)?.1),
        0x50 => CoreType::Module(parse_core_module_type(ctx)?.1),
        x => {
            return Err(ComponentParseError::Unsupported(format!(
                "unsupported core type opcode: {x}"
            )));
        }
    };
    Ok((ctx.reader.read_count() - start, ty))
}
