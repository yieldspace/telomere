use anyhow::{anyhow, bail, Context};
use std::{
    ffi::OsStr,
    io::{self, Read, Write},
    path::Path,
    process::ExitCode,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use telomere::{
    component_support::common::{
        module_exports, module_function_types, module_imports, ExportDesc, FuncType, ImportDesc,
        MemArg, ValType,
    },
    unstable_internals::pending_effect_count,
    unstable_internals::{
        instantiate_raw_native_module, ExecuteContext, HostFunctionDefinition, Instr, NativeModule,
    },
    JitConfig, Memory, Module, Registry, ResultValue, RuntimeConfig, Store, StoreState, VMResult,
};

pub(crate) const PREVIEW1_MODULE: &str = "wasi_snapshot_preview1";
const START_EXPORT: &str = "_start";

const ERRNO_SUCCESS: u32 = 0;
const ERRNO_BADF: u32 = 8;
const ERRNO_FAULT: u32 = 21;
const ERRNO_INVAL: u32 = 28;
const ERRNO_IO: u32 = 29;
const ERRNO_NOTSUP: u32 = 58;
const ERRNO_SPIPE: u32 = 70;
const EXIT_CODE_UNSET: u32 = u32::MAX;
const CLOCKID_REALTIME: u32 = 0;
const CLOCKID_MONOTONIC: u32 = 1;
const CLOCKID_PROCESS_CPUTIME_ID: u32 = 2;
const CLOCKID_THREAD_CPUTIME_ID: u32 = 3;
const FILETYPE_CHARACTER_DEVICE: u8 = 2;
const RIGHTS_FD_READ: u64 = 1 << 1;
const RIGHTS_FD_WRITE: u64 = 1 << 6;
const FDSTAT_SIZE: usize = 24;
const FD_STDIN_BIT: u32 = 1 << 0;
const FD_STDOUT_BIT: u32 = 1 << 1;
const FD_STDERR_BIT: u32 = 1 << 2;
const OPEN_STDIO_MASK: u32 = FD_STDIN_BIT | FD_STDOUT_BIT | FD_STDERR_BIT;
/// Staging buffer size for `fd_read` and `random_get`, so a guest-controlled
/// iovec or buffer length never sizes a host allocation directly.
const IO_CHUNK_SIZE: usize = 8192;

const RAW_MEM: MemArg = MemArg {
    align: 0,
    offset: 0,
};

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub(crate) struct CapturedStdio {
    stdin: Arc<Mutex<io::Cursor<Vec<u8>>>>,
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl CapturedStdio {
    #[cfg(test)]
    pub(crate) fn with_stdin(stdin: Vec<u8>) -> Self {
        Self {
            stdin: Arc::new(Mutex::new(io::Cursor::new(stdin))),
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn stdout(&self) -> Vec<u8> {
        self.stdout.lock().expect("stdout buffer poisoned").clone()
    }

    #[cfg(test)]
    pub(crate) fn stderr(&self) -> Vec<u8> {
        self.stderr.lock().expect("stderr buffer poisoned").clone()
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) enum StdioMode {
    Inherit,
    Capture(CapturedStdio),
}

#[derive(Debug, Clone)]
enum OutputTarget {
    Stdout,
    Stderr,
    Capture(Arc<Mutex<Vec<u8>>>),
}

impl OutputTarget {
    fn write_all(&self, bytes: &[u8]) -> io::Result<()> {
        match self {
            Self::Stdout => io::stdout().lock().write_all(bytes),
            Self::Stderr => io::stderr().lock().write_all(bytes),
            Self::Capture(buffer) => {
                buffer
                    .lock()
                    .expect("captured stdio buffer poisoned")
                    .extend_from_slice(bytes);
                Ok(())
            }
        }
    }

    fn flush(&self) -> io::Result<()> {
        match self {
            Self::Stdout => io::stdout().lock().flush(),
            Self::Stderr => io::stderr().lock().flush(),
            Self::Capture(_) => Ok(()),
        }
    }
}

/// Test double for a stdin source whose writer stays open (a terminal or a
/// pipe): each queued chunk is delivered by a single `read`, truncated to
/// the caller's buffer with the remainder kept for the next read, and once
/// the queue is drained a further read reports `WouldBlock` instead of EOF,
/// so a host function that keeps reading after a short read fails loudly
/// instead of silently observing end-of-input.
#[cfg(test)]
#[derive(Debug)]
struct ChunkedStdin {
    chunks: std::collections::VecDeque<Vec<u8>>,
    reads: usize,
}

#[cfg(test)]
impl ChunkedStdin {
    fn new(chunks: Vec<Vec<u8>>) -> Self {
        Self {
            chunks: chunks.into(),
            reads: 0,
        }
    }

    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reads += 1;
        let Some(front) = self.chunks.front_mut() else {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "chunked stdin drained: writer still open",
            ));
        };
        let count = front.len().min(buf.len());
        buf[..count].copy_from_slice(&front[..count]);
        front.drain(..count);
        if front.is_empty() {
            self.chunks.pop_front();
        }
        Ok(count)
    }
}

#[derive(Debug, Clone)]
enum InputSource {
    Inherit,
    Buffer(Arc<Mutex<io::Cursor<Vec<u8>>>>),
    #[cfg(test)]
    Chunked(Arc<Mutex<ChunkedStdin>>),
}

impl InputSource {
    fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Inherit => io::stdin().lock().read(buf),
            Self::Buffer(cursor) => cursor
                .lock()
                .expect("captured stdin buffer poisoned")
                .read(buf),
            #[cfg(test)]
            Self::Chunked(stdin) => stdin.lock().expect("chunked stdin poisoned").read(buf),
        }
    }
}

#[derive(Debug, Clone)]
struct StdioDescriptor {
    output: Option<OutputTarget>,
    rights_base: u64,
}

struct CoreWasiPreview1State {
    args: Vec<Vec<u8>>,
    environ: Vec<Vec<u8>>,
    stdin: InputSource,
    stdout: OutputTarget,
    stderr: OutputTarget,
    open_fds: AtomicU32,
    wall_clock_origin: SystemTime,
    monotonic_origin: Instant,
    exit_code: AtomicU32,
}

unsafe fn proc_exit_wait_for_effects(
    _tail_code: *const Instr,
    ctx: &mut ExecuteContext,
) -> VMResult<()> {
    if pending_effect_count(ctx) != 0 {
        ctx.cont = PROC_EXIT_PROGRAM.as_ptr();
        return VMResult::Success(());
    }
    ctx.cont = std::ptr::null();
    VMResult::Success(())
}

const PROC_EXIT_PROGRAM: [Instr; 1] = [Instr::from_op(proc_exit_wait_for_effects)];

