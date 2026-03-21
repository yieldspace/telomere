use anyhow::{anyhow, bail, Context};
use std::{
    ffi::OsStr,
    io::{self, Write},
    path::Path,
    process::ExitCode,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc, Mutex,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use telomere::{
    common::{
        ExportDesc, FuncType, HostCallContext, HostCallControl, HostFunctionDefinition, ImportDesc,
        MemArg, Memory, NativeModule, StoreState, VMResult, ValType,
    },
    runtime::instantiate_native_module,
    Module, Registry, ResultValue, Store,
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
const RAW_MEM: MemArg = MemArg {
    align: 0,
    offset: 0,
};

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Debug, Clone, Default)]
pub(crate) struct CapturedStdio {
    stdout: Arc<Mutex<Vec<u8>>>,
    stderr: Arc<Mutex<Vec<u8>>>,
}

impl CapturedStdio {
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

#[derive(Debug, Clone)]
struct StdioDescriptor {
    output: Option<OutputTarget>,
    rights_base: u64,
}

struct CoreWasiPreview1State {
    args: Vec<Vec<u8>>,
    stdout: OutputTarget,
    stderr: OutputTarget,
    open_fds: AtomicU32,
    wall_clock_origin: SystemTime,
    monotonic_origin: Instant,
    exit_code: AtomicU32,
}

impl CoreWasiPreview1State {
    fn new(path: &Path, args: &[String], stdio: StdioMode) -> Self {
        let args = guest_args(path, args)
            .into_iter()
            .map(encode_nul_terminated)
            .collect();
        let (stdout, stderr) = match stdio {
            StdioMode::Inherit => (OutputTarget::Stdout, OutputTarget::Stderr),
            StdioMode::Capture(captured) => (
                OutputTarget::Capture(captured.stdout.clone()),
                OutputTarget::Capture(captured.stderr.clone()),
            ),
        };

        Self {
            args,
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
            CLOCKID_PROCESS_CPUTIME_ID | CLOCKID_THREAD_CPUTIME_ID => Err(ERRNO_NOTSUP),
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
    module
        .imports
        .0
        .iter()
        .any(|import| import.module == PREVIEW1_MODULE)
        && matches!(module.exs.find(START_EXPORT), Some(ExportDesc::Func(_)))
}

pub(crate) async fn run(
    module: Module,
    path: &Path,
    guest_argv: &[String],
    stdio: StdioMode,
) -> anyhow::Result<ExitCode> {
    validate_preview1_module(&module)?;

    let state = Box::new(CoreWasiPreview1State::new(path, guest_argv, stdio));
    let store = Store::new_with_state(unsafe {
        StoreState::from_ptr(state.as_ref() as *const CoreWasiPreview1State)
    });
    let mut registry = Registry::new();
    let host = vm_result_to_anyhow(
        instantiate_native_module(build_preview1_native_module(), &store, &registry).await,
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
    for import in module
        .imports
        .0
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
        let actual = module
            .fts
            .get(type_idx)
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
            "unsupported `{PREVIEW1_MODULE}` imports: {}",
            unsupported.join(", ")
        );
    }
    if !matches!(module.exs.find(START_EXPORT), Some(ExportDesc::Func(_))) {
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
                name: Some("fd_write".to_owned()),
                signature: expected_signature("fd_write").unwrap(),
                fp: host_fd_write,
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

fn preview1_state(store: &Store) -> &CoreWasiPreview1State {
    unsafe { store.state.get::<CoreWasiPreview1State>() }
        .expect("core preview1 host functions require CoreWasiPreview1State in StoreState")
}

fn param_u32(ctx: &HostCallContext<'_, '_>, index: usize) -> u32 {
    match ctx.param(index) {
        Some(telomere::WasmValue::I32(value)) => *value as u32,
        other => panic!("expected i32 param at {index}, got {other:?}"),
    }
}

fn param_i64(ctx: &HostCallContext<'_, '_>, index: usize) -> i64 {
    match ctx.param(index) {
        Some(telomere::WasmValue::I64(value)) => *value,
        other => panic!("expected i64 param at {index}, got {other:?}"),
    }
}

fn return_errno(errno: u32) -> VMResult<HostCallControl> {
    VMResult::Success(HostCallControl::Return(ResultValue::new(vec![
        telomere::WasmValue::I32(errno as i32),
    ])))
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
        VMResult::InvalidOperand => "invalid operand",
        VMResult::UnalignedAtomic => "unaligned atomic",
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

fn write_guest_fdstat(memory: &mut Memory, ptr: u32, rights_base: u64) -> Result<(), u32> {
    write_guest_bytes(memory, ptr, &[0; FDSTAT_SIZE])?;
    vm_result_to_errno(memory.write_u8(RAW_MEM, ptr, FILETYPE_CHARACTER_DEVICE))?;
    write_guest_u16(memory, ptr + 2, 0)?;
    write_guest_u64(memory, ptr + 8, rights_base)?;
    write_guest_u64(memory, ptr + 16, 0)?;
    Ok(())
}

fn host_clock_time_get(mut ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let clock_id = param_u32(&ctx, 0);
    let _precision = param_i64(&ctx, 1);
    let time_ptr = param_u32(&ctx, 2);
    let errno = match preview1_state(ctx.store()).clock_time_ns(clock_id) {
        Ok(value) => {
            match ctx.with_caller_memory(|memory| write_guest_u64(memory, time_ptr, value)) {
                Some(Ok(())) => ERRNO_SUCCESS,
                Some(Err(errno)) => errno,
                None => ERRNO_FAULT,
            }
        }
        Err(errno) => errno,
    };
    return_errno(errno)
}

fn host_args_sizes_get(mut ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let args = preview1_state(ctx.store()).args.clone();
    let argc_ptr = param_u32(&ctx, 0);
    let argv_buf_size_ptr = param_u32(&ctx, 1);

    let errno = match ctx.with_caller_memory(|memory| {
        let argc = args.len() as u32;
        let argv_buf_size = args.iter().map(|arg| arg.len() as u32).sum::<u32>();
        write_guest_u32(memory, argc_ptr, argc)
            .and_then(|_| write_guest_u32(memory, argv_buf_size_ptr, argv_buf_size))
    }) {
        Some(Ok(())) => ERRNO_SUCCESS,
        Some(Err(errno)) => errno,
        None => ERRNO_FAULT,
    };

    return_errno(errno)
}

fn host_args_get(mut ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let args = preview1_state(ctx.store()).args.clone();
    let argv_ptr = param_u32(&ctx, 0);
    let argv_buf_ptr = param_u32(&ctx, 1);

    let errno = match ctx.with_caller_memory(|memory| {
        let mut cursor = argv_buf_ptr;
        args.iter().enumerate().try_for_each(|(index, arg)| {
            let entry_ptr = argv_ptr
                .checked_add((index as u32) * 4)
                .ok_or(ERRNO_FAULT)?;
            write_guest_u32(memory, entry_ptr, cursor)?;
            write_guest_bytes(memory, cursor, arg)?;
            cursor = cursor.checked_add(arg.len() as u32).ok_or(ERRNO_FAULT)?;
            Ok(())
        })
    }) {
        Some(Ok(())) => ERRNO_SUCCESS,
        Some(Err(errno)) => errno,
        None => ERRNO_FAULT,
    };

    return_errno(errno)
}

fn host_fd_write(mut ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let state = preview1_state(ctx.store());
    let fd = param_u32(&ctx, 0);
    let iovs_ptr = param_u32(&ctx, 1);
    let iovs_len = param_u32(&ctx, 2);
    let nwritten_ptr = param_u32(&ctx, 3);

    let output = match state.descriptor_for_fd(fd) {
        Ok(StdioDescriptor {
            output: Some(output),
            ..
        }) => output,
        _ => return return_errno(ERRNO_BADF),
    };

    let mut total = 0u32;
    let errno = (|| -> Result<u32, u32> {
        for index in 0..iovs_len {
            let base = iovs_ptr
                .checked_add(index.checked_mul(8).ok_or(ERRNO_FAULT)?)
                .ok_or(ERRNO_FAULT)?;
            let (ptr, len) = match ctx.with_caller_memory(|memory| {
                Ok::<_, u32>((
                    read_guest_u32(memory, base)?,
                    read_guest_u32(memory, base + 4)?,
                ))
            }) {
                Some(Ok(pair)) => pair,
                Some(Err(errno)) => return Err(errno),
                None => return Err(ERRNO_FAULT),
            };
            match ctx.with_caller_memory(|memory| {
                let bytes = read_guest_bytes(memory, ptr, len)?;
                output.write_all(bytes).map_err(|_| ERRNO_IO)
            }) {
                Some(Ok(())) => {}
                Some(Err(errno)) => return Err(errno),
                None => return Err(ERRNO_FAULT),
            }
            total = total.checked_add(len).ok_or(ERRNO_FAULT)?;
        }
        output.flush().map_err(|_| ERRNO_IO)?;
        match ctx.with_caller_memory(|memory| write_guest_u32(memory, nwritten_ptr, total)) {
            Some(Ok(())) => Ok(ERRNO_SUCCESS),
            Some(Err(errno)) => Err(errno),
            None => Err(ERRNO_FAULT),
        }
    })()
    .unwrap_or_else(|errno| errno);

    return_errno(errno)
}

fn host_fd_close(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let fd = param_u32(&ctx, 0);
    let errno = preview1_state(ctx.store())
        .close_fd(fd)
        .map(|_| ERRNO_SUCCESS)
        .unwrap_or_else(|errno| errno);
    return_errno(errno)
}

fn host_fd_fdstat_get(mut ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let fd = param_u32(&ctx, 0);
    let stat_ptr = param_u32(&ctx, 1);
    let errno = match preview1_state(ctx.store()).descriptor_for_fd(fd) {
        Ok(descriptor) => match ctx.with_caller_memory(|memory| {
            write_guest_fdstat(memory, stat_ptr, descriptor.rights_base)
        }) {
            Some(Ok(())) => ERRNO_SUCCESS,
            Some(Err(errno)) => errno,
            None => ERRNO_FAULT,
        },
        Err(errno) => errno,
    };
    return_errno(errno)
}

fn host_fd_seek(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let fd = param_u32(&ctx, 0);
    let _offset = param_i64(&ctx, 1);
    let whence = param_u32(&ctx, 2);
    let _new_offset_ptr = param_u32(&ctx, 3);
    let errno = match whence {
        0..=2 => preview1_state(ctx.store())
            .descriptor_for_fd(fd)
            .map(|_| ERRNO_SPIPE)
            .unwrap_or_else(|errno| errno),
        _ => ERRNO_INVAL,
    };
    return_errno(errno)
}

fn host_proc_exit(ctx: HostCallContext<'_, '_>) -> VMResult<HostCallControl> {
    let exit_code = param_u32(&ctx, 0);
    preview1_state(ctx.store()).set_exit_code(exit_code);
    VMResult::Success(HostCallControl::EndProgram)
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
        let state = Box::new(CoreWasiPreview1State::new(
            Path::new("guest.wasm"),
            guest_args,
            StdioMode::Capture(CapturedStdio::default()),
        ));
        let store = Store::new_with_state(unsafe {
            StoreState::from_ptr(state.as_ref() as *const CoreWasiPreview1State)
        });
        let mut registry = Registry::new();
        let host = instantiate_native_module(build_preview1_native_module(), &store, &registry)
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
            StdioMode::Capture(captured.clone()),
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
                i32.const 2
                i64.const 0
                i32.const 16
                call $clock_time_get
                i32.const 58
                i32.ne
                if
                  i32.const 5
                  return
                end
                i32.const 3
                i64.const 0
                i32.const 16
                call $clock_time_get
                i32.const 58
                i32.ne
                if
                  i32.const 6
                  return
                end
                i32.const 9
                i64.const 0
                i32.const 16
                call $clock_time_get
                i32.const 28
                i32.ne
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
}
