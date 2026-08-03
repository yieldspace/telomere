//! `telomere-cli`, the command-line runner for the telomere runtime.
//!
//! It has three entry points: calling an exported function of a core Wasm
//! module, running a core module as a WASI preview1 command, and running a WASI
//! 0.2 component that exports `wasi:cli/run@0.2.6`. Argument parsing lives in
//! [`cli`], the preview1 host functions in [`core_wasi_preview1`], and the
//! component host setup in [`component_cli`].

use anyhow::Context;
use clap::Parser;
use cli::{Cli, Command, CoreCommand};
use std::{ffi::OsString, fs, path::Path, process::ExitCode};
use telomere::{JitConfig, ResultValue, RuntimeConfig, WasmValue};

mod cli;
mod component_cli;
mod core_wasi_preview1;

#[tokio::main]
async fn main() -> ExitCode {
    match try_main().await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("{error:?}");
            ExitCode::FAILURE
        }
    }
}

async fn try_main() -> anyhow::Result<ExitCode> {
    let raw_args: Vec<OsString> = std::env::args_os().collect();
    let args = Cli::parse_from(raw_args.clone());

    if let Some(Command::Component(command)) = args.command {
        return component_cli::run(command).await;
    }

    let command = args.core_command(&raw_args)?;
    run_core_module(command).await
}

async fn run_core_module(command: CoreCommand) -> anyhow::Result<ExitCode> {
    run_core_module_with_stdio(command, core_wasi_preview1::StdioMode::Inherit).await
}

async fn run_core_module_with_stdio(
    command: CoreCommand,
    stdio: core_wasi_preview1::StdioMode,
) -> anyhow::Result<ExitCode> {
    let module = parse_module_from_path(&command.name)?;
    if should_run_wasi(&command, &module) {
        return core_wasi_preview1::run(
            module,
            &command.name,
            &command.tail_args,
            stdio,
            command.jit,
            command.jit_code_cache_mib,
        )
        .await;
    }

    if command.args_after_separator {
        anyhow::bail!(
            "`{}` uses guest argv syntax (`-- ...`) but does not import `{}` with an exported `_start`",
            command.name.display(),
            core_wasi_preview1::PREVIEW1_MODULE
        );
    }
    let (func, wasm_args) = legacy_core_invocation(&command, &module)?;
    let ret = run_exported_core_function(
        module,
        &func,
        &wasm_args,
        command.jit,
        command.jit_code_cache_mib,
    )
    .await?;
    println!("{}", format_result_values(&ret));
    Ok(ExitCode::SUCCESS)
}

fn parse_module_from_path(path: &Path) -> anyhow::Result<telomere::Module> {
    let bytes = fs::read(path)
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to read module `{}`", path.display()))?;
    let mut reader = telomere::IoReadBinaryReader::from(&bytes[..]);
    let mut parser = telomere::WasmParser::new(&mut reader);
    parser
        .parse_module()
        .map_err(anyhow::Error::from)
        .with_context(|| format!("failed to parse module `{}`", path.display()))
}

fn should_run_wasi(command: &CoreCommand, module: &telomere::Module) -> bool {
    core_wasi_preview1::is_preview1_command_module(module)
        && (command.args_after_separator || command.tail_args.is_empty())
}