impl CoreWasiPreview1State {
    fn with_environ(path: &Path, args: &[String], stdio: StdioMode, environ: Vec<Vec<u8>>) -> Self {
        let args = guest_args(path, args)
            .into_iter()
            .map(encode_nul_terminated)
            .collect();
        let (stdin, stdout, stderr) = match stdio {
            StdioMode::Inherit => (
                InputSource::Inherit,
                OutputTarget::Stdout,
                OutputTarget::Stderr,
            ),
            StdioMode::Capture(captured) => (
                InputSource::Buffer(captured.stdin.clone()),
                OutputTarget::Capture(captured.stdout.clone()),
                OutputTarget::Capture(captured.stderr.clone()),
            ),
        };

        Self {
            args,
            environ,
            stdin,
            stdout,
            stderr,
            open_fds: AtomicU32::new(OPEN_STDIO_MASK),
            wall_clock_origin: SystemTime::now(),
            monotonic_origin: Instant::now(),
            exit_code: AtomicU32::new(EXIT_CODE_UNSET),
        }
    }

    fn exit_code(&self) -> Option<u8> {
        let code = self.exit_code.load(Ordering::SeqCst);
        (code != EXIT_CODE_UNSET).then_some(code as u8)
    }

    fn set_exit_code(&self, code: u32) {
        self.exit_code.store(code, Ordering::SeqCst);
    }

    fn descriptor_for_fd(&self, fd: u32) -> Result<StdioDescriptor, u32> {
        let bit = fd_bit(fd).ok_or(ERRNO_BADF)?;
        if self.open_fds.load(Ordering::Acquire) & bit == 0 {
            return Err(ERRNO_BADF);
        }
        match fd {
            0 => Ok(StdioDescriptor {
                output: None,
                rights_base: RIGHTS_FD_READ,
            }),
            1 => Ok(StdioDescriptor {
                output: Some(self.stdout.clone()),
                rights_base: RIGHTS_FD_WRITE,
            }),
            2 => Ok(StdioDescriptor {
                output: Some(self.stderr.clone()),
                rights_base: RIGHTS_FD_WRITE,
            }),
            _ => Err(ERRNO_BADF),
        }
    }

    fn close_fd(&self, fd: u32) -> Result<(), u32> {
        let bit = fd_bit(fd).ok_or(ERRNO_BADF)?;
        self.open_fds
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                (current & bit != 0).then_some(current & !bit)
            })
            .map(|_| ())
            .map_err(|_| ERRNO_BADF)
    }

    fn clock_time_ns(&self, clock_id: u32) -> Result<u64, u32> {
        match clock_id {
            CLOCKID_REALTIME => {
                let now = self
                    .wall_clock_origin
                    .checked_add(self.monotonic_origin.elapsed())
                    .unwrap_or(self.wall_clock_origin);
                Ok(system_time_to_ns(now))
            }
            CLOCKID_MONOTONIC => Ok(duration_to_ns(self.monotonic_origin.elapsed())),
            CLOCKID_PROCESS_CPUTIME_ID => process_cpu_time_ns(),
            CLOCKID_THREAD_CPUTIME_ID => thread_cpu_time_ns(),
            _ => Err(ERRNO_INVAL),
        }
    }
}

fn fd_bit(fd: u32) -> Option<u32> {
    match fd {
        0 => Some(FD_STDIN_BIT),
        1 => Some(FD_STDOUT_BIT),
        2 => Some(FD_STDERR_BIT),
        _ => None,
    }
}

pub(crate) fn is_preview1_command_module(module: &Module) -> bool {
    module_imports(module)
        .iter()
        .any(|import| import.module == PREVIEW1_MODULE)
        && module_exports(module)
            .iter()
            .any(|export| export.0 == START_EXPORT && matches!(export.1, ExportDesc::Func(_)))
}

pub(crate) async fn run(
    module: Module,
    path: &Path,
    guest_argv: &[String],
    environ: Vec<Vec<u8>>,
    stdio: StdioMode,
    jit: bool,
    jit_code_cache_mib: u32,
) -> anyhow::Result<ExitCode> {
    validate_preview1_module(&module)?;
    if jit && !telomere::jit_supported() {
        bail!(
            "`--jit` requires the `jit` feature on a supported target: macOS arm64, macOS/Linux x86_64, or Linux-gnu riscv64gc"
        );
    }

    let state = Box::new(CoreWasiPreview1State::with_environ(
        path, guest_argv, stdio, environ,
    ));
    let state_ptr = unsafe { StoreState::from_ptr(state.as_ref() as *const CoreWasiPreview1State) };
    let mut runtime_config = RuntimeConfig::default();
    runtime_config.jit = JitConfig { enabled: jit };
    runtime_config.limits.max_code_cache_bytes = jit_code_cache_mib.saturating_mul(1024 * 1024);
    let store = Store::new_with_state_and_runtime_config(state_ptr, runtime_config);
    let mut registry = Registry::new();
    let host = vm_result_to_anyhow(
        instantiate_raw_native_module(build_preview1_native_module(), &store, &registry).await,
        "failed to instantiate `wasi_snapshot_preview1` host module",
    )?;
    registry.register(PREVIEW1_MODULE, host);
    let instance = vm_result_to_anyhow(
        telomere::instantiate(module, &store, &registry).await,
        format!("failed to instantiate `{}`", path.display()),
    )?;

    let start_result = telomere::component_support::runtime::run_core_export_sync_reentrant(
        &instance,
        &store,
        START_EXPORT,
        &ResultValue::new(vec![]),
    )
    .map_err(|error| anyhow!("failed to invoke `{START_EXPORT}` synchronously: {error}"))?;
    match start_result {
        VMResult::Success(_) => Ok(ExitCode::from(state.exit_code().unwrap_or(0))),
        _ if state.exit_code().is_some() => Ok(ExitCode::from(state.exit_code().unwrap())),
        other => Err(anyhow!(
            "failed to invoke `{START_EXPORT}` in `{}`: {}",
            path.display(),
            describe_vm_result(&other)
        )),
    }
}

fn validate_preview1_module(module: &Module) -> anyhow::Result<()> {
    let mut unsupported = Vec::new();
    for import in module_imports(module)
        .iter()
        .filter(|import| import.module == PREVIEW1_MODULE)
    {
        let ImportDesc::TypeIdx(type_idx) = import.desc else {
            bail!(
                "unsupported `{PREVIEW1_MODULE}` import `{}`: only function imports are supported",
                import.name
            );
        };
        let expected = if let Some(expected) = expected_signature(&import.name) {
            expected
        } else {
            unsupported.push(import.name.clone());
            continue;
        };
        let actual = module_function_types(module)
            .get(type_idx.0 as usize)
            .with_context(|| format!("missing function type for import `{}`", import.name))?;
        if actual != &expected {
            bail!(
                "unsupported `{PREVIEW1_MODULE}` import `{}`: signature mismatch (expected {:?}, got {:?})",
                import.name,
                expected,
                actual
            );
        }
    }
    if !unsupported.is_empty() {
        unsupported.sort();
        bail!(
            "unsupported `{PREVIEW1_MODULE}` imports: {} (the preview1 runner implements a scoped subset of preview1; see docs/core/preview1-scope.md)",
            unsupported.join(", ")
        );
    }
    if !module_exports(module)
        .iter()
        .any(|export| export.0 == START_EXPORT && matches!(export.1, ExportDesc::Func(_)))
    {
        bail!("`{PREVIEW1_MODULE}` modules must export `{START_EXPORT}`");
    }
    Ok(())
}

