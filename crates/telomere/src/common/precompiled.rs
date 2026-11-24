//! Precompiled core-module codec (#196).
//!
//! A parsed [`Module`] — including each function's optimizer output
//! ([`LoweredFunction`]) — can be serialized into a compact artifact and
//! restored without re-running the wasm parser or the optimizer. Loading
//! an artifact therefore skips both the parse and optimize phases; only
//! `instantiate`'s store-side work (including `materialize`) still runs.
//!
//! # Contract: exact-build pinning
//!
//! The format makes **no cross-version stability promise**. Every
//! artifact carries a build fingerprint (format version, crate version,
//! rustc version, target triple, feature set, a hash of the crate's
//! source tree, and a hash of the op-ID table). The source-tree hash
//! (computed in `build.rs`) is what pins the fingerprint to the actual
//! handler implementations: changing a handler's operand interpretation
//! without renaming it changes the fingerprint, so artifacts from the
//! older build are rejected instead of being executed by the new one. [`deserialize_module`] rejects any artifact whose fingerprint
//! does not match the running build exactly, with a typed
//! [`PrecompiledError`]; the library never falls back to re-parsing.
//! Artifacts are a cache for one exact build, nothing more.
//!
//! # Format
//!
//! ```text
//! header:  magic "TELOPRE1" | format_version (LEB u32)
//!          crate_version (string) | rustc_version (string)
//!          target (string) | features (string) | source_hash (string)
//!          | op_table_hash (u64 LE)
//! body:    length-prefixed sections in fixed order, each tagged:
//!          types, functions, imports, mems, globals, exports, tables,
//!          elems, data, start, name, codes
//! ```
//!
//! Counts and indices are canonical (minimal) unsigned LEB128; fixed-size
//! numeric payloads are little-endian. The decoder is total: every read
//! is bounds-checked, every count is checked against the remaining input
//! before allocating, decoded cross-references are range-validated, and
//! no input can make it panic.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::custom_section::NameSubSection;
use super::op_registry;
use super::{
    CallRecipeRef, CodeSection, ConstExpr, Data, DataMode, DataSection, Elem, ElemInit, ElemMode,
    ElementSection, Export, ExportDesc, ExportSection, Func, FuncIdx, FuncType, FunctionBody,
    GlobalIdx, GlobalType, Import, ImportDesc, ImportSection, Limits, LocalsData, LoweredBlockMap,
    LoweredFunction, LoweredJumpTarget, LoweredOp, LoweredOperand, MemIdx, MemType, Module, Mut,
    Op, RefType, ResultType, TableIdx, TableType, TypeIdx, TypeSection, ValType,
};

/// Magic bytes at the start of every precompiled artifact.
const MAGIC: &[u8; 8] = b"TELOPRE1";

/// Format version written by this build. Bumped on any format change;
/// artifacts with any other version are rejected, never migrated.
const FORMAT_VERSION: u32 = 2;

// Section tags, in the fixed order they appear in the body.
const SEC_TYPES: u8 = 1;
const SEC_FUNCTIONS: u8 = 2;
const SEC_IMPORTS: u8 = 3;
const SEC_MEMS: u8 = 4;
const SEC_GLOBALS: u8 = 5;
const SEC_EXPORTS: u8 = 6;
const SEC_TABLES: u8 = 7;
const SEC_ELEMS: u8 = 8;
const SEC_DATA: u8 = 9;
const SEC_START: u8 = 10;
const SEC_NAME: u8 = 11;
const SEC_CODES: u8 = 12;

/// Largest operand count accepted for one lowered op. `br_table`
/// carries one operand per target, so wide branch tables are legitimately
/// large; the bound is exactly what keeps `materialize`'s `u16` length
/// conversion total on decoded input (a parsed module already satisfies
/// it, since it materialized once at parse time).
const MAX_OP_OPERANDS: usize = u16::MAX as usize - 1;

/// Largest total local count (params + declared + temps) accepted for
/// one function. Mirrors the parser's `TooManyLocals` posture with a
/// generous ceiling no real module approaches.
const MAX_LOCALS_TOTAL: u64 = 1 << 24;

/// Errors returned by the precompiled-module codec.
///
/// Every failure to serialize or deserialize is one of these typed
/// errors; the decoder in particular never panics on malformed input.
#[derive(Debug, thiserror::Error)]
pub enum PrecompiledError {
    /// The input does not start with the precompiled magic bytes.
    #[error("not a telomere precompiled module (bad magic)")]
    BadMagic,
    /// The artifact's format version is not the one this build writes.
    #[error("unsupported precompiled format version {found} (this build writes {supported})")]
    UnsupportedFormatVersion {
        /// Version found in the artifact.
        found: u32,
        /// Version this build writes.
        supported: u32,
    },
    /// A build-fingerprint field differs between artifact and build.
    #[error(
        "precompiled artifact was produced by a different build: {field} differs \
         (artifact: {artifact}, this build: {current})"
    )]
    FingerprintMismatch {
        /// Which fingerprint field differs.
        field: &'static str,
        /// The artifact's value.
        artifact: String,
        /// This build's value.
        current: String,
    },
    /// The input ended before a complete value could be read.
    #[error("precompiled artifact is truncated at offset {offset} (needed {needed} bytes)")]
    Truncated {
        /// Offset at which the read failed.
        offset: usize,
        /// Bytes the read needed.
        needed: usize,
    },
    /// The input is structurally invalid (non-canonical encoding,
    /// out-of-range reference, inconsistent section, ...).
    #[error("precompiled artifact is corrupt: {0}")]
    Corrupt(&'static str),
    /// Bytes remain after the final section.
    #[error("precompiled artifact has {trailing} trailing bytes after the final section")]
    TrailingBytes {
        /// Number of trailing bytes.
        trailing: usize,
    },
    /// The artifact names an op id this build's registry does not have.
    #[error("unknown op id {0} in precompiled artifact")]
    UnknownOpId(u16),
    /// A handler used by the module has no stable op id, so the module
    /// cannot be serialized. Register the handler by regenerating the
    /// registry with `tools/gen-op-registry.py`.
    #[error(
        "an interpreter handler used by this module has no stable op id; \
         regenerate the registry with tools/gen-op-registry.py"
    )]
    UnregisteredOp,
    /// Host function bodies have no lowered form and cannot be serialized.
    #[error(
        "module contains a host function body at code index {index}, which cannot be serialized"
    )]
    HostFunctionBody {
        /// Index into the module's code section.
        index: usize,
    },
}

