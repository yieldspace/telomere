use std::collections::HashSet;
use tracing::trace;

use crate::common::custom_section::NameSubSection;
use crate::common::{ConstExpr, ElemInit, Func, FunctionBody, Instr, Locals, LocalsData, Operand};
use crate::parser::core::instruction_generator::InstructionGenerator;
use crate::parser::core::jump_resolver::{JumpResolver, JumpResolverDSL};
use crate::parser::core::optimizer;
use crate::parser::core::type_checker::TypeChecker;
use crate::parser::core::validate::validate_locals;
use crate::parser::core::InstructionParser;
use crate::runtime::vm;
use crate::{
    binary::BinaryReader,
    common::{
        CodeSection, Data, DataCountVerifier, DataMode, DataSection, Elem, ElemMode,
        ElementSection, Export, ExportDesc, ExportSection, FuncIdx, FuncType, FunctionSection,
        Global, GlobalIdx, GlobalType, Import, ImportDesc, ImportSection, MemIdx, MemType, Mut,
        RefType, ResultType, Table, TableIdx, TableType, TypeIdx, TypeSection, ValType,
    },
    Module,
};

use super::base::WasmBaseParser;
use super::custom_section::CustomSectionParser;
#[cfg(feature = "simd")]
use super::simd_instruction::v128_const;
use super::validate::{assert_memory, assert_valtype, validate_active_elem};
use super::{Result, WasmParserError};

fn validate_table(tables: &[TableType], idx: u32) -> Result<()> {
    if idx as usize >= tables.len() {
        return Err(WasmParserError::InvalidTableIndex(idx));
    }
    Ok(())
}

fn validate_const_expr_type(
    globals: &[GlobalType],
    funcs: &[TypeIdx],
    exprs: &[ConstExpr],
    expected: ValType,
) -> Result<()> {
    if exprs.len() != 1 {
        Err(WasmParserError::InvalidStackValTypeAny)?;
    }
    match exprs[0] {
        ConstExpr::I32(_) => assert_valtype(expected, Some(ValType::I32))?,
        ConstExpr::I64(_) => assert_valtype(expected, Some(ValType::I64))?,
        ConstExpr::GlobalGet(idx) => {
            let gt = globals
                .get(idx as usize)
                .ok_or(WasmParserError::UnknownGlobal)?;
            if gt.1 != Mut::Const {
                Err(WasmParserError::InvalidGlobalAccess)?;
            }
            assert_valtype(expected, Some(gt.0))?;
        }
        ConstExpr::RefNull(t) => assert_valtype(expected, Some(t.into()))?,
        ConstExpr::F32(_) => assert_valtype(expected, Some(ValType::F32))?,
        ConstExpr::F64(_) => assert_valtype(expected, Some(ValType::F64))?,
        ConstExpr::V128(_) => assert_valtype(expected, Some(ValType::V128))?,

        ConstExpr::FuncRef(idx) => {
            if funcs.get(idx as usize).is_none() {
                return Err(WasmParserError::InvalidFuncIdx(FuncIdx(idx)));
            }
            assert_valtype(expected, Some(ValType::FuncRef))?
        }
    }

    Ok(())
}
fn validate_offset_const_expr(globals: &[GlobalType], exprs: &[ConstExpr]) -> Result<()> {
    validate_const_expr_type(globals, &[], exprs, ValType::I32)
}

#[repr(u8)]
#[derive(Debug, PartialEq, Eq)]
enum WasmSectionType {
    Custom = 0,
    Type = 1,
    Import = 2,
    Function = 3,
    Table = 4,
    Memory = 5,
    Global = 6,
    Export = 7,
    Start = 8,
    Element = 9,
    Code = 10,
    Data = 11,
    DataCount = 12,
    Unknown(u8),
}
const SECTION_ORDER: [WasmSectionType; 12] = {
    use WasmSectionType::*;
    [
        Type, Import, Function, Table, Memory, Global, Export, Start, Element, DataCount, Code,
        Data,
    ]
};
enum NameData {
    NameSection(NameSubSection),
    Unknown(String),
}
pub struct WasmParser<'a, R: BinaryReader> {
    reader: &'a mut R,
}