fn build_preview1_native_module() -> NativeModule {
    NativeModule {
        functions: vec![
            HostFunctionDefinition {
                name: Some("args_sizes_get".to_owned()),
                signature: expected_signature("args_sizes_get").unwrap(),
                fp: host_args_sizes_get,
            },
            HostFunctionDefinition {
                name: Some("args_get".to_owned()),
                signature: expected_signature("args_get").unwrap(),
                fp: host_args_get,
            },
            HostFunctionDefinition {
                name: Some("environ_sizes_get".to_owned()),
                signature: expected_signature("environ_sizes_get").unwrap(),
                fp: host_environ_sizes_get,
            },
            HostFunctionDefinition {
                name: Some("environ_get".to_owned()),
                signature: expected_signature("environ_get").unwrap(),
                fp: host_environ_get,
            },
            HostFunctionDefinition {
                name: Some("clock_time_get".to_owned()),
                signature: expected_signature("clock_time_get").unwrap(),
                fp: host_clock_time_get,
            },
            HostFunctionDefinition {
                name: Some("fd_close".to_owned()),
                signature: expected_signature("fd_close").unwrap(),
                fp: host_fd_close,
            },
            HostFunctionDefinition {
                name: Some("fd_fdstat_get".to_owned()),
                signature: expected_signature("fd_fdstat_get").unwrap(),
                fp: host_fd_fdstat_get,
            },
            HostFunctionDefinition {
                name: Some("fd_seek".to_owned()),
                signature: expected_signature("fd_seek").unwrap(),
                fp: host_fd_seek,
            },
            HostFunctionDefinition {
                name: Some("fd_read".to_owned()),
                signature: expected_signature("fd_read").unwrap(),
                fp: host_fd_read,
            },
            HostFunctionDefinition {
                name: Some("fd_write".to_owned()),
                signature: expected_signature("fd_write").unwrap(),
                fp: host_fd_write,
            },
            HostFunctionDefinition {
                name: Some("random_get".to_owned()),
                signature: expected_signature("random_get").unwrap(),
                fp: host_random_get,
            },
            HostFunctionDefinition {
                name: Some("proc_exit".to_owned()),
                signature: expected_signature("proc_exit").unwrap(),
                fp: host_proc_exit,
            },
        ],
    }
}

fn expected_signature(name: &str) -> Option<FuncType> {
    let i32x1 = vec![ValType::I32];
    let i32x2 = vec![ValType::I32, ValType::I32];
    let i32x4 = vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32];
    Some(match name {
        "args_sizes_get" => FuncType::new(i32x2.clone(), i32x1.clone()),
        "args_get" => FuncType::new(i32x2, i32x1.clone()),
        "clock_time_get" => FuncType::new(
            vec![ValType::I32, ValType::I64, ValType::I32],
            i32x1.clone(),
        ),
        "fd_close" => FuncType::new(vec![ValType::I32], i32x1.clone()),
        "fd_fdstat_get" => FuncType::new(vec![ValType::I32, ValType::I32], i32x1.clone()),
        "fd_seek" => FuncType::new(
            vec![ValType::I32, ValType::I64, ValType::I32, ValType::I32],
            i32x1.clone(),
        ),
        "fd_write" => FuncType::new(i32x4, i32x1.clone()),
        "proc_exit" => FuncType::new(vec![ValType::I32], vec![]),
        "environ_sizes_get" => FuncType::new(vec![ValType::I32, ValType::I32], i32x1.clone()),
        "environ_get" => FuncType::new(vec![ValType::I32, ValType::I32], i32x1.clone()),
        "fd_read" => FuncType::new(
            vec![ValType::I32, ValType::I32, ValType::I32, ValType::I32],
            i32x1.clone(),
        ),
        "random_get" => FuncType::new(vec![ValType::I32, ValType::I32], i32x1),
        _ => return None,
    })
}

fn encode_nul_terminated(value: impl Into<String>) -> Vec<u8> {
    let mut bytes = value.into().into_bytes();
    bytes.push(0);
    bytes
}

fn guest_args(path: &Path, args: &[String]) -> Vec<String> {
    let arg0 = path
        .file_name()
        .and_then(OsStr::to_str)
        .map(str::to_owned)
        .unwrap_or_else(|| path.display().to_string());
    std::iter::once(arg0).chain(args.iter().cloned()).collect()
}

/// Collect the guest environment for a preview1 command module, mirroring
/// the component subcommand's semantics: the host process environment is
/// inherited unless `inherit` is false, and each `KEY=VALUE` spec then
/// replaces or adds one entry. Entries are returned in preview1 wire form
/// (`KEY=VALUE` bytes with a trailing NUL).
pub(crate) fn collect_guest_environ(
    inherit: bool,
    specs: &[String],
) -> anyhow::Result<Vec<Vec<u8>>> {
    let mut entries: Vec<Vec<u8>> = Vec::new();
    if inherit {
        for (key, value) in std::env::vars_os() {
            let mut entry = os_str_bytes(&key);
            entry.push(b'=');
            entry.extend_from_slice(&os_str_bytes(&value));
            entries.push(entry);
        }
    }
    for spec in specs {
        let (key, value) = spec
            .split_once('=')
            .ok_or_else(|| anyhow!("invalid --env `{spec}`: expected KEY=VALUE"))?;
        if key.is_empty() {
            bail!("invalid --env `{spec}`: key must not be empty");
        }
        let mut entry = key.as_bytes().to_vec();
        entry.push(b'=');
        entry.extend_from_slice(value.as_bytes());
        if let Some(existing) = entries
            .iter_mut()
            .find(|entry| environ_entry_key(entry) == key.as_bytes())
        {
            *existing = entry;
        } else {
            entries.push(entry);
        }
    }
    Ok(entries
        .into_iter()
        .map(|mut entry| {
            entry.push(0);
            entry
        })
        .collect())
}

fn environ_entry_key(entry: &[u8]) -> &[u8] {
    entry.split(|byte| *byte == b'=').next().unwrap_or(entry)
}

#[cfg(unix)]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    std::os::unix::ffi::OsStrExt::as_bytes(value).to_vec()
}

#[cfg(not(unix))]
fn os_str_bytes(value: &OsStr) -> Vec<u8> {
    value.to_string_lossy().into_owned().into_bytes()
}

fn preview1_state(store: &Store) -> &CoreWasiPreview1State {
    unsafe { store.state.get::<CoreWasiPreview1State>() }
        .expect("core preview1 host functions require CoreWasiPreview1State in StoreState")
}

fn local_u32(ctx: &ExecuteContext, local_addr: usize) -> u32 {
    u32::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), local_addr, 4)
            .try_into()
            .expect("u32 arguments must occupy 4 bytes"),
    )
}

fn local_i64(ctx: &ExecuteContext, local_addr: usize) -> i64 {
    i64::from_le_bytes(
        ctx.stack
            .local_bytes(&ctx.local_reference(), local_addr, 8)
            .try_into()
            .expect("i64 arguments must occupy 8 bytes"),
    )
}

