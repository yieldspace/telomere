use super::*;

const MAX_FLAT_ASYNC_PARAMS: usize = 4;

pub(super) fn direct_wasm_to_component_value(
    value: WasmValue,
) -> Result<ComponentValue, ComponentError> {
    Ok(match value {
        WasmValue::I32(v) => ComponentValue::I32(v),
        WasmValue::I64(v) => ComponentValue::I64(v),
        WasmValue::F32(v) => ComponentValue::F32(v),
        WasmValue::F64(v) => ComponentValue::F64(v),
        WasmValue::FuncRef(v) => ComponentValue::Own(v),
        WasmValue::ExternRef(v) => ComponentValue::Borrow(v),
        WasmValue::V128(_) => {
            return Err(ComponentError::Unsupported(
                "v128 values are not supported in component runtime".to_owned(),
            ))
        }
    })
}

pub(super) fn component_value_to_direct_wasm(
    value: &ComponentValue,
) -> Result<WasmValue, ComponentError> {
    Ok(match value {
        ComponentValue::Bool(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S8(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::S16(v) => WasmValue::I32(i32::from(*v)),
        ComponentValue::U32(v) => WasmValue::I32(*v as i32),
        ComponentValue::S32(v) | ComponentValue::I32(v) => WasmValue::I32(*v),
        ComponentValue::U64(v) => WasmValue::I64(*v as i64),
        ComponentValue::S64(v) | ComponentValue::I64(v) => WasmValue::I64(*v),
        ComponentValue::F32(v) => WasmValue::F32(*v),
        ComponentValue::F64(v) => WasmValue::F64(*v),
        ComponentValue::Own(v) | ComponentValue::Borrow(v) => WasmValue::I32(*v as i32),
        other => {
            return Err(ComponentError::Unsupported(format!(
                "direct core invocation does not support {other:?}"
            )))
        }
    })
}

pub(super) fn lift_error_context_debug_message(
    options: &RuntimeCanonicalOptions,
    store: &Store,
    ptr: u32,
    len: u32,
) -> Result<String, ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    read_string_from_memory(store, memory, ptr, len, options.string_encoding)
}

pub(super) fn lower_error_context_debug_message(
    options: &RuntimeCanonicalOptions,
    store: &Store,
    message: &str,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let encoding = options
        .string_encoding
        .unwrap_or(CanonicalStringEncoding::Utf8);
    let align = if matches!(
        encoding,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16
    ) {
        2
    } else {
        1
    };
    let utf16;
    let bytes = match encoding {
        CanonicalStringEncoding::Utf8 => message.as_bytes(),
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            utf16 = encode_string_utf16(message);
            &utf16
        }
    };
    let message_ptr = call_realloc(realloc, store, 0, 0, align, bytes.len() as i32)?;
    write_memory(store, memory, message_ptr as u32, bytes)?;
    let message_len = match encoding {
        CanonicalStringEncoding::Utf8 => bytes.len() as i32,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            (bytes.len() / 2) as i32
        }
    };
    write_memory(store, memory, ptr, &message_ptr.to_le_bytes())?;
    write_memory(store, memory, ptr + 4, &message_len.to_le_bytes())
}

pub(super) fn lower_component_args(
    func_type: &FuncType,
    args: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    if func_type.params.len() != args.len() {
        return Err(ComponentError::InvalidArgument(format!(
            "expected {} component arguments, got {}",
            func_type.params.len(),
            args.len()
        )));
    }
    let total_flat_len = function_params_flat_len(func_type, program)?;
    if total_flat_len > MAX_FLAT_PARAMS {
        options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let realloc = options.realloc.as_ref().ok_or_else(|| {
            ComponentError::Runtime("canonical option `realloc` is required".to_owned())
        })?;
        let offsets = function_param_offsets(func_type, program)?;
        let total_size = function_param_size(func_type, program)?;
        let ptr = if total_size == 0 {
            0
        } else {
            call_realloc(realloc, store, 0, 0, 4, total_size as i32)? as u32
        };
        for ((value, ty), offset) in args
            .iter()
            .zip(func_type.params.iter())
            .zip(offsets.iter().copied())
        {
            write_value_to_memory(value, ty, options, program, store, ptr + offset)?;
        }
        return Ok(vec![WasmValue::I32(ptr as i32)]);
    }
    let mut lowered = Vec::new();
    for (value, ty) in args.iter().zip(func_type.params.iter()) {
        lower_value(value, ty, options, program, store, &mut lowered)?;
    }
    Ok(lowered)
}

pub(super) fn lift_component_args(
    func_type: &FuncType,
    args: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let max_flat_params = if options.async_ {
        MAX_FLAT_ASYNC_PARAMS
    } else {
        MAX_FLAT_PARAMS
    };
    if function_params_flat_len(func_type, program)? > max_flat_params {
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let ptr = match args.first() {
            Some(WasmValue::I32(ptr)) => *ptr as u32,
            Some(other) => {
                return Err(ComponentError::Runtime(format!(
                    "indirect canonical parameter area must be an i32 pointer, got {other:?}"
                )))
            }
            None => {
                return Err(ComponentError::Runtime(
                    "indirect canonical parameter area is missing".to_owned(),
                ))
            }
        };
        let offsets = function_param_offsets(func_type, program)?;
        return func_type
            .params
            .iter()
            .zip(offsets.iter().copied())
            .map(|(ty, offset)| {
                read_value_from_memory(ty, options, program, store, &memory, ptr + offset)
            })
            .collect();
    }
    let mut cursor = CoreValueCursor::new(args);
    func_type
        .params
        .iter()
        .map(|ty| lift_value(ty, options, program, store, &mut cursor))
        .collect()
}

pub(super) fn lift_component_results(
    func_type: &FuncType,
    results: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<ComponentValue>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        return Ok(Vec::new());
    };
    let mut cursor = CoreValueCursor::new(results);
    let result = if value_flat_len(result_ty, program)? > MAX_FLAT_RESULTS {
        let pointer = cursor.next_i32()? as u32;
        let memory = options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        read_value_from_memory(result_ty, options, program, store, &memory, pointer)?
    } else {
        lift_value(result_ty, options, program, store, &mut cursor)?
    };
    Ok(vec![result])
}

pub(super) fn lower_component_results(
    func_type: &FuncType,
    results: &[ComponentValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    result_area: Option<u32>,
) -> Result<Vec<WasmValue>, ComponentError> {
    if options.async_ {
        let Some(result_ty) = &func_type.result else {
            if results.is_empty() {
                return Ok(vec![WasmValue::I32(SubtaskState::Returned as i32)]);
            }
            return Err(ComponentError::InvalidArgument(
                "function does not return a value".to_owned(),
            ));
        };
        let value = results.first().ok_or_else(|| {
            ComponentError::InvalidArgument("function result is missing".to_owned())
        })?;
        let return_ptr = result_area
            .ok_or_else(|| ComponentError::Runtime("async result area is missing".to_owned()))?;
        options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        write_value_to_memory(value, result_ty, options, program, store, return_ptr)?;
        return Ok(vec![WasmValue::I32(SubtaskState::Returned as i32)]);
    }

    let Some(result_ty) = &func_type.result else {
        if results.is_empty() {
            return Ok(Vec::new());
        }
        return Err(ComponentError::InvalidArgument(
            "function does not return a value".to_owned(),
        ));
    };
    let value = results
        .first()
        .ok_or_else(|| ComponentError::InvalidArgument("function result is missing".to_owned()))?;
    let flat_len = value_flat_len(result_ty, program)?;
    if flat_len > MAX_FLAT_RESULTS {
        options.memory.clone().ok_or_else(|| {
            ComponentError::Runtime("canonical option `memory` is required".to_owned())
        })?;
        let return_ptr = result_area
            .ok_or_else(|| ComponentError::Runtime("indirect result area is missing".to_owned()))?;
        write_value_to_memory(value, result_ty, options, program, store, return_ptr)?;
        Ok(Vec::new())
    } else {
        lower_value_to_flat(value, result_ty, options, program, store)
    }
}

pub(super) fn lowered_indirect_result_area(
    func_type: &FuncType,
    args: &[WasmValue],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
) -> Result<Option<u32>, ComponentError> {
    let Some(result_ty) = &func_type.result else {
        return Ok(None);
    };
    if !options.async_ && value_flat_len(result_ty, program)? <= MAX_FLAT_RESULTS {
        return Ok(None);
    }
    match args.last() {
        Some(WasmValue::I32(ptr)) => Ok(Some(*ptr as u32)),
        Some(other) => Err(ComponentError::Runtime(format!(
            "indirect result area must be an i32 pointer, got {other:?}"
        ))),
        None => Err(ComponentError::Runtime(
            "indirect result area is missing".to_owned(),
        )),
    }
}

#[derive(Clone, Copy)]
enum SubtaskState {
    Returned = 2,
}

pub(super) const COPY_COMPLETED: i32 = 0;
pub(super) const COPY_BLOCKED: i32 = -1;

fn lower_value(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    out: &mut Vec<WasmValue>,
) -> Result<(), ComponentError> {
    out.extend(lower_value_to_flat(value, ty, options, program, store)?);
    Ok(())
}

fn lower_value_to_flat(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lower_primitive(value, prim, options, store, program),
        ValType::Type(type_id) => lower_defined_value(value, *type_id, options, program, store),
    }
}