impl<R: BinaryReader> WasmBaseParser<R> for WasmParser<'_, R> {
    fn reader(&mut self) -> &mut R {
        self.reader
    }
}

impl<'a, R: BinaryReader> WasmParser<'a, R> {
    fn parse_typeidx(&mut self) -> Result<(usize, TypeIdx)> {
        let (len, v) = self.parse_u32()?;
        Ok((len, TypeIdx(v)))
    }

    fn parse_import_desc(&mut self, type_section: &TypeSection) -> Result<(usize, ImportDesc)> {
        let ty = self.reader.read_exact_one()?;
        Ok(match ty {
            0x00 => {
                let (len, idx) = self.parse_u32()?;
                type_section
                    .get(TypeIdx(idx))
                    .ok_or(WasmParserError::InvalidTypeIdx(TypeIdx(idx)))?;
                (1 + len, ImportDesc::TypeIdx(TypeIdx(idx)))
            }
            0x01 => {
                let (len, tt) = self.parse_table_type()?;
                (1 + len, ImportDesc::TableType(tt))
            }
            0x02 => {
                let (len, mem) = self.parse_memtype()?;
                (1 + len, ImportDesc::MemType(mem))
            }
            0x03 => {
                let (len, gt) = self.parse_global_type()?;
                (1 + len, ImportDesc::GlobalType(gt))
            }
            unknown => Err(WasmParserError::InvalidImportDesc(unknown))?,
        })
    }
    fn parse_import(&mut self, type_section: &TypeSection) -> Result<(usize, Import)> {
        let (len, module) = self.parse_name()?;
        let (len2, name) = self.parse_name()?;
        let (len3, desc) = self.parse_import_desc(type_section)?;
        Ok((len + len2 + len3, Import { desc, module, name }))
    }
    fn skip_section(&mut self, size: u32) -> Result<()> {
        for _idx in 0..size {
            self.reader.read_exact_one()?;
        }
        Ok(())
    }
    fn parse_exportdesc(
        &mut self,
        functions: &[TypeIdx],
        globals: &[GlobalType],
        tables: &[TableType],
        mems: &[MemType],
    ) -> Result<(usize, ExportDesc)> {
        let mut read_bytes = 0;
        let (len, ty) = self.parse_byte()?;
        read_bytes += len;
        let (len, idx) = self.parse_u32()?;
        let desc = match ty {
            0x00 => {
                if functions.get(idx as usize).is_none() {
                    return Err(WasmParserError::UnknownExport);
                }
                ExportDesc::Func(FuncIdx(idx))
            }
            0x01 => {
                if tables.get(idx as usize).is_none() {
                    return Err(WasmParserError::UnknownExport);
                }
                ExportDesc::Table(TableIdx(idx))
            }
            0x02 => {
                if mems.get(idx as usize).is_none() {
                    return Err(WasmParserError::UnknownExport);
                }
                ExportDesc::Mem(MemIdx(idx))
            }
            0x03 => {
                if globals.get(idx as usize).is_none() {
                    return Err(WasmParserError::UnknownExport);
                }
                ExportDesc::Global(GlobalIdx(idx))
            }
            unknown => Err(WasmParserError::InvalidExportDesc(unknown))?,
        };
        read_bytes += len;
        Ok((read_bytes, desc))
    }

    fn parse_table(&mut self) -> Result<(usize, Table)> {
        let (len, tt) = self.parse_table_type()?;
        Ok((len, Table(tt)))
    }

