use super::common::next_handle;
use super::filesystem::map_io_error;
use super::WasiHost;
use crate::bindings::imports::{
    wasi_io_error as io_error, wasi_io_poll as io_poll, wasi_io_streams as io_streams,
};
use crate::bindings::types::{
    WasiFilesystemTypesErrorCode, WasiIoErrorError, WasiIoErrorErrorBorrow, WasiIoPollPollable,
    WasiIoPollPollableBorrow, WasiIoStreamsInputStreamBorrow, WasiIoStreamsOutputStreamBorrow,
    WasiIoStreamsStreamError,
};
use crate::state::{ErrorEntry, InputStreamSource, OutputStreamKind, PollableEntry};
use std::fs;
use std::io::{ErrorKind, Write};
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

#[cfg(unix)]
const HOST_STDIN_FD: libc::c_int = libc::STDIN_FILENO;
#[cfg(not(unix))]
const HOST_STDIN_FD: i32 = 0;

#[cfg(unix)]
fn poll_fd(fd: libc::c_int, timeout_ms: libc::c_int) -> Result<bool, ComponentError> {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
        revents: 0,
    };
    loop {
        let ready = unsafe { libc::poll(&mut descriptor, 1, timeout_ms) };
        if ready >= 0 {
            return Ok(ready > 0);
        }
        let error = std::io::Error::last_os_error();
        if error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(ComponentError::Trap(format!(
            "failed to poll fd {fd}: {error}"
        )));
    }
}

#[cfg(not(unix))]
fn poll_fd(_fd: i32, _timeout_ms: i32) -> Result<bool, ComponentError> {
    Ok(true)
}

#[cfg(unix)]
fn read_fd(fd: libc::c_int, len: usize, blocking: bool) -> Result<Vec<u8>, std::io::Error> {
    if len == 0 {
        return Ok(Vec::new());
    }

    if !blocking
        && unsafe {
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                revents: 0,
            };
            libc::poll(&mut descriptor, 1, 0)
        } == 0
    {
        return Ok(Vec::new());
    }

    if blocking {
        loop {
            let mut descriptor = libc::pollfd {
                fd,
                events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
                revents: 0,
            };
            let ready = unsafe { libc::poll(&mut descriptor, 1, -1) };
            if ready >= 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    let mut chunk = vec![0; len];
    loop {
        let bytes_read = unsafe { libc::read(fd, chunk.as_mut_ptr().cast::<libc::c_void>(), len) };
        if bytes_read >= 0 {
            chunk.truncate(bytes_read as usize);
            return Ok(chunk);
        }

        let error = std::io::Error::last_os_error();
        match error.kind() {
            ErrorKind::Interrupted => continue,
            ErrorKind::WouldBlock if !blocking => return Ok(Vec::new()),
            _ => return Err(error),
        }
    }
}

#[cfg(not(unix))]
fn read_fd(_fd: i32, len: usize, _blocking: bool) -> Result<Vec<u8>, std::io::Error> {
    let mut chunk = vec![0; len];
    let stdin = std::io::stdin();
    let mut handle = stdin.lock();
    let bytes_read = std::io::Read::read(&mut handle, &mut chunk)?;
    chunk.truncate(bytes_read);
    Ok(chunk)
}

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    io_error::add_to_linker(linker, Rc::clone(&host));
    io_poll::add_to_linker(linker, Rc::clone(&host));
    io_streams::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    io_error::add_to_linker_async(linker, Rc::clone(&host));
    io_poll::add_to_linker_async(linker, Rc::clone(&host));
    io_streams::add_to_linker_async(linker, host);
}

impl WasiHost {
    fn requested_len(len: u64, context: &str) -> Result<usize, ComponentError> {
        usize::try_from(len).map_err(|_| {
            ComponentError::Trap(format!(
                "{context} length {len} exceeds host addressable memory"
            ))
        })
    }

    pub(super) fn allocate_pollable(&self, pollable: PollableEntry) -> WasiIoPollPollable {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.pollables.insert(handle, pollable);
        WasiIoPollPollable::new(handle)
    }

