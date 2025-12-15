use super::{
    gc::MemoryPool, Data, Elem, ExportSection, FuncType, GlobalType, MemType, TableType, TypeIdx,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

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
    pub gc: Rc<RefCell<MemoryPool>>,
    pub instance_id: u32,
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
            data: HashMap::new(),
            elems: HashMap::new(),
            gc: Rc::new(RefCell::new(MemoryPool::new())),
            instance_id: 0,
            state,
        }
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
}

#[cfg(test)]
mod test {
    #[test]
    fn test_state() {
        use super::StoreState;
        let data = vec![1, 2, 3];
        let state = StoreState::new(Some(&data));
        assert_eq!(state.get::<Vec<i32>>().unwrap(), &data);
    }
}