    fn parse_const_expr(&mut self) -> Result<(usize, Vec<ConstExpr>)> {
        let mut total_len = 0;
        let mut values = vec![];
        loop {
            let v = self.reader.read_exact_one()?;
            let (len, value) = match v {
                0x0B => return Ok((1 + total_len, values)),
                0x23 => {
                    let (len, operand) = self.parse_u32()?;
                    (1 + len, ConstExpr::GlobalGet(operand))
                }
                0x41 => {
                    let (len, operand) = self.parse_i32()?;
                    (1 + len, ConstExpr::I32(operand))
                }
                0x42 => {
                    let (len, operand) = self.parse_i64()?;
                    (1 + len, ConstExpr::I64(operand))
                }
                0x43 => {
                    let (len, operand) = self.parse_f32()?;
                    (1 + len, ConstExpr::F32(operand))
                }
                0x44 => {
                    let (len, operand) = self.parse_f64()?;
                    (1 + len, ConstExpr::F64(operand))
                }
                0xD0 => {
                    let (len, t) = self.parse_reftype()?;
                    (1 + len, ConstExpr::RefNull(t))
                }
                0xD2 => {
                    let (len, idx) = self.parse_u32()?;
                    (1 + len, ConstExpr::FuncRef(idx))
                }
                0xFD => {
                    #[cfg(not(feature = "simd"))]
                    {
                        Err(WasmParserError::unsupported_feature(
                            super::ProposalFeature::Simd,
                            [0xFD, 0, 0, 0],
                        ))?
                    }
                    #[cfg(feature = "simd")]
                    {
                        let (len, code) = self.parse_u32()?;
                        if code == v128_const::CODE {
                            let v = self.reader.read_exact::<16>()?;
                            (1 + len + 16, ConstExpr::V128(u128::from_le_bytes(v)))
                        } else {
                            Err(WasmParserError::InvalidConstInstruction(0xFD))?
                        }
                    }
                }
                unknown => Err(WasmParserError::InvalidConstInstruction(unknown))?,
            };
            total_len += len;
            values.push(value);
        }
    }

    fn parse_global(
        &mut self,
        globals: &[GlobalType],
        funcs: &[TypeIdx],
    ) -> Result<(usize, Global)> {
        let (len, gt) = self.parse_global_type()?;
        let (len2, init) = self.parse_const_expr()?;
        validate_const_expr_type(globals, funcs, &init, gt.0)?;
        tracing::trace!("parse_global: {init:?}");
        Ok((len + len2, Global(gt, init)))
    }

    fn parse_export(
        &mut self,
        functions: &[TypeIdx],
        globals: &[GlobalType],
        tables: &[TableType],
        mems: &[MemType],
    ) -> Result<(usize, Export)> {
        let mut read_bytes = 0;
        let (len, name) = self.parse_name()?;
        read_bytes += len;
        let (len, desc) = self.parse_exportdesc(functions, globals, tables, mems)?;
        read_bytes += len;
        Ok((read_bytes, Export(name, desc)))
    }