    fn pollable_ready(&self, handle: u32) -> Result<bool, ComponentError> {
        let entry = self
            .state
            .inner
            .borrow()
            .pollables
            .get(&handle)
            .cloned()
            .ok_or_else(|| ComponentError::Trap(format!("unknown pollable handle {handle}")))?;
        Ok(match entry {
            PollableEntry::Ready => true,
            PollableEntry::InputStream(stream) => self.input_stream_ready(stream)?,
            PollableEntry::MonotonicDeadline(deadline) => {
                (self.state.inner.borrow().monotonic_clock)() >= deadline
            }
        })
    }

    fn pollable_block(&self, handle: u32) -> Result<(), ComponentError> {
        loop {
            if self.pollable_ready(handle)? {
                return Ok(());
            }
            match self.state.inner.borrow().pollables.get(&handle).cloned() {
                Some(PollableEntry::InputStream(stream)) => {
                    self.block_input_stream(stream)?;
                    return Ok(());
                }
                Some(PollableEntry::MonotonicDeadline(deadline)) => {
                    let now = (self.state.inner.borrow().monotonic_clock)();
                    let sleep = deadline.saturating_sub(now).min(Duration::from_millis(5));
                    if !sleep.is_zero() {
                        thread::sleep(sleep);
                    }
                }
                Some(PollableEntry::Ready) => {}
                None => {
                    return Err(ComponentError::Trap(format!(
                        "unknown pollable handle {handle}"
                    )));
                }
            }
        }
    }