fn legacy_core_invocation(
    command: &CoreCommand,
    module: &telomere::Module,
) -> anyhow::Result<(String, ResultValue)> {
    let Some((func, arg_values)) = command.tail_args.split_first() else {
        if core_wasi_preview1::is_preview1_command_module(module) {
            anyhow::bail!(
                "use `-- ...` to pass guest argv to the preview1 module `{}`",
                command.name.display()
            );
        }
        anyhow::bail!(
            "function name is required unless `{}` imports `{}` and exports `_start`",
            command.name.display(),
            core_wasi_preview1::PREVIEW1_MODULE
        );
    };

    let wasm_args = arg_values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            value.parse::<i32>().map(WasmValue::I32).map_err(|error| {
                anyhow::anyhow!(
                    "legacy core argument {} (`{}`) must be an i32: {error}",
                    index + 1,
                    value
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok((func.clone(), ResultValue::new(wasm_args)))
}

async fn run_exported_core_function(
    module: telomere::Module,
    func: &str,
    wasm_args: &ResultValue,
    jit: bool,
    jit_code_cache_mib: u32,
) -> anyhow::Result<ResultValue> {
    if jit && !telomere::jit_supported() {
        anyhow::bail!(
            "`--jit` requires the `jit` feature on a supported target: macOS arm64, macOS/Linux x86_64, or Linux-gnu riscv64gc"
        );
    }
    let store = if jit {
        let mut runtime_config = RuntimeConfig::default();
        runtime_config.jit = JitConfig {
            enabled: true,
            code_cache_max_bytes: jit_code_cache_mib.saturating_mul(1024 * 1024),
        };
        telomere::Store::new_with_runtime_config(runtime_config)
    } else {
        telomere::Store::new()
    };
    let registry = telomere::Registry::new();
    let instance = vm_result_to_anyhow(
        telomere::instantiate(module, &store, &registry).await,
        "failed to instantiate core module",
    )?;
    vm_result_to_anyhow(
        telomere::run_module_function(&instance, &store, func, wasm_args).await,
        format!("failed to invoke exported function `{func}`"),
    )
}

fn format_result_values(ret: &ResultValue) -> String {
    ret.iter()
        .map(|value| match value {
            WasmValue::I32(value) => value.to_string(),
            WasmValue::I64(value) => value.to_string(),
            WasmValue::F32(value) => value.to_string(),
            WasmValue::F64(value) => value.to_string(),
            WasmValue::V128(value) => value.to_string(),
            WasmValue::FuncRef(value) => value.to_string(),
            WasmValue::ExternRef(value) => value.to_string(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn vm_result_to_anyhow<T>(
    result: telomere::VMResult<T>,
    context: impl Into<String>,
) -> anyhow::Result<T> {
    let context = context.into();
    match result {
        telomere::VMResult::Success(value) => Ok(value),
        telomere::VMResult::Unreachable => Err(anyhow::anyhow!("{context}: unreachable")),
        telomere::VMResult::StackOverflow => Err(anyhow::anyhow!("{context}: stack overflow")),
        telomere::VMResult::MemoryIndexOutOfRange => {
            Err(anyhow::anyhow!("{context}: memory index out of range"))
        }
        telomere::VMResult::TableIndexOutOfRange => {
            Err(anyhow::anyhow!("{context}: table index out of range"))
        }
        telomere::VMResult::CallIndirectInvalidType => {
            Err(anyhow::anyhow!("{context}: call_indirect invalid type"))
        }
        telomere::VMResult::TableUninitialized => {
            Err(anyhow::anyhow!("{context}: table uninitialized"))
        }
        telomere::VMResult::Unlinkable => Err(anyhow::anyhow!("{context}: unlinkable")),
        telomere::VMResult::MemoryAllocationFailed => {
            Err(anyhow::anyhow!("{context}: memory allocation failed"))
        }
        telomere::VMResult::InvalidOperand => Err(anyhow::anyhow!("{context}: invalid operand")),
        telomere::VMResult::UnalignedAtomic => Err(anyhow::anyhow!("{context}: unaligned atomic")),
        telomere::VMResult::Unimplemented => Err(anyhow::anyhow!("{context}: unimplemented")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_path(stem: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time must be after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("telomere-cli-{stem}-{nonce}.wasm"))
    }

    fn example_path(file: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples")
            .join(file)
    }

    /// Run the committed preview1 example with `args` and return its stdout.
    async fn run_preview1_hello(args: Vec<String>) -> String {
        let path = example_path("wasi-preview1-hello.wasm");
        let command = CoreCommand {
            name: path,
            tail_args: args,
            args_after_separator: true,
            jit: false,
            jit_code_cache_mib: 4,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("the preview1 example should run");
        assert_eq!(exit, ExitCode::SUCCESS);

        String::from_utf8(captured.stdout()).expect("the example only writes utf-8")
    }

    fn write_wasm(stem: &str, wat: &str) -> std::path::PathBuf {
        let path = unique_temp_path(stem);
        let bytes = wat::parse_str(wat).expect("wat must parse");
        fs::write(&path, bytes).expect("wasm must be writable");
        path
    }

    const PREVIEW1_GREETING: &str = "hello from telomere (wasi preview1)";
    const PREVIEW1_ARGV0: &str = "wasi-preview1-hello.wasm";

    #[tokio::test]
    async fn preview1_example_echoes_the_documented_argv() {
        let stdout = run_preview1_hello(vec!["one".to_owned(), "two".to_owned()]).await;

        assert_eq!(
            stdout,
            format!("{PREVIEW1_GREETING}\n{PREVIEW1_ARGV0}\none\ntwo\n")
        );
    }

    /// The example used to place the argv pointer array at a fixed 64 and the
    /// string buffer at a fixed 256, so 49 arguments made the pointer array run
    /// into the strings. It now sizes both regions from `args_sizes_get`.
    #[tokio::test]
    async fn preview1_example_survives_a_pointer_array_longer_than_the_old_fixed_slot() {
        let args: Vec<String> = (1..=64).map(|index| format!("a{index}")).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    /// The example used to hold argv strings in a fixed 768-byte window that sat
    /// below the static newline at 1088, so ~832 bytes of arguments overwrote
    /// the separator. The buffer is now sized by `argv_buf_size`, and the static
    /// data all lives below the argv region.
    #[tokio::test]
    async fn preview1_example_survives_argv_longer_than_the_old_fixed_buffer() {
        let args: Vec<String> = (0..11).map(|_| "x".repeat(80)).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    /// argv larger than the example's one declared page has to make the guest
    /// grow linear memory rather than write out of bounds.
    #[tokio::test]
    async fn preview1_example_grows_memory_for_argv_larger_than_one_page() {
        let args: Vec<String> = (0..900).map(|_| "y".repeat(100)).collect();
        let stdout = run_preview1_hello(args.clone()).await;

        let mut expected = vec![PREVIEW1_GREETING.to_owned(), PREVIEW1_ARGV0.to_owned()];
        expected.extend(args);
        assert_eq!(stdout.lines().collect::<Vec<_>>(), expected);
    }

    #[tokio::test]
    async fn core_runner_errors_when_function_is_missing_for_non_wasi_module() {
        let path = write_wasm(
            "non-wasi",
            r#"
            (module
              (func (export "main") (result i32) (i32.const 42)))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
        };

        let error = run_core_module(command)
            .await
            .expect_err("missing function name must fail");
        assert!(error.to_string().contains("function name is required"));

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_runner_auto_detects_preview1_start() {
        let path = write_wasm(
            "preview1",
            r#"
            (module
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (func (export "_start")
                i32.const 3
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
        };

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(core_wasi_preview1::CapturedStdio::default()),
        )
        .await
        .expect("preview1 auto detection should succeed");
        assert_eq!(exit, ExitCode::from(3));

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_wasi_preview1_call_indirect_stdout_survives_optimizer() {
        let path = write_wasm(
            "preview1-indirect-stdout",
            r#"
            (module
              (type $fdwrite (func (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (type $fdwrite)))
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $write_via_import)
              (memory 1)
              (data (i32.const 0) "\08\00\00\00\0f\00\00\00")
              (data (i32.const 8) "hello indirect\n")
              (func $write_via_import (type $fdwrite)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                call $fd_write)
              (func (export "_start")
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 32
                i32.const 0
                call_indirect (type $fdwrite)
                drop
                i32.const 0
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("preview1 indirect stdout path should succeed");

        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(captured.stdout(), b"hello indirect\n");

        let _ = fs::remove_file(path);
    }

    #[tokio::test]
    async fn core_wasi_preview1_call_indirect_index_loaded_from_memory_survives_optimizer() {
        let path = write_wasm(
            "preview1-indirect-memory-index",
            r#"
            (module
              (type $fdwrite (func (param i32 i32 i32 i32) (result i32)))
              (import "wasi_snapshot_preview1" "fd_write" (func $fd_write (type $fdwrite)))
              (import "wasi_snapshot_preview1" "proc_exit" (func $proc_exit (param i32)))
              (table 1 funcref)
              (elem (i32.const 0) $write_via_import)
              (memory 1)
              (data (i32.const 0) "\08\00\00\00\0f\00\00\00")
              (data (i32.const 8) "hello indirect\n")
              (func $write_via_import (type $fdwrite)
                local.get 0
                local.get 1
                local.get 2
                local.get 3
                call $fd_write)
              (func (export "_start")
                i32.const 64
                i32.const 0
                i32.store
                i32.const 64
                i32.const 1
                i32.store
                i32.const 64
                i32.const 0
                i32.store
                i32.const 1
                i32.const 0
                i32.const 1
                i32.const 32
                i32.const 64
                i32.load
                call_indirect (type $fdwrite)
                drop
                i32.const 0
                call $proc_exit))
            "#,
        );
        let command = CoreCommand {
            name: path.clone(),
            tail_args: vec![],
            args_after_separator: false,
            jit: false,
            jit_code_cache_mib: 4,
        };
        let captured = core_wasi_preview1::CapturedStdio::default();

        let exit = run_core_module_with_stdio(
            command,
            core_wasi_preview1::StdioMode::Capture(captured.clone()),
        )
        .await
        .expect("preview1 indirect memory index path should succeed");

        assert_eq!(exit, ExitCode::SUCCESS);
        assert_eq!(captured.stdout(), b"hello indirect\n");

        let _ = fs::remove_file(path);
    }
}