    fn parse_elem(
        &mut self,
        globals: &[GlobalType],
        funcs: &[TypeIdx],
        tables: &[TableType],
    ) -> Result<(usize, Elem)> {
        let (len, kind) = self.parse_u32()?;
        let r = match kind {
            0 => {
                validate_table(tables, 0)?;
                let (len2, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if funcs.get(*funcidx as usize).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                validate_active_elem(tables, 0, RefType::FuncRef)?;
                (
                    len + len2 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: ElemInit::FuncIdx(funcidx.to_vec()),
                        mode: ElemMode::Active(TableIdx(0), offset),
                    },
                )
            }
            1 => {
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if funcs.get(*funcidx as usize).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + 1 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: ElemInit::FuncIdx(funcidx.to_vec()),
                        mode: ElemMode::Passive,
                    },
                )
            }
            2 => {
                let (len2, tableidx) = self.parse_u32()?;
                validate_table(tables, tableidx)?;
                let (len3, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len5, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if funcs.get(*funcidx as usize).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                validate_active_elem(tables, tableidx, RefType::FuncRef)?;

                (
                    len + len2 + len3 + 1 + len5,
                    Elem {
                        kind: RefType::FuncRef,
                        init: ElemInit::FuncIdx(funcidx.to_vec()),
                        mode: ElemMode::Active(TableIdx(tableidx), offset),
                    },
                )
            }
            3 => {
                let elemkind = self.reader.read_exact_one()?;
                if elemkind != 0x00 {
                    Err(WasmParserError::InvalidElemKind(elemkind))?
                }
                let (len3, funcidx) = self.parse_vec(Self::parse_u32)?;
                for funcidx in &funcidx {
                    if funcs.get(*funcidx as usize).is_none() {
                        Err(WasmParserError::InvalidFuncIdx(FuncIdx(*funcidx)))?;
                    }
                }
                (
                    len + 1 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: ElemInit::FuncIdx(funcidx.to_vec()),
                        mode: ElemMode::Declarative,
                    },
                )
            }
            4 => {
                validate_table(tables, 0)?;
                let (len2, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;
                let (len3, init) = self.parse_vec(Self::parse_const_expr)?;
                for expr in &init {
                    validate_const_expr_type(globals, funcs, expr, ValType::FuncRef)?;
                }
                validate_active_elem(tables, 0, RefType::FuncRef)?;

                (
                    len + len2 + len3,
                    Elem {
                        kind: RefType::FuncRef,
                        init: ElemInit::ConstExpr(init),
                        mode: ElemMode::Active(TableIdx(0), offset),
                    },
                )
            }
            5 => {
                let (len2, rt) = self.parse_reftype()?;
                let (len3, init) = self.parse_vec(Self::parse_const_expr)?;
                for expr in &init {
                    validate_const_expr_type(globals, funcs, expr, rt.into())?;
                }
                (
                    len + len2 + len3,
                    Elem {
                        kind: rt,
                        init: ElemInit::ConstExpr(init),
                        mode: ElemMode::Passive,
                    },
                )
            }
            6 => {
                let (len2, tableidx) = self.parse_u32()?;
                validate_table(tables, tableidx)?;
                let (len3, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;
                let (len4, rt) = self.parse_reftype()?;
                let (len5, init) = self.parse_vec(Self::parse_const_expr)?;
                for expr in &init {
                    validate_const_expr_type(globals, funcs, expr, rt.into())?;
                }
                validate_active_elem(tables, 0, rt)?;

                (
                    len + len2 + len3 + len4 + len5,
                    Elem {
                        kind: rt,
                        init: ElemInit::ConstExpr(init),
                        mode: ElemMode::Active(TableIdx(tableidx), offset),
                    },
                )
            }
            7 => {
                let (len2, rt) = self.parse_reftype()?;
                let (len3, init) = self.parse_vec(Self::parse_const_expr)?;
                for expr in &init {
                    validate_const_expr_type(globals, funcs, expr, rt.into())?;
                }
                (
                    len + len2 + len3,
                    Elem {
                        kind: rt,
                        init: ElemInit::ConstExpr(init),
                        mode: ElemMode::Declarative,
                    },
                )
            }
            unknown => Err(WasmParserError::InvalidElementSectionType(unknown))?,
        };
        Ok(r)
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_code_section(
        &mut self,
        type_section: &TypeSection,
        functions: &[TypeIdx],
        imports: &ImportSection,
        globals: &[GlobalType],
        tables: &[TableType],
        mems: &[MemType],
        elems: &[Elem],
        data_count_section: &mut DataCountVerifier,
        size: u32,
    ) -> Result<CodeSection> {
        let mut idx = 0;
        let mut icode = vec![];
        for import in &imports.0 {
            if let ImportDesc::TypeIdx(_tidx) = import.desc {
                idx += 1;
            }
        }
        let imported_function_len = idx;

        let (len, mut codes) = self.parse_vec(|me| {
            let (len, func) = me.parse_code(
                FuncIdx(idx),
                type_section,
                functions,
                imported_function_len,
                mems,
                globals,
                tables,
                elems,
                data_count_section,
            )?;

            idx += 1;
            Ok((len, FunctionBody::Wasm(func)))
        })?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        icode.append(&mut codes);
        Ok(CodeSection(icode))
    }
    fn parse_data(&mut self, globals: &[GlobalType], mems: &[MemType]) -> Result<(usize, Data)> {
        let (len, kind) = self.parse_u32()?;
        match kind {
            0x00 => {
                assert_memory(mems)?;
                let (len2, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;

                let (len3, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2 + len3,
                    Data {
                        init: bytes,
                        mode: DataMode::Active(MemIdx(0), offset),
                    },
                ))
            }
            0x01 => {
                let (len2, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2,
                    Data {
                        init: bytes,
                        mode: DataMode::Passive,
                    },
                ))
            }
            0x02 => {
                let (len2, memidx) = self.parse_u32()?;
                if mems.len() as u32 <= memidx {
                    Err(WasmParserError::InvalidMemIdx(memidx))?;
                }
                let (len3, offset) = self.parse_const_expr()?;
                validate_offset_const_expr(globals, &offset)?;

                let (len4, bytes) = self.parse_vec(Self::parse_byte)?;
                Ok((
                    len + len2 + len3 + len4,
                    Data {
                        init: bytes,
                        mode: DataMode::Active(MemIdx(memidx), offset),
                    },
                ))
            }
            unknown => Err(WasmParserError::InvalidDataKind(unknown)),
        }
    }
    fn parse_data_section(
        &mut self,
        globals: &[GlobalType],
        mems: &[MemType],
        size: u32,
    ) -> Result<DataSection> {
        let (len, d) = self.parse_vec(|me| me.parse_data(globals, mems))?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(DataSection(d))
    }
    fn parse_section_body<V>(
        &mut self,
        mut f: impl FnMut(&mut Self, u32) -> Result<V>,
    ) -> Result<V> {
        let (_len, size) = self.parse_u32()?;
        f(self, size)
    }
    fn parse_magic(&mut self) -> Result<()> {
        let magic = self.reader.read_exact::<4>()?;
        if matches!(&magic, &[0x00, 0x61, 0x73, 0x6d]) {
            Ok(())
        } else {
            Err(WasmParserError::InvalidMagic(magic))
        }
    }
    fn parse_version(&mut self) -> Result<()> {
        let version = self.reader.read_exact::<4>()?;
        if matches!(&version, &[0x01, 0x00, 0x00, 0x00]) {
            Ok(())
        } else {
            Err(WasmParserError::InvalidVersion(version))
        }
    }
    fn parse_section_type(&mut self) -> Result<Option<WasmSectionType>> {
        let kind = self.reader.read_one()?;
        let kind = if let Some(kind) = kind {
            kind
        } else {
            return Ok(None);
        };
        trace!("parse_section_type: {kind}");
        use WasmSectionType::*;
        Ok(Some(match kind {
            0 => Custom,
            1 => Type,
            2 => Import,
            3 => Function,
            4 => Table,
            5 => Memory,
            6 => Global,
            7 => Export,
            8 => Start,
            9 => Element,
            10 => Code,
            11 => Data,
            12 => DataCount,
            other => Unknown(other),
        }))
    }