fn lower_primitive(
    value: &ComponentValue,
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &Store,
    program: &ComponentProgram,
) -> Result<Vec<WasmValue>, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => vec![WasmValue::I32(i32::from(expect_bool(value)?))],
        PrimValType::S8 => vec![WasmValue::I32(expect_i32(value)? as i8 as i32)],
        PrimValType::U8 => vec![WasmValue::I32(expect_u32(value)? as u8 as i32)],
        PrimValType::S16 => vec![WasmValue::I32(expect_i32(value)? as i16 as i32)],
        PrimValType::U16 => vec![WasmValue::I32(expect_u32(value)? as u16 as i32)],
        PrimValType::S32 => vec![WasmValue::I32(expect_i32(value)?)],
        PrimValType::U32 => vec![WasmValue::I32(expect_u32(value)? as i32)],
        PrimValType::S64 => vec![WasmValue::I64(expect_i64(value)?)],
        PrimValType::U64 => vec![WasmValue::I64(expect_u64(value)? as i64)],
        PrimValType::F32 => vec![WasmValue::F32(expect_f32(value)?)],
        PrimValType::F64 => vec![WasmValue::F64(expect_f64(value)?)],
        PrimValType::Char => vec![WasmValue::I32(expect_char(value)? as u32 as i32)],
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => vec![WasmValue::I32(expect_error_context(value)? as i32)],
        PrimValType::String => lower_string(value, options, program, store)?,
    })
}

fn lower_defined_value(
    value: &ComponentValue,
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            lower_primitive(value, prim, options, store, program)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            lower_record_value(value, fields, options, program, store)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            lower_variant_value(value, cases, options, program, store)
        }
        Type::DefVal(DefValType::Flags(labels)) => lower_flags_value(value, labels),
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            lower_list_value(value, elem, *maybe_len, options, program, store)
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Stream(_)) => {
            Ok(vec![WasmValue::I32(expect_stream(value)? as i32)])
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Future(_)) => {
            Ok(vec![WasmValue::I32(expect_future(value)? as i32)])
        }
        Type::DefVal(DefValType::Own(resource)) | Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(vec![WasmValue::I32(expect_handle(value)? as i32)])
        }
        Type::Resource(_resource) => Ok(vec![WasmValue::I32(expect_handle(value)? as i32)]),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_value(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match ty {
        ValType::Primitive(prim) => lift_primitive(prim, options, store, cursor),
        ValType::Type(type_id) => lift_defined_value(*type_id, options, program, store, cursor),
    }
}

fn lift_defined_value(
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => lift_primitive(prim, options, store, cursor),
        Type::DefVal(DefValType::Record(fields)) => {
            lift_record_value(fields, options, program, store, cursor)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            lift_variant_value(cases, options, program, store, cursor)
        }
        Type::DefVal(DefValType::Flags(labels)) => lift_flags_value(labels, cursor),
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            lift_list_value(elem, *maybe_len, options, program, store, cursor)
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Stream(_)) => {
            Ok(ComponentValue::Stream(cursor.next_i32()? as u32))
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Future(_)) => {
            Ok(ComponentValue::Future(cursor.next_i32()? as u32))
        }
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Own(cursor.next_i32()? as u32))
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Borrow(cursor.next_i32()? as u32))
        }
        Type::Resource(_resource) => Ok(ComponentValue::Own(cursor.next_i32()? as u32)),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn lift_primitive(
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => ComponentValue::Bool(cursor.next_i32()? != 0),
        PrimValType::S8 => ComponentValue::S8(cursor.next_i32()? as i8),
        PrimValType::U8 => ComponentValue::U8(cursor.next_i32()? as u8),
        PrimValType::S16 => ComponentValue::S16(cursor.next_i32()? as i16),
        PrimValType::U16 => ComponentValue::U16(cursor.next_i32()? as u16),
        PrimValType::S32 => ComponentValue::S32(cursor.next_i32()?),
        PrimValType::U32 => ComponentValue::U32(cursor.next_i32()? as u32),
        PrimValType::S64 => ComponentValue::S64(cursor.next_i64()?),
        PrimValType::U64 => ComponentValue::U64(cursor.next_i64()? as u64),
        PrimValType::F32 => ComponentValue::F32(cursor.next_f32()?),
        PrimValType::F64 => ComponentValue::F64(cursor.next_f64()?),
        PrimValType::Char => ComponentValue::Char(
            char::from_u32(cursor.next_i32()? as u32)
                .ok_or_else(|| ComponentError::Trap("invalid char scalar".to_owned()))?,
        ),
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => ComponentValue::ErrorContext(cursor.next_i32()? as u32),
        PrimValType::String => {
            let memory = options.memory.as_ref().ok_or_else(|| {
                ComponentError::Runtime("canonical option `memory` is required".to_owned())
            })?;
            let ptr = cursor.next_i32()? as u32;
            let len = cursor.next_i32()? as u32;
            ComponentValue::String(read_string_from_memory(
                store,
                memory,
                ptr,
                len,
                options.string_encoding,
            )?)
        }
    })
}

fn lower_string(
    value: &ComponentValue,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let string = value
        .as_str()
        .ok_or_else(|| ComponentError::InvalidArgument("expected string value".to_owned()))?;
    let encoding = options
        .string_encoding
        .unwrap_or(CanonicalStringEncoding::Utf8);
    let align = if matches!(
        encoding,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16
    ) {
        2
    } else {
        1
    };
    let utf16;
    let bytes = match encoding {
        CanonicalStringEncoding::Utf8 => string.as_bytes(),
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            utf16 = encode_string_utf16(string);
            &utf16
        }
    };
    let ptr = call_realloc(realloc, store, 0, 0, align, bytes.len() as i32)? as u32;
    write_memory(store, &memory, ptr, bytes)?;
    let len = match encoding {
        CanonicalStringEncoding::Utf8 => bytes.len() as u32,
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            (bytes.len() / 2) as u32
        }
    };
    Ok(vec![WasmValue::I32(ptr as i32), WasmValue::I32(len as i32)])
}

fn encode_string_utf16(value: &str) -> Vec<u8> {
    value
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect()
}

#[derive(Clone, Copy)]
struct MemoryAbiInfo {
    size: u32,
    align: u32,
}

