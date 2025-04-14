use std::{collections::HashMap, io::Write};

use super::{
    ConstExpr, Data, Elem, ExportSection, FuncType, FunctionBody, GlobalType, Instance, MemType,
    Memory, TableInstance, TableType, TypeIdx, VMResult,
};

pub struct GlobalStore(pub Vec<u8>);
impl GlobalStore {
    pub(crate) fn init(
        &mut self,
        init: &ConstExpr,
        globals: &[u32],
        funcs: &[u32],
        gts: &[GlobalType],
    ) -> VMResult<u32> {
        let addr: usize = self.0.len();
        tracing::trace!("global init: {init:?}");

        match match init {
            ConstExpr::I32(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::I64(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F32(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::F64(v) => self.0.write_all(&v.to_le_bytes()),
            ConstExpr::RefNull(_t) => self.0.write_all(&u32::to_le_bytes(0)),
            ConstExpr::FuncRef(v) => {
                let addr = funcs.get(*v as usize);
                if let Some(addr) = addr {
                    self.0.write_all(&addr.to_le_bytes())
                } else {
                    return VMResult::InvalidOperand;
                }
            }
            ConstExpr::GlobalGet(idx) => {
                let idx = *idx as usize;
                let addr = globals[idx] as usize;
                let gt = gts[idx];
                let new_addr = self.0.len();
                self.0.resize(self.0.len() + gt.0.stack_size().usize(), 0);
                self.0
                    .copy_within(addr..addr + gt.0.stack_size().usize(), new_addr);
                Ok(())
            }
        } {
            Ok(_) => VMResult::Success(addr as u32),
            Err(_) => panic!(), //FIXME:
        }
    }
}

pub struct FunctionInstance {
    pub instance_addr: u32,
    pub funcidx: u32,
    pub body: FunctionBody,
}
pub struct FunctionStore(pub Vec<FunctionInstance>);
pub struct ModuleInstance {
    pub exports: ExportSection,
    pub tables: Vec<TableType>,
    pub globals: Vec<GlobalType>,
    pub functions: Vec<TypeIdx>,
    pub function_types: Vec<FuncType>,
    pub data: Vec<Data>,
    //pub elem: Vec<Elem>,
    pub mems: Vec<MemType>,
}
pub struct Store {
    pub globals: GlobalStore,
    pub funcs: FunctionStore,
    pub modules: Vec<ModuleInstance>,
    pub instances: Vec<Instance>,
    pub tables: Vec<TableInstance>,
    pub memory: Vec<Memory>,
    pub elems: HashMap<(u32, u32), Elem>,
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
            funcs: FunctionStore(vec![]),
            modules: vec![],
            instances: vec![],
            tables: vec![],
            memory: vec![],
            elems: HashMap::new(),
        }
    }
}
