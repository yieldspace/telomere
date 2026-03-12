use super::*;

pub(super) fn flatten_func_type(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
    mode: CanonMode,
) -> ParseResult<CoreFuncType> {
    let mut params = abi_flatten_params(&flatten_param_types_ctx(func_ty, ctx)?);
    let results = abi_flatten_results(&flatten_result_types_ctx(func_ty, ctx)?, mode, &mut params);
    Ok(CoreFuncType::new(params, results))
}

pub(super) fn flatten_param_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut params = Vec::new();
    for param in &func_ty.params {
        flatten_val_type(param, ctx, &mut params)?;
    }
    Ok(params)
}

pub(super) fn flatten_result_types_ctx(
    func_ty: &FuncType,
    ctx: &ParseContext<impl BinaryReader>,
) -> ParseResult<Vec<CoreValType>> {
    let mut results = Vec::new();
    if let Some(result) = &func_ty.result {
        flatten_val_type(result, ctx, &mut results)?;
    }
    Ok(results)
}

fn abi_flatten_params(flat: &[CoreValType]) -> Vec<CoreValType> {
    if flat.len() > MAX_FLAT_PARAMS {
        vec![CoreValType::I32]
    } else {
        flat.to_vec()
    }
}

fn abi_flatten_results(
    flat: &[CoreValType],
    mode: CanonMode,
    params: &mut Vec<CoreValType>,
) -> Vec<CoreValType> {
    if flat.len() > MAX_FLAT_RESULTS {
        match mode {
            CanonMode::Lift => vec![CoreValType::I32],
            CanonMode::Lower => {
                params.push(CoreValType::I32);
                Vec::new()
            }
        }
    } else {
        flat.to_vec()
    }
}

fn flatten_val_type(
    ty: &ValType,
    ctx: &ParseContext<impl BinaryReader>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        ValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        ValType::Type(type_id) => flatten_type(ctx.validator.get_type(*type_id)?, ctx, out),
    }
}

fn flatten_type(
    ty: &Type,
    ctx: &ParseContext<impl BinaryReader>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match ty {
        Type::DefVal(def) => flatten_defval(def, ctx, out),
        Type::Resource(_) | Type::Generic(_) => {
            out.push(CoreValType::I32);
            Ok(())
        }
        _ => Err(ComponentParseError::TypeMismatch(
            "not a function type".to_owned(),
        )),
    }
}

fn flatten_defval(
    def: &DefValType,
    ctx: &ParseContext<impl BinaryReader>,
    out: &mut Vec<CoreValType>,
) -> ParseResult<()> {
    match def {
        DefValType::Primitive(prim) => {
            flatten_primitive(prim, out);
            Ok(())
        }
        DefValType::Record(fields) => {
            for field in fields {
                flatten_val_type(&field.ty, ctx, out)?;
            }
            Ok(())
        }
        DefValType::Variant(cases) => {
            let mut payload = Vec::new();
            for case in cases {
                if let Some(ty) = &case.ty {
                    let mut flat = Vec::new();
                    flatten_val_type(ty, ctx, &mut flat)?;
                    for (index, ty) in flat.into_iter().enumerate() {
                        if let Some(current) = payload.get_mut(index) {
                            *current = join_core_types(*current, ty);
                        } else {
                            payload.push(ty);
                        }
                    }
                }
            }
            out.push(CoreValType::I32);
            out.extend(payload);
            Ok(())
        }
        DefValType::Flags(labels) => {
            for _ in 0..labels.len().div_ceil(32) {
                out.push(CoreValType::I32);
            }
            Ok(())
        }
        DefValType::List(_, _) => {
            out.push(CoreValType::I32);
            out.push(CoreValType::I32);
            Ok(())
        }
        DefValType::Own(_) | DefValType::Borrow(_) => {
            out.push(CoreValType::I32);
            Ok(())
        }
    }
}

fn join_core_types(lhs: CoreValType, rhs: CoreValType) -> CoreValType {
    if lhs == rhs {
        lhs
    } else if matches!(
        (lhs, rhs),
        (CoreValType::I32, CoreValType::F32) | (CoreValType::F32, CoreValType::I32)
    ) {
        CoreValType::I32
    } else {
        CoreValType::I64
    }
}

fn flatten_primitive(prim: &PrimValType, out: &mut Vec<CoreValType>) {
    match prim {
        PrimValType::Bool
        | PrimValType::S8
        | PrimValType::U8
        | PrimValType::S16
        | PrimValType::U16
        | PrimValType::S32
        | PrimValType::U32
        | PrimValType::Char => out.push(CoreValType::I32),
        PrimValType::S64 | PrimValType::U64 => out.push(CoreValType::I64),
        PrimValType::F32 => out.push(CoreValType::F32),
        PrimValType::F64 => out.push(CoreValType::F64),
        PrimValType::String => {
            out.push(CoreValType::I32);
            out.push(CoreValType::I32);
        }
    }
}
