use super::common::next_handle;
use super::WasiHost;
use crate::bindings::types::{
    WasiFilesystemTypesDescriptorBorrow, WasiFilesystemTypesDescriptorFlags,
    WasiFilesystemTypesDescriptorType, WasiFilesystemTypesDirectoryEntry,
    WasiFilesystemTypesDirectoryEntryStreamBorrow, WasiFilesystemTypesErrorCode,
    WasiFilesystemTypesOpenFlags, WasiFilesystemTypesPathFlags, WasiIoErrorErrorBorrow,
    WasiIoStreamsInputStreamBorrow, WasiIoStreamsOutputStreamBorrow, WasiIoStreamsStreamError,
};
use crate::state::OutputStreamKind;
use crate::state::{Preview3FutureEntry, Preview3SocketAddressFamily, WasiState};
use crate::{
    WASI_PREVIEW3_CLI_VERSION, WASI_PREVIEW3_CLOCKS_VERSION, WASI_PREVIEW3_FILESYSTEM_VERSION,
    WASI_PREVIEW3_IO_COMPAT_VERSION, WASI_PREVIEW3_RANDOM_VERSION, WASI_PREVIEW3_SOCKETS_VERSION,
};
use std::net::IpAddr;
use std::rc::Rc;
use std::time::Duration;
use telomere_component::{
    ComponentError, ComponentLinker, ComponentLinkerInstance, ComponentValue,
};

const CLI_ENVIRONMENT: &str = "wasi:cli/environment";
const CLI_EXIT: &str = "wasi:cli/exit";
const CLI_STDERR: &str = "wasi:cli/stderr";
const CLI_STDIN: &str = "wasi:cli/stdin";
const CLI_STDOUT: &str = "wasi:cli/stdout";
const CLOCKS_MONOTONIC: &str = "wasi:clocks/monotonic-clock";
const CLOCKS_WALL: &str = "wasi:clocks/wall-clock";
const FILESYSTEM_PREOPENS: &str = "wasi:filesystem/preopens";
const FILESYSTEM_TYPES: &str = "wasi:filesystem/types";
const IO_ERROR: &str = "wasi:io/error";
const IO_POLL: &str = "wasi:io/poll";
const IO_STREAMS: &str = "wasi:io/streams";
const RANDOM_RANDOM: &str = "wasi:random/random";
const RANDOM_INSECURE: &str = "wasi:random/insecure";
const RANDOM_INSECURE_SEED: &str = "wasi:random/insecure-seed";
const SOCKETS_IP_NAME_LOOKUP: &str = "wasi:sockets/ip-name-lookup";
const SOCKETS_TYPES: &str = "wasi:sockets/types";

pub(crate) fn add_to_linker_async(
    linker: &mut ComponentLinker,
    state: WasiState,
) -> Result<(), ComponentError> {
    let host = Rc::new(WasiHost::new(state));
    register_cli_environment(linker, Rc::clone(&host));
    register_cli_exit(linker, Rc::clone(&host));
    register_cli_stdio(linker, Rc::clone(&host));
    register_clocks(linker, Rc::clone(&host));
    register_filesystem_preopens(linker, Rc::clone(&host));
    register_filesystem_types(linker, Rc::clone(&host));
    register_io_error(linker, Rc::clone(&host));
    register_io_poll(linker, Rc::clone(&host));
    register_io_streams(linker, Rc::clone(&host));
    register_random(linker, Rc::clone(&host));
    register_insecure_random(linker, Rc::clone(&host));
    register_insecure_seed(linker, Rc::clone(&host));
    register_sockets(linker, host);
    Ok(())
}

pub(crate) fn unsupported_interface_error(interface: &str) -> ComponentError {
    ComponentError::Link(format!(
        "{interface} is not supported by telomere-component-wasi Preview 3 provider"
    ))
}

fn interface_name(base: &str) -> String {
    let version = if base.starts_with("wasi:cli/") {
        WASI_PREVIEW3_CLI_VERSION
    } else if base.starts_with("wasi:clocks/") {
        WASI_PREVIEW3_CLOCKS_VERSION
    } else if base.starts_with("wasi:random/") {
        WASI_PREVIEW3_RANDOM_VERSION
    } else if base.starts_with("wasi:filesystem/") {
        WASI_PREVIEW3_FILESYSTEM_VERSION
    } else if base.starts_with("wasi:sockets/") {
        WASI_PREVIEW3_SOCKETS_VERSION
    } else if base.starts_with("wasi:io/") {
        WASI_PREVIEW3_IO_COMPAT_VERSION
    } else {
        unreachable!("unversioned Preview 3 interface base: {base}");
    };
    format!("{base}@{version}")
}

fn register_cli_environment(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_typed_async("get-environment", {
        let host = Rc::clone(&host);
        move |_, ()| {
            let host = Rc::clone(&host);
            Box::pin(async move { Ok(host.environment()) })
        }
    });
    instance.register_func_typed_async("get-arguments", {
        let host = Rc::clone(&host);
        move |_, ()| {
            let host = Rc::clone(&host);
            Box::pin(async move { Ok(host.arguments()) })
        }
    });
    instance.register_func_typed_async("initial-cwd", move |_, ()| {
        let host = Rc::clone(&host);
        Box::pin(async move { Ok(host.initial_cwd()) })
    });
    linker.register_import_instance(interface_name(CLI_ENVIRONMENT), instance);
}

fn register_cli_exit(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("exit", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let status = expect_exit_status("wasi:cli/exit.exit", args)?;
            host.exit(status)?;
            Ok(Vec::new())
        })
    });
    linker.register_import_instance(interface_name(CLI_EXIT), instance);
}