type Result<T> = std::result::Result<T, PrecompiledError>;

/// Per-phase timings for serialize/deserialize, for cold-start
/// measurement (#196). All durations are wall-clock for the named phase.
#[derive(Debug, Default, Clone, Copy)]
pub struct PrecompiledTimings {
    /// Header write/read including fingerprint validation.
    pub header: Duration,
    /// Module sections other than function bodies.
    pub sections: Duration,
    /// Function bodies (lowered code) encode/decode, including
    /// `materialize` on the decode side.
    pub functions: Duration,
    /// Whole operation.
    pub total: Duration,
}

/// The feature-set string pinned into the fingerprint. Only features
/// that change the handler set or value layouts are listed.
fn feature_set_string() -> String {
    let mut parts: Vec<&'static str> = Vec::new();
    if cfg!(feature = "simd") {
        parts.push("simd");
    }
    if cfg!(feature = "threads") {
        parts.push("threads");
    }
    if cfg!(feature = "jit") {
        parts.push("jit");
    }
    if cfg!(feature = "aot-load") {
        parts.push("aot-load");
    }
    if cfg!(feature = "vm-profile") {
        parts.push("vm-profile");
    }
    if cfg!(feature = "vm-diagnostics") {
        parts.push("vm-diagnostics");
    }
    if cfg!(feature = "measure-switches") {
        parts.push("measure-switches");
    }
    parts.join(",")
}

/// Whether `bytes` starts with the precompiled-module magic. Used by
/// loaders that accept either wasm or precompiled input.
pub fn is_precompiled_module(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn new() -> Self {
        Self { buf: Vec::new() }
    }

    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    fn fixed_u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn fixed_u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn fixed_u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    fn leb_u64(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    fn leb_u32(&mut self, v: u32) {
        self.leb_u64(u64::from(v));
    }

    fn leb_usize(&mut self, v: usize) {
        self.leb_u64(v as u64);
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    fn string(&mut self, s: &str) {
        self.leb_usize(s.len());
        self.raw(s.as_bytes());
    }

    fn count(&mut self, n: usize) {
        self.leb_usize(n);
    }
}

// ---------------------------------------------------------------------------
// Reader
// ---------------------------------------------------------------------------

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    fn is_done(&self) -> bool {
        self.pos == self.buf.len()
    }

    fn bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        if self.remaining() < n {
            return Err(PrecompiledError::Truncated {
                offset: self.pos,
                needed: n,
            });
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    fn u8(&mut self) -> Result<u8> {
        Ok(self.bytes(1)?[0])
    }

    fn bool(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(PrecompiledError::Corrupt("invalid boolean byte")),
        }
    }

    fn fixed_u16(&mut self) -> Result<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn fixed_u32(&mut self) -> Result<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn fixed_u64(&mut self) -> Result<u64> {
        let b = self.bytes(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Canonical unsigned LEB128. Non-minimal encodings and values that
    /// overflow u64 are corruption, never a panic.
    fn leb_u64(&mut self) -> Result<u64> {
        let start = self.pos;
        let mut value: u64 = 0;
        let mut shift = 0u32;
        loop {
            let byte = self.u8()?;
            if shift == 63 {
                // Tenth byte: only bit 0 may be set, and it must terminate.
                if byte > 1 {
                    return Err(PrecompiledError::Corrupt("LEB128 value overflows u64"));
                }
                value |= u64::from(byte) << shift;
                break;
            }
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                break;
            }
            shift += 7;
            if self.pos - start >= 10 {
                return Err(PrecompiledError::Corrupt("LEB128 value overflows u64"));
            }
        }
        // Canonical form: re-encoding the value must take the same
        // number of bytes as the input did.
        let consumed = self.pos - start;
        let mut minimal = 1usize;
        let mut rest = value >> 7;
        while rest != 0 {
            minimal += 1;
            rest >>= 7;
        }
        if consumed != minimal {
            return Err(PrecompiledError::Corrupt("non-canonical LEB128 encoding"));
        }
        Ok(value)
    }

    fn leb_u32(&mut self) -> Result<u32> {
        let v = self.leb_u64()?;
        u32::try_from(v).map_err(|_| PrecompiledError::Corrupt("value does not fit in u32"))
    }

    fn leb_usize(&mut self) -> Result<usize> {
        let v = self.leb_u64()?;
        usize::try_from(v).map_err(|_| PrecompiledError::Corrupt("value does not fit in usize"))
    }

    /// A vector element count. Every element occupies at least one byte
    /// in this format, so a count larger than the remaining input is
    /// corruption; this check runs before any allocation sized by it.
    fn count(&mut self) -> Result<usize> {
        let n = self.leb_usize()?;
        if n > self.remaining() {
            return Err(PrecompiledError::Corrupt(
                "element count exceeds remaining input",
            ));
        }
        Ok(n)
    }

    fn string(&mut self) -> Result<String> {
        let n = self.leb_usize()?;
        let bytes = self.bytes(n)?;
        String::from_utf8(bytes.to_vec())
            .map_err(|_| PrecompiledError::Corrupt("invalid UTF-8 string"))
    }

    fn section(&mut self, expected_tag: u8) -> Result<Reader<'a>> {
        let tag = self.u8()?;
        if tag != expected_tag {
            return Err(PrecompiledError::Corrupt("unexpected section tag or order"));
        }
        let len = self.leb_usize()?;
        let body = self.bytes(len)?;
        Ok(Reader::new(body))
    }
}

fn expect_section_consumed(r: &Reader<'_>) -> Result<()> {
    if r.is_done() {
        Ok(())
    } else {
        Err(PrecompiledError::Corrupt(
            "section body has unconsumed bytes",
        ))
    }
}

// ---------------------------------------------------------------------------
// Value codecs
// ---------------------------------------------------------------------------

fn write_val_type(w: &mut Writer, ty: &ValType) {
    w.u8(match ty {
        ValType::I32 => 0,
        ValType::I64 => 1,
        ValType::F32 => 2,
        ValType::F64 => 3,
        ValType::V128 => 4,
        ValType::FuncRef => 5,
        ValType::ExternRef => 6,
    });
}

fn read_val_type(r: &mut Reader<'_>) -> Result<ValType> {
    Ok(match r.u8()? {
        0 => ValType::I32,
        1 => ValType::I64,
        2 => ValType::F32,
        3 => ValType::F64,
        4 => ValType::V128,
        5 => ValType::FuncRef,
        6 => ValType::ExternRef,
        _ => return Err(PrecompiledError::Corrupt("unknown value type tag")),
    })
}

fn write_ref_type(w: &mut Writer, ty: &RefType) {
    w.u8(match ty {
        RefType::FuncRef => 0,
        RefType::ExternRef => 1,
    });
}

fn read_ref_type(r: &mut Reader<'_>) -> Result<RefType> {
    Ok(match r.u8()? {
        0 => RefType::FuncRef,
        1 => RefType::ExternRef,
        _ => return Err(PrecompiledError::Corrupt("unknown reference type tag")),
    })
}

fn write_result_type(w: &mut Writer, ty: &ResultType) {
    w.count(ty.0.len());
    for t in &ty.0 {
        write_val_type(w, t);
    }
}

fn read_result_type(r: &mut Reader<'_>) -> Result<ResultType> {
    let n = r.count()?;
    let mut types = Vec::with_capacity(n);
    for _ in 0..n {
        types.push(read_val_type(r)?);
    }
    Ok(ResultType(types))
}

fn write_limits(w: &mut Writer, limits: &Limits) {
    w.bool(limits.max.is_some());
    w.fixed_u32(limits.min);
    if let Some(max) = limits.max {
        w.fixed_u32(max);
    }
}

fn read_limits(r: &mut Reader<'_>) -> Result<Limits> {
    let has_max = r.bool()?;
    let min = r.fixed_u32()?;
    let max = if has_max { Some(r.fixed_u32()?) } else { None };
    Ok(Limits { min, max })
}

fn write_const_expr(w: &mut Writer, expr: &ConstExpr) {
    match expr {
        ConstExpr::I32(v) => {
            w.u8(0);
            w.fixed_u32(*v as u32);
        }
        ConstExpr::I64(v) => {
            w.u8(1);
            w.fixed_u64(*v as u64);
        }
        ConstExpr::F32(v) => {
            w.u8(2);
            w.fixed_u32(v.to_bits());
        }
        ConstExpr::F64(v) => {
            w.u8(3);
            w.fixed_u64(v.to_bits());
        }
        ConstExpr::V128(v) => {
            w.u8(4);
            w.raw(&v.to_le_bytes());
        }
        ConstExpr::GlobalGet(idx) => {
            w.u8(5);
            w.leb_u32(*idx);
        }
        ConstExpr::RefNull(rt) => {
            w.u8(6);
            write_ref_type(w, rt);
        }
        ConstExpr::FuncRef(idx) => {
            w.u8(7);
            w.leb_u32(*idx);
        }
    }
}

fn read_const_expr(r: &mut Reader<'_>) -> Result<ConstExpr> {
    Ok(match r.u8()? {
        0 => ConstExpr::I32(r.fixed_u32()? as i32),
        1 => ConstExpr::I64(r.fixed_u64()? as i64),
        2 => ConstExpr::F32(f32::from_bits(r.fixed_u32()?)),
        3 => ConstExpr::F64(f64::from_bits(r.fixed_u64()?)),
        4 => {
            let b = r.bytes(16)?;
            let mut v = [0u8; 16];
            v.copy_from_slice(b);
            ConstExpr::V128(u128::from_le_bytes(v))
        }
        5 => ConstExpr::GlobalGet(r.leb_u32()?),
        6 => ConstExpr::RefNull(read_ref_type(r)?),
        7 => ConstExpr::FuncRef(r.leb_u32()?),
        _ => return Err(PrecompiledError::Corrupt("unknown const expression tag")),
    })
}

fn write_const_exprs(w: &mut Writer, exprs: &[ConstExpr]) {
    w.count(exprs.len());
    for e in exprs {
        write_const_expr(w, e);
    }
}

fn read_const_exprs(r: &mut Reader<'_>) -> Result<Vec<ConstExpr>> {
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_const_expr(r)?);
    }
    Ok(out)
}

