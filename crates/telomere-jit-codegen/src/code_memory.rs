//! Executable code memory for the baseline JIT.
//!
//! Compiled functions are packed into code slabs instead of receiving one
//! page-rounded mapping each. A slab is a single shared backing object
//! mapped twice: a writable view used to stage code while the slab is
//! open, and an executable view that is never writable at any point in
//! its lifetime. On Linux the backing is a memfd mapped read/write and
//! read/execute; on macOS the writable view is an anonymous mapping and
//! the executable view is an alias of the same pages made with
//! `mach_vm_remap` (an executable `mmap` of a POSIX shared-memory object
//! faults with SIGBUS when executed on Apple Silicon).
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

const CODE_ALIGNMENT: usize = 16;

/// The OS page size: `sysconf(_SC_PAGESIZE)` on Unix (falling back to
/// 4096 when the query fails), the platform query on other targets.
///
/// This is a local definition rather than `mmap_rs::MmapOptions`'s
/// because the standalone mapping below is implemented directly on
/// Unix: mmap-rs's Unix backend declares the libgcc builtin
/// `__clear_cache` as an unconditional extern, so every binary linking
/// it fails on targets whose linker never provides that symbol —
/// Rust's self-contained musl linking being the observed case
/// (`undefined reference to __clear_cache` from the telomere-jit-codegen
/// lib test for x86_64-unknown-linux-musl). mmap-rs remains a
/// dependency only on non-Unix targets, whose backend resolves the
/// cache flush through the OS and carries no such assumption.
fn page_size() -> usize {
    #[cfg(unix)]
    {
        let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if size > 0 {
            size as usize
        } else {
            4096
        }
    }
    #[cfg(not(unix))]
    {
        mmap_rs::MmapOptions::page_size()
    }
}

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
        debug_assert!(size > 0 && size % page_size() == 0);
        let (rw, rx) = create_slab_views(size)?;
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
        unmap_rx_view(self.rx, self.size);
    }
}