    fn parse_type_section(&mut self, size: u32) -> Result<TypeSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_functype)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(TypeSection(funcs))
    }
    fn parse_namedata(&mut self, size: u32) -> Result<NameData> {
        let (len1, name) = self.parse_name()?;
        if name == "name" {
            let mut child_reader = self.reader.take(size.saturating_sub(len1 as u32) as usize);

            let subsec = CustomSectionParser::new(&mut child_reader).parse_name_subsec()?;
            Ok(NameData::NameSection(subsec))
        } else {
            self.skip_section(size.saturating_sub(len1 as u32))?;
            Ok(NameData::Unknown(name))
        }
    }
    fn parse_import_section(
        &mut self,
        type_section: &TypeSection,
        size: u32,
    ) -> Result<ImportSection> {
        let (len, imports) = self.parse_vec(|me| me.parse_import(type_section))?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ImportSection(imports))
    }
    fn parse_function_section(&mut self, size: u32) -> Result<FunctionSection> {
        let (len, funcs) = self.parse_vec(&Self::parse_typeidx)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(FunctionSection(funcs))
    }
    fn parse_table_section(&mut self, size: u32) -> Result<Vec<Table>> {
        let (len, tables) = self.parse_vec(&Self::parse_table)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(tables)
    }
    fn parse_global_section(
        &mut self,
        globals: &[GlobalType],
        funcs: &[TypeIdx],
        size: u32,
    ) -> Result<Vec<Global>> {
        let (len, globals) = self.parse_vec(|me| me.parse_global(globals, funcs))?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(globals)
    }
    fn parse_memory_section(&mut self, size: u32) -> Result<Vec<MemType>> {
        let (len, memories) = self.parse_vec(&Self::parse_memtype)?;
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(memories)
    }
    fn parse_export_section(
        &mut self,
        functions: &[TypeIdx],
        globals: &[GlobalType],
        tables: &[TableType],
        mems: &[MemType],
        size: u32,
    ) -> Result<ExportSection> {
        let (len, exports) =
            self.parse_vec(|me| me.parse_export(functions, globals, tables, mems))?;
        let mut set = HashSet::new();
        for export in &exports {
            if !set.insert(&export.0) {
                Err(WasmParserError::DuplicatedExport(export.0.clone()))?
            }
        }
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ExportSection(exports))
    }
    fn parse_element_section(
        &mut self,
        globals: &[GlobalType],
        functions: &[TypeIdx],
        tables: &[TableType],
        size: u32,
    ) -> Result<ElementSection> {
        trace!("{:?}", functions);
        let (len, elems) = self.parse_vec(|me| me.parse_elem(globals, functions, tables))?;
        tracing::trace!("elems: {elems:?}");
        if len != size as usize {
            Err(WasmParserError::InvalidSectionSize)?
        }
        Ok(ElementSection(elems))
    }
    fn parse_locals(&mut self) -> Result<(usize, Locals)> {
        let (len, n) = self.parse_u32()?;
        let (len2, t) = self.parse_valtype()?;
        Ok((len + len2, Locals { n, t }))
    }

    #[allow(clippy::too_many_arguments)]
    fn parse_code_inner(
        &mut self,
        funcidx: FuncIdx,
        typeidx: TypeIdx,
        functype: &FuncType,
        type_section: &TypeSection,
        functions: &[TypeIdx],
        imported_function_len: u32,
        mems: &[MemType],
        globals: &[GlobalType],
        table_section: &[TableType],
        elems: &[Elem],
        data_count_section: &mut DataCountVerifier,
        size: u32,
    ) -> Result<Func> {
        let (len, locals) = self.parse_vec(&Self::parse_locals)?;
        let slice = &locals[..];
        let locals_data = LocalsData::from(slice);
        let local_reassign = locals_data.create_reassignment_table(&locals)?;
        validate_locals(&locals)?;
        let mut instrs = InstructionGenerator::new();
        let mut instruction_meta = Vec::new();
        let mut checker = TypeChecker::new(typeidx);
        let mut jump_resolver = JumpResolver::new();
        let mut else_addr = None;
        let mut parser = InstructionParser::new(
            self.reader(),
            type_section,
            functions,
            imported_function_len,
            funcidx,
            mems,
            functype,
            &local_reassign,
            globals,
            table_section,
            elems,
        );
        jump_resolver.push(JumpResolverDSL::EnterForwardJumpBlock);
        instrs.enter_block();
        let len2 = parser.parse_instrs(
            data_count_section,
            &mut instrs,
            &mut instruction_meta,
            &mut checker,
            &mut jump_resolver,
            &mut else_addr,
        )?;
        trace!("function return");

        checker.op(&functype.1 .0, &[])?;

        checker.leave_block()?;

        if len + len2 != size as usize {
            Err(WasmParserError::InvalidInstructionSize(
                size,
                (len + len2) as u32,
            ))?
        }
        instrs.leave_block();
        let function_return_start = instrs.len();
        instrs.push(Instr {
            op: vm::special_function_return,
        });
        instrs.push(Instr {
            operand: Operand {
                drop_size: functype.1.iter().map(|v| v.stack_size().u32()).sum(),
            },
        });
        instruction_meta.push(optimizer::InstructionMeta {
            start: function_return_start,
            len: instrs.len() - function_return_start,
            stack_before: crate::parser::core::type_checker::StackSnapshot {
                reachable: true,
                types: functype.1 .0.clone(),
            },
            stack_after: crate::parser::core::type_checker::StackSnapshot {
                reachable: true,
                types: Vec::new(),
            },
        });
        jump_resolver.evaluate(&mut instrs);
        let instrs = optimizer::optimize_function(
            funcidx,
            functype,
            &locals_data,
            instrs.build(),
            instruction_meta,
        );
        Ok(Func {
            locals: locals_data,
            expr: instrs,
        })
    }
    #[allow(clippy::too_many_arguments)]
    pub fn parse_code(
        &mut self,
        funcidx: FuncIdx,
        type_section: &TypeSection,
        functions: &[TypeIdx],
        imported_function_len: u32,
        mems: &[MemType],
        globals: &[GlobalType],
        table_section: &[TableType],
        elems: &[Elem],
        data_count_section: &mut DataCountVerifier,
    ) -> Result<(usize, Func)> {
        let (len, size) = self.parse_u32()?;
        let typeidx = *functions
            .get(funcidx.0 as usize)
            .ok_or(WasmParserError::InvalidFuncIdx(funcidx))?;
        let functype = type_section
            .get(typeidx)
            .ok_or(WasmParserError::InvalidTypeIdx(typeidx))?;
        tracing::trace!("parse_code: {funcidx:?} {typeidx:?} {functype:?}");

        let func = self.parse_code_inner(
            funcidx,
            typeidx,
            functype,
            type_section,
            functions,
            imported_function_len,
            mems,
            globals,
            table_section,
            elems,
            data_count_section,
            size,
        )?;
        Ok((len + size as usize, func))
    }
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    pub fn parse_module(&mut self) -> Result<Module> {
        self.parse_magic()?;
        self.parse_version()?;
        let mut type_section: Option<TypeSection> = None;
        let mut export_section: Option<ExportSection> = None;
        let mut element_section: Option<ElementSection> = None;
        let mut code_section: Option<CodeSection> = None;
        let mut import_section: Option<ImportSection> = None;
        let mut data_section: Option<DataSection> = None;
        let mut start: Option<FuncIdx> = None;
        let mut functions = vec![];
        let mut globals = vec![];
        let mut imported_function_len = 0;
        let mut imported_global_len = 0;
        let mut global_init = vec![];
        let mut tables = vec![];
        let mut mems = vec![];
        let mut current_section_pos: Option<usize> = None;
        let mut data_count_verifier = DataCountVerifier::Lazy { max_data_idx: None };
        let mut name_section = None;
        loop {
            let st = self.parse_section_type()?;

            let st = if let Some(st) = st {
                st
            } else {
                let data_section = data_section.unwrap_or_else(|| DataSection(vec![]));
                match data_count_verifier {
                    DataCountVerifier::Lazy {
                        max_data_idx: Some(max_data_idx),
                    } => {
                        if max_data_idx as usize >= data_section.0.len() {
                            Err(WasmParserError::InvalidDataSectionCount)?;
                        }
                    }
                    DataCountVerifier::Lazy { max_data_idx: None } => {
                        //ok
                    }
                    DataCountVerifier::OnePass(count) => {
                        if data_section.0.len() != (count as usize) {
                            Err(WasmParserError::InvalidDataSectionCount)?;
                        }
                    }
                }
                let code_section = code_section.unwrap_or_else(|| CodeSection(vec![]));
                if functions.len() - imported_function_len != code_section.0.len() {
                    Err(WasmParserError::FunctionAndCodeSectionLengthMismatch)?
                }
                return Ok(Module {
                    fts: type_section.unwrap_or_else(|| TypeSection(vec![])),
                    imports: import_section.unwrap_or_else(|| ImportSection(vec![])),
                    functions,
                    globals,
                    global_init,
                    tables,
                    mems,
                    elems: element_section.unwrap_or_else(|| ElementSection(vec![])),
                    exs: export_section.unwrap_or_else(|| ExportSection(vec![])),
                    codes: code_section,
                    data: data_section,
                    start,
                    name: name_section,
                });
            };
            let new_pos = SECTION_ORDER.iter().position(|x| *x == st);
            if let Some(new_pos) = new_pos {
                if let Some(current_section_pos) = &current_section_pos {
                    if current_section_pos >= &new_pos {
                        Err(WasmParserError::InvalidSectionOrder)?;
                    }
                }
                current_section_pos = Some(new_pos);
            }

            match st {
                WasmSectionType::Unknown(id) => Err(WasmParserError::InvalidSectionType(id))?,
                WasmSectionType::Custom => match self.parse_section_body(Self::parse_namedata)? {
                    NameData::NameSection(subsec) => name_section = Some(subsec),
                    NameData::Unknown(name) => {
                        tracing::warn!("encounted unknown custom section: {name}")
                    }
                },
                WasmSectionType::Type => {
                    trace!("type section");
                    type_section = Some(self.parse_section_body(Self::parse_type_section)?);
                }
                WasmSectionType::Import => {
                    let type_section = type_section.get_or_insert_with(|| TypeSection(vec![]));
                    let section = self.parse_section_body(|me, size| {
                        me.parse_import_section(type_section, size)
                    })?;
                    for import in &section.0 {
                        match &import.desc {
                            ImportDesc::TypeIdx(type_idx) => {
                                functions.push(*type_idx);
                            }
                            ImportDesc::TableType(table_type) => {
                                tables.push(*table_type);
                            }
                            ImportDesc::MemType(mem_type) => {
                                mems.push(*mem_type);
                            }
                            ImportDesc::GlobalType(global_type) => {
                                globals.push(*global_type);
                            }
                        }
                    }
                    imported_global_len = globals.len();
                    imported_function_len = functions.len();
                    import_section = Some(section);
                }
                WasmSectionType::Function => {
                    let function_section = self.parse_section_body(Self::parse_function_section)?;
                    for function in function_section.0 {
                        functions.push(function);
                    }
                }
                WasmSectionType::Table => {
                    for table in self.parse_section_body(Self::parse_table_section)? {
                        tables.push(table.0);
                    }
                }
                WasmSectionType::Memory => {
                    let section = self.parse_section_body(Self::parse_memory_section)?;
                    for mt in section {
                        mems.push(mt);
                    }
                }
                WasmSectionType::Global => {
                    let local_globals = self.parse_section_body(|me, size| {
                        me.parse_global_section(&globals, &functions, size)
                    })?;
                    for global in local_globals {
                        globals.push(global.0);
                        global_init.push(global.1[0]);
                    }
                }
                WasmSectionType::Export => {
                    export_section = Some(self.parse_section_body(|me, size| {
                        me.parse_export_section(&functions, &globals, &tables, &mems, size)
                    })?);
                }
                WasmSectionType::Start => {
                    start = Some(self.parse_section_body(|me, size| {
                        let (len, start) = me.parse_u32()?;
                        if let Some(tidx) = functions.get(start as usize) {
                            if let Some(sec) = &type_section {
                                let ft = sec
                                    .get(*tidx)
                                    .ok_or(WasmParserError::InvalidTypeIdx(*tidx))?;
                                if ft != &FuncType(ResultType(vec![]), ResultType(vec![])) {
                                    return Err(WasmParserError::StartFunction);
                                }
                            } else {
                                return Err(WasmParserError::InvalidFuncIdx(FuncIdx(start)));
                            }
                        } else {
                            return Err(WasmParserError::InvalidFuncIdx(FuncIdx(start)));
                        }
                        if len != size as usize {
                            return Err(WasmParserError::InvalidSectionSize);
                        }
                        Ok(FuncIdx(start))
                    })?);
                }
                WasmSectionType::Element => {
                    trace!("element section");

                    element_section = Some(self.parse_section_body(|me, size| {
                        me.parse_element_section(
                            &globals[..imported_global_len],
                            &functions,
                            &tables,
                            size,
                        )
                    })?);
                }
                WasmSectionType::Code => {
                    let type_section = type_section.get_or_insert_with(|| TypeSection(vec![]));
                    let imports = import_section.get_or_insert_with(|| ImportSection(vec![]));
                    let elems = element_section.get_or_insert_with(|| ElementSection(vec![]));

                    code_section = Some(self.parse_section_body(|me, size| {
                        Self::parse_code_section(
                            me,
                            type_section,
                            &functions,
                            imports,
                            &globals,
                            &tables,
                            &mems,
                            &elems.0,
                            &mut data_count_verifier,
                            size,
                        )
                    })?);
                }
                WasmSectionType::Data => {
                    let sec = self.parse_section_body(|me, size| {
                        me.parse_data_section(&globals[..imported_global_len], &mems, size)
                    })?;
                    match data_count_verifier {
                        DataCountVerifier::OnePass(v) if (v as usize) != sec.0.len() => {
                            Err(WasmParserError::InvalidDataSectionCount)?
                        }
                        _ => {} // ok
                    };
                    data_section = Some(sec);
                }
                WasmSectionType::DataCount => {
                    let count = self.parse_section_body(|me, size| {
                        let (len, count) = me.parse_u32()?;
                        if size as usize != len {
                            Err(WasmParserError::InvalidSectionSize)?
                        }
                        Ok(count)
                    })?;
                    data_count_verifier = DataCountVerifier::OnePass(count);
                }
            }
        }
    }
}
