use std::io::Write;

use super::{VMResult, WasmValue};

pub struct GlobalStore(pub Vec<u8>);
impl GlobalStore {
    pub(crate) fn init(&mut self, init: &WasmValue) -> VMResult<u32> {
        let addr: usize = self.0.len();
        match match init {
            WasmValue::I32(v) => self.0.write_all(&v.to_le_bytes()),
            WasmValue::I64(v) => self.0.write_all(&v.to_le_bytes()),
            WasmValue::F32(v) => self.0.write_all(&v.to_le_bytes()),
            WasmValue::F64(v) => self.0.write_all(&v.to_le_bytes()),
            WasmValue::FuncRef(_) => todo!(),
        } {
            Ok(_) => VMResult::Success(addr as u32),
            Err(_) => panic!(), //FIXME:
        }
    }
}
pub struct Store {
    pub globals: GlobalStore,
}
impl Store {
    pub fn new() -> Self {
        Store {
            globals: GlobalStore(vec![]),
        }
    }
}
