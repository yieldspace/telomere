use super::{
    gc::{GcRef, Header, InstanceData, MemoryPool, ObjectType},
    ConstExpr, Data, Elem, ExportSection, FuncType, FunctionBody, GlobalType, Instance, MemType,
    Memory, TableInstance, TableType, TypeIdx, VMResult,
};
use std::{collections::HashMap, io::Write};

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
    pub instance_addr: GcRef,
    pub funcidx: u32,
    pub body: FunctionBody,
}
pub struct FunctionStore(pub Vec<FunctionInstance>);
#[derive(Debug)]
pub struct ModuleInstance {
    pub exports: ExportSection,
    pub tables: Vec<TableType>,
    pub globals: Vec<GlobalType>,
    pub functions: Vec<TypeIdx>,
    pub function_types: Vec<FuncType>,
    pub mems: Vec<MemType>,
}

pub struct Store {
    pub gc: MemoryPool,
    pub instance_id: u32,
    pub globals: GlobalStore,
    pub funcs: FunctionStore,
    pub modules: Vec<ModuleInstance>,
    pub tables: Vec<TableInstance>,
    pub memory: Vec<Memory>,
    pub data: HashMap<(u32, u32), Data>,
    pub elems: HashMap<(u32, u32), Elem>,
    pub state: StoreState,
}
impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

impl Store {
    pub fn new() -> Self {
        Self::new_with_state(StoreState::default())
    }

    pub fn new_with_state(state: StoreState) -> Self {
        Store {
            globals: GlobalStore(vec![]),
            funcs: FunctionStore(vec![]),
            modules: vec![],
            tables: vec![],
            memory: vec![],
            data: HashMap::new(),
            elems: HashMap::new(),
            gc: MemoryPool::new(),
            instance_id: 0,
            state,
        }
    }
    pub(crate) unsafe fn get_instance_unchecked(&self, addr: GcRef) -> *const InstanceData {
        self.gc.get_instance_unchecked(addr)
    }
    pub(crate) fn allocate(&mut self, object_type: ObjectType, size: usize) -> GcRef {
        self.gc.allocate(Header::new(object_type, size))
    }
    pub(crate) unsafe fn place_instance_unchecked(&mut self, addr: GcRef, instance: &Instance) {
        self.gc.place_instance_unchecked(addr, instance)
    }
    pub(crate) fn new_instance_id(&mut self) -> u32 {
        let instance_id = self.instance_id;
        self.instance_id += 1;
        instance_id
    }
}

#[derive(Default)]
pub struct StoreState(usize);

impl StoreState {
    pub fn new<'a, 'b, T>(data: Option<&'a T>) -> Self
    where
        Self: 'b,
        'a: 'b,
    {
        if let Some(data) = data {
            StoreState(data as *const T as usize)
        } else {
            StoreState(0)
        }
    }

    #[inline]
    pub fn get<T>(&self) -> Option<&T> {
        let ptr = self.0 as *const T;
        unsafe { ptr.as_ref() }
    }

    #[inline]
    pub fn get_mut<T>(&self) -> Option<&mut T> {
        let ptr = self.0 as *mut T;
        unsafe { ptr.as_mut() }
    }
}

#[cfg(test)]
mod test {
    use crate::Instance;

    #[test]
    fn test_state() {
        use super::StoreState;
        let data = vec![1, 2, 3];
        let state = StoreState::new(Some(&data));
        unsafe {
            assert_eq!(state.get::<Vec<i32>>().unwrap(), &data);
        }
    }
}
