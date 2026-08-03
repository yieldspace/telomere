use crate::bindings::types::{
    WasiFilesystemTypesDescriptorFlags, WasiFilesystemTypesDescriptorType,
    WasiFilesystemTypesDirectoryEntry, WasiFilesystemTypesErrorCode,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime};

/// Configuration state shared by a WASI host and its embedding application.
///
/// Clones share the same interior state. Keep one clone after registering it
/// with a linker to inspect captured standard output, standard error, or a
/// guest-requested exit status. Standard output and error are always captured;
/// inherited standard I/O additionally mirrors them to host streams. Construct
/// it with [WasiState::builder] to control process-derived guest settings.
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

/// Builds a [WasiState] with explicit process-derived guest settings.
///
/// A new builder does not inherit arguments, environment variables, filesystem
/// paths, or process standard I/O. It does provide default wall and monotonic
/// clocks plus secure random values; those provider defaults are not configured
/// through this builder. Standard output and error are captured in the resulting
/// state; inheritance additionally mirrors them to host streams.
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
    /// Starts a WASI state builder with no inherited process data or filesystem paths.
    ///
    /// The initial builder has empty arguments and environment, no preopened
    /// directories, buffered empty standard input, and captured standard output
    /// and error. Default provider wall and monotonic clocks and secure random
    /// values remain available; configure process-derived settings before build.
    pub fn builder() -> WasiStateBuilder {
        WasiStateBuilder::new()
    }

    /// Returns the status requested through the WASI CLI exit interface.
    ///
    /// This is None until the guest calls that interface; it does not infer a
    /// status from a successful component return.
    pub fn exit_code(&self) -> Option<u8> {
        self.inner.borrow().exit_code
    }

    /// Returns a snapshot of bytes captured from guest standard output.
    ///
    /// Output is captured even when [WasiStateBuilder::inherit_stdio] also
    /// mirrors it to the embedding process stream.
    pub fn stdout(&self) -> Vec<u8> {
        self.inner.borrow().stdout.clone()
    }

    /// Returns a snapshot of bytes captured from guest standard error.
    ///
    /// Error output is captured even when [WasiStateBuilder::inherit_stdio]
    /// also mirrors it to the embedding process stream.
    pub fn stderr(&self) -> Vec<u8> {
        self.inner.borrow().stderr.clone()
    }
}

impl WasiStateBuilder {
    /// Creates a builder with no inherited process data or filesystem paths.
    ///
    /// Defaults are empty arguments and environment, empty buffered input, no
    /// preopened directories, and captured output. The provider exposes the
    /// host wall clock, a monotonic clock measured from construction, and secure
    /// random values through getrandom. The deterministic 0x5eed seed controls
    /// only the insecure random interfaces.
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

    /// Replaces the guest argument vector.
    ///
    /// The default is an empty vector, so a guest sees no arguments unless this
    /// method or [Self::inherit_args] is used.
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Replaces the guest environment with the supplied key/value pairs.
    ///
    /// The default environment is empty. This grants only the values supplied;
    /// use [Self::inherit_env] to opt into the embedding process environment.
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

    /// Lets guest standard input, output, and error use the embedding process streams.
    ///
    /// By default input is an empty buffer and output is captured in the state.
    /// This method grants ambient terminal or pipe access and additionally
    /// mirrors guest output to the host streams.
    pub fn inherit_stdio(mut self) -> Self {
        self.inherit_stdio = true;
        self
    }

    /// Lets guest standard input read from the embedding process input stream.
    ///
    /// Without this call, the guest reads only the empty buffer or bytes set by
    /// [Self::stdin]. Output remains captured unless [Self::inherit_stdio] is
    /// also used.
    pub fn inherit_stdin(mut self) -> Self {
        self.inherit_stdin = true;
        self
    }

    /// Adds the embedding process environment to the guest environment.
    ///
    /// The default exposes no environment values. Explicit values added by
    /// [Self::env] remain unless a process variable has the same key.
    pub fn inherit_env(mut self) -> Self {
        for (key, value) in std::env::vars() {
            self.env.insert(key, value);
        }
        self
    }

    /// Replaces guest arguments with the embedding process argument vector.
    ///
    /// The default is no arguments. This deliberately includes the embedding
    /// program name, following std::env::args semantics.
    pub fn inherit_args(mut self) -> Self {
        self.args = std::env::args().collect();
        self
    }

    /// Supplies a fixed byte buffer as guest standard input.
    ///
    /// The default buffer is empty. This does not grant access to the embedding
    /// process input stream and is overridden by [Self::inherit_stdin].
    pub fn stdin(mut self, bytes: impl Into<Vec<u8>>) -> Self {
        self.stdin = bytes.into();
        self
    }

    /// Grants the guest a read-only preopened directory at a chosen guest path.
    ///
    /// No filesystem paths are visible by default. Each call adds one named
    /// read-only preopen backed by the supplied host directory.
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

    /// Replaces the guest wall clock source.
    ///
    /// The default calls SystemTime::now, which exposes host wall time. Supply
    /// a deterministic source when reproducible tests require it.
    pub fn wall_clock(mut self, clock: impl Fn() -> SystemTime + 'static) -> Self {
        self.wall_clock = Rc::new(clock);
        self
    }

    /// Replaces the guest monotonic clock source.
    ///
    /// By default it measures elapsed time since builder construction. A custom
    /// source can make time-dependent guests deterministic.
    pub fn monotonic_clock(mut self, clock: impl Fn() -> Duration + 'static) -> Self {
        self.monotonic_clock = Rc::new(clock);
        self
    }

    /// Sets the deterministic seed used by the provider's seeded random state.
    ///
    /// The default seed is 0x5eed. It controls only the insecure random
    /// interfaces; secure random values are independently supplied by getrandom.
    /// Choose an explicit seed for reproducible component tests.
    pub fn random_seed(mut self, seed: u64) -> Self {
        self.random_seed = seed;
        self
    }

    /// Creates the shared WASI state and materializes preopened directory handles.
    ///
    /// The returned state can be cloned into add_to_linker_sync or
    /// add_to_linker_async while the original clone observes captured output and
    /// exit status after guest execution.
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