fn return_errno(ctx: &mut ExecuteContext, errno: u32) -> VMResult<*const Instr> {
    ctx.return_slot().write(&errno.to_le_bytes());
    let (prev_local_ref, return_addr) = ctx.stack.function_return_in_place(
        &ctx.local_reference,
        std::mem::size_of::<u32>(),
        ctx.gc,
    );
    ctx.set_local_reference(prev_local_ref);
    VMResult::Success(return_addr)
}

fn write_guest_u32(memory: &mut Memory, ptr: u32, value: u32) -> Result<(), u32> {
    vm_result_to_errno(memory.write_u32(RAW_MEM, ptr, value))
}

fn write_guest_u16(memory: &mut Memory, ptr: u32, value: u16) -> Result<(), u32> {
    vm_result_to_errno(memory.write_u16(RAW_MEM, ptr, value))
}

fn write_guest_u64(memory: &mut Memory, ptr: u32, value: u64) -> Result<(), u32> {
    vm_result_to_errno(memory.write_u64(RAW_MEM, ptr, value))
}

fn read_guest_u32(memory: &Memory, ptr: u32) -> Result<u32, u32> {
    vm_result_to_errno(memory.read_u32(RAW_MEM, ptr))
}

fn write_guest_bytes(memory: &mut Memory, ptr: u32, bytes: &[u8]) -> Result<(), u32> {
    let start = usize::try_from(ptr).map_err(|_| ERRNO_FAULT)?;
    let end = start.checked_add(bytes.len()).ok_or(ERRNO_FAULT)?;
    let target = memory.get_mut(start..end).ok_or(ERRNO_FAULT)?;
    target.copy_from_slice(bytes);
    Ok(())
}

fn read_guest_bytes(memory: &Memory, ptr: u32, len: u32) -> Result<&[u8], u32> {
    let start = usize::try_from(ptr).map_err(|_| ERRNO_FAULT)?;
    let end = start
        .checked_add(usize::try_from(len).map_err(|_| ERRNO_FAULT)?)
        .ok_or(ERRNO_FAULT)?;
    memory.get(start..end).ok_or(ERRNO_FAULT)
}

fn vm_result_to_errno<T>(result: VMResult<T>) -> Result<T, u32> {
    match result {
        VMResult::Success(value) => Ok(value),
        _ => Err(ERRNO_FAULT),
    }
}

fn vm_result_to_anyhow<T>(result: VMResult<T>, context: impl Into<String>) -> anyhow::Result<T> {
    let context = context.into();
    match result {
        VMResult::Success(value) => Ok(value),
        other => Err(anyhow!("{context}: {}", describe_vm_result(&other))),
    }
}

fn describe_vm_result<T>(result: &VMResult<T>) -> &'static str {
    match result {
        VMResult::Success(_) => "success",
        VMResult::Unreachable => "unreachable",
        VMResult::StackOverflow => "stack overflow",
        VMResult::MemoryIndexOutOfRange => "memory index out of range",
        VMResult::TableIndexOutOfRange => "table index out of range",
        VMResult::CallIndirectInvalidType => "call_indirect invalid type",
        VMResult::TableUninitialized => "table uninitialized",
        VMResult::Unlinkable => "unlinkable",
        VMResult::MemoryAllocationFailed => "memory allocation failed",
        VMResult::InvalidOperand => "invalid operand",
        VMResult::DivideByZero => "integer divide by zero",
        VMResult::IntegerOverflow => "integer overflow",
        VMResult::InvalidConversion => "invalid conversion to integer",
        VMResult::UnalignedAtomic => "unaligned atomic",
        VMResult::Unimplemented => "unimplemented",
        VMResult::FuelExhausted => "fuel exhausted",
        VMResult::Cancelled => "cancelled",
        VMResult::ResourceLimitExceeded => "resource limit exceeded",
    }
}

fn duration_to_ns(duration: std::time::Duration) -> u64 {
    duration
        .as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(duration.subsec_nanos()))
}

fn system_time_to_ns(time: SystemTime) -> u64 {
    duration_to_ns(time.duration_since(UNIX_EPOCH).unwrap_or_default())
}

#[cfg(unix)]
fn process_cpu_time_ns() -> Result<u64, u32> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
    if result != 0 {
        return Err(ERRNO_IO);
    }

    let usage = unsafe { usage.assume_init() };
    Ok(timeval_to_ns(usage.ru_utime).saturating_add(timeval_to_ns(usage.ru_stime)))
}

#[cfg(not(unix))]
fn process_cpu_time_ns() -> Result<u64, u32> {
    Err(ERRNO_NOTSUP)
}

#[cfg(unix)]
fn timeval_to_ns(time: libc::timeval) -> u64 {
    let secs = u64::try_from(time.tv_sec).unwrap_or_default();
    let micros = u64::try_from(time.tv_usec).unwrap_or_default();
    secs.saturating_mul(1_000_000_000)
        .saturating_add(micros.saturating_mul(1_000))
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn thread_cpu_time_ns() -> Result<u64, u32> {
    let mut time = std::mem::MaybeUninit::<libc::timespec>::zeroed();
    let result = unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, time.as_mut_ptr()) };
    if result != 0 {
        return Err(ERRNO_NOTSUP);
    }

    Ok(timespec_to_ns(unsafe { time.assume_init() }))
}

#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
fn thread_cpu_time_ns() -> Result<u64, u32> {
    Err(ERRNO_NOTSUP)
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn timespec_to_ns(time: libc::timespec) -> u64 {
    let secs = u64::try_from(time.tv_sec).unwrap_or_default();
    let nanos = u64::try_from(time.tv_nsec).unwrap_or_default();
    secs.saturating_mul(1_000_000_000).saturating_add(nanos)
}

fn write_guest_fdstat(memory: &mut Memory, ptr: u32, rights_base: u64) -> Result<(), u32> {
    write_guest_bytes(memory, ptr, &[0; FDSTAT_SIZE])?;
    vm_result_to_errno(memory.write_u8(RAW_MEM, ptr, FILETYPE_CHARACTER_DEVICE))?;
    write_guest_u16(memory, ptr + 2, 0)?;
    write_guest_u64(memory, ptr + 8, rights_base)?;
    write_guest_u64(memory, ptr + 16, 0)?;
    Ok(())
}

fn host_clock_time_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let clock_id = local_u32(ctx, 0);
    let _precision = local_i64(ctx, 4);
    let time_ptr = local_u32(ctx, 12);
    let errno = match preview1_state(ctx.store).clock_time_ns(clock_id) {
        Ok(value) => {
            if let Some(memory) = ctx.caller_memory() {
                write_guest_u64(memory, time_ptr, value)
                    .map(|_| ERRNO_SUCCESS)
                    .unwrap_or_else(|errno| errno)
            } else {
                ERRNO_FAULT
            }
        }
        Err(errno) => errno,
    };
    return_errno(ctx, errno)
}