fn register_cli_stdio(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut stdin = ComponentLinkerInstance::new();
    stdin.register_func_async("read-via-stream", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                expect_no_args("wasi:cli/stdin.read-via-stream", args)?;
                let stream = host.input_stream_from_stdin().handle();
                let future = host.allocate_preview3_ready_future();
                Ok(vec![ComponentValue::Tuple(vec![
                    ComponentValue::Stream(stream),
                    ComponentValue::Future(future),
                ])])
            })
        }
    });
    stdin.register_func_async("get-stdin", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                expect_no_args("wasi:cli/stdin.get-stdin", args)?;
                Ok(vec![ComponentValue::Own(
                    host.input_stream_from_stdin().handle(),
                )])
            })
        }
    });
    linker.register_import_instance(interface_name(CLI_STDIN), stdin);

    let mut stdout = ComponentLinkerInstance::new();
    stdout.register_func_async("write-via-stream", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_stream_handle("wasi:cli/stdout.write-via-stream", args)?;
                host.preview3_write_stream_to_output(stream, OutputStreamKind::Stdout)?;
                Ok(vec![ComponentValue::Future(
                    host.allocate_preview3_ready_future(),
                )])
            })
        }
    });
    stdout.register_func_async("get-stdout", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                expect_no_args("wasi:cli/stdout.get-stdout", args)?;
                Ok(vec![ComponentValue::Own(
                    host.output_stream(OutputStreamKind::Stdout).handle(),
                )])
            })
        }
    });
    linker.register_import_instance(interface_name(CLI_STDOUT), stdout);

    let mut stderr = ComponentLinkerInstance::new();
    stderr.register_func_async("write-via-stream", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_stream_handle("wasi:cli/stderr.write-via-stream", args)?;
                host.preview3_write_stream_to_output(stream, OutputStreamKind::Stderr)?;
                Ok(vec![ComponentValue::Future(
                    host.allocate_preview3_ready_future(),
                )])
            })
        }
    });
    stderr.register_func_async("get-stderr", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            expect_no_args("wasi:cli/stderr.get-stderr", args)?;
            Ok(vec![ComponentValue::Own(
                host.output_stream(OutputStreamKind::Stderr).handle(),
            )])
        })
    });
    linker.register_import_instance(interface_name(CLI_STDERR), stderr);
}

impl WasiHost {
    fn allocate_preview3_ready_future(&self) -> u32 {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner
            .preview3_futures
            .insert(handle, Preview3FutureEntry::ReadyUnitResult);
        handle
    }

    fn preview3_write_stream_to_output(
        &self,
        stream: u32,
        kind: OutputStreamKind,
    ) -> Result<(), ComponentError> {
        let output = self.output_stream(kind);
        loop {
            let chunk =
                self.input_stream_read(WasiIoStreamsInputStreamBorrow::new(stream), 4096)?;
            match chunk {
                Ok(bytes) if bytes.is_empty() => return Ok(()),
                Ok(bytes) => {
                    match self.output_stream_write_bytes(
                        WasiIoStreamsOutputStreamBorrow::new(output.handle()),
                        &bytes,
                    )? {
                        Ok(()) => {}
                        Err(error) => {
                            return Err(ComponentError::Unsupported(format!(
                                "Preview 3 stdio write-via-stream failed before future completion: {error:?}"
                            )));
                        }
                    }
                }
                Err(error) => {
                    return Err(ComponentError::Unsupported(format!(
                        "Preview 3 stdio write-via-stream input stream failed before future completion: {error:?}"
                    )));
                }
            }
        }
    }
}

fn register_clocks(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut wall = ComponentLinkerInstance::new();
    wall.register_func_async("now", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                expect_no_args("wasi:clocks/wall-clock.now", args)?;
                Ok(vec![wall_clock_datetime(host.wall_clock_now())])
            })
        }
    });
    wall.register_func_async("resolution", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                expect_no_args("wasi:clocks/wall-clock.resolution", args)?;
                Ok(vec![wall_clock_datetime(host.wall_clock_resolution())])
            })
        }
    });
    linker.register_import_instance(interface_name(CLOCKS_WALL), wall);

    let mut monotonic = ComponentLinkerInstance::new();
    monotonic.register_func_typed_async("now", {
        let host = Rc::clone(&host);
        move |_, ()| {
            let host = Rc::clone(&host);
            Box::pin(async move { Ok(host.monotonic_now()) })
        }
    });
    monotonic.register_func_typed_async("resolution", {
        let host = Rc::clone(&host);
        move |_, ()| {
            let host = Rc::clone(&host);
            Box::pin(async move { Ok(host.monotonic_resolution()) })
        }
    });
    monotonic.register_func_typed_async("get-resolution", {
        let host = Rc::clone(&host);
        move |_, ()| {
            let host = Rc::clone(&host);
            Box::pin(async move { Ok(host.monotonic_resolution()) })
        }
    });
    monotonic.register_func_async("wait-until", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let when = expect_one_u64("wasi:clocks/monotonic-clock.wait-until", args)?;
                host.substrate()
                    .wait_monotonic_deadline(Duration::from_nanos(when))
                    .await?;
                Ok(Vec::new())
            })
        }
    });
    monotonic.register_func_async("wait-for", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let duration = expect_one_u64("wasi:clocks/monotonic-clock.wait-for", args)?;
                host.substrate()
                    .wait_monotonic_duration(Duration::from_nanos(duration))
                    .await?;
                Ok(Vec::new())
            })
        }
    });
    monotonic.register_func_async("subscribe-instant", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let when = expect_one_u64("wasi:clocks/monotonic-clock.subscribe-instant", args)?;
                let pollable = host.monotonic_subscribe_instant(when);
                Ok(vec![ComponentValue::Own(pollable.handle())])
            })
        }
    });
    monotonic.register_func_async("subscribe-duration", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let when = expect_one_u64("wasi:clocks/monotonic-clock.subscribe-duration", args)?;
                let pollable = host.monotonic_subscribe_duration(when);
                Ok(vec![ComponentValue::Own(pollable.handle())])
            })
        }
    });
    linker.register_import_instance(interface_name(CLOCKS_MONOTONIC), monotonic);
}

fn register_filesystem_preopens(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("get-directories", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            expect_no_args("wasi:filesystem/preopens.get-directories", args)?;
            Ok(vec![ComponentValue::List(
                host.preopen_directories()
                    .into_iter()
                    .map(|(descriptor, path)| {
                        ComponentValue::Tuple(vec![
                            ComponentValue::Own(descriptor.handle()),
                            ComponentValue::String(path),
                        ])
                    })
                    .collect(),
            )])
        })
    });
    linker.register_import_instance(interface_name(FILESYSTEM_PREOPENS), instance);
}

