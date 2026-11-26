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
    Ok((reader.read_count() - start, WasmMemType::new(limits, false)))
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
        // `core:alias` fixes the sort byte to 0x10 (type); any other
        // byte is an invalid leading byte for the outer alias kind.
        return Err(ComponentParseError::InvalidLeadingByte(
            sort,
            "outer alias kind",
        ));
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
            // `core:alias` fixes the target byte to 0x01 (outer); any
            // other byte is an invalid leading byte for the target.
            return Err(ComponentParseError::InvalidLeadingByte(
                x,
                "outer alias target",
            ));
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
                // A module type declarator contains a `core:rectype`
                // in the plain Core WebAssembly encoding: a bare 0x50
                // is a non-final `sub` here and a `core:moduletype`
                // cannot be nested (design/mvp/Binary.md).
                let base_len = local_types.len();
                let resolve_existing = |idx: u32| {
                    local_types.get(idx as usize).map(|ty| SuperTypeInfo {
                        is_func: matches!(ty, LocalCoreType::Func(_)),
                        is_final: None,
                    })
                };
                let leading = ctx.reader.read_exact_one()?;
                let (_, defined) =
                    parse_core_rectype(ctx.reader, leading, base_len, &resolve_existing)?;
                for defined in defined {
                    local_types.push(LocalCoreType::Func(defined.func));
                }
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

/// A core subtype decoded from a `core:rectype`: the func type it
/// defines and whether the definition is final.
///
/// `CoreType` does not record finality or supertypes — the component
/// decoder's core type model is just the defined types themselves —
/// but finality is still needed while a rectype is being decoded: a
/// supertype index that names a type defined earlier by the same
/// rectype must name a non-final one.
struct DefinedCoreSubtype {
    func: CoreFuncType,
    is_final: bool,
}

/// What a supertype index in a `core:rectype` must resolve to.
struct SuperTypeInfo {
    /// Whether the referenced type is a func type. Func is the only
    /// comptype kind this decoder models, so a supertype of any other
    /// kind cannot be the supertype of a subtype decoded here.
    is_func: bool,
    /// Whether the referenced type is final, when known. Types defined
    /// by earlier definitions do not record finality in `CoreType`, so
    /// they report `None` and only the kind check applies to them.
    is_final: Option<bool>,
}

/// Decodes a `core:comptype` whose leading byte has been read. Only
/// the func comptype is modelled by this decoder; struct and array
/// comptypes are rejected as unsupported, as they are in every other
/// core type position here.
fn parse_core_comptype(reader: &mut impl BinaryReader, leading: u8) -> SizedResult<CoreFuncType> {
    let start = reader.read_count() - 1;
    let func = match leading {
        0x60 => parse_wasm_functype_body(reader, start)?.1,
        0x5e | 0x5f => {
            return Err(ComponentParseError::Unsupported(format!(
                "unsupported core comptype opcode: {leading}"
            )));
        }
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid leading byte (0x{x:x}) for core comptype"
            )));
        }
    };
    Ok((reader.read_count() - start, func))
}

/// Decodes the body of a `core:subtype` after a `sub` (0x50) or
/// `sub final` (0x4f) prefix: the supertype index vector and the
/// comptype. Pushes the defined type onto `defined`.
///
/// Supertype indices below `base_len` name types that were already in
/// the index space when the enclosing rectype started and are resolved
/// through `resolve_existing`; indices at or above it name types
/// defined earlier by the same rectype.
fn parse_core_subtype_body(
    reader: &mut impl BinaryReader,
    is_final: bool,
    base_len: usize,
    defined: &mut Vec<DefinedCoreSubtype>,
    resolve_existing: &dyn Fn(u32) -> Option<SuperTypeInfo>,
) -> SizedResult<()> {
    let start = reader.read_count();
    let (_, supertypes) = parse_vec(reader, |v| v, parse_u32)?;
    for idx in supertypes {
        let info = if (idx as usize) < base_len {
            resolve_existing(idx)
        } else {
            defined
                .get(idx as usize - base_len)
                .map(|defined| SuperTypeInfo {
                    is_func: true,
                    is_final: Some(defined.is_final),
                })
        }
        .ok_or(ComponentParseError::TypeIdxNotFound(idx))?;
        if !info.is_func {
            return Err(ComponentParseError::InvalidType(format!(
                "core subtype supertype {idx} is not a func type"
            )));
        }
        if info.is_final == Some(true) {
            return Err(ComponentParseError::InvalidType(format!(
                "core subtype supertype {idx} is final"
            )));
        }
    }
    let leading = reader.read_exact_one()?;
    let (_, func) = parse_core_comptype(reader, leading)?;
    defined.push(DefinedCoreSubtype { func, is_final });
    Ok((reader.read_count() - start, ()))
}