fn check_const_expr(expr: &ConstExpr, module: &ModuleShape) -> Result<()> {
    match expr {
        ConstExpr::GlobalGet(idx) if (*idx as usize) >= module.globals => Err(
            PrecompiledError::Corrupt("global.get index out of range in const expression"),
        ),
        ConstExpr::FuncRef(idx) if (*idx as usize) >= module.functions => Err(
            PrecompiledError::Corrupt("ref.func index out of range in const expression"),
        ),
        _ => Ok(()),
    }
}

/// Index-space sizes of the module being decoded, for range checks.
#[derive(Default, Clone, Copy)]
struct ModuleShape {
    types: usize,
    functions: usize,
    tables: usize,
    mems: usize,
    globals: usize,
}

fn write_func_type(w: &mut Writer, ty: &FuncType) {
    write_result_type(w, &ty.0);
    write_result_type(w, &ty.1);
}

fn read_func_type(r: &mut Reader<'_>) -> Result<FuncType> {
    Ok(FuncType(read_result_type(r)?, read_result_type(r)?))
}

fn write_mem_type(w: &mut Writer, ty: &MemType) {
    write_limits(w, &ty.limits);
    w.bool(ty.shared);
}

fn read_mem_type(r: &mut Reader<'_>) -> Result<MemType> {
    Ok(MemType {
        limits: read_limits(r)?,
        shared: r.bool()?,
    })
}

fn write_global_type(w: &mut Writer, ty: &GlobalType) {
    write_val_type(w, &ty.0);
    w.u8(match ty.1 {
        Mut::Const => 0,
        Mut::Var => 1,
    });
}

fn read_global_type(r: &mut Reader<'_>) -> Result<GlobalType> {
    let vt = read_val_type(r)?;
    let m = match r.u8()? {
        0 => Mut::Const,
        1 => Mut::Var,
        _ => return Err(PrecompiledError::Corrupt("unknown mutability tag")),
    };
    Ok(GlobalType(vt, m))
}

fn write_table_type(w: &mut Writer, ty: &TableType) {
    write_ref_type(w, &ty.reftype);
    write_limits(w, &ty.limits);
}

fn read_table_type(r: &mut Reader<'_>) -> Result<TableType> {
    Ok(TableType {
        reftype: read_ref_type(r)?,
        limits: read_limits(r)?,
    })
}

