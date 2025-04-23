use super::Instance;
pub(crate) mod encode;
mod view;
pub use view::*;

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Instance = 2,
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
}

const HEADER_LEN: usize = 1;
impl MemoryPool {
    unsafe fn get_object<'a, T>(&'a self, offset: usize) -> &'a T {
        std::mem::transmute::<*const u32, &'a T>(self.memory.as_ptr().add(offset))
    }
    pub fn new() -> Self {
        Self { memory: vec![] }
    }
    pub fn allocate(&mut self, header: Header) -> GcRef {
        let offset = self.memory.len();
        let expected_len = HEADER_LEN + offset + header.word_size() as usize;
        if expected_len > self.memory.capacity() {
            let additional = expected_len - self.memory.len();
            // FIXME: handle memory allocation fail
            self.memory.try_reserve(additional).unwrap();
        }
        GcRef(offset as u32)
    }
    unsafe fn place(&mut self, header: Header, addr: GcRef, data: &[u32]) {
        let header_pos = self.memory.as_mut_ptr().add(addr.get_usize());
        *header_pos = header.get();
        std::ptr::copy_nonoverlapping(data.as_ptr(), header_pos.add(HEADER_LEN), data.len());
    }
    pub(crate) fn write_header(&mut self, addr: GcRef, header: Header) {
        self.memory[addr.get_usize()] = header.get();
    }
    pub(crate) fn read_header(&mut self, addr: GcRef) -> Header {
        Header(self.memory[addr.get_usize()])
    }
    pub(crate) fn new_instance(&mut self, instance: &Instance) -> GcRef {
        let size = encode::size_of_instance(&instance);
        let dst = self.allocate(Header::new(ObjectType::Instance, size).initialized());
        let value_dst = dst.get_value_addr_usize();
        let mut value_ptr = self.memory[value_dst..].as_mut_ptr();
        unsafe { encode::encode_instance(instance, &mut value_ptr) };
        dst
    }
    pub(crate) unsafe fn place_instance_unchecked(&mut self, dst: GcRef, instance: &Instance) {
        let size = encode::size_of_instance(&instance);
        let value_dst = dst.get_value_addr_usize();
        let mut value_ptr = self.memory[value_dst..].as_mut_ptr();
        unsafe { encode::encode_instance(instance, &mut value_ptr) };
        #[cfg(debug_assertions)]
        {
            let header = self.read_header(dst);
            debug_assert_eq!(header.object_type(), ObjectType::Instance);
        }
        self.write_header(dst, Header::new(ObjectType::Instance, size));
    }
    pub(crate) unsafe fn get_instance_unchecked(&self, addr: GcRef) -> InstanceView {
        InstanceView::from_ptr(self.memory.as_ptr().add(addr.get_value_addr_usize()))
    }
}
