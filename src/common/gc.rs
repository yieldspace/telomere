use super::Instance;
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
#[derive(Debug, Clone, Copy)]
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
    pub fn is_initialized(&self) -> bool {
        self.0 & INIT_MASK != 0
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
    allocated: u32,
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
                Header::new(ObjectType::RootTable, 2).initialized().get(),
                0,
                3,
                Header::new(ObjectType::Raw, 0).initialized().get(),
            ],
            allocated: 4,
            root: GcRef(0),
        }
    }
    pub fn allocate(&mut self, header: Header) -> GcRef {
        let offset: usize = self.allocated as usize;

        let expected_len = HEADER_LEN + offset + header.word_size() as usize;
        if expected_len > self.memory.capacity() {
            let additional = expected_len - self.memory.len();
            // FIXME: handle memory allocation fail
            self.memory.try_reserve_exact(additional).unwrap();
            self.memory.resize(self.memory.capacity(), 0);
        }
        self.memory[offset] = header.get();
        self.allocated = expected_len as u32;
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
    fn new_raw_region(&mut self, data: *const u32, len: usize) -> GcRef {
        let addr = self.allocate(Header::new(ObjectType::Raw, len).initialized());
        unsafe { self.write(addr, 0, data, len) };
        addr
    }
    fn new_u32_fixed_array(&mut self, data: &[u32]) -> U32FixedArray {
        U32FixedArray(self.new_raw_region(data.as_ptr() as *const u32, data.len()))
    }
    fn gc_ref_fixed_array(&mut self, data: &[GcRef]) -> GcRefFixedArray {
        GcRefFixedArray(self.new_raw_region(data.as_ptr() as *const u32, data.len()))
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
    pub(crate) unsafe fn write(
        &mut self,
        addr: GcRef,
        offset: usize,
        values: *const u32,
        len: usize,
    ) {
        std::ptr::copy_nonoverlapping(
            values,
            self.memory
                .as_mut_ptr()
                .add(addr.get_value_addr_usize())
                .add(offset),
            len,
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
        let new_region = self.allocate(Header::new(ObjectType::Raw, new_cap as usize).initialized());
        self.relocate(old_region, new_region);
        new_region
    }
    unsafe fn get_u32_dynamic_array(&self,obj: GcRef,offset: usize) -> &U32DynamicArray{
        (self.memory.as_ptr().add(obj.get_value_addr_usize() + offset) as *const U32DynamicArray).as_ref().unwrap_unchecked()
    }
    unsafe fn get_u32_dynamic_array_mut(&mut self,obj: GcRef,offset: usize) -> &mut U32DynamicArray{
        (self.memory.as_mut_ptr().add(obj.get_value_addr_usize() + offset) as *mut U32DynamicArray).as_mut().unwrap_unchecked()
    }
    pub(crate) unsafe fn u32_array_push_vec(
        &mut self,
        obj: GcRef,
        offset: usize,
        values: &[u32],
    ) {
        let array = self.get_u32_dynamic_array(obj,offset);
        let old_region = array.array.0;
        let old_len = array.len;
        let expected_len = array.len + values.len() as u32;
        let dst_ref = if (array.cap(self) as u32) < expected_len {
            let new_cap = expected_len * 2;
            self.raw_region_extend(old_region, new_cap)
        } else {
            array.array.0
        };
        self.write(
            dst_ref,
            old_len as usize,
            values.as_ptr() as *const u32,
            values.len(),
        );
        let array = &mut *self.get_u32_dynamic_array_mut(obj,offset);

        array.array = U32FixedArray(dst_ref);
        array.len = expected_len;
    }
    pub(crate) unsafe fn gc_ref_array_push_vec(
        &mut self,
        obj: GcRef,
        offset: usize,
        values: &[GcRef],
    ) {
        let array = self.get_ref_dynamic_array(obj,offset);
        let old_region = array.array.0;
        let old_len = array.len;
        let expected_len = array.len + values.len() as u32;
        let dst_ref = if (array.cap(self) as u32) < expected_len {
            let new_cap = expected_len * 2;
            self.raw_region_extend(old_region, new_cap)
        } else {
            array.array.0
        };
        self.write(
            dst_ref,
            old_len as usize,
            values.as_ptr() as *const u32,
            values.len(),
        );
        let array = &mut *self.get_ref_dynamic_array_mut(obj,offset);

        array.array = GcRefFixedArray(dst_ref);
        array.len = expected_len;
    }
    unsafe fn get_ref_dynamic_array(&self,item: GcRef,offset: usize) -> &GcRefDynamicArray{
        let r = self.memory.as_ptr().add(item.get_value_addr_usize()+offset) as *const GcRefDynamicArray;
        r.as_ref().unwrap_unchecked()
    }
    unsafe fn get_ref_dynamic_array_mut(&mut self,item: GcRef,offset: usize) -> &mut GcRefDynamicArray{
        (self.memory.as_mut_ptr().add(item.get_value_addr_usize()+offset) as *mut GcRefDynamicArray).as_mut().unwrap_unchecked()
    }
    // NOTE: It is the responsibility of each view to trace the raw section data.
    pub(crate) fn trace(&mut self, item: GcRef) {
        if self.mark(item) {
            return; // return if already marked
        }
        let header = self.read_header(item);
        match header.object_type() {
            ObjectType::Instance => (unsafe { &*self.get_instance_unchecked(item) }).trace(self),
            ObjectType::Raw => {
                // do nothing
            }
            ObjectType::RootTable => {
                // lifetime escape technique
                unsafe  { &*(self.get_ref_dynamic_array(item,0) as *const GcRefDynamicArray)}.trace(self);
            }
        }
    }
    pub fn mark_phase(&mut self){
        self.trace(self.root);
    }
    pub fn add_root(&mut self,item: &[GcRef]) {
        unsafe { self.gc_ref_array_push_vec(self.root, 0,item) };
    }
    pub fn iter(&self) -> impl Iterator<Item = GcRef> + use<'_>{
        let mut index = 0;
        std::iter::from_fn(move ||{
            let r =  if index == self.allocated {
                return None
            }else{
                GcRef(index)
            };
            let header = self.read_header(r);
            index += HEADER_LEN as u32+ header.word_size() as u32;
            Some(r)
        })
    }
}
#[cfg(test)]
mod tests{
    use super::MemoryPool;

    #[test]
    fn test_mark(){
        let mut pool = MemoryPool::new();
        let free_arr = pool.new_u32_fixed_array(&[1,2,3]);
        let marked_arr = pool.new_u32_fixed_array(&[1,2,3]);
        pool.add_root(&[marked_arr.0]);
        for addr in pool.iter(){
            let header =  pool.read_header(addr);
            tracing::trace!("{addr:?}: {header:?} ({:?},{},init={},marked={})",header.object_type(),header.word_size(),header.is_initialized(),header.is_marked());

            assert!(header.is_initialized());
            assert!(!header.is_marked());
        }
        pool.mark_phase();
        let mut marked = vec![];
        let mut free = vec![];
        for addr in pool.iter(){
            let header =  pool.read_header(addr);
            tracing::trace!("{addr:?}: {header:?} ({:?},{},init={},marked={})",header.object_type(),header.word_size(),header.is_initialized(),header.is_marked());

            assert!(header.is_initialized());
            if header.is_marked(){
                marked.push(addr);
            }else{
                free.push(addr);
            }
        }
        assert!(marked.contains(&marked_arr.0));
        assert_eq!(marked.len(), 3);
        assert!(free.contains(&free_arr.0));
        assert_eq!(free.len(),2);
    }
}