fn variant_discriminant_size(case_count: usize) -> Result<u32, ComponentError> {
    if case_count <= (1 << 8) {
        Ok(1)
    } else if case_count <= (1 << 16) {
        Ok(2)
    } else if (case_count as u64) <= (1 << 32) {
        Ok(4)
    } else {
        Err(ComponentError::Unsupported(
            "variants with more than 2^32 cases are not supported".to_owned(),
        ))
    }
}

fn flags_memory_abi(count: usize) -> MemoryAbiInfo {
    if count == 0 {
        MemoryAbiInfo { size: 0, align: 1 }
    } else if count <= 8 {
        MemoryAbiInfo { size: 1, align: 1 }
    } else if count <= 16 {
        MemoryAbiInfo { size: 2, align: 2 }
    } else {
        MemoryAbiInfo {
            size: 4 * count.div_ceil(32) as u32,
            align: 4,
        }
    }
}

fn memory_abi_for_primitive(prim: &PrimValType) -> MemoryAbiInfo {
    match prim {
        PrimValType::Bool | PrimValType::S8 | PrimValType::U8 => {
            MemoryAbiInfo { size: 1, align: 1 }
        }
        PrimValType::S16 | PrimValType::U16 => MemoryAbiInfo { size: 2, align: 2 },
        PrimValType::S32 | PrimValType::U32 | PrimValType::Char | PrimValType::F32 => {
            MemoryAbiInfo { size: 4, align: 4 }
        }
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => MemoryAbiInfo { size: 4, align: 4 },
        PrimValType::S64 | PrimValType::U64 | PrimValType::F64 => {
            MemoryAbiInfo { size: 8, align: 8 }
        }
        PrimValType::String => MemoryAbiInfo { size: 8, align: 4 },
    }
}

fn memory_abi_for_type(
    type_id: TypeId,
    program: &ComponentProgram,
) -> Result<MemoryAbiInfo, ComponentError> {
    program
        .get_type_info(type_id)
        .map(|info| MemoryAbiInfo {
            size: info.indirect_size,
            align: info.indirect_align.max(1),
        })
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))
}

fn memory_abi_for_valtype(
    ty: &ValType,
    program: &ComponentProgram,
) -> Result<MemoryAbiInfo, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(memory_abi_for_primitive(prim)),
        ValType::Type(type_id) => memory_abi_for_type(*type_id, program),
    }
}

fn value_area_layout(
    values: &[ValType],
    program: &ComponentProgram,
) -> Result<(Vec<u32>, u32), ComponentError> {
    let mut offsets = Vec::with_capacity(values.len());
    let mut cursor = 0u32;
    let mut max_align = 1u32;
    for ty in values {
        let abi = memory_abi_for_valtype(ty, program)?;
        max_align = max_align.max(abi.align.max(1));
        cursor = align_to(cursor, abi.align.max(1));
        offsets.push(cursor);
        cursor = cursor.saturating_add(abi.size);
    }
    Ok((offsets, align_to(cursor, max_align)))
}

pub(super) fn element_stride(
    elem: &ValType,
    program: &ComponentProgram,
) -> Result<u32, ComponentError> {
    let abi = memory_abi_for_valtype(elem, program)?;
    Ok(align_to(abi.size, abi.align.max(1)))
}

fn function_params_flat_len(
    func_type: &FuncType,
    program: &ComponentProgram,
) -> Result<usize, ComponentError> {
    func_type
        .params
        .iter()
        .try_fold(0usize, |len, ty| Ok(len + value_flat_len(ty, program)?))
}

fn function_param_offsets(
    func_type: &FuncType,
    program: &ComponentProgram,
) -> Result<Vec<u32>, ComponentError> {
    Ok(value_area_layout(&func_type.params, program)?.0)
}

fn function_param_size(
    func_type: &FuncType,
    program: &ComponentProgram,
) -> Result<u32, ComponentError> {
    Ok(value_area_layout(&func_type.params, program)?.1)
}

fn flat_types_for_valtype(
    ty: &ValType,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(flat_types_for_primitive(prim)),
        ValType::Type(type_id) => flat_types_for_type(*type_id, program),
    }
}

fn flat_types_for_type(
    type_id: TypeId,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(def) => flat_types_for_defval(def, program),
        Type::Resource(_) | Type::Generic(_) => Ok(vec![CoreValType::I32]),
        _ => Err(ComponentError::Unsupported(
            "flat types for this component type are not implemented yet".to_owned(),
        )),
    }
}

fn flat_types_for_defval(
    def: &DefValType,
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    Ok(match def {
        DefValType::Primitive(prim) => flat_types_for_primitive(prim),
        DefValType::Record(fields) => {
            let mut types = Vec::new();
            for field in fields {
                types.extend(flat_types_for_valtype(&field.ty, program)?);
            }
            types
        }
        DefValType::Variant(cases) => {
            let mut payload = Vec::new();
            for case in cases {
                if let Some(ty) = &case.ty {
                    for (index, flat_ty) in
                        flat_types_for_valtype(ty, program)?.into_iter().enumerate()
                    {
                        if let Some(current) = payload.get_mut(index) {
                            *current = join_component_flat_types(*current, flat_ty);
                        } else {
                            payload.push(flat_ty);
                        }
                    }
                }
            }
            let mut types = Vec::with_capacity(1 + payload.len());
            types.push(CoreValType::I32);
            types.extend(payload);
            types
        }
        DefValType::Flags(labels) => {
            let flat_len = if labels.is_empty() {
                0
            } else if labels.len() <= 16 {
                1
            } else {
                labels.len().div_ceil(32)
            };
            vec![CoreValType::I32; flat_len]
        }
        DefValType::List(_, _) => vec![CoreValType::I32, CoreValType::I32],
        #[cfg(feature = "component-gated-feature-async")]
        DefValType::Stream(_) | DefValType::Future(_) => vec![CoreValType::I32],
        DefValType::Own(_) | DefValType::Borrow(_) => vec![CoreValType::I32],
    })
}

fn flat_types_for_primitive(prim: &PrimValType) -> Vec<CoreValType> {
    match prim {
        PrimValType::Bool
        | PrimValType::S8
        | PrimValType::U8
        | PrimValType::S16
        | PrimValType::U16
        | PrimValType::S32
        | PrimValType::U32
        | PrimValType::Char => vec![CoreValType::I32],
        PrimValType::S64 | PrimValType::U64 => vec![CoreValType::I64],
        PrimValType::F32 => vec![CoreValType::F32],
        PrimValType::F64 => vec![CoreValType::F64],
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => vec![CoreValType::I32],
        PrimValType::String => vec![CoreValType::I32, CoreValType::I32],
    }
}

