use crate::VMResult;
use std::fmt::Debug;

use super::Instr;
pub struct Stack {
    memory: Box<[u8]>,
    top: usize,
}
impl Debug for Stack {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Stack({},{:?})", self.top, &self.memory[0..self.top])
    }
}
#[derive(Debug, Clone, Copy)]
pub struct LocalReference {
    local_top: usize,
    local_size: usize,
}
impl Stack {
    pub fn new(size: usize) -> Self {
        let vec = vec![0; size];
        Stack {
            memory: vec.into_boxed_slice(),
            top: 0,
        }
    }
    fn add_top(&mut self, n: usize) -> VMResult<()> {
        self.top = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(())
    }
    fn sub_top(&mut self, n: usize) {
        self.top -= n;
    }
    fn get_memory(&mut self, n: usize) -> VMResult<&mut [u8]> {
        let last = vm_try!(VMResult::from_option(self.top.checked_add(n), || {
            VMResult::StackOverflow
        }));
        VMResult::Success(vm_try!(VMResult::from_option(
            self.memory.get_mut(self.top..last),
            || VMResult::StackOverflow
        )))
    }
    pub fn push_u8_array<const N: usize>(&mut self, v: [u8; N]) -> VMResult<()> {
        unsafe { std::ptr::copy(v.as_ptr(), vm_try!(self.get_memory(N)).as_mut_ptr(), N) };
        self.add_top(N)
    }

    pub fn push_slice(&mut self, v: &[u8]) -> VMResult<()> {
        unsafe {
            std::ptr::copy(
                v.as_ptr(),
                vm_try!(self.get_memory(v.len())).as_mut_ptr(),
                v.len(),
            )
        };
        self.add_top(v.len())
    }
    pub fn pop_u8_array<const N: usize>(&mut self) -> [u8; N] {
        self.sub_top(N);
        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn pop_u8_array_generic<const N: usize>(&mut self, n: usize) -> [u8; N] {
        self.sub_top(n);

        let mut arr = [0u8; N];
        unsafe { std::ptr::copy(self.memory.as_ptr().add(self.top), arr.as_mut_ptr(), N) };
        arr
    }
    pub fn drop(&mut self, n: usize) -> &[u8] {
        self.sub_top(n);

        (&self.memory[self.top..self.top + n]) as _
    }
    pub fn push_u32(&mut self, v: u32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_u64(&mut self, v: u64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn push_i32(&mut self, v: i32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn push_f32(&mut self, v: f32) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn push_f64(&mut self, v: f64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn push_i64(&mut self, v: i64) -> VMResult<()> {
        self.push_u8_array(v.to_le_bytes())
    }
    pub fn pop_i64(&mut self) -> i64 {
        i64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn pop_f32(&mut self) -> f32 {
        f32::from_le_bytes(self.pop_u8_array::<4>())
    }
    pub fn pop_f64(&mut self) -> f64 {
        f64::from_le_bytes(self.pop_u8_array::<8>())
    }
    pub fn access_locals(&mut self, reference: &LocalReference) -> &mut [u8] {
        &mut self.memory[reference.local_top..self.top + reference.local_size]
    }
    pub fn local_get(
        &mut self,
        reference: &LocalReference,
        local_addr: usize,
        size: usize,
    ) -> VMResult<()> {
        let new_top = vm_try!(VMResult::from_option(self.top.checked_add(size), || {
            VMResult::StackOverflow
        }));
        if new_top >= self.memory.len() {
            return VMResult::StackOverflow;
        }
        self.memory.copy_within(
            reference.local_top + local_addr..reference.local_top + local_addr + size,
            self.top,
        );
        self.top = new_top;
        VMResult::Success(())
    }
    pub fn local_set(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.top -= size;
        self.memory
            .copy_within(self.top..self.top + size, reference.local_top + local_addr);
    }
    pub fn local_tee(&mut self, reference: &LocalReference, local_addr: usize, size: usize) {
        self.memory
            .copy_within(self.top - size..self.top, reference.local_top + local_addr);
    }
    pub fn function_call(
        &mut self,
        param_size: usize,
        local_size: usize,
        return_addr: *const Instr,
    ) -> VMResult<LocalReference> {
        let local_top = vm_try!(VMResult::from_option(
            self.top.checked_sub(param_size),
            || VMResult::StackOverflow
        ));
        vm_try!(self.add_top(local_size));
        vm_try!(self.push_slice(&(return_addr as usize).to_le_bytes()));
        VMResult::Success(LocalReference {
            local_top,
            local_size: param_size + local_size + std::mem::size_of_val(&return_addr),
        })
    }
    pub fn function_return(
        &mut self,
        reference: &LocalReference,
        return_size: usize,
    ) -> *const Instr {
        let return_addr_addr =
            reference.local_top + reference.local_size - std::mem::size_of::<usize>();
        let mut buf = [0; std::mem::size_of::<usize>()];
        buf.copy_from_slice(
            &self.memory[return_addr_addr..return_addr_addr + std::mem::size_of::<usize>()],
        );
        let return_addr = usize::from_le_bytes(buf);

        self.memory
            .copy_within(self.top - return_size..self.top, reference.local_top);
        self.top = reference.local_top + return_size;
        return_addr as *const Instr
    }
    pub fn block_return(
        &mut self,
        reference: &LocalReference,
        stack_top: usize,
        return_size: usize,
    ) {
        self.memory.copy_within(
            self.top - return_size..self.top,
            reference.local_top + reference.local_size + stack_top,
        );
        self.top = reference.local_top + reference.local_size + stack_top + return_size;
    }
}
