use crate::{
    common::{
        gc::{header::PADDING_MASK, object::GcRefFixedArray, HEADER_LEN},
        word_size, Instr, LocalsData, Memory, ModuleInstance, TableInstance, TableType, PAGE_SIZE,
    },
    Instance,
};

#[cfg(test)]
use super::object::U32FixedArray;
use super::{
    object::{
        FunctionInstanceData, GcRefDynamicArray, Global16Data, Global4Data, Global8Data,
        GlobalRefData, InstanceData, RootTable,
    },
    GcRef, GcView, Header, ObjectType,
};
#[derive(Debug)]
pub struct MemoryPool {
    pub(crate) memory: Vec<u32>,
    wasm_linear_memory: Vec<Option<Memory>>,
    wasm_table: Vec<Option<TableInstance>>,
    wasm_module: Vec<Option<ModuleInstance>>,
    allocated: u32,
    root: GcRef,
}

#[allow(dead_code)]
impl Default for MemoryPool {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryPool {
    pub fn new() -> Self {
        let rt_header = Header::new(ObjectType::RootTable, 2).initialized().get();
        let memory = vec![0xFFFFFFFF, rt_header[0], rt_header[1], 0, 0];
        let allocated = memory.len() as u32;
        Self {
            memory,
            allocated,
            wasm_linear_memory: vec![],
            wasm_table: vec![],
            wasm_module: vec![],
            root: GcRef(1),
        }
    }
    pub fn allocate(&mut self, header: Header) -> GcRef {
        let padding_offset = self.allocated as usize;
        let mut offset: usize = self.allocated as usize;
        if header.is_align64() && offset % 2 != 0 {
            offset += 1;
        }
        let expected_len = HEADER_LEN + offset + header.word_size() as usize;
        if expected_len > self.memory.capacity() {
            let additional = expected_len - self.memory.len();
            // FIXME: handle memory allocation fail
            self.memory.try_reserve_exact(additional).unwrap();
            self.memory.resize(self.memory.capacity(), 0);
        }
        self.memory[padding_offset..offset].fill(PADDING_MASK);
        self.memory[offset..offset + HEADER_LEN].copy_from_slice(&header.get());
        self.allocated = expected_len as u32;
        tracing::trace!(
            "pool[{offset}] = {:?} {}",
            header.object_type(),
            header.word_size()
        );

        GcRef(offset as u32)
    }
    pub(crate) fn write_header(&mut self, addr: GcRef, header: Header) {
        unsafe {
            std::ptr::copy_nonoverlapping(
                header.get().as_ptr(),
                self.memory.as_mut_ptr().add(addr.get_usize()),
                HEADER_LEN,
            )
        };
    }
    pub(crate) fn read_header(&self, addr: GcRef) -> Header {
        debug_assert!(!addr.is_null());
        let v = self.memory[addr.get_usize()];
        if v & PADDING_MASK != 0 {
            Header::from_raw(v, 0)
        } else {
            Header::from_raw(v, self.memory[addr.get_usize() + 1])
        }
    }
    pub(crate) fn mark(&mut self, addr: GcRef) -> bool {
        if addr.is_null() {
            return false;
        }
        let header = self.read_header(addr);
        if !header.is_padding() {
            let is_marked = header.is_marked();
            self.write_header(addr, header.marked());
            is_marked
        } else {
            unreachable!()
        }
    }
    fn new_raw_region(&mut self, data: *const u32, len: usize) -> GcRef {
        let addr = self.allocate(Header::new(ObjectType::Raw, len).initialized());
        unsafe { self.write(addr, 0, data, len) };
        addr
    }
    #[cfg(test)]
    fn new_u32_fixed_array(&mut self, data: &[u32]) -> U32FixedArray {
        U32FixedArray(self.new_raw_region(data.as_ptr(), data.len()))
    }
    fn new_gc_ref_fixed_array(&mut self, data: &[GcRef]) -> GcRefFixedArray {
        GcRefFixedArray(self.new_raw_region(data.as_ptr() as *const u32, data.len()))
    }
    pub(crate) fn new_instance(&mut self, instance: &Instance) -> GcRef {
        let size = word_size::<InstanceData>();
        let dst = self.allocate(Header::new(ObjectType::Instance, size).initialized());
        let value_dst = dst.get_value_addr_usize();
        let instance_data = InstanceData {
            instance_id: instance.instance_id,
            funcs: self.new_gc_ref_fixed_array(&instance.funcs),
            globals: self.new_gc_ref_fixed_array(&instance.globals),
            mems: self.new_gc_ref_fixed_array(&instance.memory.to_vec()),
            module_addr: instance.module_addr,
            tables: self.new_gc_ref_fixed_array(&instance.tables),
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
            funcs: self.new_gc_ref_fixed_array(&instance.funcs),
            globals: self.new_gc_ref_fixed_array(&instance.globals),
            mems: self.new_gc_ref_fixed_array(&instance.memory.to_vec()),
            module_addr: instance.module_addr,
            tables: self.new_gc_ref_fixed_array(&instance.tables),
        };
        let value_ptr =
            self.memory.as_mut_ptr().add(dst.get_value_addr_usize()) as *mut InstanceData;
        unsafe {
            std::ptr::write(value_ptr, instance_data);
        }
        self.get_instance_unchecked(dst);
        self.write_header(dst, Header::new(ObjectType::Instance, size).initialized());
    }
    /// # Safety
    ///
    /// The caller must ensure that `addr` points to an allocated object in this
    /// pool and that `offset` addresses a valid, properly aligned `T` within the
    /// object's value region for the duration of the returned pointer use.
    pub unsafe fn get_value<T>(&self, addr: GcRef, offset: usize) -> *const T {
        self.memory
            .as_ptr()
            .add(addr.get_value_addr_usize() + offset) as *const T
    }
    pub(crate) unsafe fn get_value_mut<T>(&mut self, addr: GcRef, offset: usize) -> *mut T {
        self.memory
            .as_mut_ptr()
            .add(addr.get_value_addr_usize() + offset) as *mut T
    }
    pub(crate) unsafe fn get_instance_unchecked(&self, addr: GcRef) -> *const InstanceData {
        self.memory.as_ptr().add(addr.get_value_addr_usize()) as *const InstanceData
    }
    pub(crate) unsafe fn get_instance_mut_unchecked(&mut self, addr: GcRef) -> *mut InstanceData {
        self.memory.as_mut_ptr().add(addr.get_value_addr_usize()) as *mut InstanceData
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
        let new_region =
            self.allocate(Header::new(ObjectType::Raw, new_cap as usize).initialized());
        if !old_region.is_null() {
            self.relocate(old_region, new_region);
        }
        new_region
    }
    pub(crate) unsafe fn gc_ref_array_push_vec(
        &mut self,
        obj: GcRef,
        offset: usize,
        values: &[GcRef],
    ) {
        debug_assert!(!obj.is_null());
        tracing::trace!("gc_ref_array_push_vec: {values:?}");
        let array = self.get_ref_dynamic_array(obj, offset);
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
        let array = &mut *self.get_ref_dynamic_array_mut(obj, offset);
        array.array = GcRefFixedArray(dst_ref);
        array.len = expected_len;
        self.get_ref_dynamic_array_mut(obj, offset);
    }
    unsafe fn get_ref_dynamic_array(&self, item: GcRef, offset: usize) -> &GcRefDynamicArray {
        debug_assert!(!item.is_null());

        let r = self
            .memory
            .as_ptr()
            .add(item.get_value_addr_usize() + offset) as *const GcRefDynamicArray;
        tracing::trace!("{:?}", *r);

        r.as_ref().unwrap_unchecked()
    }
    unsafe fn get_ref_dynamic_array_mut(
        &mut self,
        item: GcRef,
        offset: usize,
    ) -> &mut GcRefDynamicArray {
        debug_assert!(!item.is_null());
        (self
            .memory
            .as_mut_ptr()
            .add(item.get_value_addr_usize() + offset) as *mut GcRefDynamicArray)
            .as_mut()
            .unwrap_unchecked()
    }
    // NOTE: It is the responsibility of each view to trace the raw section data.
    pub(crate) fn trace(&mut self, item: GcRef) {
        if item.is_null() {
            return;
        }
        if self.mark(item) {
            return; // return if already marked
        }
        let header = self.read_header(item);
        unsafe {
            match header.object_type() {
                ObjectType::Instance => (*self.get_instance_unchecked(item)).trace(self),
                ObjectType::Raw => {
                    // do nothing
                }
                ObjectType::RootTable => (*self.get_value::<RootTable>(item, 0)).trace(self),
                ObjectType::ExternMemoryRef
                | ObjectType::ExternTableRef
                | ObjectType::ExternModuleRef => {
                    // do nothing
                }
                ObjectType::GlobalRef => {
                    (*self.get_value::<GlobalRefData>(item, 0)).trace(self);
                }
                ObjectType::FunctionInstance => {
                    (*self.get_value::<FunctionInstanceData>(item, 0)).trace(self);
                }
            }
        }
    }
    fn mark_phase(&mut self) {
        self.trace(self.root);
    }
    fn compact_phase(&mut self) {
        tracing::trace!("compact");
        let free = self.compute_forward_addr();
        self.update_pointer();
        self.move_object();
        self.allocated = free;
    }
    fn compute_forward_addr(&mut self) -> u32 {
        tracing::trace!("compute_forward_addr");
        let mut free = 1;
        let mut live = 1;
        loop {
            let header = self.read_header(GcRef(live));
            if header.is_padding() {
                live += 1;
                continue;
            }
            let need_padding = header.is_align64() && free % 2 == 1;
            let header = if need_padding {
                free += 1;
                header.need_padding()
            } else {
                header
            };
            if header.is_marked() {
                self.write_header(GcRef(live), header.set_forwarding_pointer(free));
                free += HEADER_LEN as u32 + header.word_size() as u32;
            }
            live += HEADER_LEN as u32 + header.word_size() as u32;
            if live == self.allocated {
                break;
            }
        }
        free
    }
    fn update_pointer(&mut self) {
        tracing::trace!("update_pointer");
        let mut ptr = 1;
        loop {
            let item = GcRef(ptr);
            tracing::trace!("update_pointer: {item:?}");

            let header = self.read_header(item);
            if header.is_padding() {
                ptr += 1;
                continue;
            }
            if header.is_marked() && header.is_initialized() {
                match header.object_type() {
                    ObjectType::Instance => {
                        (unsafe { &mut *self.get_instance_mut_unchecked(item) }).update(self)
                    }
                    ObjectType::Raw => {
                        // do nothing
                    }
                    ObjectType::RootTable => {
                        // lifetime escape technique
                        unsafe {
                            &mut *(self.get_ref_dynamic_array_mut(item, 0)
                                as *mut GcRefDynamicArray)
                        }
                        .update(self);
                    }
                    ObjectType::ExternMemoryRef
                    | ObjectType::ExternTableRef
                    | ObjectType::ExternModuleRef => {
                        // ok
                    }
                    ObjectType::GlobalRef => {
                        let global_ref =
                            unsafe { &mut *self.get_value_mut::<GlobalRefData>(item, 0) };
                        global_ref.update(self);
                    }
                    ObjectType::FunctionInstance => {
                        let global_ref =
                            unsafe { &mut *self.get_value_mut::<FunctionInstanceData>(item, 0) };
                        global_ref.update(self);
                    }
                }
            }
            if !header.is_marked() {
                // perform finalizer
                match header.object_type() {
                    ObjectType::ExternMemoryRef => {
                        let idx = unsafe { *self.memory.as_ptr().add(item.get_value_addr_usize()) };
                        self.wasm_linear_memory[idx as usize] = None;
                    }
                    ObjectType::ExternTableRef => {
                        let idx = unsafe { *self.memory.as_ptr().add(item.get_value_addr_usize()) };
                        self.wasm_table[idx as usize] = None;
                    }
                    ObjectType::ExternModuleRef => {
                        let idx = unsafe { *self.memory.as_ptr().add(item.get_value_addr_usize()) };
                        self.wasm_module[idx as usize] = None;
                    }
                    _ => {}
                }
            }
            ptr += HEADER_LEN as u32 + header.word_size() as u32;
            if ptr == self.allocated {
                return;
            }
        }
    }
    fn move_object(&mut self) {
        tracing::trace!("move_object");
        let mut ptr = 1;
        loop {
            let item = GcRef(ptr);
            let header = self.read_header(item);
            if header.is_padding() {
                ptr += 1;
                continue;
            }

            self.write_header(item, header.unmarked().unneed_padding());
            if header.is_marked() {
                unsafe {
                    if header.is_need_padding() {
                        std::ptr::write(
                            self.memory
                                .as_mut_ptr()
                                .add(header.forwarding_pointer() as usize - 1),
                            PADDING_MASK,
                        );
                    }

                    std::ptr::copy(
                        self.memory.as_ptr().add(item.get_usize()),
                        self.memory
                            .as_mut_ptr()
                            .add(header.forwarding_pointer() as usize),
                        HEADER_LEN + header.word_size() as usize,
                    )
                };
            }
            ptr += HEADER_LEN as u32 + header.word_size() as u32;
            if ptr == self.allocated {
                return;
            }
        }
    }
    pub fn gc(&mut self) {
        self.mark_phase();
        self.compact_phase();
    }
    pub fn reserve_root_slot(&mut self) -> u32 {
        unsafe {
            self.gc_ref_array_push_vec(self.root, 0, &[GcRef(0)]);
            self.get_ref_dynamic_array(self.root, 0).len
        }
    }