fn write_import_desc(w: &mut Writer, desc: &ImportDesc) {
    match desc {
        ImportDesc::TypeIdx(idx) => {
            w.u8(0);
            w.leb_u32(idx.0);
        }
        ImportDesc::TableType(tt) => {
            w.u8(1);
            write_table_type(w, tt);
        }
        ImportDesc::MemType(mt) => {
            w.u8(2);
            write_mem_type(w, mt);
        }
        ImportDesc::GlobalType(gt) => {
            w.u8(3);
            write_global_type(w, gt);
        }
    }
}

fn read_import_desc(r: &mut Reader<'_>) -> Result<ImportDesc> {
    Ok(match r.u8()? {
        0 => ImportDesc::TypeIdx(TypeIdx(r.leb_u32()?)),
        1 => ImportDesc::TableType(read_table_type(r)?),
        2 => ImportDesc::MemType(read_mem_type(r)?),
        3 => ImportDesc::GlobalType(read_global_type(r)?),
        _ => return Err(PrecompiledError::Corrupt("unknown import description tag")),
    })
}

fn write_export_desc(w: &mut Writer, desc: &ExportDesc) {
    match desc {
        ExportDesc::Func(idx) => {
            w.u8(0);
            w.leb_u32(idx.0);
        }
        ExportDesc::Table(idx) => {
            w.u8(1);
            w.leb_u32(idx.0);
        }
        ExportDesc::Mem(idx) => {
            w.u8(2);
            w.leb_u32(idx.0);
        }
        ExportDesc::Global(idx) => {
            w.u8(3);
            w.leb_u32(idx.0);
        }
    }
}

fn read_export_desc(r: &mut Reader<'_>) -> Result<ExportDesc> {
    Ok(match r.u8()? {
        0 => ExportDesc::Func(FuncIdx(r.leb_u32()?)),
        1 => ExportDesc::Table(TableIdx(r.leb_u32()?)),
        2 => ExportDesc::Mem(MemIdx(r.leb_u32()?)),
        3 => ExportDesc::Global(GlobalIdx(r.leb_u32()?)),
        _ => return Err(PrecompiledError::Corrupt("unknown export description tag")),
    })
}

fn write_elem(w: &mut Writer, elem: &Elem) {
    write_ref_type(w, &elem.kind);
    match &elem.init {
        ElemInit::FuncIdx(funcs) => {
            w.u8(0);
            w.count(funcs.len());
            for f in funcs {
                w.leb_u32(*f);
            }
        }
        ElemInit::ConstExpr(exprs) => {
            w.u8(1);
            w.count(exprs.len());
            for e in exprs {
                write_const_exprs(w, e);
            }
        }
    }
    match &elem.mode {
        ElemMode::Passive => w.u8(0),
        ElemMode::Active(table, offset) => {
            w.u8(1);
            w.leb_u32(table.0);
            write_const_exprs(w, offset);
        }
        ElemMode::Declarative => w.u8(2),
    }
}

fn read_elem(r: &mut Reader<'_>) -> Result<Elem> {
    let kind = read_ref_type(r)?;
    let init = match r.u8()? {
        0 => {
            let n = r.count()?;
            let mut funcs = Vec::with_capacity(n);
            for _ in 0..n {
                funcs.push(r.leb_u32()?);
            }
            ElemInit::FuncIdx(funcs)
        }
        1 => {
            let n = r.count()?;
            let mut exprs = Vec::with_capacity(n);
            for _ in 0..n {
                exprs.push(read_const_exprs(r)?);
            }
            ElemInit::ConstExpr(exprs)
        }
        _ => return Err(PrecompiledError::Corrupt("unknown element init tag")),
    };
    let mode = match r.u8()? {
        0 => ElemMode::Passive,
        1 => ElemMode::Active(TableIdx(r.leb_u32()?), read_const_exprs(r)?),
        2 => ElemMode::Declarative,
        _ => return Err(PrecompiledError::Corrupt("unknown element mode tag")),
    };
    Ok(Elem { kind, init, mode })
}

fn write_data(w: &mut Writer, data: &Data) {
    w.leb_usize(data.init.len());
    w.raw(&data.init);
    match &data.mode {
        DataMode::Passive => w.u8(0),
        DataMode::Active(mem, offset) => {
            w.u8(1);
            w.leb_u32(mem.0);
            write_const_exprs(w, offset);
        }
    }
}

fn read_data(r: &mut Reader<'_>) -> Result<Data> {
    let n = r.leb_usize()?;
    let init = r.bytes(n)?.to_vec();
    let mode = match r.u8()? {
        0 => DataMode::Passive,
        1 => DataMode::Active(MemIdx(r.leb_u32()?), read_const_exprs(r)?),
        _ => return Err(PrecompiledError::Corrupt("unknown data mode tag")),
    };
    Ok(Data { init, mode })
}

fn write_name_section(w: &mut Writer, name: &Option<NameSubSection>) {
    w.bool(name.is_some());
    let Some(name) = name else { return };
    w.bool(name.module_name.is_some());
    if let Some(m) = &name.module_name {
        w.string(&m.0);
    }
    w.bool(name.function_name.is_some());
    if let Some(f) = &name.function_name {
        w.count(f.0.len());
        for (idx, s) in &f.0 {
            w.leb_u32(*idx);
            w.string(s);
        }
    }
    w.bool(name.local_name.is_some());
    if let Some(l) = &name.local_name {
        w.count(l.0.len());
        for (func, locals) in &l.0 {
            w.leb_u32(*func);
            w.count(locals.len());
            for (idx, s) in locals {
                w.leb_u32(*idx);
                w.string(s);
            }
        }
    }
}

fn read_name_section(r: &mut Reader<'_>) -> Result<Option<NameSubSection>> {
    if !r.bool()? {
        return Ok(None);
    }
    let module_name = if r.bool()? {
        Some(super::custom_section::ModuleNameSubSec(r.string()?))
    } else {
        None
    };
    let function_name = if r.bool()? {
        let n = r.count()?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            entries.push((r.leb_u32()?, r.string()?));
        }
        Some(super::custom_section::FuncNameSubSec(entries))
    } else {
        None
    };
    let local_name = if r.bool()? {
        let n = r.count()?;
        let mut entries = Vec::with_capacity(n);
        for _ in 0..n {
            let func = r.leb_u32()?;
            let m = r.count()?;
            let mut locals = Vec::with_capacity(m);
            for _ in 0..m {
                locals.push((r.leb_u32()?, r.string()?));
            }
            entries.push((func, locals));
        }
        Some(super::custom_section::LocalNameSubSec(entries))
    } else {
        None
    };
    Ok(Some(NameSubSection {
        module_name,
        function_name,
        local_name,
    }))
}