fn register_filesystem_types(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("[method]descriptor.get-type", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let descriptor =
                    expect_one_handle("wasi:filesystem/types.descriptor.get-type", args)?;
                let result =
                    host.descriptor_get_type(WasiFilesystemTypesDescriptorBorrow::new(descriptor))?;
                Ok(vec![filesystem_descriptor_type_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.get-flags", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let descriptor =
                    expect_one_handle("wasi:filesystem/types.descriptor.get-flags", args)?;
                let result = host
                    .descriptor_get_flags(WasiFilesystemTypesDescriptorBorrow::new(descriptor))?;
                Ok(vec![filesystem_descriptor_flags_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.open-at", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, path_flags, path, open_flags, descriptor_flags) =
                    expect_open_at_args("wasi:filesystem/types.descriptor.open-at", args)?;
                let result = host.descriptor_open_at(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    path_flags,
                    path,
                    open_flags,
                    descriptor_flags,
                )?;
                Ok(vec![filesystem_descriptor_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.read-via-stream", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, offset) =
                    expect_stream_len("wasi:filesystem/types.descriptor.read-via-stream", args)?;
                let result = host.descriptor_read_via_stream(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    offset,
                )?;
                Ok(vec![filesystem_input_stream_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.read", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, length, offset) =
                    expect_descriptor_read_args("wasi:filesystem/types.descriptor.read", args)?;
                let result = host.descriptor_read(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    length,
                    offset,
                )?;
                Ok(vec![filesystem_read_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.write", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, bytes, offset) =
                    expect_descriptor_write_args("wasi:filesystem/types.descriptor.write", args)?;
                let result = host.descriptor_write(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    bytes,
                    offset,
                )?;
                Ok(vec![filesystem_u64_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.set-size", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, size) =
                    expect_stream_len("wasi:filesystem/types.descriptor.set-size", args)?;
                let result = host.descriptor_set_size(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    size,
                )?;
                Ok(vec![filesystem_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.create-directory-at", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, path) = expect_descriptor_path_args(
                    "wasi:filesystem/types.descriptor.create-directory-at",
                    args,
                )?;
                let result = host.descriptor_create_directory_at(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    path,
                )?;
                Ok(vec![filesystem_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.remove-directory-at", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, path) = expect_descriptor_path_args(
                    "wasi:filesystem/types.descriptor.remove-directory-at",
                    args,
                )?;
                let result = host.descriptor_remove_directory_at(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    path,
                )?;
                Ok(vec![filesystem_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.unlink-file-at", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, path) = expect_descriptor_path_args(
                    "wasi:filesystem/types.descriptor.unlink-file-at",
                    args,
                )?;
                let result = host.descriptor_unlink_file_at(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    path,
                )?;
                Ok(vec![filesystem_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.rename-at", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (descriptor, old_path, new_descriptor, new_path) =
                    expect_rename_at_args("wasi:filesystem/types.descriptor.rename-at", args)?;
                let result = host.descriptor_rename_at(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                    old_path,
                    WasiFilesystemTypesDescriptorBorrow::new(new_descriptor),
                    new_path,
                )?;
                Ok(vec![filesystem_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]descriptor.read-directory", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let descriptor =
                    expect_one_handle("wasi:filesystem/types.descriptor.read-directory", args)?;
                let result = host.descriptor_read_directory(
                    WasiFilesystemTypesDescriptorBorrow::new(descriptor),
                )?;
                Ok(vec![filesystem_directory_stream_result(result)])
            })
        }
    });
    instance.register_func_async(
        "[method]directory-entry-stream.read-directory-entry",
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_handle(
                    "wasi:filesystem/types.directory-entry-stream.read-directory-entry",
                    args,
                )?;
                let result = host.directory_entry_stream_read_directory_entry(
                    WasiFilesystemTypesDirectoryEntryStreamBorrow::new(stream),
                )?;
                Ok(vec![filesystem_directory_entry_result(result)])
            })
        },
    );
    linker.register_import_instance(interface_name(FILESYSTEM_TYPES), instance);
}

fn register_io_poll(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("[method]pollable.ready", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let handle = expect_one_pollable_handle("wasi:io/poll.pollable.ready", args)?;
                Ok(vec![ComponentValue::Bool(host.pollable_ready(handle)?)])
            })
        }
    });
    instance.register_func_async("[method]pollable.block", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let handle = expect_one_pollable_handle("wasi:io/poll.pollable.block", args)?;
                host.substrate().pollable_block(handle).await?;
                Ok(Vec::new())
            })
        }
    });
    instance.register_func_async("poll", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let handles = expect_pollable_list("wasi:io/poll.poll", args)?;
            let ready = host
                .substrate()
                .poll(&handles)
                .await?
                .into_iter()
                .map(ComponentValue::U32)
                .collect();
            Ok(vec![ComponentValue::List(ready)])
        })
    });
    linker.register_import_instance(interface_name(IO_POLL), instance);
}

fn register_io_error(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("[method]error.to-debug-string", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let handle = expect_one_handle("wasi:io/error.error.to-debug-string", args)?;
            Ok(vec![ComponentValue::String(host.error_to_debug_string(
                WasiIoErrorErrorBorrow::new(handle),
            )?)])
        })
    });
    linker.register_import_instance(interface_name(IO_ERROR), instance);
}

fn register_io_streams(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("[method]input-stream.read", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) = expect_stream_len("wasi:io/streams.input-stream.read", args)?;
                let result =
                    host.input_stream_read(WasiIoStreamsInputStreamBorrow::new(stream), len)?;
                Ok(vec![stream_bytes_result(result)])
            })
        }
    });
    instance.register_func_async("[method]input-stream.blocking-read", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) =
                    expect_stream_len("wasi:io/streams.input-stream.blocking-read", args)?;
                let result = host
                    .input_stream_blocking_read(WasiIoStreamsInputStreamBorrow::new(stream), len)?;
                Ok(vec![stream_bytes_result(result)])
            })
        }
    });
    instance.register_func_async("[method]input-stream.skip", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) = expect_stream_len("wasi:io/streams.input-stream.skip", args)?;
                let result =
                    host.input_stream_skip(WasiIoStreamsInputStreamBorrow::new(stream), len)?;
                Ok(vec![stream_u64_result(result)])
            })
        }
    });
    instance.register_func_async("[method]input-stream.blocking-skip", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) =
                    expect_stream_len("wasi:io/streams.input-stream.blocking-skip", args)?;
                let result = host
                    .input_stream_blocking_skip(WasiIoStreamsInputStreamBorrow::new(stream), len)?;
                Ok(vec![stream_u64_result(result)])
            })
        }
    });
    instance.register_func_async("[method]input-stream.subscribe", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_handle("wasi:io/streams.input-stream.subscribe", args)?;
                let pollable =
                    host.input_stream_subscribe(WasiIoStreamsInputStreamBorrow::new(stream))?;
                Ok(vec![ComponentValue::Own(pollable.handle())])
            })
        }
    });
    register_output_stream_methods(&mut instance, host);
    linker.register_import_instance(interface_name(IO_STREAMS), instance);
}

