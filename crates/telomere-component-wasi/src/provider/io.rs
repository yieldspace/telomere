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
use std::io::Write;
use std::rc::Rc;
use std::thread;
use std::time::Duration;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

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
            let sleep = match self.state.inner.borrow().pollables.get(&handle).cloned() {
                Some(PollableEntry::MonotonicDeadline(deadline)) => {
                    let now = (self.state.inner.borrow().monotonic_clock)();
                    deadline.saturating_sub(now).min(Duration::from_millis(5))
                }
                Some(PollableEntry::Ready) => Duration::from_millis(0),
                None => {
                    return Err(ComponentError::Trap(format!(
                        "unknown pollable handle {handle}"
                    )));
                }
            };
            if !sleep.is_zero() {
                thread::sleep(sleep);
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

    fn input_stream_read(
        &self,
        stream: WasiIoStreamsInputStreamBorrow,
        len: u64,
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

        let read_len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
        let (chunk, next_position) = match source {
            InputStreamSource::Buffer(bytes) => {
                let start = usize::try_from(position)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let end = start.saturating_add(read_len).min(bytes.len());
                (bytes[start..end].to_vec(), end as u64)
            }
            InputStreamSource::File(path) => {
                let bytes = fs::read(&path).map_err(|error| {
                    ComponentError::Trap(format!("failed to read `{}`: {error}", path.display()))
                })?;
                let start = usize::try_from(position)
                    .unwrap_or(usize::MAX)
                    .min(bytes.len());
                let end = start.saturating_add(read_len).min(bytes.len());
                (bytes[start..end].to_vec(), end as u64)
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
        let read = self.input_stream_read(stream, len)?;
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
        Ok(self.allocate_pollable(PollableEntry::Ready))
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
        _store: &mut Store,
        self_: WasiIoErrorErrorBorrow,
    ) -> Result<String, ComponentError> {
        self.error_to_debug_string(self_)
    }
}

impl io_error::HostAsync for WasiHost {
    fn error_to_debug_string<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoErrorErrorBorrow,
    ) -> ComponentFuture<'a, Result<String, ComponentError>> {
        Box::pin(async move { self.error_to_debug_string(self_) })
    }
}

impl io_poll::Host for WasiHost {
    fn pollable_ready(
        &self,
        _store: &mut Store,
        self_: WasiIoPollPollableBorrow,
    ) -> Result<bool, ComponentError> {
        self.pollable_ready(self_.handle())
    }

    fn pollable_block(
        &self,
        _store: &mut Store,
        self_: WasiIoPollPollableBorrow,
    ) -> Result<(), ComponentError> {
        self.pollable_block(self_.handle())
    }

    fn poll(
        &self,
        _store: &mut Store,
        in_: Vec<WasiIoPollPollableBorrow>,
    ) -> Result<Vec<u32>, ComponentError> {
        self.poll(in_)
    }
}

impl io_poll::HostAsync for WasiHost {
    fn pollable_ready<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoPollPollableBorrow,
    ) -> ComponentFuture<'a, Result<bool, ComponentError>> {
        Box::pin(async move { self.pollable_ready(self_.handle()) })
    }

    fn pollable_block<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoPollPollableBorrow,
    ) -> ComponentFuture<'a, Result<(), ComponentError>> {
        Box::pin(async move { self.pollable_block(self_.handle()) })
    }

    fn poll<'a>(
        &'a self,
        _store: &'a mut Store,
        in_: Vec<WasiIoPollPollableBorrow>,
    ) -> ComponentFuture<'a, Result<Vec<u32>, ComponentError>> {
        Box::pin(async move { self.poll(in_) })
    }
}

impl io_streams::Host for WasiHost {
    fn input_stream_read(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_read(self_, len)
    }

    fn input_stream_blocking_read(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_read(self_, len)
    }

    fn input_stream_skip(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_skip(self_, len)
    }

    fn input_stream_blocking_skip(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.input_stream_skip(self_, len)
    }

    fn input_stream_subscribe(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        self.input_stream_subscribe(self_)
    }

    fn output_stream_check_write(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
    }

    fn output_stream_write(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_write_bytes(self_, &contents)
    }

    fn output_stream_blocking_write_and_flush(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_write_bytes(self_, &contents)
    }

    fn output_stream_flush(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
            .map(|result| result.map(|_| ()))
    }

    fn output_stream_blocking_flush(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_check_write(self_)
            .map(|result| result.map(|_| ()))
    }

    fn output_stream_subscribe(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> Result<WasiIoPollPollable, ComponentError> {
        self.output_stream_subscribe(self_)
    }

    fn output_stream_write_zeroes(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        let len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
        self.output_stream_write_bytes(self_, &vec![0; len])
    }

    fn output_stream_blocking_write_zeroes_and_flush(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> Result<Result<(), WasiIoStreamsStreamError>, ComponentError> {
        let len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
        self.output_stream_write_bytes(self_, &vec![0; len])
    }

    fn output_stream_splice(
        &self,
        _store: &mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> Result<Result<u64, WasiIoStreamsStreamError>, ComponentError> {
        self.output_stream_splice(self_, src, len)
    }

    fn output_stream_blocking_splice(
        &self,
        _store: &mut Store,
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
        _store: &'a mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError>>
    {
        Box::pin(async move { self.input_stream_read(self_, len) })
    }

    fn input_stream_blocking_read<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<Vec<u8>, WasiIoStreamsStreamError>, ComponentError>>
    {
        Box::pin(async move { self.input_stream_read(self_, len) })
    }

    fn input_stream_skip<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.input_stream_skip(self_, len) })
    }

    fn input_stream_blocking_skip<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.input_stream_skip(self_, len) })
    }

    fn input_stream_subscribe<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsInputStreamBorrow,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { self.input_stream_subscribe(self_) })
    }

    fn output_stream_check_write<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_check_write(self_) })
    }

    fn output_stream_write<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_write_bytes(self_, &contents) })
    }

    fn output_stream_blocking_write_and_flush<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        contents: Vec<u8>,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_write_bytes(self_, &contents) })
    }

    fn output_stream_flush<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            self.output_stream_check_write(self_)
                .map(|result| result.map(|_| ()))
        })
    }

    fn output_stream_blocking_flush<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            self.output_stream_check_write(self_)
                .map(|result| result.map(|_| ()))
        })
    }

    fn output_stream_subscribe<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
    ) -> ComponentFuture<'a, Result<WasiIoPollPollable, ComponentError>> {
        Box::pin(async move { self.output_stream_subscribe(self_) })
    }

    fn output_stream_write_zeroes<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            let len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
            self.output_stream_write_bytes(self_, &vec![0; len])
        })
    }

    fn output_stream_blocking_write_zeroes_and_flush<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<(), WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move {
            let len = usize::try_from(len).unwrap_or(usize::MAX).min(1 << 20);
            self.output_stream_write_bytes(self_, &vec![0; len])
        })
    }

    fn output_stream_splice<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_splice(self_, src, len) })
    }

    fn output_stream_blocking_splice<'a>(
        &'a self,
        _store: &'a mut Store,
        self_: WasiIoStreamsOutputStreamBorrow,
        src: WasiIoStreamsInputStreamBorrow,
        len: u64,
    ) -> ComponentFuture<'a, Result<Result<u64, WasiIoStreamsStreamError>, ComponentError>> {
        Box::pin(async move { self.output_stream_splice(self_, src, len) })
    }
}