// ---------------------------------------------------------------------------
// Function bodies
// ---------------------------------------------------------------------------

fn write_call_recipe_ref(w: &mut Writer, target: &CallRecipeRef) {
    w.leb_u32(target.funcidx);
    w.leb_u32(target.recipe_slot_plus_one);
}

fn read_call_recipe_ref(r: &mut Reader<'_>) -> Result<CallRecipeRef> {
    let funcidx = r.leb_u32()?;
    let slot_plus_one = r.leb_u32()?;
    // Direct field construction: the stored value round-trips exactly,
    // with no arithmetic that could overflow.
    Ok(CallRecipeRef {
        funcidx,
        recipe_slot_plus_one: slot_plus_one,
    })
}

fn write_locals(w: &mut Writer, locals: &LocalsData) {
    for v in [
        locals.count_i32,
        locals.count_f32,
        locals.count_func_ref,
        locals.count_extern_ref,
        locals.count_i64,
        locals.count_f64,
        locals.count_v128,
        locals.param_bytes,
        locals.temp_bytes,
    ] {
        w.fixed_u32(v);
    }
}

fn read_locals(r: &mut Reader<'_>) -> Result<LocalsData> {
    let mut fields = [0u32; 9];
    for f in &mut fields {
        *f = r.fixed_u32()?;
    }
    let total: u64 = fields[..7].iter().map(|v| u64::from(*v)).sum();
    if total > MAX_LOCALS_TOTAL {
        return Err(PrecompiledError::Corrupt("locals count exceeds limit"));
    }
    Ok(LocalsData {
        count_i32: fields[0],
        count_f32: fields[1],
        count_func_ref: fields[2],
        count_extern_ref: fields[3],
        count_i64: fields[4],
        count_f64: fields[5],
        count_v128: fields[6],
        param_bytes: fields[7],
        temp_bytes: fields[8],
    })
}

fn write_lowered(
    w: &mut Writer,
    lowered: &LoweredFunction,
    op_ids: &HashMap<usize, u16>,
) -> Result<()> {
    w.count(lowered.const_pool.len());
    for entry in &lowered.const_pool {
        w.raw(entry);
    }
    w.count(lowered.code.len());
    for op in &lowered.code {
        match op.label {
            Some(label) => {
                w.bool(true);
                w.leb_usize(label);
            }
            None => w.bool(false),
        }
        let id = op_ids
            .get(&(op.op as usize))
            .ok_or(PrecompiledError::UnregisteredOp)?;
        w.fixed_u16(*id);
        w.count(op.operands.len());
        for operand in &op.operands {
            match operand {
                LoweredOperand::Raw(bytes) => {
                    w.u8(0);
                    w.raw(bytes);
                }
                LoweredOperand::ConstPoolRef(idx) => {
                    w.u8(1);
                    w.leb_u32(*idx);
                }
                LoweredOperand::JumpTarget(label) => {
                    w.u8(2);
                    w.leb_usize(*label);
                }
                LoweredOperand::CallRecipeRef(target) => {
                    w.u8(3);
                    write_call_recipe_ref(w, target);
                }
            }
        }
    }
    w.count(lowered.call_recipes.len());
    for target in &lowered.call_recipes {
        write_call_recipe_ref(w, target);
    }
    w.count(lowered.jump_table.len());
    for target in &lowered.jump_table {
        w.leb_usize(target.label);
        w.leb_usize(target.block_id);
    }
    w.count(lowered.block_map.len());
    for block in &lowered.block_map {
        w.leb_usize(block.block_id);
        w.leb_usize(block.label);
        w.leb_usize(block.code_index);
    }
    w.fixed_u32(lowered.max_operand_bytes);
    Ok(())
}

fn read_lowered(r: &mut Reader<'_>, ops_by_id: &HashMap<u16, Op>) -> Result<LoweredFunction> {
    let pool_len = r.count()?;
    let mut const_pool = Vec::with_capacity(pool_len);
    for _ in 0..pool_len {
        let b = r.bytes(8)?;
        let mut entry = [0u8; 8];
        entry.copy_from_slice(b);
        const_pool.push(entry);
    }
    let code_len = r.count()?;
    let mut code = Vec::with_capacity(code_len);
    for _ in 0..code_len {
        let label = if r.bool()? {
            Some(r.leb_usize()?)
        } else {
            None
        };
        let id = r.fixed_u16()?;
        let op = ops_by_id
            .get(&id)
            .copied()
            .ok_or(PrecompiledError::UnknownOpId(id))?;
        let n_operands = r.count()?;
        if n_operands > MAX_OP_OPERANDS {
            return Err(PrecompiledError::Corrupt(
                "lowered op has too many operands",
            ));
        }
        let mut operands = Vec::with_capacity(n_operands);
        for _ in 0..n_operands {
            operands.push(match r.u8()? {
                0 => {
                    let b = r.bytes(8)?;
                    let mut raw = [0u8; 8];
                    raw.copy_from_slice(b);
                    LoweredOperand::Raw(raw)
                }
                1 => LoweredOperand::ConstPoolRef(r.leb_u32()?),
                2 => LoweredOperand::JumpTarget(r.leb_usize()?),
                3 => LoweredOperand::CallRecipeRef(read_call_recipe_ref(r)?),
                _ => return Err(PrecompiledError::Corrupt("unknown lowered operand tag")),
            });
        }
        code.push(LoweredOp {
            label,
            op,
            operands,
        });
    }
    let n_recipes = r.count()?;
    let mut call_recipes = Vec::with_capacity(n_recipes);
    for _ in 0..n_recipes {
        call_recipes.push(read_call_recipe_ref(r)?);
    }
    let n_jumps = r.count()?;
    let mut jump_table = Vec::with_capacity(n_jumps);
    for _ in 0..n_jumps {
        jump_table.push(LoweredJumpTarget {
            label: r.leb_usize()?,
            block_id: r.leb_usize()?,
        });
    }
    let n_blocks = r.count()?;
    let mut block_map = Vec::with_capacity(n_blocks);
    for _ in 0..n_blocks {
        block_map.push(LoweredBlockMap {
            block_id: r.leb_usize()?,
            label: r.leb_usize()?,
            code_index: r.leb_usize()?,
        });
    }
    let max_operand_bytes = r.fixed_u32()?;
    Ok(LoweredFunction {
        code,
        const_pool,
        call_recipes,
        jump_table,
        block_map,
        materialized_preview: None,
        max_operand_bytes,
    })
}

