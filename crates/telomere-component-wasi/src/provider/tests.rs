use super::WasiHost;
use crate::bindings::imports::{
    wasi_cli_exit as cli_exit, wasi_cli_stdin as cli_stdin, wasi_cli_stdout as cli_stdout,
    wasi_filesystem_preopens as filesystem_preopens, wasi_filesystem_types as filesystem_types,
    wasi_io_error as io_error, wasi_io_poll as io_poll, wasi_io_streams as io_streams,
};
use crate::bindings::types::{
    WasiFilesystemTypesDescriptorBorrow, WasiFilesystemTypesDescriptorFlags,
    WasiFilesystemTypesDescriptorType, WasiFilesystemTypesErrorCode, WasiFilesystemTypesOpenFlags,
    WasiFilesystemTypesPathFlags, WasiIoErrorErrorBorrow, WasiIoPollPollableBorrow,
    WasiIoStreamsInputStreamBorrow, WasiIoStreamsOutputStreamBorrow, WasiIoStreamsStreamError,
};
use crate::state::{InputStreamSource, PollableEntry, WasiState};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use telomere_component::ComponentError;

fn read_only_flags() -> WasiFilesystemTypesDescriptorFlags {
    WasiFilesystemTypesDescriptorFlags {
        read: true,
        write: false,
        file_integrity_sync: false,
        data_integrity_sync: false,
        requested_write_sync: false,
        mutate_directory: false,
    }
}

fn empty_open_flags() -> WasiFilesystemTypesOpenFlags {
    WasiFilesystemTypesOpenFlags {
        create: false,
        directory: false,
        exclusive: false,
        truncate: false,
    }
}

fn empty_path_flags() -> WasiFilesystemTypesPathFlags {
    WasiFilesystemTypesPathFlags {
        symlink_follow: false,
    }
}

fn temp_sandbox() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "telomere-component-wasi-test-{}-{nanos}",
        std::process::id()
    ));
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
}

