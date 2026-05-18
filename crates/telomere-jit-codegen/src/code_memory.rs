use mmap_rs::{Mmap, MmapOptions};

const CODE_ALIGNMENT: usize = 16;

#[derive(Clone, Copy, Default)]
pub struct CodeArena;

pub struct ExecutableCode {
    ptr: *mut u8,
    len: usize,
    _map: Mmap,
}

unsafe impl Send for ExecutableCode {}
unsafe impl Sync for ExecutableCode {}

impl CodeArena {
    pub fn allocate(&self, bytes: &[u8]) -> Result<ExecutableCode, CodeMemoryError> {
        if bytes.is_empty() {
            return Err(CodeMemoryError);
        }
        let code_len = align_up(bytes.len(), CODE_ALIGNMENT)?;
        let len = Self::allocation_len(bytes.len())?;
        let mut map = MmapOptions::new(len)
            .map_err(|_| CodeMemoryError)?
            .map_mut()
            .map_err(|_| CodeMemoryError)?;
        let ptr = map.as_mut_ptr();
        write_code(map.as_mut_slice(), 0, code_len, bytes)?;
        flush_instruction_cache(ptr, bytes.len())?;
        let map = map.make_exec().map_err(|_| CodeMemoryError)?;
        Ok(ExecutableCode {
            ptr,
            len,
            _map: map,
        })
    }

    pub fn allocation_len(byte_len: usize) -> Result<usize, CodeMemoryError> {
        if byte_len == 0 {
            return Err(CodeMemoryError);
        }
        let code_len = align_up(byte_len, CODE_ALIGNMENT)?;
        align_up(code_len, MmapOptions::page_size())
    }
}

impl ExecutableCode {
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    #[doc(hidden)]
    pub fn test_stub(len: usize) -> Self {
        let page_size = MmapOptions::page_size();
        let map = MmapOptions::new(page_size)
            .expect("test mapping options")
            .map_mut()
            .expect("test mapping")
            .make_exec()
            .map_err(|_| CodeMemoryError)
            .expect("test executable mapping");
        Self {
            ptr: map.as_ptr().cast_mut(),
            len: len.min(page_size),
            _map: map,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeMemoryError;

fn write_code(
    mapping: &mut [u8],
    offset: usize,
    len: usize,
    bytes: &[u8],
) -> Result<(), CodeMemoryError> {
    let end = offset.checked_add(len).ok_or(CodeMemoryError)?;
    if bytes.len() > len || end > mapping.len() {
        return Err(CodeMemoryError);
    }
    let target = &mut mapping[offset..end];
    target[..bytes.len()].copy_from_slice(bytes);
    target[bytes.len()..].fill(0);
    Ok(())
}

fn align_up(value: usize, align: usize) -> Result<usize, CodeMemoryError> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or(CodeMemoryError)
}

#[cfg(all(target_os = "linux", target_arch = "riscv64"))]
fn flush_instruction_cache(ptr: *mut u8, len: usize) -> Result<(), CodeMemoryError> {
    if len == 0 {
        return Ok(());
    }
    unsafe extern "C" {
        fn __clear_cache(start: *mut u8, end: *mut u8);
    }
    let end = unsafe { ptr.add(len) };
    unsafe { __clear_cache(ptr, end) };
    Ok(())
}

#[cfg(not(all(target_os = "linux", target_arch = "riscv64")))]
fn flush_instruction_cache(_ptr: *mut u8, _len: usize) -> Result<(), CodeMemoryError> {
    Ok(())
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;
    use crate::arch::aarch64::{A64Masm, XReg};

    type TestEntry = unsafe extern "C" fn() -> u64;

    fn mov_x0_ret(value: u16) -> [u8; 8] {
        let mut asm = A64Masm::with_capacity(8);
        asm.mov_imm_u64(XReg::new_unchecked(0), u64::from(value));
        asm.ret();
        asm.into_bytes()
            .try_into()
            .expect("small mov+ret test stub is 8 bytes")
    }

    #[test]
    fn code_arena_allocates_executable_code() {
        let arena = CodeArena;
        let code = arena.allocate(&mov_x0_ret(7)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        assert!(code.len() >= MmapOptions::page_size());
        assert_eq!(unsafe { entry() }, 7);
    }

    #[test]
    fn code_arena_does_not_rewrite_live_executable_mapping() {
        let arena = CodeArena;

        let first = arena.allocate(&mov_x0_ret(7)).expect("first allocation");
        let second = arena.allocate(&mov_x0_ret(9)).expect("second allocation");
        let first_entry: TestEntry = unsafe { std::mem::transmute(first.ptr) };
        let second_entry: TestEntry = unsafe { std::mem::transmute(second.ptr) };

        assert_ne!(first.ptr, second.ptr);
        assert_eq!(unsafe { first_entry() }, 7);
        assert_eq!(unsafe { second_entry() }, 9);
    }
}

#[cfg(all(
    test,
    any(target_os = "macos", target_os = "linux"),
    target_arch = "x86_64"
))]
mod x86_64_tests {
    use super::*;

    type TestEntry = unsafe extern "C" fn() -> u32;

    fn mov_eax_ret(value: u32) -> [u8; 6] {
        let mut bytes = [0u8; 6];
        bytes[0] = 0xb8;
        bytes[1..5].copy_from_slice(&value.to_le_bytes());
        bytes[5] = 0xc3;
        bytes
    }

    #[test]
    fn code_arena_allocates_executable_code() {
        let arena = CodeArena;
        let code = arena.allocate(&mov_eax_ret(11)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        assert!(code.len() >= MmapOptions::page_size());
        assert_eq!(unsafe { entry() }, 11);
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "riscv64", target_env = "gnu"))]
mod riscv64_tests {
    use super::*;

    type TestEntry = unsafe extern "C" fn() -> u64;

    fn addi_a0_ret(value: u16) -> [u8; 8] {
        let addi = (u32::from(value) << 20) | (10 << 7) | 0x13;
        let ret = 0x0000_8067u32;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&addi.to_le_bytes());
        bytes[4..].copy_from_slice(&ret.to_le_bytes());
        bytes
    }

    #[test]
    fn code_arena_allocates_executable_code() {
        let arena = CodeArena;
        let code = arena.allocate(&addi_a0_ret(13)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        assert!(code.len() >= MmapOptions::page_size());
        assert_eq!(unsafe { entry() }, 13);
    }
}

#[cfg(test)]
mod test_stub_tests {
    use super::*;

    #[test]
    fn test_stub_len_never_exceeds_backing_mapping() {
        let code = ExecutableCode::test_stub(MmapOptions::page_size() * 2);
        assert_eq!(code.len(), MmapOptions::page_size());
    }
}