fn join_component_flat_types(lhs: CoreValType, rhs: CoreValType) -> CoreValType {
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

pub(super) fn write_value_to_memory(
    value: &ComponentValue,
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    match ty {
        ValType::Primitive(prim) => {
            write_primitive_to_memory(value, prim, options, program, store, ptr)
        }
        ValType::Type(type_id) => {
            write_defined_value_to_memory(value, *type_id, options, program, store, ptr)
        }
    }
}

pub(super) fn read_value_from_memory(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    match ty {
        ValType::Primitive(prim) => {
            read_primitive_from_memory(prim, options, program, store, memory, ptr)
        }
        ValType::Type(type_id) => {
            read_defined_value_from_memory(*type_id, options, program, store, memory, ptr)
        }
    }
}

fn write_defined_value_to_memory(
    value: &ComponentValue,
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            write_primitive_to_memory(value, prim, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            write_record_value_to_memory(value, fields, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            write_variant_value_to_memory(value, cases, options, program, store, ptr)
        }
        Type::DefVal(DefValType::Flags(labels)) => {
            write_flags_value_to_memory(value, labels, options, program, store, ptr)
        }
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            let flat = lower_list_value(value, elem, *maybe_len, options, program, store)?;
            write_flat_values(store, memory, ptr, &flat)
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Stream(_)) => write_memory(
            store,
            memory,
            ptr,
            &(expect_stream(value)? as i32).to_le_bytes(),
        ),
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Future(_)) => write_memory(
            store,
            memory,
            ptr,
            &(expect_future(value)? as i32).to_le_bytes(),
        ),
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            write_memory(
                store,
                memory,
                ptr,
                &(expect_handle(value)? as i32).to_le_bytes(),
            )
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            write_memory(
                store,
                memory,
                ptr,
                &(expect_handle(value)? as i32).to_le_bytes(),
            )
        }
        Type::Resource(_) | Type::Generic(_) => write_memory(
            store,
            memory,
            ptr,
            &(expect_handle(value)? as i32).to_le_bytes(),
        ),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn read_defined_value_from_memory(
    type_id: TypeId,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    match program
        .get_type(type_id)
        .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned()))?
    {
        Type::DefVal(DefValType::Primitive(prim)) => {
            read_primitive_from_memory(prim, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Record(fields)) => {
            read_record_value_from_memory(fields, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Variant(cases)) => {
            read_variant_value_from_memory(cases, options, program, store, memory, ptr)
        }
        Type::DefVal(DefValType::Flags(labels)) => {
            read_flags_value_from_memory(labels, memory, store, ptr)
        }
        Type::DefVal(DefValType::List(elem, maybe_len)) => {
            read_list_value_from_memory(elem, *maybe_len, options, program, store, memory, ptr)
        }
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Stream(_)) => Ok(ComponentValue::Stream(read_i32_from_memory(
            store, memory, ptr,
        )? as u32)),
        #[cfg(feature = "component-gated-feature-async")]
        Type::DefVal(DefValType::Future(_)) => Ok(ComponentValue::Future(read_i32_from_memory(
            store, memory, ptr,
        )? as u32)),
        Type::DefVal(DefValType::Own(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Own(
                read_i32_from_memory(store, memory, ptr)? as u32,
            ))
        }
        Type::DefVal(DefValType::Borrow(resource)) => {
            validate_resource_type(program, *resource)?;
            Ok(ComponentValue::Borrow(
                read_i32_from_memory(store, memory, ptr)? as u32,
            ))
        }
        Type::Resource(_) | Type::Generic(_) => Ok(ComponentValue::Own(read_i32_from_memory(
            store, memory, ptr,
        )? as u32)),
        _ => Err(ComponentError::Unsupported(
            "canonical ABI for this type is not implemented yet".to_owned(),
        )),
    }
}

fn write_primitive_to_memory(
    value: &ComponentValue,
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match prim {
        PrimValType::Bool => write_memory(store, memory, ptr, &[u8::from(expect_bool(value)?)]),
        PrimValType::S8 => write_memory(store, memory, ptr, &[(expect_i32(value)? as i8) as u8]),
        PrimValType::U8 => write_memory(store, memory, ptr, &[expect_u32(value)? as u8]),
        PrimValType::S16 => write_memory(
            store,
            memory,
            ptr,
            &(expect_i32(value)? as i16).to_le_bytes(),
        ),
        PrimValType::U16 => write_memory(
            store,
            memory,
            ptr,
            &(expect_u32(value)? as u16).to_le_bytes(),
        ),
        PrimValType::S32 => write_memory(store, memory, ptr, &expect_i32(value)?.to_le_bytes()),
        PrimValType::U32 => write_memory(store, memory, ptr, &expect_u32(value)?.to_le_bytes()),
        PrimValType::S64 => write_memory(store, memory, ptr, &expect_i64(value)?.to_le_bytes()),
        PrimValType::U64 => write_memory(store, memory, ptr, &expect_u64(value)?.to_le_bytes()),
        PrimValType::F32 => write_memory(store, memory, ptr, &expect_f32(value)?.to_le_bytes()),
        PrimValType::F64 => write_memory(store, memory, ptr, &expect_f64(value)?.to_le_bytes()),
        PrimValType::Char => write_memory(
            store,
            memory,
            ptr,
            &(expect_char(value)? as u32).to_le_bytes(),
        ),
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => write_memory(
            store,
            memory,
            ptr,
            &(expect_error_context(value)? as i32).to_le_bytes(),
        ),
        PrimValType::String => {
            let flat = lower_string(value, options, program, store)?;
            write_flat_values(store, memory, ptr, &flat)
        }
    }
}

fn read_primitive_from_memory(
    prim: &PrimValType,
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    Ok(match prim {
        PrimValType::Bool => ComponentValue::Bool(read_u8_from_memory(store, memory, ptr)? != 0),
        PrimValType::S8 => ComponentValue::S8(read_u8_from_memory(store, memory, ptr)? as i8),
        PrimValType::U8 => ComponentValue::U8(read_u8_from_memory(store, memory, ptr)?),
        PrimValType::S16 => ComponentValue::S16(read_u16_from_memory(store, memory, ptr)? as i16),
        PrimValType::U16 => ComponentValue::U16(read_u16_from_memory(store, memory, ptr)?),
        PrimValType::S32 => ComponentValue::S32(read_i32_from_memory(store, memory, ptr)?),
        PrimValType::U32 => ComponentValue::U32(read_i32_from_memory(store, memory, ptr)? as u32),
        PrimValType::S64 => ComponentValue::S64(read_i64_from_memory(store, memory, ptr)?),
        PrimValType::U64 => ComponentValue::U64(read_i64_from_memory(store, memory, ptr)? as u64),
        PrimValType::F32 => ComponentValue::F32(f32::from_le_bytes(read_memory_array::<4>(
            store, memory, ptr,
        )?)),
        PrimValType::F64 => ComponentValue::F64(f64::from_le_bytes(read_memory_array::<8>(
            store, memory, ptr,
        )?)),
        PrimValType::Char => ComponentValue::Char(
            char::from_u32(read_i32_from_memory(store, memory, ptr)? as u32)
                .ok_or_else(|| ComponentError::Trap("invalid char scalar".to_owned()))?,
        ),
        #[cfg(feature = "component-gated-feature-async")]
        PrimValType::ErrorContext => {
            ComponentValue::ErrorContext(read_i32_from_memory(store, memory, ptr)? as u32)
        }
        PrimValType::String => {
            let string_ptr = read_i32_from_memory(store, memory, ptr)? as u32;
            let len = read_i32_from_memory(store, memory, ptr + 4)? as u32;
            ComponentValue::String(read_string_from_memory(
                store,
                memory,
                string_ptr,
                len,
                options.string_encoding,
            )?)
        }
    })
}

fn write_record_value_to_memory(
    value: &ComponentValue,
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let values = if is_tuple_fields(fields) {
        match value {
            ComponentValue::Tuple(values) => values.clone(),
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected tuple value, got {other:?}"
                )))
            }
        }
    } else {
        match value {
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected record value, got {other:?}"
                )))
            }
        }
    };

    let mut cursor = 0u32;
    for (field_value, field) in values.iter().zip(fields.iter()) {
        let abi = memory_abi_for_valtype(&field.ty, program)?;
        cursor = align_to(cursor, abi.align.max(1));
        write_value_to_memory(
            field_value,
            &field.ty,
            options,
            program,
            store,
            ptr + cursor,
        )?;
        cursor = cursor.saturating_add(abi.size);
    }
    Ok(())
}

