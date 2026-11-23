//! Executable code memory for the baseline JIT.
//!
//! Compiled functions are packed into code slabs instead of receiving one
//! page-rounded mapping each. A slab is a single shared backing object
//! (a memfd on Linux, POSIX shared memory on macOS) mapped twice: a
//! writable view used to stage code while the slab is open, and an
//! executable view that is never writable at any point in its lifetime.
//! Functions are bump-allocated into the open slab at 16-byte alignment;
//! once written and cache-flushed, a function is executable through the
//! executable view immediately. When a slab fills up or is sealed, its
//! writable view is unmapped permanently — a slab is never made writable
//! again, which keeps the W^X contract one-way. A slab's mappings are
//! released once the last [`ExecutableCode`] carved out of it is dropped,
//! so physical reclamation happens at slab granularity even though the
//! store cache evicts individual functions.
//!
//! Where shared backing is unavailable (or on non-Unix targets) the arena
//! falls back to the previous scheme: one mapping per function, created
//! writable and flipped to executable exactly once. The accounting
//! contract ([`CodeArena::accounted_len`]) is the same on both paths.

#[cfg(unix)]
use std::sync::{Arc, Mutex, MutexGuard};

use mmap_rs::{Mmap, MmapOptions};

const CODE_ALIGNMENT: usize = 16;

/// Size of one code slab. 256 KiB holds hundreds of typical baseline-JIT
/// functions while keeping the worst-case pinned waste (one hot function
/// keeping its slab alive) bounded to a single slab.
#[cfg(unix)]
const SLAB_SIZE: usize = 256 * 1024;

pub struct CodeArena {
    #[cfg(unix)]
    inner: Mutex<ArenaInner>,
}

impl Default for CodeArena {
    fn default() -> Self {
        Self {
            #[cfg(unix)]
            inner: Mutex::new(ArenaInner::default()),
        }
    }
}

#[cfg(unix)]
#[derive(Default)]
struct ArenaInner {
    /// The slab currently accepting functions, if any.
    current: Option<Arc<Slab>>,
    /// Bump offset inside `current`.
    bump: usize,
}

/// One contiguous slab of code memory with separate writable and
/// executable views of the same backing pages.
#[cfg(unix)]
struct Slab {
    /// Base of the executable view. Never writable.
    rx: *mut u8,
    /// Base of the writable view, or 0 once the slab has been retired.
    /// Written only via `retire`; read under the arena lock or through the
    /// atomic for inspection.
    rw: std::sync::atomic::AtomicUsize,
    size: usize,
}

#[cfg(unix)]
unsafe impl Send for Slab {}
#[cfg(unix)]
unsafe impl Sync for Slab {}

#[cfg(unix)]
impl Slab {
    fn new(size: usize) -> Result<Self, CodeMemoryError> {
        debug_assert!(size > 0 && size % MmapOptions::page_size() == 0);
        let fd = create_shared_backing(size)?;
        let rw = map_view(fd, libc::PROT_READ | libc::PROT_WRITE, size);
        let rx = match rw {
            Ok(_) => map_view(fd, libc::PROT_READ | libc::PROT_EXEC, size),
            Err(err) => Err(err),
        };
        // The mappings keep the backing alive; the descriptor is no
        // longer needed either way.
        unsafe { libc::close(fd) };
        let rw = rw?;
        let rx = match rx {
            Ok(ptr) => ptr,
            Err(err) => {
                unsafe { libc::munmap(rw as *mut libc::c_void, size) };
                return Err(err);
            }
        };
        Ok(Self {
            rx,
            rw: std::sync::atomic::AtomicUsize::new(rw as usize),
            size,
        })
    }

    fn writable_base(&self) -> Option<*mut u8> {
        match self.rw.load(std::sync::atomic::Ordering::Acquire) {
            0 => None,
            ptr => Some(ptr as *mut u8),
        }
    }

    /// Permanently unmaps the writable view. Idempotent: after the first
    /// call the slab can never be written to again.
    fn retire(&self) {
        let rw = self.rw.swap(0, std::sync::atomic::Ordering::AcqRel);
        if rw != 0 {
            unsafe { libc::munmap(rw as *mut libc::c_void, self.size) };
        }
    }
}

#[cfg(unix)]
impl Drop for Slab {
    fn drop(&mut self) {
        self.retire();
        unsafe { libc::munmap(self.rx as *mut libc::c_void, self.size) };
    }
}

