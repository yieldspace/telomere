use super::*;

#[derive(Clone, Copy)]
pub(super) enum TypeExpectation {
    Bool,
    U8,
    S8,
    U16,
    S16,
    U32,
    S32OrI32,
    U64,
    S64OrI64,
    F32,
    F64,
    Char,
    String,
    #[cfg(feature = "component-gated-feature-async")]
    ErrorContext,
}

#[derive(Clone, Copy)]
pub(super) enum ResourceHandleKind {
    Own,
    Borrow,
}

pub(super) fn ensure_matches_type<T>(
    ty: &ValType,
    program: &ComponentProgram,
    expected: TypeExpectation,
) -> Result<(), ComponentError> {
    if type_matches(ty, program, expected) {
        Ok(())
    } else {
        Err(ComponentError::Link(format!(
            "typed component binding does not match {}",
            std::any::type_name::<T>()
        )))
    }
}

fn type_matches(ty: &ValType, program: &ComponentProgram, expected: TypeExpectation) -> bool {
    match ty {
        ValType::Primitive(prim) => primitive_matches(prim, expected),
        ValType::Type(type_id) => match program.get_type(*type_id) {
            Some(Type::DefVal(DefValType::Primitive(prim))) => primitive_matches(prim, expected),
            _ => false,
        },
    }
}

fn primitive_matches(prim: &PrimValType, expected: TypeExpectation) -> bool {
    match (prim, expected) {
        (PrimValType::Bool, TypeExpectation::Bool)
        | (PrimValType::U8, TypeExpectation::U8)
        | (PrimValType::S8, TypeExpectation::S8)
        | (PrimValType::U16, TypeExpectation::U16)
        | (PrimValType::S16, TypeExpectation::S16)
        | (PrimValType::U32, TypeExpectation::U32)
        | (PrimValType::S32, TypeExpectation::S32OrI32)
        | (PrimValType::S64, TypeExpectation::S64OrI64)
        | (PrimValType::U64, TypeExpectation::U64)
        | (PrimValType::F32, TypeExpectation::F32)
        | (PrimValType::F64, TypeExpectation::F64)
        | (PrimValType::Char, TypeExpectation::Char)
        | (PrimValType::String, TypeExpectation::String) => true,
        #[cfg(feature = "component-gated-feature-async")]
        (PrimValType::ErrorContext, TypeExpectation::ErrorContext) => true,
        _ => false,
    }
}

fn resolve_defined_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a Type, ComponentError> {
    match ty {
        ValType::Type(type_id) => program
            .get_type(*type_id)
            .ok_or_else(|| ComponentError::Link("type id not found".to_owned())),
        ValType::Primitive(_) => Err(ComponentError::Link(
            "expected defined component type".to_owned(),
        )),
    }
}

pub(super) fn extract_list_element_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a ValType, ComponentError> {
    match resolve_defined_type(ty, program)? {
        Type::DefVal(DefValType::List(elem, _)) => Ok(elem),
        _ => Err(ComponentError::Link(
            "typed component binding expects list".to_owned(),
        )),
    }
}

pub(super) fn extract_tuple_types<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
    expected_len: usize,
) -> Result<Vec<&'a ValType>, ComponentError> {
    let Type::DefVal(DefValType::Record(fields)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects tuple".to_owned(),
        ));
    };
    if fields.len() != expected_len
        || fields
            .iter()
            .enumerate()
            .any(|(index, field)| field.label.0 != index.to_string())
    {
        return Err(ComponentError::Link(
            "typed component binding expects tuple".to_owned(),
        ));
    }
    Ok(fields.iter().map(|field| &field.ty).collect())
}

pub(super) fn extract_option_payload_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<&'a ValType, ComponentError> {
    let Type::DefVal(DefValType::Variant(cases)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects option".to_owned(),
        ));
    };
    match cases.as_slice() {
        [none, some] if none.label.0 == "none" && none.ty.is_none() && some.label.0 == "some" => {
            some.ty.as_ref().ok_or_else(|| {
                ComponentError::Link("typed component binding expects option payload".to_owned())
            })
        }
        _ => Err(ComponentError::Link(
            "typed component binding expects option".to_owned(),
        )),
    }
}

pub(super) fn extract_result_payload_types<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<(Option<&'a ValType>, Option<&'a ValType>), ComponentError> {
    let Type::DefVal(DefValType::Variant(cases)) = resolve_defined_type(ty, program)? else {
        return Err(ComponentError::Link(
            "typed component binding expects result".to_owned(),
        ));
    };
    match cases.as_slice() {
        [ok, err] if ok.label.0 == "ok" && err.label.0 == "err" => {
            Ok((ok.ty.as_ref(), err.ty.as_ref()))
        }
        _ => Err(ComponentError::Link(
            "typed component binding expects result".to_owned(),
        )),
    }
}

pub(super) fn extract_resource_handle_type(
    ty: &ValType,
    program: &ComponentProgram,
) -> Result<ResourceHandleKind, ComponentError> {
    match resolve_defined_type(ty, program)? {
        Type::DefVal(DefValType::Own(_)) | Type::Resource(_) => Ok(ResourceHandleKind::Own),
        Type::DefVal(DefValType::Borrow(_)) => Ok(ResourceHandleKind::Borrow),
        _ => Err(ComponentError::Link(
            "typed component binding expects resource handle".to_owned(),
        )),
    }
}

#[cfg(feature = "component-gated-feature-async")]
pub(super) fn extract_future_payload_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<Option<&'a ValType>, ComponentError> {
    match resolve_defined_type(ty, program)? {
        Type::DefVal(DefValType::Future(payload)) => Ok(payload.as_ref()),
        _ => Err(ComponentError::Link(
            "typed component binding expects future".to_owned(),
        )),
    }
}

#[cfg(feature = "component-gated-feature-async")]
pub(super) fn extract_stream_payload_type<'a>(
    ty: &'a ValType,
    program: &'a ComponentProgram,
) -> Result<Option<&'a ValType>, ComponentError> {
    match resolve_defined_type(ty, program)? {
        Type::DefVal(DefValType::Stream(payload)) => Ok(payload.as_ref()),
        _ => Err(ComponentError::Link(
            "typed component binding expects stream".to_owned(),
        )),
    }
}