fn read_record_value_from_memory(
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let mut values = Vec::with_capacity(fields.len());
    let mut cursor = 0u32;
    for field in fields {
        let abi = memory_abi_for_valtype(&field.ty, program)?;
        cursor = align_to(cursor, abi.align.max(1));
        values.push((
            field.label.0.clone(),
            read_value_from_memory(&field.ty, options, program, store, memory, ptr + cursor)?,
        ));
        cursor = cursor.saturating_add(abi.size);
    }
    if is_tuple_fields(fields) {
        Ok(ComponentValue::Tuple(
            values.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Ok(ComponentValue::Record(values))
    }
}

fn write_variant_value_to_memory(
    value: &ComponentValue,
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let (case_name, payload) = match value {
        ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
        ComponentValue::Enum(case) => (case.as_str(), None),
        ComponentValue::Option(value) if is_option_cases(cases) => match value {
            Some(value) => ("some", Some(value.as_ref())),
            None => ("none", None),
        },
        ComponentValue::Result { ok, err } if is_result_cases(cases) => match (ok, err) {
            (Some(value), None) => ("ok", Some(value.as_ref())),
            (None, Some(value)) => ("err", Some(value.as_ref())),
            _ => {
                return Err(ComponentError::InvalidArgument(
                    "result value must set exactly one branch".to_owned(),
                ))
            }
        },
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected variant-compatible value, got {other:?}"
            )))
        }
    };
    let case_index = cases
        .iter()
        .position(|case| case.label.0 == case_name)
        .ok_or_else(|| {
            ComponentError::InvalidArgument(format!("unknown variant case '{case_name}'"))
        })?;
    write_variant_discriminant_to_memory(store, options, ptr, cases.len(), case_index)?;

    let (payload_offset, payload_size) = variant_payload_offset_and_size(cases, program)?;
    if payload_size != 0 {
        write_memory(
            store,
            options.memory.as_ref().expect("memory already checked"),
            ptr + payload_offset,
            &vec![0; payload_size as usize],
        )?;
    }

    let case = &cases[case_index];
    if let Some(ty) = &case.ty {
        let payload = payload.ok_or_else(|| {
            ComponentError::InvalidArgument(format!(
                "variant case '{}' expects a payload",
                case.label
            ))
        })?;
        write_value_to_memory(payload, ty, options, program, store, ptr + payload_offset)?;
    } else {
        let payloadless_result_case = is_result_cases(cases)
            && matches!(
                payload,
                Some(ComponentValue::Tuple(values)) if values.is_empty()
            );
        if payload.is_some() && !payloadless_result_case {
            return Err(ComponentError::InvalidArgument(format!(
                "variant case '{}' does not accept a payload",
                case.label
            )));
        }
    }

    Ok(())
}

fn read_variant_value_from_memory(
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let discriminant = read_variant_discriminant_from_memory(store, memory, ptr, cases.len())?;
    let case = cases
        .get(discriminant)
        .ok_or_else(|| ComponentError::Trap("variant discriminant is out of bounds".to_owned()))?;
    let (payload_offset, _) = variant_payload_offset_and_size(cases, program)?;
    let payload = if let Some(ty) = &case.ty {
        Some(Box::new(read_value_from_memory(
            ty,
            options,
            program,
            store,
            memory,
            ptr + payload_offset,
        )?))
    } else {
        None
    };

    if is_option_cases(cases) {
        return Ok(ComponentValue::Option(payload));
    }
    if is_result_cases(cases) {
        let payload = if case.ty.is_none() {
            Some(Box::new(ComponentValue::Tuple(Vec::new())))
        } else {
            payload
        };
        return Ok(match case.label.0.as_str() {
            "ok" => ComponentValue::Result {
                ok: payload,
                err: None,
            },
            "err" => ComponentValue::Result {
                ok: None,
                err: payload,
            },
            _ => unreachable!(),
        });
    }
    if is_enum_cases(cases) {
        return Ok(ComponentValue::Enum(case.label.0.clone()));
    }
    Ok(ComponentValue::Variant {
        case: case.label.0.clone(),
        value: payload,
    })
}

fn write_flags_value_to_memory(
    value: &ComponentValue,
    labels: &[crate::ir::Label],
    options: &RuntimeCanonicalOptions,
    _program: &ComponentProgram,
    store: &Store,
    ptr: u32,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let words = lower_flags_value(value, labels)?
        .into_iter()
        .map(|word| match word {
            WasmValue::I32(bits) => Ok(bits as u32),
            other => Err(ComponentError::Runtime(format!(
                "flags lowered to unexpected flat value {other:?}"
            ))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let abi = flags_memory_abi(labels.len());
    match abi.size {
        0 => Ok(()),
        1 => write_memory(
            store,
            memory,
            ptr,
            &[words.first().copied().unwrap_or(0) as u8],
        ),
        2 => write_memory(
            store,
            memory,
            ptr,
            &(words.first().copied().unwrap_or(0) as u16).to_le_bytes(),
        ),
        _ => {
            for (index, word) in words.iter().enumerate() {
                write_memory(store, memory, ptr + (index as u32) * 4, &word.to_le_bytes())?;
            }
            Ok(())
        }
    }
}

fn read_flags_value_from_memory(
    labels: &[crate::ir::Label],
    memory: &CoreExportRef,
    store: &Store,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let abi = flags_memory_abi(labels.len());
    let words = match abi.size {
        0 => Vec::new(),
        1 => vec![read_u8_from_memory(store, memory, ptr)? as u32],
        2 => vec![read_u16_from_memory(store, memory, ptr)? as u32],
        _ => {
            let mut words = Vec::with_capacity(labels.len().div_ceil(32));
            for index in 0..labels.len().div_ceil(32) {
                words.push(read_i32_from_memory(store, memory, ptr + (index as u32) * 4)? as u32);
            }
            words
        }
    };
    let mut selected = Vec::new();
    for chunk_start in (0..labels.len()).step_by(32) {
        let bits = words.get(chunk_start / 32).copied().unwrap_or(0);
        for bit in 0..32 {
            let index = chunk_start + bit;
            if index >= labels.len() {
                break;
            }
            if bits & (1 << bit) != 0 {
                selected.push(labels[index].0.clone());
            }
        }
    }
    Ok(ComponentValue::Flags(selected))
}

fn read_list_value_from_memory(
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<ComponentValue, ComponentError> {
    let list_ptr = read_i32_from_memory(store, memory, ptr)? as u32;
    let len = read_i32_from_memory(store, memory, ptr + 4)? as usize;
    if let Some(expected) = fixed_len {
        if len != expected {
            return Err(ComponentError::Trap(format!(
                "fixed-length list expected {expected} elements, got {len}"
            )));
        }
    }
    let stride = element_stride(elem, program)?;
    let mut values = Vec::with_capacity(len);
    for index in 0..len {
        values.push(read_value_from_memory(
            elem,
            options,
            program,
            store,
            memory,
            list_ptr + stride * index as u32,
        )?);
    }
    Ok(ComponentValue::List(values))
}

fn variant_payload_offset_and_size(
    cases: &[Case],
    program: &ComponentProgram,
) -> Result<(u32, u32), ComponentError> {
    let discriminant_size = variant_discriminant_size(cases.len())?;
    let mut payload_size = 0u32;
    let mut payload_align = discriminant_size;
    for case in cases {
        if let Some(ty) = &case.ty {
            let abi = memory_abi_for_valtype(ty, program)?;
            payload_size = payload_size.max(abi.size);
            payload_align = payload_align.max(abi.align.max(1));
        }
    }
    Ok((
        align_to(discriminant_size, payload_align.max(1)),
        payload_size,
    ))
}

fn write_variant_discriminant_to_memory(
    store: &Store,
    options: &RuntimeCanonicalOptions,
    ptr: u32,
    case_count: usize,
    discriminant: usize,
) -> Result<(), ComponentError> {
    let memory = options.memory.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    match variant_discriminant_size(case_count)? {
        1 => write_memory(store, memory, ptr, &[discriminant as u8]),
        2 => write_memory(store, memory, ptr, &(discriminant as u16).to_le_bytes()),
        4 => write_memory(store, memory, ptr, &(discriminant as u32).to_le_bytes()),
        _ => unreachable!(),
    }
}

fn read_variant_discriminant_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    case_count: usize,
) -> Result<usize, ComponentError> {
    Ok(match variant_discriminant_size(case_count)? {
        1 => read_u8_from_memory(store, memory, ptr)? as usize,
        2 => read_u16_from_memory(store, memory, ptr)? as usize,
        4 => read_i32_from_memory(store, memory, ptr)? as usize,
        _ => unreachable!(),
    })
}

fn read_u8_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<u8, ComponentError> {
    Ok(read_memory_array::<1>(store, memory, ptr)?[0])
}

fn read_u16_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<u16, ComponentError> {
    Ok(u16::from_le_bytes(read_memory_array::<2>(
        store, memory, ptr,
    )?))
}

fn read_i32_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<i32, ComponentError> {
    Ok(i32::from_le_bytes(read_memory_array::<4>(
        store, memory, ptr,
    )?))
}

fn read_i64_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<i64, ComponentError> {
    Ok(i64::from_le_bytes(read_memory_array::<8>(
        store, memory, ptr,
    )?))
}

fn align_to(value: u32, align: u32) -> u32 {
    if align <= 1 {
        value
    } else {
        let rem = value % align;
        if rem == 0 {
            value
        } else {
            value + (align - rem)
        }
    }
}

fn lower_record_value(
    value: &ComponentValue,
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let values = if is_tuple_fields(fields) {
        match value {
            ComponentValue::Tuple(values) => values.clone(),
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected tuple value, got {other:?}"
                )))
            }
        }
    } else {
        match value {
            ComponentValue::Record(entries) => fields
                .iter()
                .map(|field| {
                    entries
                        .iter()
                        .find(|(name, _)| name == &field.label.0)
                        .map(|(_, value)| value.clone())
                        .ok_or_else(|| {
                            ComponentError::InvalidArgument(format!(
                                "record field '{}' is missing",
                                field.label
                            ))
                        })
                })
                .collect::<Result<Vec<_>, _>>()?,
            other => {
                return Err(ComponentError::InvalidArgument(format!(
                    "expected record value, got {other:?}"
                )))
            }
        }
    };
    let mut lowered = Vec::new();
    for (value, field) in values.iter().zip(fields.iter()) {
        lowered.extend(lower_value_to_flat(
            value, &field.ty, options, program, store,
        )?);
    }
    Ok(lowered)
}