fn register_output_stream_methods(instance: &mut ComponentLinkerInstance, host: Rc<WasiHost>) {
    instance.register_func_async("[method]output-stream.check-write", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_handle("wasi:io/streams.output-stream.check-write", args)?;
                let result =
                    host.output_stream_check_write(WasiIoStreamsOutputStreamBorrow::new(stream))?;
                Ok(vec![stream_u64_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.write", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, bytes) =
                    expect_stream_bytes("wasi:io/streams.output-stream.write", args)?;
                let result = host.output_stream_write_bytes(
                    WasiIoStreamsOutputStreamBorrow::new(stream),
                    &bytes,
                )?;
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.blocking-write-and-flush", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, bytes) = expect_stream_bytes(
                    "wasi:io/streams.output-stream.blocking-write-and-flush",
                    args,
                )?;
                let result = host.output_stream_write_bytes(
                    WasiIoStreamsOutputStreamBorrow::new(stream),
                    &bytes,
                )?;
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.flush", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_handle("wasi:io/streams.output-stream.flush", args)?;
                let result = host
                    .output_stream_check_write(WasiIoStreamsOutputStreamBorrow::new(stream))?
                    .map(|_| ());
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.blocking-flush", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream =
                    expect_one_handle("wasi:io/streams.output-stream.blocking-flush", args)?;
                let result = host
                    .output_stream_check_write(WasiIoStreamsOutputStreamBorrow::new(stream))?
                    .map(|_| ());
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.subscribe", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let stream = expect_one_handle("wasi:io/streams.output-stream.subscribe", args)?;
                let pollable =
                    host.output_stream_subscribe(WasiIoStreamsOutputStreamBorrow::new(stream))?;
                Ok(vec![ComponentValue::Own(pollable.handle())])
            })
        }
    });
    instance.register_func_async("[method]output-stream.write-zeroes", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) =
                    expect_stream_len("wasi:io/streams.output-stream.write-zeroes", args)?;
                let len = usize::try_from(len).map_err(|_| {
                    ComponentError::Trap(format!(
                        "output-stream.write-zeroes length {len} exceeds host addressable memory"
                    ))
                })?;
                let result = host.output_stream_write_bytes(
                    WasiIoStreamsOutputStreamBorrow::new(stream),
                    &vec![0; len],
                )?;
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.blocking-write-zeroes-and-flush", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, len) = expect_stream_len(
                    "wasi:io/streams.output-stream.blocking-write-zeroes-and-flush",
                    args,
                )?;
                let len = usize::try_from(len).map_err(|_| {
                    ComponentError::Trap(format!(
                        "output-stream.write-zeroes length {len} exceeds host addressable memory"
                    ))
                })?;
                let result = host.output_stream_write_bytes(
                    WasiIoStreamsOutputStreamBorrow::new(stream),
                    &vec![0; len],
                )?;
                Ok(vec![stream_unit_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.splice", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (stream, src, len) =
                    expect_splice_args("wasi:io/streams.output-stream.splice", args)?;
                let result = host.output_stream_splice(
                    WasiIoStreamsOutputStreamBorrow::new(stream),
                    WasiIoStreamsInputStreamBorrow::new(src),
                    len,
                )?;
                Ok(vec![stream_u64_result(result)])
            })
        }
    });
    instance.register_func_async("[method]output-stream.blocking-splice", move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let (stream, src, len) =
                expect_splice_args("wasi:io/streams.output-stream.blocking-splice", args)?;
            let result = host.output_stream_splice(
                WasiIoStreamsOutputStreamBorrow::new(stream),
                WasiIoStreamsInputStreamBorrow::new(src),
                len,
            )?;
            Ok(vec![stream_u64_result(result)])
        })
    });
}

fn register_random(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_typed_async("get-random-bytes", {
        let host = Rc::clone(&host);
        move |_, (len,): (u64,)| {
            let host = Rc::clone(&host);
            Box::pin(async move { host.secure_random_bytes(len) })
        }
    });
    instance.register_func_typed_async("get-random-u64", move |_, ()| {
        let host = Rc::clone(&host);
        Box::pin(async move { host.secure_random_u64() })
    });
    linker.register_import_instance(interface_name(RANDOM_RANDOM), instance);
}

fn register_insecure_random(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_typed_async("get-insecure-random-bytes", {
        let host = Rc::clone(&host);
        move |_, (len,): (u64,)| {
            let host = Rc::clone(&host);
            Box::pin(async move { host.insecure_random_bytes(len) })
        }
    });
    instance.register_func_typed_async("get-insecure-random-u64", move |_, ()| {
        let host = Rc::clone(&host);
        Box::pin(async move { Ok(host.next_insecure_random_u64()) })
    });
    linker.register_import_instance(interface_name(RANDOM_INSECURE), instance);
}

fn register_insecure_seed(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_typed_async("insecure-seed", move |_, ()| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            Ok((
                host.next_insecure_random_u64(),
                host.next_insecure_random_u64(),
            ))
        })
    });
    linker.register_import_instance(interface_name(RANDOM_INSECURE_SEED), instance);
}

fn register_sockets(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    register_socket_types_interface(linker, Rc::clone(&host));
    register_ip_name_lookup_interface(linker);
}