/// Creates an anonymous shared backing object of `size` bytes and returns
/// its file descriptor: a memfd on Linux, an immediately-unlinked POSIX
/// shared memory object on macOS.
#[cfg(unix)]
fn create_shared_backing(size: usize) -> Result<libc::c_int, CodeMemoryError> {
    #[cfg(target_os = "linux")]
    let fd = unsafe { libc::memfd_create(c"telomere-jit-code".as_ptr(), libc::MFD_CLOEXEC) };
    #[cfg(target_os = "macos")]
    let fd = shm_open_unique();
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    let fd = -1;
    if fd < 0 {
        return Err(CodeMemoryError);
    }
    if unsafe { libc::ftruncate(fd, size as libc::off_t) } != 0 {
        unsafe { libc::close(fd) };
        return Err(CodeMemoryError);
    }
    Ok(fd)
}

#[cfg(target_os = "macos")]
fn shm_open_unique() -> libc::c_int {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    // POSIX shm names are capped at PSHMNAMLEN (31) characters; the pid
    // plus a counter keeps the name short and collision-free in practice.
    // The object is unlinked as soon as it is opened, so names never
    // persist beyond this call.
    for _ in 0..8 {
        let name = format!(
            "/tj{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let Ok(c_name) = std::ffi::CString::new(name) else {
            return -1;
        };
        // With O_CREAT, `shm_open` takes a variadic `mode` argument, and
        // Rust forbids passing integers narrower than `c_uint` to variadic
        // functions (E0617): `mode_t` is `u16` on macOS. Go through
        // `c_uint` — on Linux `mode_t` *is* `c_uint`, so the cast is a
        // no-op there.
        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600 as libc::mode_t as libc::c_uint,
            )
        };
        if fd >= 0 {
            unsafe { libc::shm_unlink(c_name.as_ptr()) };
            return fd;
        }
        if std::io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
            return -1;
        }
    }
    -1
}

#[cfg(unix)]
fn map_view(fd: libc::c_int, prot: libc::c_int, size: usize) -> Result<*mut u8, CodeMemoryError> {
    let ptr = unsafe { libc::mmap(std::ptr::null_mut(), size, prot, libc::MAP_SHARED, fd, 0) };
    if ptr == libc::MAP_FAILED {
        return Err(CodeMemoryError);
    }
    Ok(ptr as *mut u8)
}

pub struct ExecutableCode {
    ptr: *mut u8,
    /// Accounted length: the code size rounded up to [`CODE_ALIGNMENT`].
    /// This is what the store cache charges against its cap; it is *not*
    /// the size of the backing mapping (a whole slab, or one page-rounded
    /// mapping on the fallback path).
    len: usize,
    _owner: CodeOwner,
}

/// The backing that keeps a function's code mapping alive. The payload
/// is never inspected; it is held purely so its `Drop` releases the
/// mapping when the last function referencing it goes away.
#[allow(dead_code)]
enum CodeOwner {
    #[cfg(unix)]
    Slab(Arc<Slab>),
    Standalone(Mmap),
}

unsafe impl Send for ExecutableCode {}
unsafe impl Sync for ExecutableCode {}

impl CodeArena {
    pub fn allocate(&self, bytes: &[u8]) -> Result<ExecutableCode, CodeMemoryError> {
        if bytes.is_empty() {
            return Err(CodeMemoryError);
        }
        let code_len = align_up(bytes.len(), CODE_ALIGNMENT)?;
        #[cfg(unix)]
        if let Ok(code) = self.allocate_packed(bytes, code_len) {
            return Ok(code);
        }
        self.allocate_standalone(bytes, code_len)
    }

    /// Bytes charged against the code-cache cap for a function whose
    /// emitted code is `byte_len` bytes: the code size rounded up to the
    /// code alignment. Deliberately *not* rounded up to a page — the cap
    /// accounts emitted code, not mapping granularity.
    pub fn accounted_len(byte_len: usize) -> Result<usize, CodeMemoryError> {
        if byte_len == 0 {
            return Err(CodeMemoryError);
        }
        align_up(byte_len, CODE_ALIGNMENT)
    }

    /// Size of the standalone (one mapping per function) allocation for
    /// `byte_len` bytes of code: the code size rounded up to a page.
    pub fn allocation_len(byte_len: usize) -> Result<usize, CodeMemoryError> {
        if byte_len == 0 {
            return Err(CodeMemoryError);
        }
        let code_len = align_up(byte_len, CODE_ALIGNMENT)?;
        align_up(code_len, MmapOptions::page_size())
    }

    /// Seals the currently open slab: its writable view is unmapped
    /// permanently and the next allocation starts a fresh slab. Code
    /// already carved out of the slab stays executable and alive. This is
    /// the batch boundary for callers that compile in bursts; allocation
    /// also seals implicitly once a slab is full.
    #[cfg(unix)]
    pub fn seal_current_slab(&self) {
        let mut inner = self.lock_inner();
        if let Some(slab) = inner.current.take() {
            slab.retire();
        }
        inner.bump = 0;
    }