    pub fn write_root_slot(&mut self, slot: u32, item: GcRef) {
        let idx = slot.checked_sub(1).expect("root slot ids are 1-based") as usize;
        unsafe {
            *(*self.get_value_mut::<GcRefDynamicArray>(self.root, 0))
                .as_ptr_mut(self)
                .add(idx) = item;
        }
    }

    pub fn read_root_slot(&self, slot: u32) -> GcRef {
        let idx = slot.checked_sub(1).expect("root slot ids are 1-based") as usize;
        unsafe {
            *(*self.get_value::<GcRefDynamicArray>(self.root, 0))
                .as_ptr(self)
                .add(idx)
        }
    }

    #[cfg(test)]
    pub fn add_root(&mut self, item: GcRef) -> u32 {
        let slot = self.reserve_root_slot();
        self.write_root_slot(slot, item);
        slot
    }

    pub fn remove_root(&mut self, slot: u32) {
        self.write_root_slot(slot, GcRef(0));
    }
    #[cfg(test)]
    fn scan_heap(&self) -> impl Iterator<Item = GcRef> + use<'_> {
        let mut index = 1;
        std::iter::from_fn(move || {
            let r = if index == self.allocated {
                return None;
            } else {
                GcRef(index)
            };
            let header = self.read_header(r);
            index += HEADER_LEN as u32 + header.word_size() as u32;
            Some(r)
        })
    }
    pub(crate) fn new_memory(&mut self, page_count: u32, max_page_size: u32) -> GcRef {
        let idx = self.wasm_linear_memory.len() as u32;
        self.wasm_linear_memory
            .push(Some(Memory::new(page_count, max_page_size)));
        let gc_ref = self.allocate(Header::new(ObjectType::ExternMemoryRef, 1).initialized());
        unsafe {
            *self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) = idx;
        }
        gc_ref
    }
    pub(crate) unsafe fn get_memory(&mut self, addr: GcRef) -> &mut Memory {
        let mem_idx = *self.memory.as_ptr().add(addr.get_value_addr_usize());
        self.wasm_linear_memory[mem_idx as usize]
            .as_mut()
            .unwrap_unchecked()
    }
    pub(crate) fn new_table(&mut self, tt: TableType) -> GcRef {
        let idx = self.wasm_table.len() as u32;
        self.wasm_table.push(Some(TableInstance::new(tt)));
        let gc_ref = self.allocate(Header::new(ObjectType::ExternTableRef, 1).initialized());
        unsafe {
            *self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) = idx;
        }
        gc_ref
    }
    pub(crate) unsafe fn get_table(&mut self, addr: GcRef) -> &mut TableInstance {
        let idx = *self.memory.as_ptr().add(addr.get_value_addr_usize());
        self.wasm_table[idx as usize].as_mut().unwrap_unchecked()
    }

    pub(crate) fn new_module(&mut self, instance: ModuleInstance) -> GcRef {
        let idx = self.wasm_module.len() as u32;
        self.wasm_module.push(Some(instance));
        let gc_ref = self.allocate(Header::new(ObjectType::ExternModuleRef, 1).initialized());
        unsafe {
            *self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) = idx;
        }
        gc_ref
    }
    pub(crate) unsafe fn get_module(&self, addr: GcRef) -> &ModuleInstance {
        let idx = *self.memory.as_ptr().add(addr.get_value_addr_usize());
        self.wasm_module[idx as usize].as_ref().unwrap_unchecked()
    }

    pub fn new_global_ref(&mut self, global_ref: GcRef) -> GcRef {
        let gc_ref = self.allocate(
            Header::new(ObjectType::GlobalRef, word_size::<GlobalRefData>()).initialized(),
        );
        unsafe {
            *self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) = global_ref.get();
        }
        gc_ref
    }
    pub fn new_global_data4(&mut self, data: u32) -> GcRef {
        let gc_ref =
            self.allocate(Header::new(ObjectType::Raw, word_size::<Global4Data>()).initialized());
        unsafe {
            *self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) = data;
        }
        gc_ref
    }
    pub fn new_global_data8(&mut self, data: u64) -> GcRef {
        let gc_ref =
            self.allocate(Header::new(ObjectType::Raw, word_size::<Global8Data>()).initialized());
        unsafe {
            let bytes = data.to_le_bytes();
            (self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) as *mut u8)
                .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
        }
        gc_ref
    }
    pub fn new_global_data16(&mut self, data: u128) -> GcRef {
        let gc_ref =
            self.allocate(Header::new(ObjectType::Raw, word_size::<Global16Data>()).initialized());
        unsafe {
            let bytes = data.to_le_bytes();
            (self.memory.as_mut_ptr().add(gc_ref.get_value_addr_usize()) as *mut u8)
                .copy_from_nonoverlapping(bytes.as_ptr(), bytes.len());
        }
        gc_ref
    }
    pub(crate) unsafe fn get_global(&self, addr: GcRef) -> &[u8] {
        let header = self.read_header(addr);
        let size = header.word_size() as usize * std::mem::size_of::<u32>();

        let ptr = self.memory.as_ptr().add(addr.get_value_addr_usize());
        std::slice::from_raw_parts(ptr as *const u8, size)
    }
    pub(crate) unsafe fn get_global_mut(&mut self, addr: GcRef) -> &mut [u8] {
        let header = self.read_header(addr);
        let size = header.word_size() as usize * std::mem::size_of::<u32>();

        let ptr = self.memory.as_mut_ptr().add(addr.get_value_addr_usize());
        std::slice::from_raw_parts_mut(ptr as *mut u8, size)
    }

    pub fn copy_object(&mut self, item: GcRef) -> GcRef {
        let header = self.read_header(item);
        let new_item = self.allocate(header);
        unsafe {
            std::ptr::copy_nonoverlapping(
                self.memory.as_ptr().add(item.get_usize()),
                self.memory.as_mut_ptr().add(new_item.get_usize()),
                HEADER_LEN + header.word_size() as usize,
            )
        };
        new_item
    }
    pub(crate) unsafe fn get_func(&self, addr: GcRef) -> &FunctionInstanceData {
        self.get_value::<FunctionInstanceData>(addr, 0)
            .as_ref()
            .unwrap_unchecked()
    }
    pub(crate) unsafe fn get_func_mut(&mut self, addr: GcRef) -> &mut FunctionInstanceData {
        self.get_value_mut::<FunctionInstanceData>(addr, 0)
            .as_mut()
            .unwrap_unchecked()
    }
    pub(crate) fn new_func(&mut self, data: &FunctionInstanceData) -> GcRef {
        let addr = self.allocate(Header::new(
            ObjectType::FunctionInstance,
            word_size::<FunctionInstanceData>(),
        ));
        unsafe {
            self.write(
                addr,
                0,
                data as *const FunctionInstanceData as *const u32,
                word_size::<FunctionInstanceData>(),
            )
        };
        addr
    }
    pub(crate) fn new_function_body(&mut self, locals: &LocalsData, instr: &[Instr]) -> GcRef {
        let align = align_of::<*mut Instr>();
        let align64 = align == 8;
        let size = locals.word_size()
            + locals.word_size() % (align / 4)
            + instr.len() * word_size::<Instr>();
        let addr = if align64 {
            self.allocate(Header::new(ObjectType::Raw, size).initialized().align64())
        } else {
            self.allocate(Header::new(ObjectType::Raw, size).initialized())
        };

        let mut ptr = unsafe { self.get_value_mut::<u32>(addr, 0) };
        unsafe {
            if locals.count_i32 != 0 {
                std::ptr::write(ptr, locals.count_i32);
                ptr = ptr.add(1);
            }
            if locals.count_f32 != 0 {
                std::ptr::write(ptr, locals.count_f32);
                ptr = ptr.add(1);
            }
            if locals.count_func_ref != 0 {
                std::ptr::write(ptr, locals.count_func_ref);
                ptr = ptr.add(1);
            }
            if locals.count_extern_ref != 0 {
                std::ptr::write(ptr, locals.count_extern_ref);
                ptr = ptr.add(1);
            }
            if locals.count_i64 != 0 {
                std::ptr::write(ptr, locals.count_i64);
                ptr = ptr.add(1);
            }
            if locals.count_f64 != 0 {
                std::ptr::write(ptr, locals.count_f64);
                ptr = ptr.add(1);
            }
            if locals.count_v128 != 0 {
                std::ptr::write(ptr, locals.count_v128);
                ptr = ptr.add(1);
            }
            let offset = ptr.align_offset(align_of::<*mut Instr>());
            std::ptr::copy_nonoverlapping(
                instr.as_ptr(),
                ptr.add(offset) as *mut Instr,
                instr.len(),
            );
        }
        addr
    }

    pub fn get_total_linear_memory_size(&self) -> usize {
        self.wasm_linear_memory
            .iter()
            .filter_map(|v| v.as_ref())
            .map(|v| v.page_size() as usize)
            .sum::<usize>()
            * PAGE_SIZE
    }
}