/// Creates an anonymous shared backing object of `size` bytes and returns
/// its file descriptor: a memfd on Linux. Other Unix targets have no
/// backing here and fall back to per-function mappings; macOS does not
/// use a backing object at all (see `create_slab_views` below).
#[cfg(all(unix, not(target_os = "macos")))]
fn create_shared_backing(size: usize) -> Result<libc::c_int, CodeMemoryError> {
    #[cfg(target_os = "linux")]
    let fd = unsafe { libc::memfd_create(c"telomere-jit-code".as_ptr(), libc::MFD_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
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

#[cfg(all(unix, not(target_os = "macos")))]
fn map_view(fd: libc::c_int, prot: libc::c_int, size: usize) -> Result<*mut u8, CodeMemoryError> {
    let ptr = unsafe { libc::mmap(std::ptr::null_mut(), size, prot, libc::MAP_SHARED, fd, 0) };
    if ptr == libc::MAP_FAILED {
        return Err(CodeMemoryError);
    }
    Ok(ptr as *mut u8)
}

/// Creates the two views of one slab over a shared backing object (see
/// [`create_shared_backing`]): a writable view and an executable view of
/// the same pages.
#[cfg(all(unix, not(target_os = "macos")))]
fn create_slab_views(size: usize) -> Result<(*mut u8, *mut u8), CodeMemoryError> {
    let fd = create_shared_backing(size)?;
    let rw = map_view(fd, libc::PROT_READ | libc::PROT_WRITE, size);
    let rx = match rw {
        Ok(_) => map_view(fd, libc::PROT_READ | libc::PROT_EXEC, size),
        Err(err) => Err(err),
    };
    // The mappings keep the backing alive; the descriptor is no longer
    // needed either way.
    unsafe { libc::close(fd) };
    let rw = rw?;
    let rx = match rx {
        Ok(ptr) => ptr,
        Err(err) => {
            unsafe { libc::munmap(rw as *mut libc::c_void, size) };
            return Err(err);
        }
    };
    Ok((rw, rx))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn unmap_rx_view(ptr: *mut u8, size: usize) {
    unsafe { libc::munmap(ptr as *mut libc::c_void, size) };
}

/// Creates the two views of one slab on macOS. A dual `mmap` of a POSIX
/// shared-memory object does not work here: the writable view maps and
/// stores fine, but executing through the executable view faults with
/// SIGBUS on Apple Silicon (reproduced with a minimal shm_open/mmap
/// program and by the aarch64-apple-darwin CI cell). Instead the
/// writable view is a plain anonymous mapping and the executable view
/// is an alias of the same pages created with `mach_vm_remap` and
/// protected read/execute; the alias is never writable, so the W^X
/// contract is unchanged.
#[cfg(target_os = "macos")]
fn create_slab_views(size: usize) -> Result<(*mut u8, *mut u8), CodeMemoryError> {
    let rw = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            size,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_ANON | libc::MAP_PRIVATE,
            -1,
            0,
        )
    };
    if rw == libc::MAP_FAILED {
        return Err(CodeMemoryError);
    }
    let rw = rw as *mut u8;
    let task = mach::task_self();
    let mut rx_addr: mach::MachVmAddress = 0;
    let kr = unsafe {
        mach::mach_vm_allocate(
            task,
            &mut rx_addr,
            size as mach::MachVmSize,
            mach::VM_FLAGS_ANYWHERE,
        )
    };
    if kr != mach::KERN_SUCCESS {
        unsafe { libc::munmap(rw as *mut libc::c_void, size) };
        return Err(CodeMemoryError);
    }
    let mut cur_prot: mach::VmProt = 0;
    let mut max_prot: mach::VmProt = 0;
    let kr = unsafe {
        mach::mach_vm_remap(
            task,
            &mut rx_addr,
            size as mach::MachVmSize,
            0,
            mach::VM_FLAGS_FIXED | mach::VM_FLAGS_OVERWRITE,
            task,
            rw as mach::MachVmAddress,
            0,
            &mut cur_prot,
            &mut max_prot,
            mach::VM_INHERIT_NONE,
        )
    };
    if kr != mach::KERN_SUCCESS {
        unsafe {
            mach::mach_vm_deallocate(task, rx_addr, size as mach::MachVmSize);
            libc::munmap(rw as *mut libc::c_void, size);
        }
        return Err(CodeMemoryError);
    }
    let kr = unsafe {
        mach::mach_vm_protect(
            task,
            rx_addr,
            size as mach::MachVmSize,
            0,
            mach::VM_PROT_READ | mach::VM_PROT_EXECUTE,
        )
    };
    if kr != mach::KERN_SUCCESS {
        unsafe {
            mach::mach_vm_deallocate(task, rx_addr, size as mach::MachVmSize);
            libc::munmap(rw as *mut libc::c_void, size);
        }
        return Err(CodeMemoryError);
    }
    Ok((rw, rx_addr as *mut u8))
}

#[cfg(target_os = "macos")]
fn unmap_rx_view(ptr: *mut u8, size: usize) {
    unsafe {
        mach::mach_vm_deallocate(
            mach::task_self(),
            ptr as mach::MachVmAddress,
            size as mach::MachVmSize,
        );
    }
}

/// The Mach VM calls the macOS slab alias needs. libSystem exports
/// them; they are declared here directly (the style the
/// `sys_icache_invalidate` declaration below already uses) so the
/// workspace takes no Mach-specific dependency for four calls.
#[cfg(target_os = "macos")]
mod mach {
    pub type KernReturn = libc::c_int;
    pub type MachPort = libc::c_uint;
    pub type MachVmAddress = u64;
    pub type MachVmSize = u64;
    pub type VmProt = libc::c_int;
    pub type VmInherit = libc::c_uint;
    pub type Boolean = libc::c_uint;

    pub const KERN_SUCCESS: KernReturn = 0;
    pub const VM_FLAGS_ANYWHERE: libc::c_int = 0x0001;
    pub const VM_FLAGS_FIXED: libc::c_int = 0x0000;
    pub const VM_FLAGS_OVERWRITE: libc::c_int = 0x4000;
    pub const VM_INHERIT_NONE: VmInherit = 1;
    pub const VM_PROT_READ: VmProt = 0x01;
    pub const VM_PROT_EXECUTE: VmProt = 0x04;

    extern "C" {
        #[link_name = "mach_task_self_"]
        static MACH_TASK_SELF: MachPort;

        pub fn mach_vm_allocate(
            target: MachPort,
            address: *mut MachVmAddress,
            size: MachVmSize,
            flags: libc::c_int,
        ) -> KernReturn;

        pub fn mach_vm_remap(
            target: MachPort,
            target_address: *mut MachVmAddress,
            size: MachVmSize,
            mask: MachVmAddress,
            flags: libc::c_int,
            src_task: MachPort,
            src_address: MachVmAddress,
            copy: Boolean,
            cur_protection: *mut VmProt,
            max_protection: *mut VmProt,
            inheritance: VmInherit,
        ) -> KernReturn;

        pub fn mach_vm_protect(
            target: MachPort,
            address: MachVmAddress,
            size: MachVmSize,
            set_maximum: Boolean,
            new_protection: VmProt,
        ) -> KernReturn;

        pub fn mach_vm_deallocate(
            target: MachPort,
            address: MachVmAddress,
            size: MachVmSize,
        ) -> KernReturn;
    }

    pub fn task_self() -> MachPort {
        unsafe { MACH_TASK_SELF }
    }
}

/// A standalone (one function, one mapping) allocation in its writable
/// phase. On Unix this is a private anonymous `mmap` made directly with
/// libc — the same style as the slab path above — so no dependency's
/// libgcc `__clear_cache` assumption enters the link (see [`page_size`]).
/// On non-Unix targets it wraps mmap-rs, whose non-Unix backends do not
/// carry that assumption.
struct StandaloneMut {
    #[cfg(unix)]
    ptr: *mut u8,
    #[cfg(unix)]
    len: usize,
    #[cfg(not(unix))]
    inner: mmap_rs::MmapMut,
}

unsafe impl Send for StandaloneMut {}
unsafe impl Sync for StandaloneMut {}

impl StandaloneMut {
    fn map(len: usize) -> Result<Self, CodeMemoryError> {
        if len == 0 {
            return Err(CodeMemoryError);
        }
        #[cfg(unix)]
        {
            let ptr = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    len,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANON,
                    -1,
                    0,
                )
            };
            if ptr == libc::MAP_FAILED {
                return Err(CodeMemoryError);
            }
            Ok(Self {
                ptr: ptr as *mut u8,
                len,
            })
        }
        #[cfg(not(unix))]
        {
            let inner = mmap_rs::MmapOptions::new(len)
                .map_err(|_| CodeMemoryError)?
                .map_mut()
                .map_err(|_| CodeMemoryError)?;
            Ok(Self { inner })
        }
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        #[cfg(unix)]
        {
            self.ptr
        }
        #[cfg(not(unix))]
        {
            self.inner.as_mut_ptr()
        }
    }

    fn as_mut_slice(&mut self) -> &mut [u8] {
        #[cfg(unix)]
        {
            unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
        }
        #[cfg(not(unix))]
        {
            self.inner.as_mut_slice()
        }
    }

    /// Flips the mapping to read/execute exactly once. The instruction
    /// cache flush names the written range, matching the slab path's
    /// contract in `write_into_slab`.
    fn make_exec(self) -> Result<StandaloneMap, CodeMemoryError> {
        #[cfg(unix)]
        {
            let ptr = self.ptr;
            let len = self.len;
            // The mapping is unmapped by StandaloneMap's Drop from here
            // on; do not let this phase's Drop run for it.
            std::mem::forget(self);
            let prot = libc::PROT_READ | libc::PROT_EXEC;
            if unsafe { libc::mprotect(ptr as *mut libc::c_void, len, prot) } != 0 {
                unsafe { libc::munmap(ptr as *mut libc::c_void, len) };
                return Err(CodeMemoryError);
            }
            // Mirror the flip this replaces (mmap-rs's make_exec):
            // protect first, then flush the instruction cache over the
            // mapping. Callers have normally flushed the written range
            // already; this covers the whole mapping, as mmap-rs did.
            if flush_instruction_cache(ptr, len).is_err() {
                unsafe { libc::munmap(ptr as *mut libc::c_void, len) };
                return Err(CodeMemoryError);
            }
            Ok(StandaloneMap { ptr, len })
        }
        #[cfg(not(unix))]
        {
            let inner = self.inner.make_exec().map_err(|_| CodeMemoryError)?;
            Ok(StandaloneMap { inner })
        }
    }
}

