use crate::Instance;
fn size_of_ref_vec(v: &[u32]) -> usize {
    1 + v.len()
}
pub fn size_of_instance(instance: &Instance) -> usize {
    1 // mod_addr
     + 1 // reference counter
     + size_of_ref_vec(&instance.funcs)
        + size_of_ref_vec(&instance.globals)
        + size_of_ref_vec(&instance.tables)
        + if instance.memory.is_none() {
            1
        } else {
            2
        }
}
pub unsafe fn encode_value(v: u32, dst: &mut *mut u32) {
    **dst = v;
    *dst = dst.add(1);
}
pub unsafe fn encode_slice(src: &[u32], dst: &mut *mut u32) {
    encode_value(src.len() as u32, dst);
    std::ptr::copy_nonoverlapping(src.as_ptr(), *dst, src.len());
    *dst = dst.add(src.len());
}
pub unsafe fn encode_instance(instance: &Instance, dst: &mut *mut u32) {
    encode_value(instance.module_addr, dst);
    encode_value(0, dst); // reference counter
    encode_slice(&instance.globals, dst);
    encode_slice(&instance.funcs, dst);
    encode_slice(&instance.tables, dst);
    if let Some(mem) = instance.memory {
        encode_slice(&[mem], dst);
    } else {
        encode_slice(&[], dst);
    }
}
