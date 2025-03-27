use std::io::Write;

use super::{ConstExpr, VMResult};

pub struct GlobalStore(pub Vec<u8>);
impl GlobalStore {
    pub(crate) fn init(&mut self, init: &ConstExpr) -> VMResult<u32> {
        let addr: usize = self.0.len();
        match match init {
            ConstExpr::I32(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::I64(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F32(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F64(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::FuncRef(_) => todo!(),
            ConstExpr::GlobalGet(_) => todo!()
        } {
            Ok(_) => VMResult::Success(addr as u32),
            Err(_) => panic!(), //FIXME:
        }
    }
}
pub struct Store {
    pub globals: GlobalStore,
}
impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Store {
            globals: GlobalStore(vec![]),
        }
    }
}