fn lift_record_value(
    fields: &[LabelValType],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let mut values = Vec::with_capacity(fields.len());
    for field in fields {
        values.push((
            field.label.0.clone(),
            lift_value(&field.ty, options, program, store, cursor)?,
        ));
    }
    if is_tuple_fields(fields) {
        Ok(ComponentValue::Tuple(
            values.into_iter().map(|(_, value)| value).collect(),
        ))
    } else {
        Ok(ComponentValue::Record(values))
    }
}

fn lower_variant_value(
    value: &ComponentValue,
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let (case_name, payload) = match value {
        ComponentValue::Variant { case, value } => (case.as_str(), value.as_deref()),
        ComponentValue::Enum(case) => (case.as_str(), None),
        ComponentValue::Option(value) if is_option_cases(cases) => match value {
            Some(value) => ("some", Some(value.as_ref())),
            None => ("none", None),
        },
        ComponentValue::Result { ok, err } if is_result_cases(cases) => match (ok, err) {
            (Some(value), None) => ("ok", Some(value.as_ref())),
            (None, Some(value)) => ("err", Some(value.as_ref())),
            _ => {
                return Err(ComponentError::InvalidArgument(
                    "result value must set exactly one branch".to_owned(),
                ))
            }
        },
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected variant-compatible value, got {other:?}"
            )))
        }
    };
    let case_index = cases
        .iter()
        .position(|case| case.label.0 == case_name)
        .ok_or_else(|| {
            ComponentError::InvalidArgument(format!("unknown variant case '{case_name}'"))
        })?;
    let payload_types = variant_payload_flat_types(cases, program)?;
    let mut lowered = Vec::with_capacity(1 + payload_types.len());
    lowered.push(WasmValue::I32(case_index as i32));
    let case = &cases[case_index];
    let (case_flat, case_flat_types) = if let Some(ty) = &case.ty {
        let payload = payload.ok_or_else(|| {
            ComponentError::InvalidArgument(format!(
                "variant case '{}' expects a payload",
                case.label
            ))
        })?;
        (
            lower_value_to_flat(payload, ty, options, program, store)?,
            flat_types_for_valtype(ty, program)?,
        )
    } else {
        let payloadless_result_case = is_result_cases(cases)
            && matches!(
                payload,
                Some(ComponentValue::Tuple(values)) if values.is_empty()
            );
        if payload.is_some() && !payloadless_result_case {
            return Err(ComponentError::InvalidArgument(format!(
                "variant case '{}' does not accept a payload",
                case.label
            )));
        }
        (Vec::new(), Vec::new())
    };
    for (index, payload_ty) in payload_types.iter().enumerate() {
        if let Some(value) = case_flat.get(index) {
            lowered.push(coerce_flat_value(
                *value,
                case_flat_types[index],
                *payload_ty,
            )?);
        } else {
            lowered.push(zero_wasm_value(*payload_ty));
        }
    }
    Ok(lowered)
}

fn lift_variant_value(
    cases: &[Case],
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let case_index = cursor.next_i32()? as usize;
    let case = cases
        .get(case_index)
        .ok_or_else(|| ComponentError::Trap("variant discriminant is out of bounds".to_owned()))?;
    let payload_types = variant_payload_flat_types(cases, program)?;
    let raw_payload = payload_types
        .iter()
        .map(|ty| cursor.next_for_type(*ty))
        .collect::<Result<Vec<_>, _>>()?;
    let payload = if let Some(ty) = &case.ty {
        let expected_types = flat_types_for_valtype(ty, program)?;
        let values = expected_types
            .iter()
            .enumerate()
            .map(|(index, expected)| {
                coerce_flat_value(raw_payload[index], payload_types[index], *expected)
            })
            .collect::<Result<Vec<_>, _>>()?;
        Some(Box::new(lift_value_from_flat_values(
            ty, options, program, store, &values,
        )?))
    } else {
        None
    };
    if is_option_cases(cases) {
        return Ok(ComponentValue::Option(payload));
    }
    if is_result_cases(cases) {
        let payload = if case.ty.is_none() {
            Some(Box::new(ComponentValue::Tuple(Vec::new())))
        } else {
            payload
        };
        return Ok(match case.label.0.as_str() {
            "ok" => ComponentValue::Result {
                ok: payload,
                err: None,
            },
            "err" => ComponentValue::Result {
                ok: None,
                err: payload,
            },
            _ => unreachable!(),
        });
    }
    if is_enum_cases(cases) {
        return Ok(ComponentValue::Enum(case.label.0.clone()));
    }
    Ok(ComponentValue::Variant {
        case: case.label.0.clone(),
        value: payload,
    })
}

fn lower_flags_value(
    value: &ComponentValue,
    labels: &[crate::ir::Label],
) -> Result<Vec<WasmValue>, ComponentError> {
    let selected = match value {
        ComponentValue::Flags(flags) => flags.as_slice(),
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected flags value, got {other:?}"
            )))
        }
    };
    let mut words = vec![
        0u32;
        if labels.is_empty() {
            0
        } else if labels.len() <= 16 {
            1
        } else {
            labels.len().div_ceil(32)
        }
    ];
    for flag in selected {
        let index = labels
            .iter()
            .position(|label| &label.0 == flag)
            .ok_or_else(|| ComponentError::InvalidArgument(format!("unknown flag '{flag}'")))?;
        words[index / 32] |= 1 << (index % 32);
    }
    Ok(words
        .into_iter()
        .map(|word| WasmValue::I32(word as i32))
        .collect())
}

fn lift_flags_value(
    labels: &[crate::ir::Label],
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let mut selected = Vec::new();
    for chunk_start in (0..labels.len()).step_by(32) {
        let bits = cursor.next_i32()? as u32;
        for bit in 0..32 {
            let index = chunk_start + bit;
            if index >= labels.len() {
                break;
            }
            if bits & (1 << bit) != 0 {
                selected.push(labels[index].0.clone());
            }
        }
    }
    Ok(ComponentValue::Flags(selected))
}