#[cfg(test)]
mod tests {
    use crate::common::PAGE_SIZE;

    use super::MemoryPool;
    fn debug_pool(pool: &MemoryPool) {
        for addr in pool.scan_heap() {
            let header = pool.read_header(addr);
            tracing::trace!(
                "{addr:?}: {header:?} ({:?},size={},init={},marked={})",
                header.object_type(),
                header.word_size(),
                header.is_initialized(),
                header.is_marked()
            );

            assert!(header.is_initialized());
        }
    }
    #[test]
    fn init() {
        let mut pool = MemoryPool::new();
        debug_pool(&pool);
        pool.mark_phase();
        debug_pool(&pool);
        let mut marked = vec![];
        let mut free = vec![];
        for addr in pool.scan_heap() {
            let header = pool.read_header(addr);
            assert!(header.is_initialized());
            if header.is_marked() {
                marked.push(addr);
            } else {
                free.push(addr);
            }
            assert_eq!(marked.len(), 1);
            assert!(free.is_empty());
        }
    }
    #[test]
    fn mark() {
        let mut pool = MemoryPool::new();
        let free_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let marked_arr = pool.new_u32_fixed_array(&[4]);
        pool.add_root(marked_arr.0);
        debug_pool(&pool);
        pool.mark_phase();
        debug_pool(&pool);
        let mut marked = vec![];
        let mut free = vec![];
        for addr in pool.scan_heap() {
            let header = pool.read_header(addr);
            assert!(header.is_initialized());
            if header.is_marked() {
                marked.push(addr);
            } else {
                free.push(addr);
            }
        }
        assert!(marked.contains(&marked_arr.0));
        assert_eq!(marked.len(), 3); // root table, root table buf, marked_arr
        assert!(free.contains(&free_arr.0));
        assert_eq!(free.len(), 1); // free_arr
    }
    #[test]
    fn compaction_no_root() {
        let mut pool = MemoryPool::new();
        let _free_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let _free_arr2 = pool.new_u32_fixed_array(&[1, 2, 3]);
        pool.gc();
        debug_pool(&pool);
        let mut count_object = 0;
        for addr in pool.scan_heap() {
            let _header = pool.read_header(addr);
            count_object += 1;
        }
        assert_eq!(count_object, 1); // only root table
    }
    #[test]
    fn compaction_one_root() {
        let mut pool = MemoryPool::new();
        let tracked_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let _free_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        pool.add_root(tracked_arr.0);
        pool.gc();
        let mut count_object = 0;
        for addr in pool.scan_heap() {
            let _header = pool.read_header(addr);
            count_object += 1;
        }
        assert_eq!(count_object, 3); // root table, root table buf, tracked_arr
    }
    #[test]
    fn remove_root_before_gc() {
        let mut pool = MemoryPool::new();
        let tracked_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let _free_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let idx = pool.add_root(tracked_arr.0);
        pool.remove_root(idx);

        pool.gc();
        let mut count_object = 0;
        for addr in pool.scan_heap() {
            let _header = pool.read_header(addr);
            count_object += 1;
        }
        assert_eq!(count_object, 2); // root table, root table buf
    }
    #[test]
    fn remove_root_after_gc() {
        let mut pool = MemoryPool::new();
        let tracked_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let _free_arr = pool.new_u32_fixed_array(&[1, 2, 3]);
        let idx = pool.add_root(tracked_arr.0);
        pool.gc();
        let mut count_object = 0;
        for addr in pool.scan_heap() {
            let _header = pool.read_header(addr);
            count_object += 1;
        }
        assert_eq!(count_object, 3); // root table, root table buf, tracked_arr
        pool.remove_root(idx);
        pool.gc();
        let mut count_object = 0;
        for addr in pool.scan_heap() {
            let _header = pool.read_header(addr);
            count_object += 1;
        }
        assert_eq!(count_object, 2); // root table, root table buf
    }
    #[test]
    fn linear_memory() {
        let mut pool = MemoryPool::new();
        let id = pool.new_memory(1, 1);
        assert_eq!(pool.get_total_linear_memory_size(), PAGE_SIZE);
        let idx = pool.add_root(id);
        pool.gc();
        assert_eq!(pool.get_total_linear_memory_size(), PAGE_SIZE);
        pool.remove_root(idx);
        assert_eq!(pool.get_total_linear_memory_size(), PAGE_SIZE);
        pool.gc();
        assert_eq!(pool.get_total_linear_memory_size(), 0);
    }
}
