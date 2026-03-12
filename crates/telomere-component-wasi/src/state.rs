use crate::bindings::types::{
    WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesDescriptorType,
    WasiFilesystemTypesDirectoryEntry, WasiFilesystemTypesErrorCode,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

#[derive(Clone)]
pub struct WasiState {
    pub(crate) inner: Rc<RefCell<WasiStateInner>>,
}

#[derive(Clone)]
pub(crate) struct PreopenDir {
    pub host_path: PathBuf,
    pub guest_path: String,
}

#[derive(Clone)]
pub(crate) struct DescriptorEntry {
    pub host_path: PathBuf,
    pub descriptor_type: WasiFilesystemTypesDescriptorType,
    pub flags: WasiFilesystemTypesDescriptorFlags,
}

#[derive(Clone)]
pub(crate) enum InputStreamSource {
    Buffer(Vec<u8>),
    File(PathBuf),
    HostStdin,
}

pub(crate) struct InputStreamEntry {
    pub source: InputStreamSource,
    pub position: u64,
    pub closed: bool,
}

#[derive(Clone, Copy)]
pub(crate) enum OutputStreamKind {
    Stdout,
    Stderr,
}

pub(crate) struct OutputStreamEntry {
    pub kind: OutputStreamKind,
    pub closed: bool,
}

#[derive(Clone)]
pub(crate) enum PollableEntry {
    Ready,
    InputStream(u32),
    MonotonicDeadline(Duration),
}

#[derive(Clone)]
pub(crate) struct ErrorEntry {
    pub debug_message: String,
    pub filesystem_code: Option<WasiFilesystemTypesErrorCode>,
}

pub(crate) struct DirectoryEntryStreamEntry {
    pub entries: Vec<WasiFilesystemTypesDirectoryEntry>,
    pub cursor: usize,
}

pub(crate) struct WasiStateInner {
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub stdin: Vec<u8>,
    pub inherit_stdin: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub inherit_stdio: bool,
    pub preopens: Vec<PreopenDir>,
    pub wall_clock: Rc<dyn Fn() -> SystemTime>,
    pub monotonic_clock: Rc<dyn Fn() -> Duration>,
    pub random_seed: u64,
    pub exit_code: Option<u8>,
    pub next_handle: u32,
    pub preopen_handles: Vec<(u32, String)>,
    pub descriptors: HashMap<u32, DescriptorEntry>,
    pub input_streams: HashMap<u32, InputStreamEntry>,
    pub output_streams: HashMap<u32, OutputStreamEntry>,
    pub pollables: HashMap<u32, PollableEntry>,
    pub errors: HashMap<u32, ErrorEntry>,
    pub directory_entry_streams: HashMap<u32, DirectoryEntryStreamEntry>,
}

pub struct WasiStateBuilder {
    args: Vec<String>,
    env: HashMap<String, String>,
    stdin: Vec<u8>,
    inherit_stdin: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    inherit_stdio: bool,
    preopens: Vec<PreopenDir>,
    wall_clock: Rc<dyn Fn() -> SystemTime>,
    monotonic_clock: Rc<dyn Fn() -> Duration>,
    random_seed: u64,
}

impl Default for WasiStateBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl WasiState {
    pub fn builder() -> WasiStateBuilder {
        WasiStateBuilder::new()
    }

    pub fn exit_code(&self) -> Option<u8> {
        self.inner.borrow().exit_code
    }

    pub fn stdout(&self) -> Vec<u8> {
        self.inner.borrow().stdout.clone()
    }

    pub fn stderr(&self) -> Vec<u8> {
        self.inner.borrow().stderr.clone()
    }
}

impl WasiStateBuilder {
    pub fn new() -> Self {
        let monotonic_origin = Instant::now();
        Self {
            args: Vec::new(),
            env: HashMap::new(),
            stdin: Vec::new(),
            inherit_stdin: false,
            stdout: Vec::new(),
            stderr: Vec::new(),
            inherit_stdio: false,
            preopens: Vec::new(),
            wall_clock: Rc::new(SystemTime::now),
            monotonic_clock: Rc::new(move || monotonic_origin.elapsed()),
            random_seed: 0x5eed_u64,
        }
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    pub fn env<I, K, V>(mut self, env: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.env = env
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect();
        self
    }

    pub fn inherit_stdio(mut self) -> Self {
        self.inherit_stdio = true;
        self
    }

    pub fn inherit_stdin(mut self) -> Self {
        self.inherit_stdin = true;
        self
    }

    pub fn inherit_env(mut self) -> Self {
        for (key, value) in std::env::vars() {
            self.env.insert(key, value);
        }
        self
    }

    pub fn inherit_args(mut self) -> Self {
        self.args = std::env::args().collect();
        self
    }

    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = bytes.into();
        self
    }

    pub fn preopen_dir(
        mut self,
        host_path: impl AsRef<Path>,
        guest_path: impl Into<String>,
    ) -> Self {
        self.preopens.push(PreopenDir {
            host_path: host_path.as_ref().to_path_buf(),
            guest_path: guest_path.into(),
        });
        self
    }

    pub fn wall_clock(mut self, clock: impl Fn() -> SystemTime + 'static) -> Self {
        self.wall_clock = Rc::new(clock);
        self
    }

    pub fn monotonic_clock(mut self, clock: impl Fn() -> Duration + 'static) -> Self {
        self.monotonic_clock = Rc::new(clock);
        self
    }

    pub fn random_seed(mut self, seed: u64) -> Self {
        self.random_seed = seed;
        self
    }

    pub fn build(self) -> WasiState {
        let mut inner = WasiStateInner {
            args: self.args,
            env: self.env,
            stdin: self.stdin,
            inherit_stdin: self.inherit_stdin,
            stdout: self.stdout,
            stderr: self.stderr,
            inherit_stdio: self.inherit_stdio,
            preopens: self.preopens,
            wall_clock: self.wall_clock,
            monotonic_clock: self.monotonic_clock,
            random_seed: self.random_seed,
            exit_code: None,
            next_handle: 1,
            preopen_handles: Vec::new(),
            descriptors: HashMap::new(),
            input_streams: HashMap::new(),
            output_streams: HashMap::new(),
            pollables: HashMap::new(),
            errors: HashMap::new(),
            directory_entry_streams: HashMap::new(),
        };

        for preopen in inner.preopens.clone() {
            let handle = inner.next_handle;
            inner.next_handle += 1;
            inner
                .preopen_handles
                .push((handle, preopen.guest_path.clone()));
            inner.descriptors.insert(
                handle,
                DescriptorEntry {
                    host_path: preopen.host_path,
                    descriptor_type: WasiFilesystemTypesDescriptorType::Directory,
                    flags: read_only_descriptor_flags(),
                },
            );
        }

        WasiState {
            inner: Rc::new(RefCell::new(inner)),
        }
    }
}

fn read_only_descriptor_flags() -> WasiFilesystemTypesDescriptorFlags {
    WasiFilesystemTypesDescriptorFlags {
        read: true,
        write: false,
        file_integrity_sync: false,
        data_integrity_sync: false,
        requested_write_sync: false,
        mutate_directory: false,
    }
}

#[cfg(test)]
mod tests {
    use super::WasiState;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn default_monotonic_clock_advances() {
        let state = WasiState::builder().build();
        let clock = state.inner.borrow().monotonic_clock.clone();
        let first = clock();
        for _ in 0..10 {
            thread::sleep(Duration::from_millis(1));
            if clock() > first {
                return;
            }
        }
        panic!("default monotonic clock did not advance");
    }
}
