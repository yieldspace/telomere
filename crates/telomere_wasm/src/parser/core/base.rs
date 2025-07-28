use crate::common::BlockType;
use crate::common::FuncType;
use crate::common::GlobalType;
use crate::common::MemType;
use crate::common::RefType;
use crate::common::TableType;
use crate::common::ValType;
use binary_reader::BinaryReader;

use super::types;
use super::values;
use super::Result;
pub trait WasmBaseParser<R: BinaryReader>
where
    Self: Sized,
{
    fn reader(&mut self) -> &mut R;
    fn parse_u32(&mut self) -> Result<(usize, u32)> {
        values::parse_u32(self.reader())
    }
    fn parse_i32(&mut self) -> Result<(usize, i32)> {
        values::parse_i32(self.reader())
    }
    fn parse_i64(&mut self) -> Result<(usize, i64)> {
        values::parse_i64(self.reader())
    }
    fn parse_f32(&mut self) -> Result<(usize, f32)> {
        values::parse_f32(self.reader())
    }
    fn parse_f64(&mut self) -> Result<(usize, f64)> {
        values::parse_f64(self.reader())
    }
    fn parse_vec<'a, V>(
        &'a mut self,
        f: impl for<'b> FnMut(&'b mut Self) -> Result<(usize, V)>,
    ) -> Result<(usize, Vec<V>)> {
        values::parse_vec(self, |v| v.reader(), f)
    }
    fn parse_byte(&mut self) -> Result<(usize, u8)> {
        values::parse_byte(self.reader())
    }
    fn parse_name(&mut self) -> Result<(usize, String)> {
        values::parse_name(self.reader())
    }
    fn parse_valtype(&mut self) -> Result<(usize, ValType)> {
        types::parse_valtype(self.reader())
    }
    fn parse_reftype(&mut self) -> Result<(usize, RefType)> {
        types::parse_ref_type(self.reader())
    }
    fn parse_functype(&mut self) -> Result<(usize, FuncType)> {
        types::parse_functype(self.reader())
    }
    fn parse_global_type(&mut self) -> Result<(usize, GlobalType)> {
        types::parse_global_type(self.reader())
    }
    fn parse_table_type(&mut self) -> Result<(usize, TableType)> {
        types::parse_table_type(self.reader())
    }
    fn parse_memtype(&mut self) -> Result<(usize, MemType)> {
        types::parse_memtype(self.reader())
    }
    fn parse_block_type(&mut self) -> Result<(usize, BlockType)> {
        types::parse_block_type(self.reader())
    }
}
