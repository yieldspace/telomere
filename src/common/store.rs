use std::io::Write;

use super::{ConstExpr, GlobalType, VMResult};

pub struct GlobalStore(pub Vec<u8>);
impl GlobalStore {
    pub(crate) fn init(
        &mut self,
        init: &ConstExpr,
        globals: &[u32],
        gts: &[GlobalType],
    ) -> VMResult<u32> {
        let addr: usize = self.0.len();
        match match init {
            ConstExpr::I32(v) => {
                
                self.0.write_all(&v.to_le_bytes())
            },
            ConstExpr::I64(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F32(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F64(v) => self.0.write_all(&v.to_le_bytes()),
            // TODO: is reference be 64bit?
            ConstExpr::RefNull(_t) => self.0.write_all(&u64::to_le_bytes(0)),
            ConstExpr::FuncRef(_) => todo!(),
            ConstExpr::GlobalGet(idx) => {
                let idx = *idx as usize;
                let addr = globals[idx] as usize;
                let gt = gts[idx];
                let new_addr = self.0.len();
                self.0.resize(self.0.len() + gt.0.stack_size().usize(), 0);
                self.0.copy_within(addr..addr + gt.0.stack_size().usize(), new_addr);
                Ok(())
            }
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