    fn poll(&self, pollables: Vec<WasiIoPollPollableBorrow>) -> Result<Vec<u32>, ComponentError> {
        if pollables.is_empty() {
            return Err(ComponentError::Trap(
                "wasi:io/poll.poll requires at least one pollable".to_owned(),
            ));
        }

        loop {
            let mut ready = Vec::new();
            for (index, pollable) in pollables.iter().enumerate() {
                if self.pollable_ready(pollable.handle())? {
                    ready.push(index as u32);
                }
            }
            if !ready.is_empty() {
                return Ok(ready);
            }
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn allocate_error(
        &self,
        debug_message: impl Into<String>,
        filesystem_code: Option<WasiFilesystemTypesErrorCode>,
    ) -> WasiIoErrorError {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.errors.insert(
            handle,
            ErrorEntry {
                debug_message: debug_message.into(),
                filesystem_code,
            },
        );
        WasiIoErrorError::new(handle)
    }

    fn stream_error(
        &self,
        debug_message: impl Into<String>,
        filesystem_code: Option<WasiFilesystemTypesErrorCode>,
    ) -> WasiIoStreamsStreamError {
        WasiIoStreamsStreamError::LastOperationFailed(
            self.allocate_error(debug_message, filesystem_code),
        )
    }

    fn input_stream_state(&self, handle: u32) -> Result<(InputStreamSource, bool), ComponentError> {
        let inner = self.state.inner.borrow();
        let entry = inner
            .input_streams
            .get(&handle)
            .ok_or_else(|| ComponentError::Trap(format!("unknown input-stream handle {handle}")))?;
        Ok((entry.source.clone(), entry.closed))
    }

    fn input_stream_ready(&self, handle: u32) -> Result<bool, ComponentError> {
        let (source, closed) = self.input_stream_state(handle)?;
        if closed {
            return Ok(true);
        }
        match source {
            InputStreamSource::Buffer(_) | InputStreamSource::File(_) => Ok(true),
            InputStreamSource::HostStdin => poll_fd(HOST_STDIN_FD, 0),
        }
    }

    fn block_input_stream(&self, handle: u32) -> Result<(), ComponentError> {
        let (source, closed) = self.input_stream_state(handle)?;
        if closed {
            return Ok(());
        }
        match source {
            InputStreamSource::Buffer(_) | InputStreamSource::File(_) => Ok(()),
            InputStreamSource::HostStdin => {
                poll_fd(HOST_STDIN_FD, -1)?;
                Ok(())
            }
        }
    }

    fn input_stream_read(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_read_impl(stream, len, false)
    }

    fn input_stream_read_impl(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
        blocking: bool,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        let (source, position, closed) = {
            let inner = self.state.inner.borrow();
            let entry = inner.input_streams.get(&stream.handle()).ok_or_else(|| {
                ComponentError::Trap(format!("unknown input-stream handle {}", stream.handle()))
            })?;
            (entry.source.clone(), entry.position, entry.closed)
        };

        if closed {
            return Ok(Err(WasiIoStreamsStreamError::Closed));
        }

        let read_len = Self::requested_len(len, "input-stream.read")?;
        let (chunk, next_position) = match source {
            InputStreamSource::Buffer(bytes) => {
                let start = usize::try_from(position)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let end = start.saturating_add(read_len).min(bytes.len());
                (bytes[start..end].to_vec(), end as u64)
            }
            InputStreamSource::File(path) => {
                let bytes = match fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Ok(Err(self.stream_error(
                            format!("failed to read `{}`: {error}", path.display()),
                            Some(map_io_error(&error)),
                        )));
                    }
                };
                let start = usize::try_from(position)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let end = start.saturating_add(read_len).min(bytes.len());
                (bytes[start..end].to_vec(), end as u64)
            }
            InputStreamSource::HostStdin => {
                let chunk = match read_fd(HOST_STDIN_FD, read_len, blocking) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        return Ok(Err(
                            self.stream_error(format!("failed to read stdin: {error}"), None)
                        ));
                    }
                };
                let next_position = position.saturating_add(chunk.len() as u64);
                (chunk, next_position)
            }
        };

        if let Some(entry) = self
            .state
            .inner
            .borrow_mut()
            .input_streams
            .get_mut(&stream.handle())
        {
            entry.position = next_position;
        }

        Ok(Ok(chunk))
    }

    fn input_stream_skip(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        let read = self.input_stream_read_impl(stream, len, false)?;
        Ok(read.map(|bytes| bytes.len() as u64))
    }

    fn input_stream_blocking_read(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_read_impl(stream, len, true)
    }

    fn input_stream_blocking_skip(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        let read = self.input_stream_read_impl(stream, len, true)?;
        Ok(read.map(|bytes| bytes.len() as u64))
    }

    fn input_stream_subscribe(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        let inner = self.state.inner.borrow();
        if !inner.input_streams.contains_key(&stream.handle()) {
            return Err(ComponentError::Trap(format!(
                "unknown input-stream handle {}",
                stream.handle()
            )));
        }
        drop(inner);
        Ok(self.allocate_pollable(PollableEntry::InputStream(stream.handle())))
    }

    fn output_stream_check_write(
        &self,
        stream: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        let entry = self
            .state
            .inner
            .borrow()
            .output_streams
            .get(&stream.handle())
            .map(|entry| entry.closed)
            .ok_or_else(|| {
                ComponentError::Trap(format!("unknown output-stream handle {}", stream.handle()))
            })?;
        if entry {
            return Ok(Err(WasiIoStreamsStreamError::Closed));
        }
        Ok(Ok(4096))
    }

    fn output_stream_write_bytes(
        &self,
        stream: WasiIoStreamsOutputStreamBorrow,
        bytes: &[u8],
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        let (kind, closed, inherit_stdio) = {
            let inner = self.state.inner.borrow();
            let entry = inner.output_streams.get(&stream.handle()).ok_or_else(|| {
                ComponentError::Trap(format!("unknown output-stream handle {}", stream.handle()))
            })?;
            (entry.kind, entry.closed, inner.inherit_stdio)
        };

        if closed {
            return Ok(Err(WasiIoStreamsStreamError::Closed));
        }

        {
            let mut inner = self.state.inner.borrow_mut();
            match kind {
                OutputStreamKind::Stdout => inner.stdout.extend_from_slice(bytes),
                OutputStreamKind::Stderr => inner.stderr.extend_from_slice(bytes),
            }
        }

        if inherit_stdio {
            let result = match kind {
                OutputStreamKind::Stdout => {
                    let mut stdout = std::io::stdout().lock();
                    stdout.write_all(bytes).and_then(|_| stdout.flush())
                }
                OutputStreamKind::Stderr => {
                    let mut stderr = std::io::stderr().lock();
                    stderr.write_all(bytes).and_then(|_| stderr.flush())
                }
            };
            if let Err(error) = result {
                return Ok(Err(
                    self.stream_error(error.to_string(), Some(map_io_error(&error)))
                ));
            }
        }

        Ok(Ok(()))
    }

    fn output_stream_subscribe(
        &self,
        stream: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        let inner = self.state.inner.borrow();
        if !inner.output_streams.contains_key(&stream.handle()) {
            return Err(ComponentError::Trap(format!(
                "unknown output-stream handle {}",
                stream.handle()
            )));
        }
        drop(inner);
        Ok(self.allocate_pollable(PollableEntry::Ready))
    }

    fn output_stream_splice(
        &self,
        stream: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        let bytes = match self.input_stream_read(src, len)? {
            Ok(bytes) => bytes,
            Err(error) => return Ok(Err(error)),
        };
        match self.output_stream_write_bytes(stream, &bytes)? {
            Ok(()) => Ok(Ok(bytes.len() as u64)),
            Err(error) => Ok(Err(error)),
        }
    }

    fn error_to_debug_string(
        &self,
        error: WasiIoErrorErrorBorrow,
    ) -> Result<String, ComponentError> {
        self.state
            .inner
            .borrow()
            .errors
            .get(&error.handle())
            .map(|entry| entry.debug_message.clone())
            .ok_or_else(|| ComponentError::Trap(format!("unknown error handle {}", error.handle())))
    }
}