fn lower_list_value(
    value: &ComponentValue,
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
) -> Result<Vec<WasmValue>, ComponentError> {
    let values = match value {
        ComponentValue::List(values) => values,
        other => {
            return Err(ComponentError::InvalidArgument(format!(
                "expected list value, got {other:?}"
            )))
        }
    };
    if let Some(expected) = fixed_len {
        if values.len() != expected {
            return Err(ComponentError::InvalidArgument(format!(
                "expected list length {expected}, got {}",
                values.len()
            )));
        }
    }
    let realloc = options.realloc.as_ref().ok_or_else(|| {
        ComponentError::Runtime("canonical option `realloc` is required".to_owned())
    })?;
    let stride = element_stride(elem, program)?;
    let total_len = stride.saturating_mul(values.len() as u32);
    let ptr = if total_len == 0 {
        0
    } else {
        call_realloc(realloc, store, 0, 0, 4, total_len as i32)? as u32
    };
    for (index, value) in values.iter().enumerate() {
        write_value_to_memory(
            value,
            elem,
            options,
            program,
            store,
            ptr + stride * index as u32,
        )?;
    }
    Ok(vec![
        WasmValue::I32(ptr as i32),
        WasmValue::I32(values.len() as i32),
    ])
}

fn lift_list_value(
    elem: &ValType,
    fixed_len: Option<usize>,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    cursor: &mut CoreValueCursor<'_>,
) -> Result<ComponentValue, ComponentError> {
    let memory = options.memory.clone().ok_or_else(|| {
        ComponentError::Runtime("canonical option `memory` is required".to_owned())
    })?;
    let ptr = cursor.next_i32()? as u32;
    let len = cursor.next_i32()? as usize;
    if let Some(expected) = fixed_len {
        if len != expected {
            return Err(ComponentError::Trap(format!(
                "fixed-length list expected {expected} elements, got {len}"
            )));
        }
    }
    let stride = element_stride(elem, program)?;
    let mut values = Vec::with_capacity(len);
    for index in 0..len {
        values.push(read_value_from_memory(
            elem,
            options,
            program,
            store,
            &memory,
            ptr + stride * index as u32,
        )?);
    }
    Ok(ComponentValue::List(values))
}

fn lift_value_from_flat_values(
    ty: &ValType,
    options: &RuntimeCanonicalOptions,
    program: &ComponentProgram,
    store: &Store,
    values: &[WasmValue],
) -> Result<ComponentValue, ComponentError> {
    let mut cursor = CoreValueCursor::new(values);
    lift_value(ty, options, program, store, &mut cursor)
}

fn variant_payload_flat_types(
    cases: &[Case],
    program: &ComponentProgram,
) -> Result<Vec<CoreValType>, ComponentError> {
    let mut payload = Vec::new();
    for case in cases {
        if let Some(ty) = &case.ty {
            for (index, flat_ty) in flat_types_for_valtype(ty, program)?.into_iter().enumerate() {
                if let Some(current) = payload.get_mut(index) {
                    *current = join_component_flat_types(*current, flat_ty);
                } else {
                    payload.push(flat_ty);
                }
            }
        }
    }
    Ok(payload)
}

fn zero_wasm_value(ty: CoreValType) -> WasmValue {
    match ty {
        CoreValType::I32 => WasmValue::I32(0),
        CoreValType::I64 => WasmValue::I64(0),
        CoreValType::F32 => WasmValue::F32(0.0),
        CoreValType::F64 => WasmValue::F64(0.0),
        CoreValType::FuncRef => WasmValue::FuncRef(0),
        CoreValType::ExternRef => WasmValue::ExternRef(0),
        CoreValType::V128 => WasmValue::V128(0),
    }
}

fn coerce_flat_value(
    value: WasmValue,
    from: CoreValType,
    to: CoreValType,
) -> Result<WasmValue, ComponentError> {
    if from == to {
        return Ok(value);
    }
    Ok(match (value, from, to) {
        (WasmValue::I32(value), CoreValType::I32, CoreValType::I64) => {
            WasmValue::I64((value as u32 as u64) as i64)
        }
        (WasmValue::F32(value), CoreValType::F32, CoreValType::I32) => {
            WasmValue::I32(value.to_bits() as i32)
        }
        (WasmValue::I32(value), CoreValType::I32, CoreValType::F32) => {
            WasmValue::F32(f32::from_bits(value as u32))
        }
        (WasmValue::F32(value), CoreValType::F32, CoreValType::I64) => {
            WasmValue::I64(value.to_bits() as u64 as i64)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::I32) => {
            WasmValue::I32(value as u32 as i32)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::F32) => {
            WasmValue::F32(f32::from_bits(value as u32))
        }
        (WasmValue::F64(value), CoreValType::F64, CoreValType::I64) => {
            WasmValue::I64(value.to_bits() as i64)
        }
        (WasmValue::I64(value), CoreValType::I64, CoreValType::F64) => {
            WasmValue::F64(f64::from_bits(value as u64))
        }
        (other, _, _) => {
            return Err(ComponentError::Trap(format!(
                "cannot coerce canonical value {other:?} from {from:?} to {to:?}"
            )))
        }
    })
}

fn is_tuple_fields(fields: &[LabelValType]) -> bool {
    !fields.is_empty()
        && fields
            .iter()
            .enumerate()
            .all(|(index, field)| field.label.0 == index.to_string())
}

fn is_option_cases(cases: &[Case]) -> bool {
    matches!(
        cases,
        [Case { label, ty: None }, Case { label: some, ty: Some(_) }]
            if label.0 == "none" && some.0 == "some"
    )
}

fn is_result_cases(cases: &[Case]) -> bool {
    matches!(
        cases,
        [Case { label: ok, .. }, Case { label: err, .. }] if ok.0 == "ok" && err.0 == "err"
    )
}

fn is_enum_cases(cases: &[Case]) -> bool {
    !cases.is_empty() && cases.iter().all(|case| case.ty.is_none())
}

fn read_string_from_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: u32,
    encoding: Option<CanonicalStringEncoding>,
) -> Result<String, ComponentError> {
    let encoding = encoding.unwrap_or(CanonicalStringEncoding::Utf8);
    let bytes = read_memory(
        store,
        memory,
        ptr,
        match encoding {
            CanonicalStringEncoding::Utf8 => len as usize,
            CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
                len as usize * 2
            }
        },
    )?;
    match encoding {
        CanonicalStringEncoding::Utf8 => {
            String::from_utf8(bytes).map_err(|error| ComponentError::Trap(error.to_string()))
        }
        CanonicalStringEncoding::Utf16 | CanonicalStringEncoding::CompactUtf16 => {
            let mut units = Vec::with_capacity(bytes.len() / 2);
            for chunk in bytes.chunks_exact(2) {
                units.push(u16::from_le_bytes([chunk[0], chunk[1]]));
            }
            String::from_utf16(&units).map_err(|error| ComponentError::Trap(error.to_string()))
        }
    }
}

fn read_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    len: usize,
) -> Result<Vec<u8>, ComponentError> {
    let memory = memory_addr(store, memory)?;
    crate::support::common::read_memory(store, &memory, ptr, len)
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))
}

fn read_memory_array<const N: usize>(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
) -> Result<[u8; N], ComponentError> {
    let memory = memory_addr(store, memory)?;
    crate::support::common::read_memory_array::<N>(store, &memory, ptr)
        .ok_or_else(|| ComponentError::Trap("memory access out of bounds".to_owned()))
}

pub(super) fn write_memory(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    bytes: &[u8],
) -> Result<(), ComponentError> {
    let memory = memory_addr(store, memory)?;
    if crate::support::common::write_memory(store, &memory, ptr, bytes) {
        Ok(())
    } else {
        Err(ComponentError::Trap(
            "memory access out of bounds".to_owned(),
        ))
    }
}