#[test]
fn provider_stdio_and_exit_are_recorded() {
    let state = WasiState::builder().stdin(b"abc".to_vec()).build();
    let host = WasiHost::new(state.clone());
    let store = telomere::Store::new();

    let stdin = <WasiHost as cli_stdin::Host>::get_stdin(&host, &store).unwrap();
    let stdin_borrow = WasiIoStreamsInputStreamBorrow::new(stdin.handle());
    let chunk = <WasiHost as io_streams::Host>::input_stream_read(&host, &store, stdin_borrow, 2)
        .unwrap()
        .unwrap();
    assert_eq!(chunk, b"ab");

    let stdout = <WasiHost as cli_stdout::Host>::get_stdout(&host, &store).unwrap();
    let stdout_borrow = WasiIoStreamsOutputStreamBorrow::new(stdout.handle());
    <WasiHost as io_streams::Host>::output_stream_write(
        &host,
        &store,
        stdout_borrow,
        b"hello".to_vec(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(state.stdout(), b"hello");

    let exit = <WasiHost as cli_exit::Host>::exit(&host, &store, Err(()));
    assert!(matches!(exit, Err(ComponentError::Trap(_))));
    assert_eq!(state.exit_code(), Some(1));
}

#[test]
fn provider_write_zeroes_honors_requested_length() {
    let state = WasiState::builder().build();
    let host = WasiHost::new(state.clone());
    let store = telomere::Store::new();

    let stdout = <WasiHost as cli_stdout::Host>::get_stdout(&host, &store).unwrap();
    let stdout_borrow = WasiIoStreamsOutputStreamBorrow::new(stdout.handle());

    <WasiHost as io_streams::Host>::output_stream_write_zeroes(
        &host,
        &store,
        stdout_borrow,
        (1 << 20) as u64 + 33,
    )
    .unwrap()
    .unwrap();

    assert_eq!(state.stdout().len(), (1 << 20) + 33);
}

#[test]
fn provider_inherit_stdin_uses_live_host_source() {
    let state = WasiState::builder().inherit_stdin().build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let stdin = <WasiHost as cli_stdin::Host>::get_stdin(&host, &store).unwrap();
    let inner = host.state.inner.borrow();
    let entry = inner
        .input_streams
        .get(&stdin.handle())
        .expect("stdin handle should be registered");
    assert!(matches!(entry.source, InputStreamSource::HostStdin));
}

#[test]
fn provider_buffered_input_stream_subscribe_is_ready() {
    let state = WasiState::builder().stdin(b"abc".to_vec()).build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let stdin = <WasiHost as cli_stdin::Host>::get_stdin(&host, &store).unwrap();
    let pollable = <WasiHost as io_streams::Host>::input_stream_subscribe(
        &host,
        &store,
        WasiIoStreamsInputStreamBorrow::new(stdin.handle()),
    )
    .unwrap();

    let ready = <WasiHost as io_poll::Host>::pollable_ready(
        &host,
        &store,
        WasiIoPollPollableBorrow::new(pollable.handle()),
    )
    .unwrap();
    assert!(ready, "buffered stdin should be immediately readable");
}

#[test]
fn provider_host_stdin_subscribe_registers_input_stream_pollable() {
    let state = WasiState::builder().inherit_stdin().build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let stdin = <WasiHost as cli_stdin::Host>::get_stdin(&host, &store).unwrap();
    let pollable = <WasiHost as io_streams::Host>::input_stream_subscribe(
        &host,
        &store,
        WasiIoStreamsInputStreamBorrow::new(stdin.handle()),
    )
    .unwrap();

    let inner = host.state.inner.borrow();
    assert!(matches!(
        inner.pollables.get(&pollable.handle()),
        Some(PollableEntry::InputStream(handle)) if *handle == stdin.handle()
    ));
}

#[test]
fn provider_preopen_open_and_read_work() {
    let dir = temp_sandbox();
    let file = dir.join("hello.txt");
    fs::write(&file, b"hello wasi").expect("fixture file should be written");

    let state = WasiState::builder().preopen_dir(&dir, "sandbox").build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let preopens = <WasiHost as filesystem_preopens::Host>::get_directories(&host, &store).unwrap();
    assert_eq!(preopens.len(), 1);
    assert_eq!(preopens[0].1, "sandbox");

    let root = WasiFilesystemTypesDescriptorBorrow::new(preopens[0].0.handle());
    let descriptor = <WasiHost as filesystem_types::Host>::descriptor_open_at(
        &host,
        &store,
        root,
        empty_path_flags(),
        "hello.txt".to_owned(),
        empty_open_flags(),
        read_only_flags(),
    )
    .unwrap()
    .unwrap();
    let descriptor_borrow = WasiFilesystemTypesDescriptorBorrow::new(descriptor.handle());

    let stat =
        <WasiHost as filesystem_types::Host>::descriptor_stat(&host, &store, descriptor_borrow)
            .unwrap()
            .unwrap();
    assert_eq!(stat.size, 10);
    assert_eq!(stat.type_, WasiFilesystemTypesDescriptorType::RegularFile);

    let read = <WasiHost as filesystem_types::Host>::descriptor_read(
        &host,
        &store,
        descriptor_borrow,
        5,
        0,
    )
    .unwrap()
    .unwrap();
    assert_eq!(read, (b"hello".to_vec(), false));

    let stream = <WasiHost as filesystem_types::Host>::descriptor_read_via_stream(
        &host,
        &store,
        descriptor_borrow,
        6,
    )
    .unwrap()
    .unwrap();
    let stream_borrow = WasiIoStreamsInputStreamBorrow::new(stream.handle());
    let remainder =
        <WasiHost as io_streams::Host>::input_stream_read(&host, &store, stream_borrow, 8)
            .unwrap()
            .unwrap();
    assert_eq!(remainder, b"wasi");

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn provider_stream_read_failure_returns_last_operation_failed() {
    let dir = temp_sandbox();
    let file = dir.join("hello.txt");
    fs::write(&file, b"hello wasi").expect("fixture file should be written");

    let state = WasiState::builder().preopen_dir(&dir, "sandbox").build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let preopens = <WasiHost as filesystem_preopens::Host>::get_directories(&host, &store).unwrap();
    let root = WasiFilesystemTypesDescriptorBorrow::new(preopens[0].0.handle());
    let descriptor = <WasiHost as filesystem_types::Host>::descriptor_open_at(
        &host,
        &store,
        root,
        empty_path_flags(),
        "hello.txt".to_owned(),
        empty_open_flags(),
        read_only_flags(),
    )
    .unwrap()
    .unwrap();
    let descriptor_borrow = WasiFilesystemTypesDescriptorBorrow::new(descriptor.handle());
    let stream = <WasiHost as filesystem_types::Host>::descriptor_read_via_stream(
        &host,
        &store,
        descriptor_borrow,
        0,
    )
    .unwrap()
    .unwrap();
    fs::remove_file(&file).expect("fixture file should be removed before read");

    let error = <WasiHost as io_streams::Host>::input_stream_read(
        &host,
        &store,
        WasiIoStreamsInputStreamBorrow::new(stream.handle()),
        8,
    )
    .unwrap()
    .expect_err("deleted file should surface a stream error");
    let handle = match error {
        WasiIoStreamsStreamError::LastOperationFailed(error) => error.handle(),
        other => panic!("unexpected stream error: {other:?}"),
    };

    let debug = <WasiHost as io_error::Host>::error_to_debug_string(
        &host,
        &store,
        WasiIoErrorErrorBorrow::new(handle),
    )
    .unwrap();
    assert!(debug.contains("failed to read"));

    let code = <WasiHost as filesystem_types::Host>::filesystem_error_code(
        &host,
        &store,
        WasiIoErrorErrorBorrow::new(handle),
    )
    .unwrap();
    assert_eq!(code, Some(WasiFilesystemTypesErrorCode::NoEntry));

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[test]
fn provider_rejects_reads_when_descriptor_lacks_read_rights() {
    let dir = temp_sandbox();
    let file = dir.join("hello.txt");
    fs::write(&file, b"hello wasi").expect("fixture file should be written");

    let state = WasiState::builder().preopen_dir(&dir, "sandbox").build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let preopens = <WasiHost as filesystem_preopens::Host>::get_directories(&host, &store).unwrap();
    let root = WasiFilesystemTypesDescriptorBorrow::new(preopens[0].0.handle());
    let mut no_read_flags = read_only_flags();
    no_read_flags.read = false;
    let descriptor = <WasiHost as filesystem_types::Host>::descriptor_open_at(
        &host,
        &store,
        root,
        empty_path_flags(),
        "hello.txt".to_owned(),
        empty_open_flags(),
        no_read_flags,
    )
    .unwrap()
    .unwrap();
    let descriptor_borrow = WasiFilesystemTypesDescriptorBorrow::new(descriptor.handle());

    let read = <WasiHost as filesystem_types::Host>::descriptor_read(
        &host,
        &store,
        descriptor_borrow,
        5,
        0,
    )
    .unwrap();
    assert_eq!(read, Err(WasiFilesystemTypesErrorCode::NotPermitted));

    let stream = <WasiHost as filesystem_types::Host>::descriptor_read_via_stream(
        &host,
        &store,
        descriptor_borrow,
        0,
    )
    .unwrap();
    assert_eq!(stream, Err(WasiFilesystemTypesErrorCode::NotPermitted));

    fs::remove_dir_all(dir).expect("temp dir should be removed");
}

#[cfg(unix)]
#[test]
fn provider_rejects_symlink_escape_when_not_following() {
    use std::os::unix::fs::symlink;

    let dir = temp_sandbox();
    let outside = temp_sandbox();
    fs::write(outside.join("secret.txt"), b"outside").expect("fixture file should be written");
    symlink(&outside, dir.join("escape")).expect("symlink should be created");

    let state = WasiState::builder().preopen_dir(&dir, "sandbox").build();
    let host = WasiHost::new(state);
    let store = telomere::Store::new();

    let preopens = <WasiHost as filesystem_preopens::Host>::get_directories(&host, &store).unwrap();
    let root = WasiFilesystemTypesDescriptorBorrow::new(preopens[0].0.handle());
    let result = <WasiHost as filesystem_types::Host>::descriptor_open_at(
        &host,
        &store,
        root,
        empty_path_flags(),
        "escape/secret.txt".to_owned(),
        empty_open_flags(),
        read_only_flags(),
    )
    .unwrap();
    assert_eq!(
        result,
        Err(crate::bindings::types::WasiFilesystemTypesErrorCode::NotPermitted)
    );

    fs::remove_dir_all(dir).expect("temp dir should be removed");
    fs::remove_dir_all(outside).expect("temp dir should be removed");
}
