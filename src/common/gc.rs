use std::mem::ManuallyDrop;

use super::Instance;

#[repr(u32)]
#[derive(Debug, Clone, Copy)]
pub enum ObjectType {
    Allocated = 1,
    Instance = 2,
}

pub struct Flags(u32);
impl Flags {
    pub fn object_type(&self) -> ObjectType {
        unsafe { std::mem::transmute(self.0 >> 24) }
    }
    pub fn new(ty: ObjectType, size: usize) -> u32 {
        let ty: u32 = unsafe { std::mem::transmute(ty) };
        ty << 24 | (size as u32)
    }
}
pub struct MemoryPool {
    memory: Vec<u32>,
}
const _: () = {
    ["Instance size"][size_of::<Instance>() - 88];
};
const fn word_size<T>() -> usize {
    size_of::<T>() / 4
}
const HEADER_LEN: usize = 1;
impl MemoryPool {
    unsafe fn get_object<'a, T>(&'a self, offset: usize) -> &'a T {
        std::mem::transmute::<*const u32, &'a T>(self.memory.as_ptr().add(offset))
    }
    pub fn new() -> Self {
        Self { memory: vec![] }
    }
    pub fn allocate(&mut self, size: usize) -> usize {
        let offset = self.memory.len();
        let expected_len = HEADER_LEN + offset + size;
        if expected_len > self.memory.capacity() {
            let additional = expected_len - self.memory.len();
            // FIXME: handle memory allocation fail
            self.memory.try_reserve(additional).unwrap();
        }
        self.memory[offset] = Flags::new(ObjectType::Allocated, size);
        offset
    }
    unsafe fn place(&mut self, header: u32, offset: usize, data: &[u32]) {
        let header_pos = self.memory.as_mut_ptr().add(offset);
        debug_assert_eq!(*header_pos, Flags::new(ObjectType::Allocated, data.len()));
        *header_pos = header;
        std::ptr::copy_nonoverlapping(data.as_ptr(), header_pos.add(HEADER_LEN), data.len());
    }
    pub(crate) fn new_instance(&mut self, instance: Instance) -> u32 {
        unsafe {
            // FIXME: undefined behaviour
            let data = std::mem::transmute::<Instance, [u32; word_size::<Instance>()]>(instance);
            let header = Flags::new(ObjectType::Instance, data.len());

            let offset = self.allocate(data.len());
            self.place(header, offset, &data);
            offset as u32
        }
    }
    pub(crate) fn place_instance(&mut self, instance: Instance, offset: usize) {
        unsafe {
            // FIXME: undefined behaviour
            let data = std::mem::transmute::<Instance, [u32; word_size::<Instance>()]>(instance);
            let header = Flags::new(ObjectType::Instance, data.len());

            self.place(header, offset, &data);
        }
    }
    pub(crate) fn get_instance(&self, offset: usize) -> &Instance {
        unsafe { self.get_object(offset) }
    }
}