/// Structural validation of a decoded lowered function, mirroring the
/// invariants the optimizer's own `verify` checks plus the bounds
/// `materialize` relies on. Keeps later phases panic-free on decoded
/// input that passed the byte-level checks.
fn validate_lowered(lowered: &LoweredFunction, shape: &ModuleShape) -> Result<()> {
    let label_bound = lowered
        .block_map
        .len()
        .checked_add(lowered.code.len())
        .ok_or(PrecompiledError::Corrupt("lowered function size overflow"))?;
    // Block labels are the only legal symbolic jump destinations,
    // mirroring the optimizer pipeline's own `encode::verify`: every
    // `JumpTarget` operand and every jump-table entry must name a
    // block-map label, and block labels must be unique.
    let mut block_ids: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for block in &lowered.block_map {
        if block_ids.insert(block.label, block.block_id).is_some() {
            return Err(PrecompiledError::Corrupt("duplicate block label"));
        }
    }
    for target in &lowered.jump_table {
        if block_ids.get(&target.label) != Some(&target.block_id) {
            return Err(PrecompiledError::Corrupt(
                "jump table entry does not match the block map",
            ));
        }
    }
    // The canonical branch handlers dispatch through their operands as
    // code addresses (`op_br` and friends offset the code pointer by
    // the operand unconditionally). Optimizer output carries those
    // operands in two forms: symbolic `JumpTarget`s (kernel-encoded
    // functions) and `Raw` operands holding an absolute instruction
    // index (functions lowered from the materialized stream). Both
    // forms are validated here: a `JumpTarget` must name a block-map
    // label (checked in the operand loop below), and a `Raw` branch
    // operand's `jump_addr` must land inside this function's
    // materialized instruction stream. Any other operand kind in a
    // branch op, or a branch op with the wrong operand count, would be
    // copied verbatim into the materialized stream and executed as an
    // unchecked address, so it is rejected.
    let is_branch_op = |op: &Op| {
        std::ptr::fn_addr_eq(*op, crate::runtime::vm::op_br as Op)
            || std::ptr::fn_addr_eq(*op, crate::runtime::vm::op_br_if as Op)
            || std::ptr::fn_addr_eq(*op, crate::runtime::vm::op_else as Op)
            || std::ptr::fn_addr_eq(*op, crate::runtime::vm::op_if as Op)
    };
    let is_br_table = |op: &Op| std::ptr::fn_addr_eq(*op, crate::runtime::vm::op_br_table as Op);
    let materialized_len: usize = lowered.code.iter().map(|op| 1 + op.operands.len()).sum();
    let raw_jump_in_range = |raw: &[u8; 8]| {
        let addr = u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]) as usize;
        addr < materialized_len
    };
    let branch_operand_ok = |operand: &LoweredOperand| match operand {
        LoweredOperand::JumpTarget(_) => true,
        LoweredOperand::Raw(raw) => raw_jump_in_range(raw),
        _ => false,
    };
    let mut op_labels = std::collections::HashSet::new();
    for op in &lowered.code {
        if let Some(label) = op.label {
            if label >= label_bound {
                return Err(PrecompiledError::Corrupt("op label out of range"));
            }
            op_labels.insert(label);
        }
        if is_branch_op(&op.op) {
            if op.operands.len() != 1 || !branch_operand_ok(&op.operands[0]) {
                return Err(PrecompiledError::Corrupt(
                    "branch op must carry exactly one in-range branch operand",
                ));
            }
        }
        if is_br_table(&op.op)
            && (op.operands.is_empty() || !op.operands.iter().all(branch_operand_ok))
        {
            return Err(PrecompiledError::Corrupt(
                "br_table operands must all be in-range branch operands",
            ));
        }
        for operand in &op.operands {
            match operand {
                LoweredOperand::ConstPoolRef(idx) => {
                    if (*idx as usize) >= lowered.const_pool.len() {
                        return Err(PrecompiledError::Corrupt(
                            "const pool reference out of range",
                        ));
                    }
                }
                LoweredOperand::JumpTarget(label) => {
                    if !block_ids.contains_key(label) {
                        return Err(PrecompiledError::Corrupt("jump target label out of range"));
                    }
                }
                LoweredOperand::CallRecipeRef(target) => {
                    if (target.funcidx as usize) >= shape.functions {
                        return Err(PrecompiledError::Corrupt(
                            "call recipe function index out of range",
                        ));
                    }
                }
                LoweredOperand::Raw(_) => {}
            }
        }
    }
    for target in &lowered.call_recipes {
        if (target.funcidx as usize) >= shape.functions {
            return Err(PrecompiledError::Corrupt(
                "call recipe function index out of range",
            ));
        }
    }
    for block in &lowered.block_map {
        if block.code_index >= lowered.code.len() {
            return Err(PrecompiledError::Corrupt(
                "block map code index out of range",
            ));
        }
        if lowered.code[block.code_index].label != Some(block.label) {
            return Err(PrecompiledError::Corrupt(
                "block map label does not match the op at its code index",
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry points
// ---------------------------------------------------------------------------

fn write_section(w: &mut Writer, tag: u8, body: &Writer) {
    w.u8(tag);
    w.leb_usize(body.buf.len());
    w.raw(&body.buf);
}

fn op_id_map() -> HashMap<usize, u16> {
    let mut map = HashMap::new();
    op_registry::for_each_op_entry(|entry| {
        map.insert(entry.op as usize, entry.id);
    });
    map
}

fn op_by_id_map() -> HashMap<u16, Op> {
    let mut map = HashMap::new();
    op_registry::for_each_op_entry(|entry| {
        map.insert(entry.id, entry.op);
    });
    map
}

/// Serializes a parsed module into the precompiled artifact format.
///
/// Fails with [`PrecompiledError::UnregisteredOp`] if the module uses an
/// interpreter handler that has no stable op id, and with
/// [`PrecompiledError::HostFunctionBody`] for host function bodies.
pub fn serialize_module(module: &Module) -> Result<Vec<u8>> {
    serialize_module_timed(module, None)
}

/// [`serialize_module`] with per-phase timings recorded into `timings`.
pub fn serialize_module_timed(
    module: &Module,
    timings: Option<&mut PrecompiledTimings>,
) -> Result<Vec<u8>> {
    let total_start = Instant::now();
    let mut w = Writer::new();
    w.raw(MAGIC);
    w.leb_u32(FORMAT_VERSION);
    w.string(env!("CARGO_PKG_VERSION"));
    w.string(env!("TELOMERE_BUILD_RUSTC_VERSION"));
    w.string(env!("TELOMERE_BUILD_TARGET"));
    w.string(&feature_set_string());
    w.string(env!("TELOMERE_BUILD_SOURCE_HASH"));
    w.fixed_u64(op_registry::OP_TABLE_HASH);
    let header_done = Instant::now();
    {
        let mut s = Writer::new();
        s.count(module.fts.0.len());
        for ty in &module.fts.0 {
            write_func_type(&mut s, ty);
        }
        write_section(&mut w, SEC_TYPES, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.functions.len());
        for idx in &module.functions {
            s.leb_u32(idx.0);
        }
        write_section(&mut w, SEC_FUNCTIONS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.imports.0.len());
        for import in &module.imports.0 {
            s.string(&import.module);
            s.string(&import.name);
            write_import_desc(&mut s, &import.desc);
        }
        write_section(&mut w, SEC_IMPORTS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.mems.len());
        for mem in &module.mems {
            write_mem_type(&mut s, mem);
        }
        write_section(&mut w, SEC_MEMS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.globals.len());
        for g in &module.globals {
            write_global_type(&mut s, g);
        }
        s.count(module.global_init.len());
        for init in &module.global_init {
            write_const_expr(&mut s, init);
        }
        write_section(&mut w, SEC_GLOBALS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.exs.0.len());
        for export in &module.exs.0 {
            s.string(&export.0);
            write_export_desc(&mut s, &export.1);
        }
        write_section(&mut w, SEC_EXPORTS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.tables.len());
        for table in &module.tables {
            write_table_type(&mut s, table);
        }
        write_section(&mut w, SEC_TABLES, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.elems.0.len());
        for elem in &module.elems.0 {
            write_elem(&mut s, elem);
        }
        write_section(&mut w, SEC_ELEMS, &s);
    }
    {
        let mut s = Writer::new();
        s.count(module.data.0.len());
        for data in &module.data.0 {
            write_data(&mut s, data);
        }
        write_section(&mut w, SEC_DATA, &s);
    }
    {
        let mut s = Writer::new();
        s.bool(module.start.is_some());
        if let Some(start) = &module.start {
            s.leb_u32(start.0);
        }
        write_section(&mut w, SEC_START, &s);
    }
    {
        let mut s = Writer::new();
        write_name_section(&mut s, &module.name);
        write_section(&mut w, SEC_NAME, &s);
    }
    let sections_done = Instant::now();
    {
        let op_ids = op_id_map();
        let mut s = Writer::new();
        s.count(module.codes.0.len());
        for (index, body) in module.codes.0.iter().enumerate() {
            match body {
                FunctionBody::Wasm(func) => {
                    write_locals(&mut s, &func.locals);
                    write_lowered(&mut s, &func.lowered, &op_ids)?;
                }
                FunctionBody::Host(_) => {
                    return Err(PrecompiledError::HostFunctionBody { index });
                }
            }
        }
        write_section(&mut w, SEC_CODES, &s);
    }
    let functions_done = Instant::now();

    if let Some(t) = timings {
        t.header = header_done - total_start;
        t.sections = sections_done - header_done;
        t.functions = functions_done - sections_done;
        t.total = total_start.elapsed();
    }
    Ok(w.buf)
}

/// Deserializes a precompiled artifact back into a [`Module`].
///
/// The artifact's build fingerprint must match this build exactly; any
/// mismatch, truncation, or corruption is a typed [`PrecompiledError`].
/// There is no fallback to parsing.
pub fn deserialize_module(bytes: &[u8]) -> Result<Module> {
    deserialize_module_timed(bytes, None)
}

/// [`deserialize_module`] with per-phase timings recorded into `timings`.
pub fn deserialize_module_timed(
    bytes: &[u8],
    timings: Option<&mut PrecompiledTimings>,
) -> Result<Module> {
    let total_start = Instant::now();
    let mut r = Reader::new(bytes);
    if r.bytes(MAGIC.len())? != MAGIC {
        return Err(PrecompiledError::BadMagic);
    }
    let version = r.leb_u32()?;
    if version != FORMAT_VERSION {
        return Err(PrecompiledError::UnsupportedFormatVersion {
            found: version,
            supported: FORMAT_VERSION,
        });
    }
    let check_fingerprint = |field: &'static str, artifact: String, current: &str| -> Result<()> {
        if artifact != current {
            return Err(PrecompiledError::FingerprintMismatch {
                field,
                artifact,
                current: current.to_string(),
            });
        }
        Ok(())
    };
    check_fingerprint("crate version", r.string()?, env!("CARGO_PKG_VERSION"))?;
    check_fingerprint(
        "rustc version",
        r.string()?,
        env!("TELOMERE_BUILD_RUSTC_VERSION"),
    )?;
    check_fingerprint("target triple", r.string()?, env!("TELOMERE_BUILD_TARGET"))?;
    check_fingerprint("feature set", r.string()?, &feature_set_string())?;
    check_fingerprint(
        "source hash",
        r.string()?,
        env!("TELOMERE_BUILD_SOURCE_HASH"),
    )?;
    let artifact_hash = r.fixed_u64()?;
    if artifact_hash != op_registry::OP_TABLE_HASH {
        return Err(PrecompiledError::FingerprintMismatch {
            field: "op table hash",
            artifact: format!("{artifact_hash:#018x}"),
            current: format!("{:#018x}", op_registry::OP_TABLE_HASH),
        });
    }
    let header_done = Instant::now();
    let fts = {
        let mut s = r.section(SEC_TYPES)?;
        let n = s.count()?;
        let mut types = Vec::with_capacity(n);
        for _ in 0..n {
            types.push(read_func_type(&mut s)?);
        }
        expect_section_consumed(&s)?;
        TypeSection(types)
    };
    let functions = {
        let mut s = r.section(SEC_FUNCTIONS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(TypeIdx(s.leb_u32()?));
        }
        expect_section_consumed(&s)?;
        out
    };
    let imports = {
        let mut s = r.section(SEC_IMPORTS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(Import {
                module: s.string()?,
                name: s.string()?,
                desc: read_import_desc(&mut s)?,
            });
        }
        expect_section_consumed(&s)?;
        ImportSection(out)
    };
    let mems = {
        let mut s = r.section(SEC_MEMS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(read_mem_type(&mut s)?);
        }
        expect_section_consumed(&s)?;
        out
    };
    let (globals, global_init) = {
        let mut s = r.section(SEC_GLOBALS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(read_global_type(&mut s)?);
        }
        let m = s.count()?;
        let mut inits = Vec::with_capacity(m);
        for _ in 0..m {
            inits.push(read_const_expr(&mut s)?);
        }
        expect_section_consumed(&s)?;
        (out, inits)
    };
    let exs = {
        let mut s = r.section(SEC_EXPORTS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(Export(s.string()?, read_export_desc(&mut s)?));
        }
        expect_section_consumed(&s)?;
        ExportSection(out)
    };
    let tables = {
        let mut s = r.section(SEC_TABLES)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(read_table_type(&mut s)?);
        }
        expect_section_consumed(&s)?;
        out
    };
    let elems = {
        let mut s = r.section(SEC_ELEMS)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(read_elem(&mut s)?);
        }
        expect_section_consumed(&s)?;
        ElementSection(out)
    };
    let data = {
        let mut s = r.section(SEC_DATA)?;
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(read_data(&mut s)?);
        }
        expect_section_consumed(&s)?;
        DataSection(out)
    };
    let start = {
        let mut s = r.section(SEC_START)?;
        let out = if s.bool()? {
            Some(FuncIdx(s.leb_u32()?))
        } else {
            None
        };
        expect_section_consumed(&s)?;
        out
    };
    let name = {
        let mut s = r.section(SEC_NAME)?;
        let out = read_name_section(&mut s)?;
        expect_section_consumed(&s)?;
        out
    };
    let sections_done = Instant::now();

    let shape = ModuleShape {
        types: fts.0.len(),
        functions: functions.len(),
        tables: tables.len(),
        mems: mems.len(),
        globals: globals.len(),
    };

    let codes = {
        let mut s = r.section(SEC_CODES)?;
        let ops_by_id = op_by_id_map();
        let n = s.count()?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let locals = read_locals(&mut s)?;
            let lowered = read_lowered(&mut s, &ops_by_id)?;
            validate_lowered(&lowered, &shape)?;
            let materialized = lowered.materialize();
            out.push(FunctionBody::Wasm(Func {
                locals,
                expr: materialized.instrs,
                op_lens: materialized.op_lens,
                lowered: Arc::new(lowered),
            }));
        }
        expect_section_consumed(&s)?;
        CodeSection(out)
    };
    let functions_done = Instant::now();

    if !r.is_done() {
        return Err(PrecompiledError::TrailingBytes {
            trailing: r.remaining(),
        });
    }

    let module = Module {
        fts,
        functions,
        imports,
        mems,
        globals,
        global_init,
        exs,
        tables,
        elems,
        codes,
        data,
        start,
        name,
    };
    validate_module(&module, &shape)?;

    if let Some(t) = timings {
        t.header = header_done - total_start;
        t.sections = sections_done - header_done;
        t.functions = functions_done - sections_done;
        t.total = total_start.elapsed();
    }
    Ok(module)
}