impl io_error::Host for WasiHost {
    fn error_to_debug_string(
        &self,
        _store: &Store,
        self_: WasiIoErrorErrorBorrow,
    ) -> Result<String, ComponentError> {
        self.error_to_debug_string(self_)
    }
}

impl io_error::HostAsync for WasiHost {
    fn error_to_debug_string<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoErrorErrorBorrow,
    ) -> ComponentFuture<'a, Result<String, ComponentError>> {
        Box::pin(async move { self.error_to_debug_string(self_) })
    }
}

impl io_poll::Host for WasiHost {
    fn pollable_ready(
        &self,
        _store: &Store,
        self_: WasiIoPollPollableBorrow,
    ) -> Result<bool, ComponentError> {
        self.pollable_ready(self_.handle())
    }

    fn pollable_block(
        &self,
        _store: &Store,
        self_: WasiIoPollPollableBorrow,
    ) -> Result<(), ComponentError> {
        self.pollable_block(self_.handle())
    }

    fn poll(
        &self,
        _store: &Store,
        in_: Vec<WasiIoPollPollableBorrow>,
    ) -> Result<Vec<u32>, ComponentError> {
        self.poll(in_)
    }
}

impl io_poll::HostAsync for WasiHost {
    fn pollable_ready<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoPollPollableBorrow,
    ) -> ComponentFuture<'a, Result<bool, ComponentError>> {
        Box::pin(async move { self.pollable_ready(self_.handle()) })
    }

    fn pollable_block<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoPollPollableBorrow,
    ) -> ComponentFuture<'a, Result<(), ComponentError>> {
        Box::pin(async move { self.pollable_block(self_.handle()) })
    }

    fn poll<'a>(
        &'a self,
        _store: &'a Store,
        in_: Vec<WasiIoPollPollableBorrow>,
    ) -> ComponentFuture<'a, Result<Vec<u32>, ComponentError>> {
        Box::pin(async move { self.poll(in_) })
    }
}

impl io_streams::Host for WasiHost {
    fn input_stream_read(
        &self,
        _store: &Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_read(self_, len)
    }

    fn input_stream_blocking_read(
        &self,
        _store: &Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_blocking_read(self_, len)
    }

    fn input_stream_skip(
        &self,
        _store: &Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_skip(self_, len)
    }

    fn input_stream_blocking_skip(
        &self,
        _store: &Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_blocking_skip(self_, len)
    }

    fn input_stream_subscribe(
        &self,
        _store: &Store,
        self_: WasiIoStreamsInputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        self.input_stream_subscribe(self_)
    }

    fn output_stream_check_write(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
    }

    fn output_stream_write(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_write_bytes(self_, &contents)
    }

    fn output_stream_blocking_write_and_flush(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_write_bytes(self_, &contents)
    }

