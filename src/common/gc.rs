use super::{memory, Instance};
mod view;
pub use view::*;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Raw = 1,
    Instance = 2,
    RootTable = 3,
}
const INIT_MASK: u32 = 1 << 30;
const MARK_MASK: u32 = 1 << 29;
const UNMARK_MASK: u32 = !MARK_MASK;
const SIZE_MASK: u32 = 0xFFFF;
const TYPE_MASK: u32 = 0x1FFF;
const TYPE_LOWER_BIT: u32 = 16;
#[repr(transparent)]
pub struct Header(u32);
impl Header {
    pub fn object_type(&self) -> ObjectType {
        unsafe { std::mem::transmute((self.0 >> TYPE_LOWER_BIT) & TYPE_MASK) }
    }
    pub fn word_size(&self) -> u16 {
        (self.0 & SIZE_MASK) as u16
    }
    pub fn is_marked(&self) -> bool {
        self.0 & MARK_MASK != 0
    }
    pub fn marked(self) -> Self {
        Self(self.0 | MARK_MASK)
    }
    pub fn unmarked(self) -> Self {
        Self(self.0 & UNMARK_MASK)
    }
    pub fn initialized(self) -> Self {
        Self(self.0 | INIT_MASK)
    }
    pub fn new(ty: ObjectType, size: usize) -> Header {
        let ty: u32 = unsafe { std::mem::transmute(ty) };
        if size > u16::MAX.into() {
            panic!()
        }
        Header(ty << TYPE_LOWER_BIT | (size as u32))
    }
    pub fn get(&self) -> u32 {
        self.0
    }
}
pub struct MemoryPool {
    memory: Vec<u32>,
    allocated: usize,
    root: GcRef,
}
pub const fn word_size<T>() -> usize {
    std::mem::size_of::<T>() / std::mem::size_of::<u32>()
}
const HEADER_LEN: usize = 1;
impl MemoryPool {
    pub fn new() -> Self {
        Self {
            memory: vec![
                Header::new(ObjectType::RootTable, 3).get(),
                0,
                0,
                4,
                Header::new(ObjectType::Raw, 0).get(),
            ],
            allocated: 5,
            root: GcRef(0),
        }
    }
    pub fn allocate(&mut self, header: Header) -> GcRef {
        let offset: usize = self.allocated;

        let expected_len = HEADER_LEN + offset + header.word_size() as usize;
        if expected_len > self.memory.capacity() {
            let additional = expected_len - self.memory.len();
            // FIXME: handle memory allocation fail
            self.memory.try_reserve_exact(additional).unwrap();
            self.memory.resize(self.memory.capacity(), 0);
        }
        self.memory[offset] = header.get();
        self.allocated = expected_len;
        tracing::trace!(
            "pool[{offset}] = {:?} {}",
            header.object_type(),
            header.word_size()
        );

        GcRef(offset as u32)
    }
    pub(crate) fn write_header(&mut self, addr: GcRef, header: Header) {
        unsafe { std::ptr::write(self.memory.as_mut_ptr().add(addr.get_usize()), header.get()) };
    }
    pub(crate) fn read_header(&self, addr: GcRef) -> Header {
        Header(self.memory[addr.get_usize()])
    }
    pub(crate) fn mark(&mut self, addr: GcRef) -> bool {
        let header = self.read_header(addr);
        let is_marked = header.is_marked();
        self.write_header(addr, header.marked());
        is_marked
    }
    fn new_raw_region(&mut self, data: &[u32]) -> GcRef {
        let addr = self.allocate(Header::new(ObjectType::Raw, data.len()).initialized());
        unsafe { self.write(addr, 0, data) };
        addr
    }
    fn new_u32_fixed_array(&mut self, data: &[u32]) -> U32FixedArray {
        U32FixedArray(self.new_raw_region(data))
    }
    pub(crate) fn new_instance(&mut self, instance: &Instance) -> GcRef {
        let size = word_size::<InstanceData>();
        let dst = self.allocate(Header::new(ObjectType::Instance, size).initialized());
        let value_dst = dst.get_value_addr_usize();
        let instance_data = InstanceData {
            instance_id: instance.instance_id,
            funcs: self.new_u32_fixed_array(&instance.funcs),
            globals: self.new_u32_fixed_array(&instance.globals),
            mems: self.new_u32_fixed_array(&instance.memory.iter().copied().collect::<Vec<_>>()),
            module_addr: instance.module_addr,
            tables: self.new_u32_fixed_array(&instance.tables),
        };
        unsafe {
            let value_ptr = self.memory[value_dst..].as_mut_ptr() as *mut InstanceData;
            std::ptr::write(value_ptr, instance_data);
        }
        dst
    }
    pub(crate) unsafe fn place_instance_unchecked(&mut self, dst: GcRef, instance: &Instance) {
        let size = word_size::<InstanceData>();
        #[cfg(debug_assertions)]
        {
            let header = self.read_header(dst);
            debug_assert_eq!(header.object_type(), ObjectType::Instance);
            debug_assert_eq!(header.word_size() as usize, size);
        }
        let instance_data = InstanceData {
            instance_id: instance.instance_id,
            funcs: self.new_u32_fixed_array(&instance.funcs),
            globals: self.new_u32_fixed_array(&instance.globals),
            mems: self.new_u32_fixed_array(&instance.memory.iter().copied().collect::<Vec<_>>()),
            module_addr: instance.module_addr,
            tables: self.new_u32_fixed_array(&instance.tables),
        };
        let value_ptr =
            self.memory.as_mut_ptr().add(dst.get_value_addr_usize()) as *mut InstanceData;
        unsafe {
            std::ptr::write(value_ptr, instance_data);
        }
        self.get_instance_unchecked(dst);
        self.write_header(dst, Header::new(ObjectType::Instance, size).initialized());
    }
    pub(crate) unsafe fn get_instance_unchecked(&self, addr: GcRef) -> *const InstanceData {
        tracing::trace!("get_instance_unchecked: {addr:?}");

        let ptr = self.memory.as_ptr().add(addr.get_value_addr_usize()) as *const InstanceData;
        tracing::trace!("get_instance_unchecked: {:?} {:?}", ptr, (*ptr).module_addr);

        ptr
    }
    pub(crate) unsafe fn write(&mut self, addr: GcRef, offset: usize, values: &[u32]) {
        std::ptr::copy_nonoverlapping(
            values.as_ptr(),
            self.memory
                .as_mut_ptr()
                .add(addr.get_value_addr_usize())
                .add(offset),
            values.len(),
        );
    }
    pub(crate) unsafe fn relocate(&mut self, old: GcRef, new: GcRef) {
        std::ptr::copy_nonoverlapping(
            self.memory.as_ptr().add(old.get_value_addr_usize()),
            self.memory.as_mut_ptr().add(new.get_value_addr_usize()),
            self.read_header(old).word_size().into(),
        );
    }
    pub(crate) unsafe fn raw_region_extend(&mut self, old_region: GcRef, new_cap: u32) -> GcRef {
        let new_region = self.allocate(Header::new(ObjectType::Raw, new_cap as usize));
        self.relocate(old_region, new_region);
        new_region
    }
    // TODO:
    /*pub(crate) unsafe fn push_vec(&mut self, array: &mut GcRefDynamicArray, values: &[u32]) {
        let expected_len = array.len + values.len() as u32;
        let expected_cap = expected_len; // TODO:
        if array.cap < expected_cap {
            self.extend_cap(array, expected_cap);
        }
        self.write(array.data, array.len(), values);
        array.len = expected_len
    }*/
}