/// Cross-section range validation for a decoded module. A parsed module
/// satisfies all of these by construction; a decoded one must prove it
/// before `instantiate` consumes it.
fn validate_module(module: &Module, shape: &ModuleShape) -> Result<()> {
    for idx in &module.functions {
        if (idx.0 as usize) >= shape.types {
            return Err(PrecompiledError::Corrupt(
                "function type index out of range",
            ));
        }
    }
    let mut imported_functions = 0usize;
    let mut imported_globals = 0usize;
    for import in &module.imports.0 {
        match &import.desc {
            ImportDesc::TypeIdx(idx) => {
                imported_functions += 1;
                if (idx.0 as usize) >= shape.types {
                    return Err(PrecompiledError::Corrupt(
                        "imported function type index out of range",
                    ));
                }
            }
            ImportDesc::GlobalType(_) => imported_globals += 1,
            ImportDesc::TableType(_) | ImportDesc::MemType(_) => {}
        }
    }
    if imported_functions + module.codes.0.len() != shape.functions {
        return Err(PrecompiledError::Corrupt(
            "code section does not match the function index space",
        ));
    }
    if imported_globals + module.global_init.len() != shape.globals {
        return Err(PrecompiledError::Corrupt(
            "global initializers do not match the global index space",
        ));
    }
    if let Some(start) = &module.start {
        if (start.0 as usize) >= shape.functions {
            return Err(PrecompiledError::Corrupt(
                "start function index out of range",
            ));
        }
    }
    for export in &module.exs.0 {
        let in_range = match &export.1 {
            ExportDesc::Func(idx) => (idx.0 as usize) < shape.functions,
            ExportDesc::Table(idx) => (idx.0 as usize) < shape.tables,
            ExportDesc::Mem(idx) => (idx.0 as usize) < shape.mems,
            ExportDesc::Global(idx) => (idx.0 as usize) < shape.globals,
        };
        if !in_range {
            return Err(PrecompiledError::Corrupt("export index out of range"));
        }
    }
    for init in &module.global_init {
        check_const_expr(init, shape)?;
    }
    for elem in &module.elems.0 {
        match &elem.init {
            ElemInit::FuncIdx(funcs) => {
                for f in funcs {
                    if (*f as usize) >= shape.functions {
                        return Err(PrecompiledError::Corrupt(
                            "element function index out of range",
                        ));
                    }
                }
            }
            ElemInit::ConstExpr(exprs) => {
                for expr in exprs {
                    for e in expr {
                        check_const_expr(e, shape)?;
                    }
                }
            }
        }
        if let ElemMode::Active(table, offset) = &elem.mode {
            if (table.0 as usize) >= shape.tables {
                return Err(PrecompiledError::Corrupt(
                    "element table index out of range",
                ));
            }
            for e in offset {
                check_const_expr(e, shape)?;
            }
        }
    }
    for data in &module.data.0 {
        if let DataMode::Active(mem, offset) = &data.mode {
            if (mem.0 as usize) >= shape.mems {
                return Err(PrecompiledError::Corrupt("data memory index out of range"));
            }
            for e in offset {
                check_const_expr(e, shape)?;
            }
        }
    }
    Ok(())
}