/// Decodes one `core:subtype` whose leading byte has been read,
/// pushing the defined type onto `defined`.
fn parse_core_subtype(
    reader: &mut impl BinaryReader,
    leading: u8,
    base_len: usize,
    defined: &mut Vec<DefinedCoreSubtype>,
    resolve_existing: &dyn Fn(u32) -> Option<SuperTypeInfo>,
) -> SizedResult<()> {
    let start = reader.read_count();
    match leading {
        0x50 => {
            parse_core_subtype_body(reader, false, base_len, defined, resolve_existing)?;
        }
        0x4f => {
            parse_core_subtype_body(reader, true, base_len, defined, resolve_existing)?;
        }
        0x5e..=0x60 => {
            // A bare comptype is a final subtype with no supertypes.
            let (_, func) = parse_core_comptype(reader, leading)?;
            defined.push(DefinedCoreSubtype {
                func,
                is_final: true,
            });
        }
        x => {
            return Err(ComponentParseError::InvalidType(format!(
                "invalid leading byte (0x{x:x}) for core subtype"
            )));
        }
    }
    Ok((reader.read_count() - start, ()))
}

/// Decodes a `core:rectype` in the Core WebAssembly encoding — a
/// `rec` group (0x4e) or a single subtype — whose leading byte has
/// been read. Returns the types it defines, in index order.
fn parse_core_rectype(
    reader: &mut impl BinaryReader,
    leading: u8,
    base_len: usize,
    resolve_existing: &dyn Fn(u32) -> Option<SuperTypeInfo>,
) -> SizedResult<Vec<DefinedCoreSubtype>> {
    let start = reader.read_count();
    let mut defined = Vec::new();
    match leading {
        0x4e => {
            let (_, count) = parse_u32(reader)?;
            for _ in 0..count {
                let leading = reader.read_exact_one()?;
                parse_core_subtype(reader, leading, base_len, &mut defined, resolve_existing)?;
            }
        }
        leading => {
            parse_core_subtype(reader, leading, base_len, &mut defined, resolve_existing)?;
        }
    }
    Ok((reader.read_count() - start, defined))
}

/// Decodes a `core:deftype` (a core type section entry, or the core
/// typedef inside a component/instance type). Returns every core type
/// the definition introduces into the core type index space, in index
/// order: a `core:rectype` can define several types at once (a `rec`
/// group), the other forms define exactly one.
pub fn parse_core_type(ctx: &mut ParseContext<impl BinaryReader>) -> SizedResult<Vec<CoreType>> {
    let start = ctx.reader.read_count();
    let types = match ctx.reader.read_exact_one()? {
        0x50 => vec![CoreType::Module(parse_core_module_type(ctx)?.1)],
        0x00 => {
            // `core:deftype` shares the 0x50 opcode between
            // `core:moduletype` and a non-final `core:subtype`. The
            // pre-1.0 resolution (design/mvp/Binary.md, "Type
            // Definitions") is that a non-final subtype in this
            // position is prefixed by 0x00, while a bare 0x50 is the
            // moduletype above.
            let prefix = ctx.reader.read_exact_one()?;
            if prefix != 0x50 {
                return Err(ComponentParseError::InvalidType(format!(
                    "invalid leading byte (0x{prefix:x}) for core subtype: \
                     a non-final subtype in a core type definition is \
                     encoded as 0x00 0x50"
                )));
            }
            let validator = &*ctx.validator;
            let resolve_existing = |idx: u32| {
                validator
                    .scope()
                    .core_types
                    .get(LocalIdx::new(idx))
                    .ok()
                    .map(|ty| SuperTypeInfo {
                        is_func: matches!(ty, CoreType::Func(_)),
                        is_final: None,
                    })
            };
            let mut defined = Vec::new();
            parse_core_subtype_body(
                ctx.reader,
                false,
                validator.scope().core_types.len(),
                &mut defined,
                &resolve_existing,
            )?;
            defined
                .into_iter()
                .map(|defined| CoreType::Func(defined.func))
                .collect()
        }
        leading => {
            // The remaining `core:deftype` form is a `core:rectype`
            // directly: a `rec` group, a final subtype, or a bare
            // comptype. A non-final subtype cannot start here — its
            // 0x50 is the moduletype case above, and the prefixed form
            // is the 0x00 case.
            let validator = &*ctx.validator;
            let resolve_existing = |idx: u32| {
                validator
                    .scope()
                    .core_types
                    .get(LocalIdx::new(idx))
                    .ok()
                    .map(|ty| SuperTypeInfo {
                        is_func: matches!(ty, CoreType::Func(_)),
                        is_final: None,
                    })
            };
            let (_, defined) = parse_core_rectype(
                ctx.reader,
                leading,
                validator.scope().core_types.len(),
                &resolve_existing,
            )?;
            defined
                .into_iter()
                .map(|defined| CoreType::Func(defined.func))
                .collect()
        }
    };
    Ok((ctx.reader.read_count() - start, types))
}