/// Write the (count, total byte size) pair for a preview1 string table
/// (argv or environ) at the two guest pointers.
fn string_table_sizes(
    memory: &mut Memory,
    entries: &[Vec<u8>],
    count_ptr: u32,
    buf_size_ptr: u32,
) -> Result<(), u32> {
    let count = entries.len() as u32;
    let buf_size = entries.iter().map(|entry| entry.len() as u32).sum::<u32>();
    write_guest_u32(memory, count_ptr, count)?;
    write_guest_u32(memory, buf_size_ptr, buf_size)
}

/// Write a preview1 string table (argv or environ): a pointer array at
/// `table_ptr` addressing the NUL-terminated entries laid out from `buf_ptr`.
fn write_string_table(
    memory: &mut Memory,
    entries: &[Vec<u8>],
    table_ptr: u32,
    buf_ptr: u32,
) -> Result<(), u32> {
    let mut cursor = buf_ptr;
    for (index, entry) in entries.iter().enumerate() {
        let entry_ptr = table_ptr
            .checked_add((index as u32) * 4)
            .ok_or(ERRNO_FAULT)?;
        write_guest_u32(memory, entry_ptr, cursor)?;
        write_guest_bytes(memory, cursor, entry)?;
        cursor = cursor.checked_add(entry.len() as u32).ok_or(ERRNO_FAULT)?;
    }
    Ok(())
}

fn host_args_sizes_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let argc_ptr = local_u32(ctx, 0);
    let argv_buf_size_ptr = local_u32(ctx, 4);

    let errno = if let Some(memory) = ctx.caller_memory() {
        string_table_sizes(memory, &state.args, argc_ptr, argv_buf_size_ptr)
            .map(|_| ERRNO_SUCCESS)
            .unwrap_or_else(|errno| errno)
    } else {
        ERRNO_FAULT
    };

    return_errno(ctx, errno)
}

fn host_args_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let argv_ptr = local_u32(ctx, 0);
    let argv_buf_ptr = local_u32(ctx, 4);

    let errno = if let Some(memory) = ctx.caller_memory() {
        write_string_table(memory, &state.args, argv_ptr, argv_buf_ptr)
            .map(|_| ERRNO_SUCCESS)
            .unwrap_or_else(|errno| errno)
    } else {
        ERRNO_FAULT
    };

    return_errno(ctx, errno)
}

fn host_environ_sizes_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let count_ptr = local_u32(ctx, 0);
    let buf_size_ptr = local_u32(ctx, 4);

    let errno = if let Some(memory) = ctx.caller_memory() {
        string_table_sizes(memory, &state.environ, count_ptr, buf_size_ptr)
            .map(|_| ERRNO_SUCCESS)
            .unwrap_or_else(|errno| errno)
    } else {
        ERRNO_FAULT
    };

    return_errno(ctx, errno)
}

fn host_environ_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let environ_ptr = local_u32(ctx, 0);
    let environ_buf_ptr = local_u32(ctx, 4);

    let errno = if let Some(memory) = ctx.caller_memory() {
        write_string_table(memory, &state.environ, environ_ptr, environ_buf_ptr)
            .map(|_| ERRNO_SUCCESS)
            .unwrap_or_else(|errno| errno)
    } else {
        ERRNO_FAULT
    };

    return_errno(ctx, errno)
}

fn host_fd_read(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let fd = local_u32(ctx, 0);
    let iovs_ptr = local_u32(ctx, 4);
    let iovs_len = local_u32(ctx, 8);
    let nread_ptr = local_u32(ctx, 12);

    // Only stdin is readable in this runner: fds 1 and 2 are write-only and
    // every other descriptor is unknown (descriptor_for_fd also reports
    // BADF once fd 0 has been closed). See docs/core/preview1-scope.md.
    if fd != 0 || state.descriptor_for_fd(fd).is_err() {
        return return_errno(ctx, ERRNO_BADF);
    }

    let mut total = 0u32;
    let errno = (|| -> Result<u32, u32> {
        let mut chunk = [0u8; IO_CHUNK_SIZE];
        'iovs: for index in 0..iovs_len {
            let base = iovs_ptr
                .checked_add(index.checked_mul(8).ok_or(ERRNO_FAULT)?)
                .ok_or(ERRNO_FAULT)?;
            let (ptr, len) = {
                let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
                (
                    read_guest_u32(memory, base)?,
                    read_guest_u32(memory, base + 4)?,
                )
            };
            let mut filled = 0u32;
            while filled < len {
                let want = usize::try_from(len - filled)
                    .map_err(|_| ERRNO_FAULT)?
                    .min(IO_CHUNK_SIZE);
                let read = state.stdin.read(&mut chunk[..want]).map_err(|_| ERRNO_IO)?;
                if read == 0 {
                    // EOF: stop filling this and any later iovecs.
                    break 'iovs;
                }
                let dest = ptr.checked_add(filled).ok_or(ERRNO_FAULT)?;
                let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
                write_guest_bytes(memory, dest, &chunk[..read])?;
                filled += read as u32;
                total = total.checked_add(read as u32).ok_or(ERRNO_FAULT)?;
                if read < want {
                    // Short read: the source delivered everything it
                    // currently has (one terminal line, or a pipe chunk
                    // whose writer is still open). Return the accumulated
                    // count instead of blocking on another read to fill
                    // this iovec, and do not touch later iovecs.
                    break 'iovs;
                }
            }
        }
        let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
        write_guest_u32(memory, nread_ptr, total)?;
        Ok(ERRNO_SUCCESS)
    })()
    .unwrap_or_else(|errno| errno);

    return_errno(ctx, errno)
}

fn host_fd_write(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let state = preview1_state(ctx.store);
    let fd = local_u32(ctx, 0);
    let iovs_ptr = local_u32(ctx, 4);
    let iovs_len = local_u32(ctx, 8);
    let nwritten_ptr = local_u32(ctx, 12);

    let output = match state.descriptor_for_fd(fd) {
        Ok(StdioDescriptor {
            output: Some(output),
            ..
        }) => output,
        _ => return return_errno(ctx, ERRNO_BADF),
    };

    let mut total = 0u32;
    let errno = (|| -> Result<u32, u32> {
        for index in 0..iovs_len {
            let base = iovs_ptr
                .checked_add(index.checked_mul(8).ok_or(ERRNO_FAULT)?)
                .ok_or(ERRNO_FAULT)?;
            let (ptr, len) = {
                let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
                (
                    read_guest_u32(memory, base)?,
                    read_guest_u32(memory, base + 4)?,
                )
            };
            {
                let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
                let bytes = read_guest_bytes(memory, ptr, len)?;
                output.write_all(bytes).map_err(|_| ERRNO_IO)?;
            }
            total = total.checked_add(len).ok_or(ERRNO_FAULT)?;
        }
        output.flush().map_err(|_| ERRNO_IO)?;
        let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
        write_guest_u32(memory, nwritten_ptr, total)?;
        Ok(ERRNO_SUCCESS)
    })()
    .unwrap_or_else(|errno| errno);

    return_errno(ctx, errno)
}