    #[cfg(unix)]
    fn lock_inner(&self) -> MutexGuard<'_, ArenaInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Bump-allocates `bytes` into the open slab (or a dedicated slab for
    /// oversize functions) and returns code that is executable through
    /// the slab's executable view.
    #[cfg(unix)]
    fn allocate_packed(
        &self,
        bytes: &[u8],
        code_len: usize,
    ) -> Result<ExecutableCode, CodeMemoryError> {
        if code_len > SLAB_SIZE {
            // Oversize functions get a dedicated slab, sealed at once:
            // nothing else will ever be written into it.
            let size = align_up(code_len, MmapOptions::page_size())?;
            let slab = Arc::new(Slab::new(size)?);
            write_into_slab(&slab, 0, bytes, code_len)?;
            slab.retire();
            return Ok(ExecutableCode {
                ptr: slab.rx,
                len: code_len,
                _owner: CodeOwner::Slab(slab),
            });
        }
        let mut inner = self.lock_inner();
        let fits = match &inner.current {
            Some(slab) => inner.bump + code_len <= slab.size,
            None => false,
        };
        if !fits {
            if let Some(old) = inner.current.take() {
                old.retire();
            }
            inner.current = Some(Arc::new(Slab::new(SLAB_SIZE)?));
            inner.bump = 0;
        }
        let slab = inner
            .current
            .as_ref()
            .expect("current slab was just installed")
            .clone();
        let offset = inner.bump;
        // If the write fails the bump offset is left unchanged; any
        // partially written bytes are dead space nobody can reach.
        write_into_slab(&slab, offset, bytes, code_len)?;
        inner.bump += code_len;
        Ok(ExecutableCode {
            ptr: unsafe { slab.rx.add(offset) },
            len: code_len,
            _owner: CodeOwner::Slab(slab),
        })
    }

    /// One writable mapping per function, flipped to executable exactly
    /// once. Used where slab backing is unavailable and on non-Unix
    /// targets.
    fn allocate_standalone(
        &self,
        bytes: &[u8],
        code_len: usize,
    ) -> Result<ExecutableCode, CodeMemoryError> {
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
            len: code_len,
            _owner: CodeOwner::Standalone(map),
        })
    }
}

#[cfg(unix)]
fn write_into_slab(
    slab: &Slab,
    offset: usize,
    bytes: &[u8],
    code_len: usize,
) -> Result<(), CodeMemoryError> {
    let rw = slab.writable_base().ok_or(CodeMemoryError)?;
    let end = offset.checked_add(code_len).ok_or(CodeMemoryError)?;
    if end > slab.size {
        return Err(CodeMemoryError);
    }
    let target = unsafe { std::slice::from_raw_parts_mut(rw.add(offset), code_len) };
    write_code(target, 0, code_len, bytes)?;
    // The flush must name the executable view's addresses: those are the
    // ones that will be fetched as instructions.
    flush_instruction_cache(unsafe { slab.rx.add(offset) }, bytes.len())
}

impl ExecutableCode {
    pub fn ptr(&self) -> *mut u8 {
        self.ptr
    }

    /// The accounted length of this function's code (see
    /// [`CodeArena::accounted_len`]).
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
            _owner: CodeOwner::Standalone(map),
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

#[cfg(all(
    target_os = "linux",
    any(target_arch = "aarch64", target_arch = "riscv64")
))]
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

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn flush_instruction_cache(ptr: *mut u8, len: usize) -> Result<(), CodeMemoryError> {
    if len == 0 {
        return Ok(());
    }
    unsafe extern "C" {
        fn sys_icache_invalidate(start: *mut u8, len: usize);
    }
    unsafe { sys_icache_invalidate(ptr, len) };
    Ok(())
}