fn write_flat_values(
    store: &Store,
    memory: &CoreExportRef,
    ptr: u32,
    values: &[WasmValue],
) -> Result<(), ComponentError> {
    let mut cursor = ptr;
    for value in values {
        match value {
            WasmValue::I32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::I64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::F32(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::F64(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 8;
            }
            WasmValue::FuncRef(v) | WasmValue::ExternRef(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 4;
            }
            WasmValue::V128(v) => {
                write_memory(store, memory, cursor, &v.to_le_bytes())?;
                cursor += 16;
            }
        }
    }
    Ok(())
}

fn memory_addr(
    store: &Store,
    memory: &CoreExportRef,
) -> Result<crate::support::common::CoreMemoryHandle, ComponentError> {
    crate::support::common::memory_export(&memory.instance, store, &memory.export_name)
        .map_err(ComponentError::Link)
}

fn call_realloc(
    realloc: &RuntimeCoreFunc,
    store: &Store,
    old_ptr: i32,
    old_len: i32,
    align: i32,
    new_len: i32,
) -> Result<i32, ComponentError> {
    let result = realloc.call_sync(
        store,
        &[
            WasmValue::I32(old_ptr),
            WasmValue::I32(old_len),
            WasmValue::I32(align),
            WasmValue::I32(new_len),
        ],
    )?;
    match result.as_slice() {
        [WasmValue::I32(ptr)] => Ok(*ptr),
        _ => Err(ComponentError::Runtime(
            "realloc returned an unexpected result".to_owned(),
        )),
    }
}

pub(super) fn program_func_type(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<FuncType, ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Func(func_type)) => Ok(func_type.clone()),
        _ => Err(ComponentError::Runtime(
            "function type is missing".to_owned(),
        )),
    }
}

fn validate_resource_type(
    program: &ComponentProgram,
    type_id: TypeId,
) -> Result<(), ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Resource(_)) => Ok(()),
        Some(Type::DefVal(DefValType::Own(inner)))
        | Some(Type::DefVal(DefValType::Borrow(inner))) => validate_resource_type(program, *inner),
        Some(Type::Generic(generic)) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => validate_resource_type(program, inner),
            crate::ir::types::GenericBound::Sub => Ok(()),
        },
        _ => Err(ComponentError::Runtime(
            "resource type is missing".to_owned(),
        )),
    }
}

pub(super) fn runtime_resource_id(
    program: &ComponentProgram,
    shared: &SharedState,
    type_id: TypeId,
) -> Result<ResourceId, ComponentError> {
    match program.get_type(type_id) {
        Some(Type::Resource(resource)) => Ok(*resource),
        Some(Type::DefVal(DefValType::Own(inner)))
        | Some(Type::DefVal(DefValType::Borrow(inner))) => {
            runtime_resource_id(program, shared, *inner)
        }
        Some(Type::Generic(generic)) => match generic.bound {
            crate::ir::types::GenericBound::Eq(inner) => {
                runtime_resource_id(program, shared, inner)
            }
            crate::ir::types::GenericBound::Sub => {
                let mut resources = shared.generic_resources.borrow_mut();
                Ok(*resources
                    .entry(type_id)
                    .or_insert_with(ResourceId::synthetic))
            }
        },
        _ => Err(ComponentError::Runtime(
            "resource type is missing".to_owned(),
        )),
    }
}

fn value_flat_len(ty: &ValType, program: &ComponentProgram) -> Result<usize, ComponentError> {
    match ty {
        ValType::Primitive(prim) => Ok(flat_types_for_primitive(prim).len()),
        ValType::Type(type_id) => program
            .get_type_info(*type_id)
            .map(|info| info.flat_len)
            .ok_or_else(|| ComponentError::Runtime("type id not found".to_owned())),
    }
}

struct CoreValueCursor<'a> {
    values: &'a [WasmValue],
    offset: usize,
}

impl<'a> CoreValueCursor<'a> {
    fn new(values: &'a [WasmValue]) -> Self {
        Self { values, offset: 0 }
    }

    fn next(&mut self) -> Result<WasmValue, ComponentError> {
        let value = *self
            .values
            .get(self.offset)
            .ok_or_else(|| ComponentError::Trap("canonical ABI value underflow".to_owned()))?;
        self.offset += 1;
        Ok(value)
    }

    fn next_i32(&mut self) -> Result<i32, ComponentError> {
        match self.next()? {
            WasmValue::I32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i32, got {other:?}"))),
        }
    }

    fn next_i64(&mut self) -> Result<i64, ComponentError> {
        match self.next()? {
            WasmValue::I64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected i64, got {other:?}"))),
        }
    }

    fn next_f32(&mut self) -> Result<f32, ComponentError> {
        match self.next()? {
            WasmValue::F32(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f32, got {other:?}"))),
        }
    }

    fn next_f64(&mut self) -> Result<f64, ComponentError> {
        match self.next()? {
            WasmValue::F64(v) => Ok(v),
            other => Err(ComponentError::Trap(format!("expected f64, got {other:?}"))),
        }
    }

    fn next_for_type(&mut self, ty: CoreValType) -> Result<WasmValue, ComponentError> {
        match ty {
            CoreValType::I32 => self.next().and_then(|value| match value {
                WasmValue::I32(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected i32, got {other:?}"))),
            }),
            CoreValType::I64 => self.next().and_then(|value| match value {
                WasmValue::I64(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected i64, got {other:?}"))),
            }),
            CoreValType::F32 => self.next().and_then(|value| match value {
                WasmValue::F32(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected f32, got {other:?}"))),
            }),
            CoreValType::F64 => self.next().and_then(|value| match value {
                WasmValue::F64(_) => Ok(value),
                other => Err(ComponentError::Trap(format!("expected f64, got {other:?}"))),
            }),
            other => Err(ComponentError::Unsupported(format!(
                "core cursor does not support {other:?}"
            ))),
        }
    }
}

fn expect_bool(value: &ComponentValue) -> Result<bool, ComponentError> {
    value
        .as_bool()
        .ok_or_else(|| ComponentError::InvalidArgument("expected bool".to_owned()))
}

fn expect_u32(value: &ComponentValue) -> Result<u32, ComponentError> {
    value
        .as_u32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected u32".to_owned()))
}

fn expect_i32(value: &ComponentValue) -> Result<i32, ComponentError> {
    value
        .as_i32()
        .ok_or_else(|| ComponentError::InvalidArgument("expected i32".to_owned()))
}

fn expect_i64(value: &ComponentValue) -> Result<i64, ComponentError> {
    match value {
        ComponentValue::I64(v) | ComponentValue::S64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected i64".to_owned())),
    }
}

fn expect_u64(value: &ComponentValue) -> Result<u64, ComponentError> {
    match value {
        ComponentValue::U64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected u64".to_owned())),
    }
}

fn expect_f32(value: &ComponentValue) -> Result<f32, ComponentError> {
    match value {
        ComponentValue::F32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f32".to_owned())),
    }
}

fn expect_f64(value: &ComponentValue) -> Result<f64, ComponentError> {
    match value {
        ComponentValue::F64(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected f64".to_owned())),
    }
}

fn expect_char(value: &ComponentValue) -> Result<char, ComponentError> {
    match value {
        ComponentValue::Char(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument("expected char".to_owned())),
    }
}

fn expect_handle(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Own(v) | ComponentValue::Borrow(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected resource handle".to_owned(),
        )),
    }
}

#[cfg(feature = "component-gated-feature-async")]
fn expect_error_context(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::ErrorContext(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected error-context handle".to_owned(),
        )),
    }
}

#[cfg(feature = "component-gated-feature-async")]
fn expect_future(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Future(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected future handle".to_owned(),
        )),
    }
}

#[cfg(feature = "component-gated-feature-async")]
fn expect_stream(value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Stream(v) | ComponentValue::U32(v) => Ok(*v),
        _ => Err(ComponentError::InvalidArgument(
            "expected stream handle".to_owned(),
        )),
    }
}