    fn output_stream_flush(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
            .map(|result| result.map(|_| ()))
    }

    fn output_stream_blocking_flush(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
            .map(|result| result.map(|_| ()))
    }

    fn output_stream_subscribe(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        self.output_stream_subscribe(self_)
    }

    fn output_stream_write_zeroes(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        let len = Self::requested_len(len, "output-stream.write-zeroes")?;
        self.output_stream_write_bytes(self_, &vec![0; len])
    }

    fn output_stream_blocking_write_zeroes_and_flush(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        let len = Self::requested_len(len, "output-stream.write-zeroes")?;
        self.output_stream_write_bytes(self_, &vec![0; len])
    }

    fn output_stream_splice(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_splice(self_, src, len)
    }

    fn output_stream_blocking_splice(
        &self,
        _store: &Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_splice(self_, src, len)
    }
}

impl io_streams::HostAsync for WasiHost {
    fn input_stream_read<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError>>
    {
        Box::pin(async move { self.input_stream_read(self_, len) })
    }

    fn input_stream_blocking_read<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError>>
    {
        Box::pin(async move { self.input_stream_blocking_read(self_, len) })
    }

    fn input_stream_skip<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.input_stream_skip(self_, len) })
    }

    fn input_stream_blocking_skip<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.input_stream_blocking_skip(self_, len) })
    }

    fn input_stream_subscribe<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsInputStreamBorrow,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { self.input_stream_subscribe(self_) })
    }

    fn output_stream_check_write<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_check_write(self_) })
    }

    fn output_stream_write<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_write_bytes(self_, &contents) })
    }

    fn output_stream_blocking_write_and_flush<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_write_bytes(self_, &contents) })
    }

    fn output_stream_flush<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            self.output_stream_check_write(self_)
                .map(|result| result.map(|_| ()))
        })
    }

    fn output_stream_blocking_flush<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            self.output_stream_check_write(self_)
                .map(|result| result.map(|_| ()))
        })
    }

    fn output_stream_subscribe<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { self.output_stream_subscribe(self_) })
    }

    fn output_stream_write_zeroes<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            let len = Self::requested_len(len, "output-stream.write-zeroes")?;
            self.output_stream_write_bytes(self_, &vec![0; len])
        })
    }

    fn output_stream_blocking_write_zeroes_and_flush<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            let len = Self::requested_len(len, "output-stream.write-zeroes")?;
            self.output_stream_write_bytes(self_, &vec![0; len])
        })
    }

    fn output_stream_splice<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_splice(self_, src, len) })
    }

    fn output_stream_blocking_splice<'a>(
        &'a self,
        _store: &'a Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_splice(self_, src, len) })
    }
}

#[cfg(test)]
mod tests {
    use super::{poll_fd, read_fd};

    #[cfg(unix)]
    #[test]
    fn poll_fd_reports_pipe_readiness() {
        let mut fds = [0; 2];
        let create = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(create, 0, "pipe should be created");

        let read_fd = fds[0];
        let write_fd = fds[1];

        assert!(!poll_fd(read_fd, 0).expect("empty pipe should not be ready"));

        let payload = [0x41_u8];
        let written = unsafe {
            libc::write(
                write_fd,
                payload.as_ptr().cast::<libc::c_void>(),
                payload.len(),
            )
        };
        assert_eq!(written, 1, "pipe write should succeed");

        assert!(poll_fd(read_fd, 0).expect("pipe with data should be ready"));

        unsafe {
            libc::close(read_fd);
            libc::close(write_fd);
        }
    }

    #[cfg(unix)]
    #[test]
    fn read_fd_returns_prompt_empty_result_when_pipe_not_ready() {
        let mut fds = [0; 2];
        let create = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(create, 0, "pipe should be created");

        let read_fd_end = fds[0];
        let write_fd_end = fds[1];

        let chunk = read_fd(read_fd_end, 8, false).expect("non-blocking read should succeed");
        assert!(chunk.is_empty(), "empty pipe should yield an empty read");

        unsafe {
            libc::close(read_fd_end);
            libc::close(write_fd_end);
        }
    }
}