fn register_socket_types_interface(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("[static]tcp-socket.create", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let family =
                    expect_ip_address_family("wasi:sockets/types.tcp-socket.create", args)?;
                Ok(vec![ok_result(ComponentValue::Own(
                    host.allocate_preview3_tcp_socket(family),
                ))])
            })
        }
    });
    instance.register_func_async("[method]tcp-socket.get-is-listening", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let handle =
                    expect_first_handle("wasi:sockets/types.tcp-socket.get-is-listening", args)?;
                host.preview3_tcp_family(handle)?;
                Ok(vec![ComponentValue::Bool(false)])
            })
        }
    });
    instance.register_func_async("[method]tcp-socket.get-address-family", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let handle =
                    expect_first_handle("wasi:sockets/types.tcp-socket.get-address-family", args)?;
                Ok(vec![socket_address_family_value(
                    host.preview3_tcp_family(handle)?,
                )])
            })
        }
    });
    instance.register_func_async("[method]tcp-socket.set-listen-backlog-size", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (handle, _) = expect_handle_and_u64(
                    "wasi:sockets/types.tcp-socket.set-listen-backlog-size",
                    args,
                )?;
                host.preview3_tcp_family(handle)?;
                Ok(vec![socket_not_supported_result()])
            })
        }
    });
    register_tcp_result_method(&mut instance, Rc::clone(&host), "bind");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "connect");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "listen");
    register_tcp_unsupported_method(&mut instance, Rc::clone(&host), "send");
    register_tcp_unsupported_method(&mut instance, Rc::clone(&host), "receive");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-local-address");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-remote-address");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-keep-alive-enabled");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-keep-alive-enabled");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-keep-alive-idle-time");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-keep-alive-idle-time");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-keep-alive-interval");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-keep-alive-interval");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-keep-alive-count");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-keep-alive-count");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-hop-limit");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-hop-limit");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-receive-buffer-size");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-receive-buffer-size");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "get-send-buffer-size");
    register_tcp_result_method(&mut instance, Rc::clone(&host), "set-send-buffer-size");

    instance.register_func_async("[static]udp-socket.create", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let family =
                    expect_ip_address_family("wasi:sockets/types.udp-socket.create", args)?;
                Ok(vec![ok_result(ComponentValue::Own(
                    host.allocate_preview3_udp_socket(family),
                ))])
            })
        }
    });
    instance.register_func_async("[method]udp-socket.get-address-family", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let handle =
                    expect_first_handle("wasi:sockets/types.udp-socket.get-address-family", args)?;
                Ok(vec![socket_address_family_value(
                    host.preview3_udp_family(handle)?,
                )])
            })
        }
    });
    instance.register_func_async("[method]udp-socket.set-receive-buffer-size", {
        let host = Rc::clone(&host);
        move |_, args| {
            let host = Rc::clone(&host);
            Box::pin(async move {
                let (handle, _) = expect_handle_and_u64(
                    "wasi:sockets/types.udp-socket.set-receive-buffer-size",
                    args,
                )?;
                host.preview3_udp_family(handle)?;
                Ok(vec![socket_not_supported_result()])
            })
        }
    });
    register_udp_result_method(&mut instance, Rc::clone(&host), "bind");
    register_udp_result_method(&mut instance, Rc::clone(&host), "connect");
    register_udp_result_method(&mut instance, Rc::clone(&host), "disconnect");
    register_udp_result_method(&mut instance, Rc::clone(&host), "send");
    register_udp_result_method(&mut instance, Rc::clone(&host), "receive");
    register_udp_result_method(&mut instance, Rc::clone(&host), "get-local-address");
    register_udp_result_method(&mut instance, Rc::clone(&host), "get-remote-address");
    register_udp_result_method(&mut instance, Rc::clone(&host), "get-unicast-hop-limit");
    register_udp_result_method(&mut instance, Rc::clone(&host), "set-unicast-hop-limit");
    register_udp_result_method(&mut instance, Rc::clone(&host), "get-receive-buffer-size");
    register_udp_result_method(&mut instance, Rc::clone(&host), "get-send-buffer-size");
    register_udp_result_method(&mut instance, Rc::clone(&host), "set-send-buffer-size");
    linker.register_import_instance(interface_name(SOCKETS_TYPES), instance);
}

fn register_tcp_result_method(
    instance: &mut ComponentLinkerInstance,
    host: Rc<WasiHost>,
    method: &'static str,
) {
    let export = format!("[method]tcp-socket.{method}");
    instance.register_func_async(export, move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let handle =
                expect_first_handle(&format!("wasi:sockets/types.tcp-socket.{method}"), args)?;
            host.preview3_tcp_family(handle)?;
            Ok(vec![socket_not_supported_result()])
        })
    });
}

fn register_tcp_unsupported_method(
    instance: &mut ComponentLinkerInstance,
    host: Rc<WasiHost>,
    method: &'static str,
) {
    let export = format!("[method]tcp-socket.{method}");
    instance.register_func_async(export, move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let handle =
                expect_first_handle(&format!("wasi:sockets/types.tcp-socket.{method}"), args)?;
            host.preview3_tcp_family(handle)?;
            Err(ComponentError::Unsupported(format!(
                "Preview 3 TCP socket {method} requires connected I/O support"
            )))
        })
    });
}

fn register_udp_result_method(
    instance: &mut ComponentLinkerInstance,
    host: Rc<WasiHost>,
    method: &'static str,
) {
    let export = format!("[method]udp-socket.{method}");
    instance.register_func_async(export, move |_, args| {
        let host = Rc::clone(&host);
        Box::pin(async move {
            let handle =
                expect_first_handle(&format!("wasi:sockets/types.udp-socket.{method}"), args)?;
            host.preview3_udp_family(handle)?;
            Ok(vec![socket_not_supported_result()])
        })
    });
}

fn register_ip_name_lookup_interface(linker: &mut ComponentLinker) {
    let mut instance = ComponentLinkerInstance::new();
    instance.register_func_async("resolve-addresses", move |_, args| {
        Box::pin(async move {
            let name = expect_one_string("wasi:sockets/ip-name-lookup.resolve-addresses", args)?;
            let addresses = resolve_literal_ip_addresses(&name)?;
            Ok(vec![ok_result(ComponentValue::List(addresses))])
        })
    });
    linker.register_import_instance(interface_name(SOCKETS_IP_NAME_LOOKUP), instance);
}

impl WasiHost {
    fn allocate_preview3_tcp_socket(&self, family: Preview3SocketAddressFamily) -> u32 {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.preview3_tcp_sockets.insert(handle, family);
        handle
    }

    fn allocate_preview3_udp_socket(&self, family: Preview3SocketAddressFamily) -> u32 {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.preview3_udp_sockets.insert(handle, family);
        handle
    }

    fn preview3_tcp_family(
        &self,
        handle: u32,
    ) -> Result<Preview3SocketAddressFamily, ComponentError> {
        self.state
            .inner
            .borrow()
            .preview3_tcp_sockets
            .get(&handle)
            .copied()
            .ok_or_else(|| ComponentError::Trap(format!("TCP socket handle {handle} is invalid")))
    }

    fn preview3_udp_family(
        &self,
        handle: u32,
    ) -> Result<Preview3SocketAddressFamily, ComponentError> {
        self.state
            .inner
            .borrow()
            .preview3_udp_sockets
            .get(&handle)
            .copied()
            .ok_or_else(|| ComponentError::Trap(format!("UDP socket handle {handle} is invalid")))
    }
}

fn expect_no_args(
    context: &str,
    args: &[telomere_component::ComponentValue],
) -> Result<(), ComponentError> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(ComponentError::InvalidArgument(format!(
            "{context} expects 0 arguments, got {}",
            args.len()
        )))
    }
}