fn host_fd_close(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let fd = local_u32(ctx, 0);
    let errno = preview1_state(ctx.store)
        .close_fd(fd)
        .map(|_| ERRNO_SUCCESS)
        .unwrap_or_else(|errno| errno);
    return_errno(ctx, errno)
}

fn host_fd_fdstat_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let fd = local_u32(ctx, 0);
    let stat_ptr = local_u32(ctx, 4);
    let errno = match preview1_state(ctx.store).descriptor_for_fd(fd) {
        Ok(descriptor) => {
            if let Some(memory) = ctx.caller_memory() {
                write_guest_fdstat(memory, stat_ptr, descriptor.rights_base)
                    .map(|_| ERRNO_SUCCESS)
                    .unwrap_or_else(|errno| errno)
            } else {
                ERRNO_FAULT
            }
        }
        Err(errno) => errno,
    };
    return_errno(ctx, errno)
}

fn host_fd_seek(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let fd = local_u32(ctx, 0);
    let _offset = local_i64(ctx, 4);
    let whence = local_u32(ctx, 12);
    let _new_offset_ptr = local_u32(ctx, 16);
    let errno = match whence {
        0..=2 => preview1_state(ctx.store)
            .descriptor_for_fd(fd)
            .map(|_| ERRNO_SPIPE)
            .unwrap_or_else(|errno| errno),
        _ => ERRNO_INVAL,
    };
    return_errno(ctx, errno)
}

fn host_random_get(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let buf_ptr = local_u32(ctx, 0);
    let buf_len = local_u32(ctx, 4);

    // OS entropy via getrandom, the same source the component WASI random
    // provider uses (crates/telomere-component-wasi/src/provider/random.rs).
    let errno = (|| -> Result<u32, u32> {
        let mut chunk = [0u8; IO_CHUNK_SIZE];
        let mut filled = 0u32;
        while filled < buf_len {
            let want = usize::try_from(buf_len - filled)
                .map_err(|_| ERRNO_FAULT)?
                .min(IO_CHUNK_SIZE);
            getrandom::fill(&mut chunk[..want]).map_err(|_| ERRNO_IO)?;
            let dest = buf_ptr.checked_add(filled).ok_or(ERRNO_FAULT)?;
            let memory = ctx.caller_memory().ok_or(ERRNO_FAULT)?;
            write_guest_bytes(memory, dest, &chunk[..want])?;
            filled += want as u32;
        }
        Ok(ERRNO_SUCCESS)
    })()
    .unwrap_or_else(|errno| errno);

    return_errno(ctx, errno)
}

fn host_proc_exit(ctx: &mut ExecuteContext) -> VMResult<*const Instr> {
    let exit_code = local_u32(ctx, 0);
    preview1_state(ctx.store).set_exit_code(exit_code);

    let mut local_reference = ctx.local_reference;
    while local_reference.local_size != 0 {
        let (prev_local_ref, _return_addr) =
            ctx.stack
                .function_return_in_place(&local_reference, 0, ctx.gc);
        local_reference = prev_local_ref;
    }
    ctx.set_local_reference(local_reference);

    VMResult::Success(PROC_EXIT_PROGRAM.as_ptr())
}

#[cfg(test)]
mod tests {
    use super::*;
    use telomere::WasmValue;

    fn parse_module(wat: &str) -> Module {
        let bytes = wat::parse_str(wat).expect("wat must parse");
        let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
        let mut parser = telomere::WasmParser::new(&mut reader);
        parser.parse_module().expect("module must parse")
    }

    async fn invoke_export(module: Module, export: &str, guest_args: &[String]) -> ResultValue {
        invoke_export_with_stdio(
            module,
            export,
            guest_args,
            StdioMode::Capture(CapturedStdio::default()),
            Vec::new(),
        )
        .await
    }

    async fn invoke_export_with_stdio(
        module: Module,
        export: &str,
        guest_args: &[String],
        stdio: StdioMode,
        environ: Vec<Vec<u8>>,
    ) -> ResultValue {
        let state = Box::new(CoreWasiPreview1State::with_environ(
            Path::new("guest.wasm"),
            guest_args,
            stdio,
            environ,
        ));
        let store = Store::new_with_state(unsafe {
            StoreState::from_ptr(state.as_ref() as *const CoreWasiPreview1State)
        });
        let mut registry = Registry::new();
        let host = instantiate_raw_native_module(build_preview1_native_module(), &store, &registry)
            .await
            .unwrap();
        registry.register(PREVIEW1_MODULE, host);
        let instance = telomere::instantiate(module, &store, &registry)
            .await
            .unwrap();
        telomere::run_module_function(&instance, &store, export, &ResultValue::new(vec![]))
            .await
            .unwrap()
    }

    async fn invoke_export_with_chunked_stdin(
        module: Module,
        export: &str,
        chunks: Vec<Vec<u8>>,
    ) -> (ResultValue, Arc<Mutex<ChunkedStdin>>) {
        let stdin = Arc::new(Mutex::new(ChunkedStdin::new(chunks)));
        let mut state = Box::new(CoreWasiPreview1State::with_environ(
            Path::new("guest.wasm"),
            &[],
            StdioMode::Capture(CapturedStdio::default()),
            Vec::new(),
        ));
        state.stdin = InputSource::Chunked(stdin.clone());
        let store = Store::new_with_state(unsafe {
            StoreState::from_ptr(state.as_ref() as *const CoreWasiPreview1State)
        });
        let mut registry = Registry::new();
        let host = instantiate_raw_native_module(build_preview1_native_module(), &store, &registry)
            .await
            .unwrap();
        registry.register(PREVIEW1_MODULE, host);
        let instance = telomere::instantiate(module, &store, &registry)
            .await
            .unwrap();
        let result =
            telomere::run_module_function(&instance, &store, export, &ResultValue::new(vec![]))
                .await
                .unwrap();
        (result, stdin)
    }

    fn expect_single_i32(result: &ResultValue) -> i32 {
        let values = result.iter().collect::<Vec<_>>();
        assert_eq!(values.len(), 1, "expected exactly one return value");
        let WasmValue::I32(value) = values[0] else {
            panic!("expected i32 result, got {values:?}");
        };
        *value
    }

