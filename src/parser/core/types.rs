use crate::binary::BinaryReader;
use crate::common::{
    FuncType, GlobalType, Limits, MemType, Mut, RefType, ResultType, TableType, ValType,
};
use crate::parser::core::{parse_u32, parse_vec};
use crate::WasmParserError;
use tracing::trace;

pub type Result<R> = std::result::Result<R, WasmParserError>;

pub fn parse_valtype<R: BinaryReader>(reader: &mut R) -> Result<(usize, ValType)> {
    let v = reader.read_exact_one()?;
    let ty = match v {
        0x7f => ValType::I32,
        0x7e => ValType::I64,
        0x7d => ValType::F32,
        0x7c => ValType::F64,
        0x7b => ValType::V128,
        0x70 => ValType::FuncRef,
        0x6f => ValType::ExternRef,
        unknown => Err(WasmParserError::InvalidValueType(unknown))?,
    };
    Ok((1, ty))
}

pub fn parse_result_type<R: BinaryReader>(reader: &mut R) -> Result<(usize, ResultType)> {
    let (len, v) = parse_vec(reader, |v| v, parse_valtype)?;
    Ok((len, ResultType(v)))
}

pub fn parse_functype<R: BinaryReader>(reader: &mut R) -> Result<(usize, FuncType)> {
    let mut read_bytes = 0;
    let signature = reader.read_exact_one()?;
    trace!("parse_functype: {signature}");
    read_bytes += 1;
    if signature != 0x60 {
        Err(WasmParserError::InvalidFunctionTypeSignature(signature))?
    }
    let (len, input) = parse_result_type(reader)?;
    trace!("parse_functype: {len} {input:?}");

    read_bytes += len;
    let (len, output) = parse_result_type(reader)?;
    read_bytes += len;
    trace!("parse_functype: {len} {output:?}");

    Ok((read_bytes, FuncType(input, output)))
}

pub fn parse_ref_type<R: BinaryReader>(reader: &mut R) -> Result<(usize, RefType)> {
    let v = reader.read_exact_one()?;
    Ok((
        1,
        match v {
            0x70 => RefType::FuncRef,
            0x6f => RefType::ExternRef,
            unknown => Err(WasmParserError::InvalidValueType(unknown))?,
        },
    ))
}

pub fn parse_table_type<R: BinaryReader>(reader: &mut R) -> Result<(usize, TableType)> {
    let (len, reftype) = parse_ref_type(reader)?;
    let (len2, limits) = parse_limits(reader)?;
    Ok((len + len2, TableType { reftype, limits }))
}

pub fn parse_limits<R: BinaryReader>(reader: &mut R) -> Result<(usize, Limits)> {
    match reader.read_exact_one()? {
        0x00 => {
            let (len, min) = parse_u32(reader)?;
            Ok((1 + len, Limits { min, max: None }))
        }
        0x01 => {
            let (len, min) = parse_u32(reader)?;
            let (len2, max) = parse_u32(reader)?;
            if min > max {
                Err(WasmParserError::InvalidLimit)?
            }
            Ok((
                1 + len + len2,
                Limits {
                    min,
                    max: Some(max),
                },
            ))
        }
        _ => todo!(),
    }
}

pub fn parse_global_type<R: BinaryReader>(reader: &mut R) -> Result<(usize, GlobalType)> {
    let (len, vt) = parse_valtype(reader)?;
    let m = match reader.read_exact_one()? {
        0x00 => Mut::Const,
        0x01 => Mut::Var,
        unknown => Err(WasmParserError::InvalidMut(unknown))?,
    };
    Ok((1 + len, GlobalType(vt, m)))
}

pub fn parse_memtype<R: BinaryReader>(reader: &mut R) -> Result<(usize, MemType)> {
    let (len, limits) = parse_limits(reader)?;

    if limits.min > 65536 {
        Err(WasmParserError::InvalidMemorySize(limits))?
    }
    if limits.max.map(|max| max > 65536).unwrap_or_else(|| false) {
        Err(WasmParserError::InvalidMemorySize(limits))?
    }
    Ok((len, MemType(limits)))
}
