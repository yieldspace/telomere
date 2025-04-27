use crate::common::{
    gc::{GcRef, GcView, MemoryPool},
    HostFunction, Instr, LocalsData,
};

const HOST_FUNC_MASK: u32 = 0x80000000;

const LOCALS_ENCODED_I32: u32 = 0x1 << 0;
const LOCALS_ENCODED_F32: u32 = LOCALS_ENCODED_I32 << 1;
const LOCALS_ENCODED_FUNC_REF: u32 = LOCALS_ENCODED_I32 << 2;
const LOCALS_ENCODED_EXTERN_REF: u32 = LOCALS_ENCODED_I32 << 3;
const LOCALS_ENCODED_I64: u32 = LOCALS_ENCODED_I32 << 4;
const LOCALS_ENCODED_F64: u32 = LOCALS_ENCODED_I32 << 5;
const LOCALS_ENCODED_V128: u32 = 0x1 << 6;

#[repr(C)]
pub struct FunctionInstanceData {
    pub instance_addr: GcRef,
    pub funcidx: u32,
    pub function_flags: u32, //TODO: more efficient encoding
    pub body: GcRef, // raw, reference to wasm locals and code, or reference to native function pointer
}

impl FunctionInstanceData {
    pub fn is_host_func(&self) -> bool {
        self.function_flags & HOST_FUNC_MASK != 0
    }
    pub(crate) fn create_wasm_flags(data: &LocalsData) -> u32 {
        let mut flags = 0;
        if data.count_extern_ref != 0 {
            flags |= LOCALS_ENCODED_EXTERN_REF;
        }
        if data.count_f32 != 0 {
            flags |= LOCALS_ENCODED_F32;
        }
        if data.count_f64 != 0 {
            flags |= LOCALS_ENCODED_F64;
        }
        if data.count_func_ref != 0 {
            flags |= LOCALS_ENCODED_FUNC_REF;
        }
        if data.count_i32 != 0 {
            flags |= LOCALS_ENCODED_I32;
        }
        if data.count_i64 != 0 {
            flags |= LOCALS_ENCODED_I64;
        }
        if data.count_v128 != 0 {
            flags |= LOCALS_ENCODED_V128;
        }
        flags
    }
    pub(crate) fn create_host_flags() -> u32 {
        HOST_FUNC_MASK
    }
    pub fn locals_and_code_offset(&self, pool: &MemoryPool) -> (LocalsData, usize) {
        unsafe {
            let flags = self.function_flags;
            let addr = self.body;
            let mut offset = 0usize;
            let mut locals = LocalsData::default();
            if flags & LOCALS_ENCODED_I32 != 0 {
                locals.count_i32 = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_F32 != 0 {
                locals.count_f32 = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_FUNC_REF != 0 {
                locals.count_func_ref = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_EXTERN_REF != 0 {
                locals.count_extern_ref = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_I64 != 0 {
                locals.count_i64 = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_F64 != 0 {
                locals.count_f64 = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            if flags & LOCALS_ENCODED_V128 != 0 {
                locals.count_v128 = *pool.get_value::<u32>(addr, offset);
                offset += 1;
            }
            (locals, offset + (offset % (align_of::<*mut Instr>() / 4)))
        }
    }
    pub fn host_code_pointer(&self, pool: &MemoryPool) -> HostFunction {
        unsafe { *pool.get_value::<HostFunction>(self.body, 0) }
    }
}

impl GcView for FunctionInstanceData {
    fn trace(&self, pool: &mut MemoryPool) {
        self.instance_addr.trace(pool);
        self.body.trace(pool);
    }
    fn update(&mut self, pool: &mut MemoryPool) {
        self.instance_addr.update(pool);
        self.body.update(pool);
    }
}