    #[tokio::test]
    async fn preview1_runner_writes_stdout_and_maps_proc_exit() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (memory (export "memory") 1)
              (data (i32.const 32) "hello\n")
              (func (export "_start")
                i32.const 0
                i32.const 32
                i32.store
                i32.const 4
                i32.const 6
                i32.store
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 8
                call $fd_write
                drop
                i32.const 7
                call $proc_exit))
            "#,
        );
        let captured = CapturedStdio::default();
        let exit = run(
            module,
            Path::new("guest.wasm"),
            &[],
            Vec::new(),
            StdioMode::Capture(captured.clone()),
            false,
            4,
        )
        .await
        .expect("preview1 run should succeed");

        assert_eq!(exit, ExitCode::from(7));
        assert_eq!(captured.stdout(), b"hello\n");
        assert!(captured.stderr().is_empty());
    }

    #[tokio::test]
    async fn preview1_runner_populates_args() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "args_sizes_get" (func $args_sizes_get (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "args_get" (func $args_get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 4
                call $args_sizes_get
                drop
                i32.const 0
                i32.load
                i32.const 3
                i32.ne
                if
                  i32.const 1
                  return
                end
                i32.const 16
                i32.const 64
                call $args_get
                drop
                i32.const 16
                i32.const 4
                i32.add
                i32.load
                i32.load8_u
                i32.const 111
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 16
                i32.const 4
                i32.add
                i32.load
                i32.const 1
                i32.add
                i32.load8_u
                i32.const 110
                i32.ne
                if
                  i32.const 3
                  return
                end
                i32.const 16
                i32.const 4
                i32.add
                i32.load
                i32.const 2
                i32.add
                i32.load8_u
                i32.const 101
                i32.ne
                if
                  i32.const 4
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &["one".to_owned(), "two".to_owned()]).await;

        assert_eq!(expect_single_i32(&result), 0);
    }

    #[cfg_attr(
        debug_assertions,
        ignore = "caller-memory regression is validated in release"
    )]
    #[tokio::test]
    async fn preview1_runner_repeated_args_sizes_get_keeps_caller_memory() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "args_sizes_get" (func $args_sizes_get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                (local $remaining i32)
                i32.const 256
                local.set $remaining
                block $done
                  loop $loop
                    local.get $remaining
                    i32.eqz
                    br_if $done
                    i32.const 0
                    i32.const 4
                    call $args_sizes_get
                    drop
                    i32.const 0
                    i32.load
                    i32.const 3
                    i32.ne
                    if
                      i32.const 1
                      return
                    end
                    local.get $remaining
                    i32.const 1
                    i32.sub
                    local.set $remaining
                    br $loop
                  end
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &["one".to_owned(), "two".to_owned()]).await;

        assert_eq!(expect_single_i32(&result), 0);
    }

    #[test]
    fn preview1_validation_lists_unsupported_imports() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "poll_oneoff" (func (param i32 i32 i32 i32) (result i32)))
              (func (export "_start")))
            "#,
        );

        let error = validate_preview1_module(&module).expect_err("unsupported import must fail");
        assert!(error.to_string().contains("poll_oneoff"));
    }

    #[tokio::test]
    async fn preview1_runner_reports_clock_time_results() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "clock_time_get" (func $clock_time_get (param i32 i64 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                (local $errno i32)
                i32.const 0
                i64.const -1
                i64.store
                i32.const 0
                i64.const 0
                i32.const 0
                call $clock_time_get
                if
                  i32.const 1
                  return
                end
                i32.const 0
                i64.load
                i64.const -1
                i64.eq
                if
                  i32.const 2
                  return
                end
                i32.const 8
                i64.const -1
                i64.store
                i32.const 1
                i64.const 0
                i32.const 8
                call $clock_time_get
                if
                  i32.const 3
                  return
                end
                i32.const 8
                i64.load
                i64.const -1
                i64.eq
                if
                  i32.const 4
                  return
                end
                i32.const 16
                i64.const -1
                i64.store
                i32.const 2
                i64.const 0
                i32.const 16
                call $clock_time_get
                local.set $errno
                local.get $errno
                if
                  i32.const 5
                  return
                end
                i32.const 16
                i64.load
                i64.const -1
                i64.eq
                if
                  i32.const 6
                  return
                end
                i32.const 24
                i64.const -1
                i64.store
                i32.const 3
                i64.const 0
                i32.const 24
                call $clock_time_get
                local.set $errno
                local.get $errno
                i32.const 58
                i32.eq
                if
                  i32.const 24
                  i64.load
                  i64.const -1
                  i64.ne
                  if
                    i32.const 8
                    return
                  end
                else
                  local.get $errno
                  if
                    i32.const 7
                    return
                  end
                  i32.const 24
                  i64.load
                  i64.const -1
                  i64.eq
                  if
                    i32.const 8
                    return
                  end
                end
                i32.const 9
                i64.const 0
                i32.const 32
                call $clock_time_get
                i32.const 28
                i32.ne
                if
                  i32.const 9
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;
        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_process_cpu_clock_reports_success_for_coremark_style_timer() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "clock_time_get" (func $clock_time_get (param i32 i64 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i64.const 0
                i64.store
                i32.const 2
                i64.const 0
                i32.const 0
                call $clock_time_get
                if
                  i32.const 7
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;
        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_runner_reports_fdstat_for_stdio_and_badf_after_close() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_fdstat_get" (func $fd_fdstat_get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 0
                call $fd_fdstat_get
                if
                  i32.const 1
                  return
                end
                i32.const 0
                i32.load8_u
                i32.const 2
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 8
                i64.load
                i64.const 2
                i64.ne
                if
                  i32.const 3
                  return
                end
                i32.const 1
                i32.const 24
                call $fd_fdstat_get
                if
                  i32.const 4
                  return
                end
                i32.const 24
                i32.load8_u
                i32.const 2
                i32.ne
                if
                  i32.const 5
                  return
                end
                i32.const 32
                i64.load
                i64.const 64
                i64.ne
                if
                  i32.const 6
                  return
                end
                i32.const 1
                call $fd_close
                if
                  i32.const 7
                  return
                end
                i32.const 1
                i32.const 48
                call $fd_fdstat_get
                i32.const 8
                i32.ne
                if
                  i32.const 8
                  return
                end
                i32.const 1
                call $fd_close
                i32.const 8
                i32.ne
                if
                  i32.const 9
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;
        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_runner_close_changes_fd_write_to_badf() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (data (i32.const 64) "x")
              (func (export "check") (result i32)
                i32.const 0
                i32.const 64
                i32.store
                i32.const 4
                i32.const 1
                i32.store
                i32.const 1
                call $fd_close
                if
                  i32.const 1
                  return
                end
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 8
                call $fd_write
                i32.const 8
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;
        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_runner_fd_seek_reports_spipe_badf_and_inval() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_seek" (func $fd_seek (param i32 i64 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 1
                i64.const 0
                i32.const 0
                i32.const 0
                call $fd_seek
                i32.const 70
                i32.ne
                if
                  i32.const 1
                  return
                end
                i32.const 1
                i64.const 0
                i32.const 9
                i32.const 0
                call $fd_seek
                i32.const 28
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 2
                call $fd_close
                if
                  i32.const 3
                  return
                end
                i32.const 2
                i64.const 0
                i32.const 0
                i32.const 0
                call $fd_seek
                i32.const 8
                i32.ne
                if
                  i32.const 4
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;
        assert_eq!(expect_single_i32(&result), 0);
    }
    #[tokio::test]
    async fn preview1_runner_populates_environ() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "environ_sizes_get" (func $sizes (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "environ_get" (func $get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 4
                call $sizes
                if
                  i32.const 1
                  return
                end
                i32.const 0
                i32.load
                i32.const 2
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 4
                i32.load
                i32.const 10
                i32.ne
                if
                  i32.const 3
                  return
                end
                i32.const 16
                i32.const 64
                call $get
                if
                  i32.const 4
                  return
                end
                i32.const 16
                i32.load
                i32.const 64
                i32.ne
                if
                  i32.const 5
                  return
                end
                i32.const 20
                i32.load
                i32.const 68
                i32.ne
                if
                  i32.const 6
                  return
                end
                i32.const 64
                i32.load8_u
                i32.const 65
                i32.ne
                if
                  i32.const 7
                  return
                end
                i32.const 65
                i32.load8_u
                i32.const 61
                i32.ne
                if
                  i32.const 8
                  return
                end
                i32.const 68
                i32.load8_u
                i32.const 66
                i32.ne
                if
                  i32.const 9
                  return
                end
                i32.const 0))
            "#,
        );
        let environ = vec![b"A=1\0".to_vec(), b"B=two\0".to_vec()];
        let result = invoke_export_with_stdio(
            module,
            "check",
            &[],
            StdioMode::Capture(CapturedStdio::default()),
            environ,
        )
        .await;

        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_runner_reads_stdin_until_eof() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_close" (func $fd_close (param i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 64
                i32.store
                i32.const 4
                i32.const 8
                i32.store
                i32.const 0
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                if
                  i32.const 1
                  return
                end
                i32.const 16
                i32.load
                i32.const 3
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 64
                i32.load8_u
                i32.const 97
                i32.ne
                if
                  i32.const 3
                  return
                end
                i32.const 66
                i32.load8_u
                i32.const 99
                i32.ne
                if
                  i32.const 4
                  return
                end
                i32.const 0
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                if
                  i32.const 5
                  return
                end
                i32.const 16
                i32.load
                i32.const 0
                i32.ne
                if
                  i32.const 6
                  return
                end
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                i32.const 8
                i32.ne
                if
                  i32.const 7
                  return
                end
                i32.const 0
                call $fd_close
                if
                  i32.const 8
                  return
                end
                i32.const 0
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                i32.const 8
                i32.ne
                if
                  i32.const 9
                  return
                end
                i32.const 0))
            "#,
        );
        let result = invoke_export_with_stdio(
            module,
            "check",
            &[],
            StdioMode::Capture(CapturedStdio::with_stdin(b"abc".to_vec())),
            Vec::new(),
        )
        .await;

        assert_eq!(expect_single_i32(&result), 0);
    }

    #[tokio::test]
    async fn preview1_runner_fd_read_returns_short_read_without_rereading() {
        // The chunked source holds one 2-byte chunk and reports WouldBlock
        // (not EOF) if read again, modelling a pipe whose writer stays
        // open. fd_read must return the 2 available bytes after exactly
        // one source read instead of blocking to fill the 4-byte iovec,
        // and must not touch the second iovec after the short read.
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 64
                i32.store
                i32.const 4
                i32.const 4
                i32.store
                i32.const 8
                i32.const 128
                i32.store
                i32.const 12
                i32.const 4
                i32.store
                i32.const 0
                i32.const 0
                i32.const 2
                i32.const 16
                call $fd_read
                if
                  i32.const 1
                  return
                end
                i32.const 16
                i32.load
                i32.const 2
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 64
                i32.load8_u
                i32.const 97
                i32.ne
                if
                  i32.const 3
                  return
                end
                i32.const 65
                i32.load8_u
                i32.const 98
                i32.ne
                if
                  i32.const 4
                  return
                end
                i32.const 128
                i32.load8_u
                if
                  i32.const 5
                  return
                end
                i32.const 0))
            "#,
        );
        let (result, stdin) =
            invoke_export_with_chunked_stdin(module, "check", vec![b"ab".to_vec()]).await;

        assert_eq!(expect_single_i32(&result), 0);
        assert_eq!(stdin.lock().expect("chunked stdin poisoned").reads, 1);
    }

    #[tokio::test]
    async fn preview1_runner_fd_read_continues_after_full_reads() {
        // A chunk larger than the iovec is delivered across two calls: the
        // first read fills the iovec exactly, the second returns the
        // remainder as a short read without a third, blocking read.
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_read" (func $fd_read (param i32 i32 i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                i32.const 0
                i32.const 64
                i32.store
                i32.const 4
                i32.const 4
                i32.store
                i32.const 0
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                if
                  i32.const 1
                  return
                end
                i32.const 16
                i32.load
                i32.const 4
                i32.ne
                if
                  i32.const 2
                  return
                end
                i32.const 64
                i32.load8_u
                i32.const 97
                i32.ne
                if
                  i32.const 3
                  return
                end
                i32.const 67
                i32.load8_u
                i32.const 100
                i32.ne
                if
                  i32.const 4
                  return
                end
                i32.const 0
                i32.const 0
                i32.const 1
                i32.const 16
                call $fd_read
                if
                  i32.const 5
                  return
                end
                i32.const 16
                i32.load
                i32.const 2
                i32.ne
                if
                  i32.const 6
                  return
                end
                i32.const 64
                i32.load8_u
                i32.const 101
                i32.ne
                if
                  i32.const 7
                  return
                end
                i32.const 65
                i32.load8_u
                i32.const 102
                i32.ne
                if
                  i32.const 8
                  return
                end
                i32.const 0))
            "#,
        );
        let (result, stdin) =
            invoke_export_with_chunked_stdin(module, "check", vec![b"abcdef".to_vec()]).await;

        assert_eq!(expect_single_i32(&result), 0);
        assert_eq!(stdin.lock().expect("chunked stdin poisoned").reads, 2);
    }

    #[tokio::test]
    async fn preview1_runner_random_get_fills_guest_buffer() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "random_get" (func $random_get (param i32 i32) (result i32)))
              (memory (export "memory") 1)
              (func (export "check") (result i32)
                (local $i i32)
                (local $first i32)
                i32.const 0
                i32.const 32
                call $random_get
                if
                  i32.const 1
                  return
                end
                i32.const 0
                i32.load8_u
                local.set $first
                i32.const 1
                local.set $i
                block $done
                  loop $loop
                    local.get $i
                    i32.const 32
                    i32.ge_u
                    br_if $done
                    local.get $i
                    i32.load8_u
                    local.get $first
                    i32.ne
                    if
                      i32.const 0
                      return
                    end
                    local.get $i
                    i32.const 1
                    i32.add
                    local.set $i
                    br $loop
                  end
                end
                i32.const 2))
            "#,
        );
        let result = invoke_export(module, "check", &[]).await;

        // A buffer of 32 identical bytes means the fill did not happen;
        // real entropy produces that with probability around 2^-248.
        assert_eq!(expect_single_i32(&result), 0);
    }

    #[test]
    fn preview1_validation_still_rejects_filesystem_imports() {
        let module = parse_module(
            r#"
            (module
              (import "wasi_snapshot_preview1" "fd_prestat_get" (func (param i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "path_open" (func (param i32 i32 i32 i64 i64 i32 i32) (result i32)))
              (func (export "_start")))
            "#,
        );

        let error = validate_preview1_module(&module)
            .expect_err("filesystem imports must stay fail-closed");
        let message = error.to_string();
        assert!(message.contains("fd_prestat_get"), "{message}");
        assert!(message.contains("path_open"), "{message}");
    }
}