fn expect_one_u64(context: &str, args: &[ComponentValue]) -> Result<u64, ComponentError> {
    match args {
        [ComponentValue::U64(value)] => Ok(*value),
        [other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a u64 argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_one_string(context: &str, args: &[ComponentValue]) -> Result<String, ComponentError> {
    match args {
        [ComponentValue::String(value)] => Ok(value.clone()),
        [other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a string argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_exit_status(context: &str, args: &[ComponentValue]) -> Result<u8, ComponentError> {
    match args {
        [ComponentValue::Result {
            ok: Some(payload),
            err: None,
        }] if matches!(payload.as_ref(), ComponentValue::Tuple(values) if values.is_empty()) => {
            Ok(0)
        }
        [ComponentValue::Result {
            ok: None,
            err: Some(payload),
        }] if matches!(payload.as_ref(), ComponentValue::Tuple(values) if values.is_empty()) => {
            Ok(1)
        }
        [ComponentValue::Result { .. }] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects result with no payloads"
        ))),
        [other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a result status argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_ip_address_family(
    context: &str,
    args: &[ComponentValue],
) -> Result<Preview3SocketAddressFamily, ComponentError> {
    match args {
        [ComponentValue::Enum(family)] if family == "ipv4" => Ok(Preview3SocketAddressFamily::Ipv4),
        [ComponentValue::Enum(family)] if family == "ipv6" => Ok(Preview3SocketAddressFamily::Ipv6),
        [other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects an ip-address-family enum, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_one_pollable_handle(
    context: &str,
    args: &[ComponentValue],
) -> Result<u32, ComponentError> {
    match args {
        [value] => expect_handle(context, value),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_pollable_list(
    context: &str,
    args: &[ComponentValue],
) -> Result<Vec<u32>, ComponentError> {
    let [ComponentValue::List(values)] = args else {
        return Err(ComponentError::InvalidArgument(format!(
            "{context} expects a list<borrow<pollable>> argument"
        )));
    };
    values
        .iter()
        .map(|value| expect_handle(context, value))
        .collect()
}

fn expect_one_handle(context: &str, args: &[ComponentValue]) -> Result<u32, ComponentError> {
    match args {
        [value] => expect_handle(context, value),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_first_handle(context: &str, args: &[ComponentValue]) -> Result<u32, ComponentError> {
    args.first()
        .map(|value| expect_handle(context, value))
        .unwrap_or_else(|| {
            Err(ComponentError::InvalidArgument(format!(
                "{context} expects at least 1 argument, got 0"
            )))
        })
}

fn expect_handle_and_u64(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, u64), ComponentError> {
    match args {
        [handle, ComponentValue::U64(value)] => Ok((expect_handle(context, handle)?, *value)),
        [_, other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a u64 second argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn expect_one_stream_handle(context: &str, args: &[ComponentValue]) -> Result<u32, ComponentError> {
    match args {
        [ComponentValue::Stream(handle)] | [ComponentValue::U32(handle)] => Ok(*handle),
        [other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a stream handle, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 1 argument, got {}",
            args.len()
        ))),
    }
}

fn expect_handle(context: &str, value: &ComponentValue) -> Result<u32, ComponentError> {
    match value {
        ComponentValue::Borrow(handle)
        | ComponentValue::Own(handle)
        | ComponentValue::U32(handle) => Ok(*handle),
        other => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a resource handle, got {other:?}"
        ))),
    }
}

fn expect_stream_len(context: &str, args: &[ComponentValue]) -> Result<(u32, u64), ComponentError> {
    match args {
        [stream, ComponentValue::U64(len)] => Ok((expect_handle(context, stream)?, *len)),
        [_, other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a u64 length argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn expect_stream_bytes(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, Vec<u8>), ComponentError> {
    match args {
        [stream, ComponentValue::List(bytes)] => Ok((
            expect_handle(context, stream)?,
            bytes
                .iter()
                .map(|value| match value {
                    ComponentValue::U8(byte) => Ok(*byte),
                    other => Err(ComponentError::InvalidArgument(format!(
                        "{context} expects list<u8>, got element {other:?}"
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?,
        )),
        [_, other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a list<u8> argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn expect_splice_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, u32, u64), ComponentError> {
    match args {
        [stream, src, ComponentValue::U64(len)] => Ok((
            expect_handle(context, stream)?,
            expect_handle(context, src)?,
            *len,
        )),
        [_, _, other] => Err(ComponentError::InvalidArgument(format!(
            "{context} expects a u64 length argument, got {other:?}"
        ))),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects 3 arguments, got {}",
            args.len()
        ))),
    }
}

fn expect_open_at_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<
    (
        u32,
        WasiFilesystemTypesPathFlags,
        String,
        WasiFilesystemTypesOpenFlags,
        WasiFilesystemTypesDescriptorFlags,
    ),
    ComponentError,
> {
    match args {
        [descriptor, path_flags, ComponentValue::String(path), open_flags, descriptor_flags] => {
            Ok((
                expect_handle(context, descriptor)?,
                expect_path_flags(context, path_flags)?,
                path.clone(),
                expect_open_flags(context, open_flags)?,
                expect_descriptor_flags(context, descriptor_flags)?,
            ))
        }
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects descriptor, path-flags, string, open-flags, descriptor-flags"
        ))),
    }
}

fn expect_descriptor_read_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, u64, u64), ComponentError> {
    match args {
        [descriptor, ComponentValue::U64(length), ComponentValue::U64(offset)] => {
            Ok((expect_handle(context, descriptor)?, *length, *offset))
        }
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects descriptor, length, offset"
        ))),
    }
}

fn expect_descriptor_write_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, Vec<u8>, u64), ComponentError> {
    match args {
        [descriptor, ComponentValue::List(bytes), ComponentValue::U64(offset)] => Ok((
            expect_handle(context, descriptor)?,
            bytes
                .iter()
                .map(|value| match value {
                    ComponentValue::U8(byte) => Ok(*byte),
                    other => Err(ComponentError::InvalidArgument(format!(
                        "{context} expects list<u8>, got element {other:?}"
                    ))),
                })
                .collect::<Result<Vec<_>, _>>()?,
            *offset,
        )),
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects descriptor, list<u8>, offset"
        ))),
    }
}

fn expect_descriptor_path_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, String), ComponentError> {
    match args {
        [descriptor, ComponentValue::String(path)] => {
            Ok((expect_handle(context, descriptor)?, path.clone()))
        }
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects descriptor and path string"
        ))),
    }
}

fn expect_rename_at_args(
    context: &str,
    args: &[ComponentValue],
) -> Result<(u32, String, u32, String), ComponentError> {
    match args {
        [descriptor, ComponentValue::String(old_path), new_descriptor, ComponentValue::String(new_path)] => {
            Ok((
                expect_handle(context, descriptor)?,
                old_path.clone(),
                expect_handle(context, new_descriptor)?,
                new_path.clone(),
            ))
        }
        _ => Err(ComponentError::InvalidArgument(format!(
            "{context} expects descriptor, old-path, new-descriptor, new-path"
        ))),
    }
}

fn expect_path_flags(
    context: &str,
    value: &ComponentValue,
) -> Result<WasiFilesystemTypesPathFlags, ComponentError> {
    let flags = expect_flags(context, value)?;
    Ok(WasiFilesystemTypesPathFlags {
        symlink_follow: flags.iter().any(|flag| flag == "symlink-follow"),
    })
}

fn expect_open_flags(
    context: &str,
    value: &ComponentValue,
) -> Result<WasiFilesystemTypesOpenFlags, ComponentError> {
    let flags = expect_flags(context, value)?;
    Ok(WasiFilesystemTypesOpenFlags {
        create: flags.iter().any(|flag| flag == "create"),
        directory: flags.iter().any(|flag| flag == "directory"),
        exclusive: flags.iter().any(|flag| flag == "exclusive"),
        truncate: flags.iter().any(|flag| flag == "truncate"),
    })
}

fn expect_descriptor_flags(
    context: &str,
    value: &ComponentValue,
) -> Result<WasiFilesystemTypesDescriptorFlags, ComponentError> {
    let flags = expect_flags(context, value)?;
    Ok(WasiFilesystemTypesDescriptorFlags {
        read: flags.iter().any(|flag| flag == "read"),
        write: flags.iter().any(|flag| flag == "write"),
        file_integrity_sync: flags.iter().any(|flag| flag == "file-integrity-sync"),
        data_integrity_sync: flags.iter().any(|flag| flag == "data-integrity-sync"),
        requested_write_sync: flags.iter().any(|flag| flag == "requested-write-sync"),
        mutate_directory: flags.iter().any(|flag| flag == "mutate-directory"),
    })
}

fn expect_flags<'a>(
    context: &str,
    value: &'a ComponentValue,
) -> Result<&'a [String], ComponentError> {
    match value {
        ComponentValue::Flags(flags) => Ok(flags),
        other => Err(ComponentError::InvalidArgument(format!(
            "{context} expects flags, got {other:?}"
        ))),
    }
}

fn stream_bytes_result(result: Result<Vec<u8>, WasiIoStreamsStreamError>) -> ComponentValue {
    match result {
        Ok(bytes) => ok_result(ComponentValue::List(
            bytes.into_iter().map(ComponentValue::U8).collect(),
        )),
        Err(error) => err_result(stream_error_value(error)),
    }
}

fn stream_u64_result(result: Result<u64, WasiIoStreamsStreamError>) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::U64(value)),
        Err(error) => err_result(stream_error_value(error)),
    }
}