#[cfg(not(any(
    all(
        target_os = "linux",
        any(target_arch = "aarch64", target_arch = "riscv64")
    ),
    all(target_os = "macos", target_arch = "aarch64")
)))]
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
        let arena = CodeArena::default();
        let code = arena.allocate(&mov_x0_ret(7)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        // len() is the accounted length: 8 bytes of code at 16-byte
        // alignment, not the size of the backing mapping.
        assert_eq!(code.len(), 16);
        assert_eq!(unsafe { entry() }, 7);
    }

    #[test]
    fn code_arena_does_not_rewrite_live_executable_mapping() {
        let arena = CodeArena::default();

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
        let arena = CodeArena::default();
        let code = arena.allocate(&mov_eax_ret(11)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        assert_eq!(code.len(), 16);
        assert_eq!(unsafe { entry() }, 11);
    }

    #[test]
    fn packed_functions_execute_side_by_side() {
        let arena = CodeArena::default();
        let first = arena.allocate(&mov_eax_ret(21)).expect("first allocation");
        let second = arena.allocate(&mov_eax_ret(34)).expect("second allocation");
        let first_entry: TestEntry = unsafe { std::mem::transmute(first.ptr) };
        let second_entry: TestEntry = unsafe { std::mem::transmute(second.ptr) };

        assert_eq!(unsafe { first_entry() }, 21);
        assert_eq!(unsafe { second_entry() }, 34);
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
        let arena = CodeArena::default();
        let code = arena.allocate(&addi_a0_ret(13)).expect("allocation");
        let entry: TestEntry = unsafe { std::mem::transmute(code.ptr) };

        assert_eq!(code.len(), 16);
        assert_eq!(unsafe { entry() }, 13);
    }
}

#[cfg(all(test, unix))]
mod slab_tests {
    use super::*;

    #[test]
    fn accounted_len_rounds_up_to_code_alignment_only() {
        assert_eq!(CodeArena::accounted_len(1).expect("len"), 16);
        assert_eq!(CodeArena::accounted_len(16).expect("len"), 16);
        assert_eq!(CodeArena::accounted_len(17).expect("len"), 32);
        assert!(CodeArena::accounted_len(0).is_err());
        // The standalone mapping size stays page-rounded; the two notions
        // must not be conflated.
        assert!(CodeArena::allocation_len(1).expect("len") >= MmapOptions::page_size());
    }

    #[test]
    fn allocations_pack_densely_into_one_slab() {
        let arena = CodeArena::default();
        let first = arena.allocate(&[0x90; 24]).expect("first allocation");
        let second = arena.allocate(&[0x90; 24]).expect("second allocation");

        assert_eq!(first.len(), 32);
        assert_eq!(second.len(), 32);
        // 16-byte aligned bump allocation: the second function starts
        // exactly one padded slot after the first — no page per function.
        assert_eq!(second.ptr() as usize - first.ptr() as usize, 32);
    }

    #[test]
    fn sealing_retires_the_writable_view_forever() {
        let arena = CodeArena::default();
        let code = arena.allocate(&[0x90; 8]).expect("allocation");
        let CodeOwner::Slab(slab) = &code._owner else {
            panic!("expected slab-backed code on unix");
        };
        assert!(
            slab.writable_base().is_some(),
            "open slab must have a writable view"
        );
        arena.seal_current_slab();
        assert!(
            slab.writable_base().is_none(),
            "sealed slab must never become writable again"
        );
        // The sealed slab's code stays alive and owned; new allocations
        // go to a different slab.
        let next = arena.allocate(&[0x90; 8]).expect("next allocation");
        let base_distance = (next.ptr() as isize - code.ptr() as isize).unsigned_abs();
        assert!(
            base_distance >= SLAB_SIZE,
            "allocation after sealing must not land in the sealed slab"
        );
    }

    #[test]
    fn oversize_function_gets_a_dedicated_slab() {
        let arena = CodeArena::default();
        let big = vec![0xcc; SLAB_SIZE + 16];
        let code = arena.allocate(&big).expect("oversize allocation");
        assert_eq!(code.len(), SLAB_SIZE + 16);
        let CodeOwner::Slab(slab) = &code._owner else {
            panic!("expected slab-backed code on unix");
        };
        assert!(
            slab.writable_base().is_none(),
            "dedicated slab is sealed immediately"
        );
        // The arena still serves small allocations afterwards.
        let small = arena.allocate(&[0x90; 8]).expect("small allocation");
        assert_eq!(small.len(), 16);
        assert_ne!(small.ptr(), code.ptr());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn executable_view_is_never_writable() {
        let arena = CodeArena::default();
        let code = arena.allocate(&[0x90; 8]).expect("allocation");
        let addr = code.ptr() as usize;
        let maps = std::fs::read_to_string("/proc/self/maps").expect("read maps");
        let line = maps
            .lines()
            .find(|line| {
                let mut fields = line.split_whitespace();
                let Some(range) = fields.next() else {
                    return false;
                };
                let Some((start, end)) = range.split_once('-') else {
                    return false;
                };
                let start = usize::from_str_radix(start, 16).expect("map start");
                let end = usize::from_str_radix(end, 16).expect("map end");
                (start..end).contains(&addr)
            })
            .expect("mapping containing the code pointer");
        let perms = line.split_whitespace().nth(1).expect("map perms");
        assert!(
            perms.contains('x'),
            "code mapping must be executable: {line}"
        );
        assert!(
            !perms.contains('w'),
            "code mapping must not be writable: {line}"
        );
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