#[cfg(unix)]
impl Drop for StandaloneMut {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
}

/// A standalone mapping after its one-way flip to executable. Owns the
/// mapping; dropping unmaps it.
struct StandaloneMap {
    #[cfg(unix)]
    ptr: *mut u8,
    #[cfg(unix)]
    len: usize,
    #[cfg(not(unix))]
    inner: mmap_rs::Mmap,
}

unsafe impl Send for StandaloneMap {}
unsafe impl Sync for StandaloneMap {}

impl StandaloneMap {
    fn as_ptr(&self) -> *mut u8 {
        #[cfg(unix)]
        {
            self.ptr
        }
        #[cfg(not(unix))]
        {
            self.inner.as_ptr().cast_mut()
        }
    }
}

#[cfg(unix)]
impl Drop for StandaloneMap {
    fn drop(&mut self) {
        unsafe { libc::munmap(self.ptr as *mut libc::c_void, self.len) };
    }
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
    Standalone(StandaloneMap),
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
        align_up(code_len, page_size())
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
            let size = align_up(code_len, page_size())?;
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
        let mut map = StandaloneMut::map(len)?;
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
        let page_size = page_size();
        let map = StandaloneMut::map(page_size)
            .expect("test mapping")
            .make_exec()
            .expect("test executable mapping");
        Self {
            ptr: map.as_ptr(),
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
        assert!(CodeArena::allocation_len(1).expect("len") >= page_size());
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
        let code = ExecutableCode::test_stub(page_size() * 2);
        assert_eq!(code.len(), page_size());
    }
}