fn stream_unit_result(result: Result<(), WasiIoStreamsStreamError>) -> ComponentValue {
    match result {
        Ok(()) => ok_result(ComponentValue::Tuple(Vec::new())),
        Err(error) => err_result(stream_error_value(error)),
    }
}

fn filesystem_descriptor_type_result(
    result: Result<WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode>,
) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::Enum(descriptor_type_name(value).to_owned())),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_descriptor_flags_result(
    result: Result<WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesErrorCode>,
) -> ComponentValue {
    match result {
        Ok(value) => ok_result(descriptor_flags_value(value)),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_descriptor_result(
    result: Result<
        crate::bindings::types::WasiFilesystemTypesDescriptor,
        WasiFilesystemTypesErrorCode,
    >,
) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::Own(value.handle())),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_input_stream_result(
    result: Result<crate::bindings::types::WasiIoStreamsInputStream, WasiFilesystemTypesErrorCode>,
) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::Own(value.handle())),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_read_result(
    result: Result<(Vec<u8>, bool), WasiFilesystemTypesErrorCode>,
) -> ComponentValue {
    match result {
        Ok((bytes, end)) => ok_result(ComponentValue::Tuple(vec![
            ComponentValue::List(bytes.into_iter().map(ComponentValue::U8).collect()),
            ComponentValue::Bool(end),
        ])),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_u64_result(result: Result<u64, WasiFilesystemTypesErrorCode>) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::U64(value)),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_unit_result(result: Result<(), WasiFilesystemTypesErrorCode>) -> ComponentValue {
    match result {
        Ok(()) => ok_result(ComponentValue::Tuple(Vec::new())),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_directory_stream_result(
    result: Result<
        crate::bindings::types::WasiFilesystemTypesDirectoryEntryStream,
        WasiFilesystemTypesErrorCode,
    >,
) -> ComponentValue {
    match result {
        Ok(value) => ok_result(ComponentValue::Own(value.handle())),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn filesystem_directory_entry_result(
    result: Result<Option<WasiFilesystemTypesDirectoryEntry>, WasiFilesystemTypesErrorCode>,
) -> ComponentValue {
    match result {
        Ok(entry) => ok_result(ComponentValue::Option(entry.map(|entry| {
            Box::new(ComponentValue::Record(vec![
                (
                    "type".to_owned(),
                    ComponentValue::Enum(descriptor_type_name(entry.type_).to_owned()),
                ),
                ("name".to_owned(), ComponentValue::String(entry.name)),
            ]))
        }))),
        Err(error) => err_result(filesystem_error_code_value(error)),
    }
}

fn descriptor_flags_value(flags: WasiFilesystemTypesDescriptorFlags) -> ComponentValue {
    let mut value = Vec::new();
    if flags.read {
        value.push("read".to_owned());
    }
    if flags.write {
        value.push("write".to_owned());
    }
    if flags.file_integrity_sync {
        value.push("file-integrity-sync".to_owned());
    }
    if flags.data_integrity_sync {
        value.push("data-integrity-sync".to_owned());
    }
    if flags.requested_write_sync {
        value.push("requested-write-sync".to_owned());
    }
    if flags.mutate_directory {
        value.push("mutate-directory".to_owned());
    }
    ComponentValue::Flags(value)
}

fn descriptor_type_name(value: WasiFilesystemTypesDescriptorType) -> &'static str {
    match value {
        WasiFilesystemTypesDescriptorType::Unknown => "unknown",
        WasiFilesystemTypesDescriptorType::BlockDevice => "block-device",
        WasiFilesystemTypesDescriptorType::CharacterDevice => "character-device",
        WasiFilesystemTypesDescriptorType::Directory => "directory",
        WasiFilesystemTypesDescriptorType::Fifo => "fifo",
        WasiFilesystemTypesDescriptorType::SymbolicLink => "symbolic-link",
        WasiFilesystemTypesDescriptorType::RegularFile => "regular-file",
        WasiFilesystemTypesDescriptorType::Socket => "socket",
    }
}

fn filesystem_error_code_value(error: WasiFilesystemTypesErrorCode) -> ComponentValue {
    ComponentValue::Enum(filesystem_error_code_name(error).to_owned())
}

fn filesystem_error_code_name(error: WasiFilesystemTypesErrorCode) -> &'static str {
    match error {
        WasiFilesystemTypesErrorCode::Access => "access",
        WasiFilesystemTypesErrorCode::WouldBlock => "would-block",
        WasiFilesystemTypesErrorCode::Already => "already",
        WasiFilesystemTypesErrorCode::BadDescriptor => "bad-descriptor",
        WasiFilesystemTypesErrorCode::Busy => "busy",
        WasiFilesystemTypesErrorCode::Deadlock => "deadlock",
        WasiFilesystemTypesErrorCode::Quota => "quota",
        WasiFilesystemTypesErrorCode::Exist => "exist",
        WasiFilesystemTypesErrorCode::FileTooLarge => "file-too-large",
        WasiFilesystemTypesErrorCode::IllegalByteSequence => "illegal-byte-sequence",
        WasiFilesystemTypesErrorCode::InProgress => "in-progress",
        WasiFilesystemTypesErrorCode::Interrupted => "interrupted",
        WasiFilesystemTypesErrorCode::Invalid => "invalid",
        WasiFilesystemTypesErrorCode::Io => "io",
        WasiFilesystemTypesErrorCode::IsDirectory => "is-directory",
        WasiFilesystemTypesErrorCode::Loop => "loop",
        WasiFilesystemTypesErrorCode::TooManyLinks => "too-many-links",
        WasiFilesystemTypesErrorCode::MessageSize => "message-size",
        WasiFilesystemTypesErrorCode::NameTooLong => "name-too-long",
        WasiFilesystemTypesErrorCode::NoDevice => "no-device",
        WasiFilesystemTypesErrorCode::NoEntry => "no-entry",
        WasiFilesystemTypesErrorCode::NoLock => "no-lock",
        WasiFilesystemTypesErrorCode::InsufficientMemory => "insufficient-memory",
        WasiFilesystemTypesErrorCode::InsufficientSpace => "insufficient-space",
        WasiFilesystemTypesErrorCode::NotDirectory => "not-directory",
        WasiFilesystemTypesErrorCode::NotEmpty => "not-empty",
        WasiFilesystemTypesErrorCode::NotRecoverable => "not-recoverable",
        WasiFilesystemTypesErrorCode::Unsupported => "unsupported",
        WasiFilesystemTypesErrorCode::NoTty => "no-tty",
        WasiFilesystemTypesErrorCode::NoSuchDevice => "no-such-device",
        WasiFilesystemTypesErrorCode::Overflow => "overflow",
        WasiFilesystemTypesErrorCode::NotPermitted => "not-permitted",
        WasiFilesystemTypesErrorCode::Pipe => "pipe",
        WasiFilesystemTypesErrorCode::ReadOnly => "read-only",
        WasiFilesystemTypesErrorCode::InvalidSeek => "invalid-seek",
        WasiFilesystemTypesErrorCode::TextFileBusy => "text-file-busy",
        WasiFilesystemTypesErrorCode::CrossDevice => "cross-device",
    }
}

fn ok_result(value: ComponentValue) -> ComponentValue {
    ComponentValue::Result {
        ok: Some(Box::new(value)),
        err: None,
    }
}

fn err_result(value: ComponentValue) -> ComponentValue {
    ComponentValue::Result {
        ok: None,
        err: Some(Box::new(value)),
    }
}

fn socket_not_supported_result() -> ComponentValue {
    err_result(ComponentValue::Enum("not-supported".to_owned()))
}

fn socket_address_family_value(family: Preview3SocketAddressFamily) -> ComponentValue {
    let family = match family {
        Preview3SocketAddressFamily::Ipv4 => "ipv4",
        Preview3SocketAddressFamily::Ipv6 => "ipv6",
    };
    ComponentValue::Enum(family.to_owned())
}

fn resolve_literal_ip_addresses(name: &str) -> Result<Vec<ComponentValue>, ComponentError> {
    let address = name.parse::<IpAddr>().map_err(|_| {
        ComponentError::Unsupported(
            "Preview 3 ip-name-lookup only resolves literal IP addresses without DNS".to_owned(),
        )
    })?;
    let value = match address {
        IpAddr::V4(address) => ComponentValue::Variant {
            case: "ipv4".to_owned(),
            value: Some(Box::new(ComponentValue::Tuple(
                address
                    .octets()
                    .into_iter()
                    .map(ComponentValue::U8)
                    .collect(),
            ))),
        },
        IpAddr::V6(address) => ComponentValue::Variant {
            case: "ipv6".to_owned(),
            value: Some(Box::new(ComponentValue::Tuple(
                address
                    .segments()
                    .into_iter()
                    .map(ComponentValue::U16)
                    .collect(),
            ))),
        },
    };
    Ok(vec![value])
}

fn stream_error_value(error: WasiIoStreamsStreamError) -> ComponentValue {
    match error {
        WasiIoStreamsStreamError::LastOperationFailed(error) => ComponentValue::Variant {
            case: "last-operation-failed".to_owned(),
            value: Some(Box::new(ComponentValue::Own(error.handle()))),
        },
        WasiIoStreamsStreamError::Closed => ComponentValue::Variant {
            case: "closed".to_owned(),
            value: None,
        },
    }
}

fn wall_clock_datetime(
    datetime: crate::bindings::types::WasiClocksWallClockDatetime,
) -> telomere_component::ComponentValue {
    telomere_component::ComponentValue::Record(vec![
        (
            "seconds".to_owned(),
            telomere_component::ComponentValue::U64(datetime.seconds),
        ),
        (
            "nanoseconds".to_owned(),
            telomere_component::ComponentValue::U32(datetime.nanoseconds),
        ),
    ])
}
