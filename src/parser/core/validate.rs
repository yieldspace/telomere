use crate::common::Locals;
use crate::common::MemType;
use crate::common::RefType;
use crate::common::TableType;
use crate::common::ValType;

use super::Result;
use super::WasmParserError;

pub(crate) fn assert_valtype(expected: ValType, actual: Option<ValType>) -> Result<()> {
    if let Some(actual) = actual {
        if expected == actual {
            Ok(())
        } else {
            Err(WasmParserError::InvalidStackValType(expected, Some(actual)))
        }
    } else {
        Err(WasmParserError::InvalidStackValType(expected, actual))
    }
}

pub(crate) fn assert_memory(sec: &[MemType]) -> Result<()> {
    if sec.is_empty() {
        Err(WasmParserError::InvalidMemIdx(0))?;
    }
    Ok(())
}
pub(crate) fn validate_active_elem(
    tables: &[TableType],
    table_idx: u32,
    rt: RefType,
) -> Result<()> {
    let tt = tables
        .get(table_idx as usize)
        .ok_or(WasmParserError::InvalidTableIndex(table_idx))?;
    assert_valtype(tt.reftype.into(), Some(rt.into()))?;
    Ok(())
}
pub(crate) fn validate_locals(locals: &[Locals]) -> Result<()> {
    let mut count = 0u32;
    for local in locals {
        count = count
            .checked_add(local.n)
            .ok_or_else(|| WasmParserError::TooManyLocals)?;
    }
    Ok(())
}
