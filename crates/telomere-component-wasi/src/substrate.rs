use crate::state::{InputStreamSource, PollableEntry, WasiState};
use std::time::Duration;
use telomere_component::ComponentError;

#[cfg(unix)]
const HOST_STDIN_FD: libc::c_int = libc::STDIN_FILENO;
#[cfg(not(unix))]
const HOST_STDIN_FD: i32 = 0;

#[derive(Clone)]
pub(crate) struct WasiSubstrate {
    state: WasiState,
}

enum WaitPlan {
    Sleep(Duration),
    HostStdin,
}

impl WasiSubstrate {
    pub(crate) fn new(state: WasiState) -> Self {
        Self { state }
    }

    pub(crate) fn pollable_ready(&self, handle: u32) -> Result<bool, ComponentError> {
        let entry = self.pollable_entry(handle)?;
        Ok(match entry {
            PollableEntry::Ready => true,
            PollableEntry::InputStream(stream) => self.input_stream_ready(stream)?,
            PollableEntry::MonotonicDeadline(deadline) => self.monotonic_now() >= deadline,
        })
    }

    pub(crate) fn pollable_block_ready_only(&self, handle: u32) -> Result<(), ComponentError> {
        if self.pollable_ready(handle)? {
            Ok(())
        } else {
            Err(ComponentError::Unsupported(format!(
                "pollable {handle} is not ready during sync WASI execution"
            )))
        }
    }

    pub(crate) fn poll_ready_only(&self, handles: &[u32]) -> Result<Vec<u32>, ComponentError> {
        ensure_non_empty(handles)?;
        let ready = self.ready_indices(handles)?;
        if ready.is_empty() {
            Err(ComponentError::Unsupported(
                "wasi:io/poll.poll found no ready pollables during sync WASI execution".to_owned(),
            ))
        } else {
            Ok(ready)
        }
    }

    pub(crate) async fn pollable_block(&self, handle: u32) -> Result<(), ComponentError> {
        loop {
            if self.pollable_ready(handle)? {
                return Ok(());
            }
            let plan = self.wait_plan_for_pollable(handle)?;
            self.await_plan(plan).await?;
        }
    }

    pub(crate) async fn poll(&self, handles: &[u32]) -> Result<Vec<u32>, ComponentError> {
        ensure_non_empty(handles)?;
        loop {
            let ready = self.ready_indices(handles)?;
            if !ready.is_empty() {
                return Ok(ready);
            }
            let plan = self.wait_plan_for_pollables(handles)?;
            self.await_plan(plan).await?;
        }
    }

    pub(crate) async fn wait_monotonic_deadline(
        &self,
        deadline: Duration,
    ) -> Result<(), ComponentError> {
        loop {
            let now = self.monotonic_now();
            if now >= deadline {
                return Ok(());
            }
            tokio::time::sleep(deadline.saturating_sub(now)).await;
        }
    }

    pub(crate) async fn wait_monotonic_duration(
        &self,
        duration: Duration,
    ) -> Result<(), ComponentError> {
        let deadline = self.monotonic_now().saturating_add(duration);
        self.wait_monotonic_deadline(deadline).await
    }

    fn ready_indices(&self, handles: &[u32]) -> Result<Vec<u32>, ComponentError> {
        let mut ready = Vec::new();
        for (index, handle) in handles.iter().copied().enumerate() {
            if self.pollable_ready(handle)? {
                ready.push(index as u32);
            }
        }
        Ok(ready)
    }

    fn wait_plan_for_pollables(&self, handles: &[u32]) -> Result<WaitPlan, ComponentError> {
        let mut has_host_stdin = false;
        let mut earliest_deadline = None;
        for handle in handles.iter().copied() {
            match self.wait_plan_for_pollable(handle)? {
                WaitPlan::Sleep(delay) => {
                    let deadline = self.monotonic_now().saturating_add(delay);
                    earliest_deadline = match earliest_deadline {
                        Some(current) if current <= deadline => Some(current),
                        _ => Some(deadline),
                    };
                }
                WaitPlan::HostStdin => has_host_stdin = true,
            }
        }

        if let Some(deadline) = earliest_deadline {
            let now = self.monotonic_now();
            return Ok(WaitPlan::Sleep(deadline.saturating_sub(now)));
        }
        if has_host_stdin {
            return Ok(WaitPlan::HostStdin);
        }
        Err(ComponentError::Unsupported(
            "wasi:io/poll.poll has no waitable non-ready pollables".to_owned(),
        ))
    }

    fn wait_plan_for_pollable(&self, handle: u32) -> Result<WaitPlan, ComponentError> {
        match self.pollable_entry(handle)? {
            PollableEntry::Ready => Ok(WaitPlan::Sleep(Duration::ZERO)),
            PollableEntry::InputStream(stream) => self.input_stream_wait_plan(stream),
            PollableEntry::MonotonicDeadline(deadline) => Ok(WaitPlan::Sleep(
                deadline.saturating_sub(self.monotonic_now()),
            )),
        }
    }

    async fn await_plan(&self, plan: WaitPlan) -> Result<(), ComponentError> {
        match plan {
            WaitPlan::Sleep(duration) => {
                if duration.is_zero() {
                    tokio::task::yield_now().await;
                } else {
                    tokio::time::sleep(duration).await;
                }
                Ok(())
            }
            WaitPlan::HostStdin => wait_host_stdin_readable().await,
        }
    }

    fn pollable_entry(&self, handle: u32) -> Result<PollableEntry, ComponentError> {
        self.state
            .inner
            .borrow()
            .pollables
            .get(&handle)
            .cloned()
            .ok_or_else(|| ComponentError::Trap(format!("unknown pollable handle {handle}")))
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

    fn input_stream_wait_plan(&self, handle: u32) -> Result<WaitPlan, ComponentError> {
        let (source, closed) = self.input_stream_state(handle)?;
        if closed {
            return Ok(WaitPlan::Sleep(Duration::ZERO));
        }
        match source {
            InputStreamSource::Buffer(_) | InputStreamSource::File(_) => {
                Ok(WaitPlan::Sleep(Duration::ZERO))
            }
            InputStreamSource::HostStdin => Ok(WaitPlan::HostStdin),
        }
    }

    fn monotonic_now(&self) -> Duration {
        (self.state.inner.borrow().monotonic_clock)()
    }
}

fn ensure_non_empty(handles: &[u32]) -> Result<(), ComponentError> {
    if handles.is_empty() {
        Err(ComponentError::Trap(
            "wasi:io/poll.poll requires at least one pollable".to_owned(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
pub(crate) fn poll_fd(fd: libc::c_int, timeout_ms: libc::c_int) -> Result<bool, ComponentError> {
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
pub(crate) fn poll_fd(_fd: i32, _timeout_ms: i32) -> Result<bool, ComponentError> {
    Ok(true)
}

#[cfg(unix)]
async fn wait_host_stdin_readable() -> Result<(), ComponentError> {
    let result =
        tokio::task::spawn_blocking(|| poll_fd(HOST_STDIN_FD, -1).map_err(|e| e.to_string()))
            .await
            .map_err(|error| {
                ComponentError::Runtime(format!("stdin readiness task failed: {error}"))
            })?;
    result.map_err(ComponentError::Trap)?;
    Ok(())
}

#[cfg(not(unix))]
async fn wait_host_stdin_readable() -> Result<(), ComponentError> {
    tokio::task::yield_now().await;
    Ok(())
}
