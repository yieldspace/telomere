use mmap_rs::{Mmap, MmapOptions};

const CODE_ALIGNMENT: usize = 16;

#[derive(Clone, Copy, Default)]
pub(crate) struct CodeArena;

pub(crate) struct ExecutableCode {
    ptr: *mut u8,
    len: usize,
    _map: Mmap,
}

unsafe impl Send for ExecutableCode {}
unsafe impl Sync for ExecutableCode {}

impl CodeArena {
    pub(crate) fn allocate(&self, bytes: &[u8]) -> Result<ExecutableCode, ()> {
        if bytes.is_empty() {
            return Err(());
        }
        let code_len = align_up(bytes.len(), CODE_ALIGNMENT)?;
        let len = Self::allocation_len(bytes.len())?;
        let mut map = MmapOptions::new(len)
            .map_err(|_| ())?
            .map_mut()
            .map_err(|_| ())?;
        let ptr = map.as_mut_ptr();
        write_code(map.as_mut_slice(), 0, code_len, bytes)?;
        let map = map.make_exec().map_err(|_| ())?;
        Ok(ExecutableCode {
            ptr,
            len,
            _map: map,
        })
    }

    pub(crate) fn allocation_len(byte_len: usize) -> Result<usize, ()> {
        if byte_len == 0 {
            return Err(());
        }
        let code_len = align_up(byte_len, CODE_ALIGNMENT)?;
        align_up(code_len, MmapOptions::page_size())
    }
}

impl ExecutableCode {
    pub(crate) fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    #[cfg(test)]
    pub(crate) fn test_stub(len: usize) -> Self {
        let map = MmapOptions::new(MmapOptions::page_size())
            .expect("test mapping options")
            .map_mut()
            .expect("test mapping")
            .make_exec()
            .map_err(|_| ())
            .expect("test executable mapping");
        Self {
            ptr: map.as_ptr().cast_mut(),
            len,
            _map: map,
        }
    }
}

fn write_code(mapping: &mut [u8], offset: usize, len: usize, bytes: &[u8]) -> Result<(), ()> {
    let end = offset.checked_add(len).ok_or(())?;
    if bytes.len() > len || end > mapping.len() {
        return Err(());
    }
    let target = &mut mapping[offset..end];
    target[..bytes.len()].copy_from_slice(bytes);
    target[bytes.len()..].fill(0);
    Ok(())
}

fn align_up(value: usize, align: usize) -> Result<usize, ()> {
    debug_assert!(align.is_power_of_two());
    value
        .checked_add(align - 1)
        .map(|value| value & !(align - 1))
        .ok_or(())
}

#[cfg(all(test, target_os = "macos", target_arch = "aarch64"))]
mod tests {
    use super::*;

    type TestEntry = unsafe extern "C" fn() -> u64;

    fn mov_x0_ret(value: u16) -> [u8; 8] {
        let mov = 0xd2800000u32 | ((value as u32) << 5);
        let ret = 0xd65f03c0u32;
        let mut code = [0; 8];
        code[..4].copy_from_slice(&mov.to_le_bytes());
        code[4..].copy_from_slice(&ret.to_le_bytes());
        code
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
