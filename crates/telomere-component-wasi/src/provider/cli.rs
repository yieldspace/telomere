use super::common::next_handle;
use super::WasiHost;
use crate::bindings::imports::{
    wasi_cli_environment as cli_environment, wasi_cli_exit as cli_exit,
    wasi_cli_stderr as cli_stderr, wasi_cli_stdin as cli_stdin, wasi_cli_stdout as cli_stdout,
    wasi_cli_terminal_input as cli_terminal_input, wasi_cli_terminal_output as cli_terminal_output,
    wasi_cli_terminal_stderr as cli_terminal_stderr, wasi_cli_terminal_stdin as cli_terminal_stdin,
    wasi_cli_terminal_stdout as cli_terminal_stdout,
};
use crate::bindings::types::{
    WasiCliTerminalInputTerminalInput, WasiCliTerminalOutputTerminalOutput,
    WasiIoStreamsInputStream, WasiIoStreamsOutputStream,
};
use crate::state::{InputStreamEntry, InputStreamSource, OutputStreamEntry, OutputStreamKind};
use std::rc::Rc;
use telomere_component::{ComponentError, ComponentFuture, ComponentLinker, Store};

pub(super) fn add_to_linker_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    cli_environment::add_to_linker(linker, Rc::clone(&host));
    cli_exit::add_to_linker(linker, Rc::clone(&host));
    cli_stdin::add_to_linker(linker, Rc::clone(&host));
    cli_stdout::add_to_linker(linker, Rc::clone(&host));
    cli_stderr::add_to_linker(linker, Rc::clone(&host));
    cli_terminal_input::add_to_linker(linker, Rc::clone(&host));
    cli_terminal_output::add_to_linker(linker, Rc::clone(&host));
    cli_terminal_stdin::add_to_linker(linker, Rc::clone(&host));
    cli_terminal_stdout::add_to_linker(linker, Rc::clone(&host));
    cli_terminal_stderr::add_to_linker(linker, host);
}

pub(super) fn add_to_linker_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    cli_environment::add_to_linker_async(linker, Rc::clone(&host));
    cli_exit::add_to_linker_async(linker, Rc::clone(&host));
    cli_stdin::add_to_linker_async(linker, Rc::clone(&host));
    cli_stdout::add_to_linker_async(linker, Rc::clone(&host));
    cli_stderr::add_to_linker_async(linker, Rc::clone(&host));
    cli_terminal_input::add_to_linker_async(linker, Rc::clone(&host));
    cli_terminal_output::add_to_linker_async(linker, Rc::clone(&host));
    cli_terminal_stdin::add_to_linker_async(linker, Rc::clone(&host));
    cli_terminal_stdout::add_to_linker_async(linker, Rc::clone(&host));
    cli_terminal_stderr::add_to_linker_async(linker, host);
}

impl WasiHost {
    fn environment(&self) -> Vec<(String, String)> {
        let inner = self.state.inner.borrow();
        let mut env = inner
            .env
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<Vec<_>>();
        env.sort_by(|left, right| left.0.cmp(&right.0));
        env
    }

    fn arguments(&self) -> Vec<String> {
        self.state.inner.borrow().args.clone()
    }

    fn initial_cwd(&self) -> Option<String> {
        self.state
            .inner
            .borrow()
            .preopen_handles
            .first()
            .map(|(_, guest_path)| guest_path.clone())
    }

    fn exit(&self, code: u8) -> Result<(), ComponentError> {
        self.state.inner.borrow_mut().exit_code = Some(code);
        Err(ComponentError::Trap(format!("wasi cli exit({code})")))
    }

    fn input_stream_from_stdin(&self) -> WasiIoStreamsInputStream {
        let mut inner = self.state.inner.borrow_mut();
        let source = if inner.inherit_stdin {
            InputStreamSource::HostStdin
        } else {
            InputStreamSource::Buffer(inner.stdin.clone())
        };
        let handle = next_handle(&mut inner.next_handle);
        inner.input_streams.insert(
            handle,
            InputStreamEntry {
                source,
                position: 0,
                closed: false,
            },
        );
        WasiIoStreamsInputStream::new(handle)
    }

    fn output_stream(&self, kind: OutputStreamKind) -> WasiIoStreamsOutputStream {
        let mut inner = self.state.inner.borrow_mut();
        let handle = next_handle(&mut inner.next_handle);
        inner.output_streams.insert(
            handle,
            OutputStreamEntry {
                kind,
                closed: false,
            },
        );
        WasiIoStreamsOutputStream::new(handle)
    }
}

impl cli_environment::Host for WasiHost {
    fn get_environment(&self, _store: &mut Store) -> Result<Vec<(String, String)>, ComponentError> {
        Ok(self.environment())
    }

    fn get_arguments(&self, _store: &mut Store) -> Result<Vec<String>, ComponentError> {
        Ok(self.arguments())
    }

    fn initial_cwd(&self, _store: &mut Store) -> Result<Option<String>, ComponentError> {
        Ok(self.initial_cwd())
    }
}

impl cli_environment::HostAsync for WasiHost {
    fn get_environment<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Vec<(String, String)>, ComponentError>> {
        Box::pin(async move { Ok(self.environment()) })
    }

    fn get_arguments<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Vec<String>, ComponentError>> {
        Box::pin(async move { Ok(self.arguments()) })
    }

    fn initial_cwd<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Option<String>, ComponentError>> {
        Box::pin(async move { Ok(self.initial_cwd()) })
    }
}

impl cli_exit::Host for WasiHost {
    fn exit(&self, _store: &mut Store, status: Result<(), ()>) -> Result<(), ComponentError> {
        self.exit(if status.is_ok() { 0 } else { 1 })
    }
}

impl cli_exit::HostAsync for WasiHost {
    fn exit<'a>(
        &'a self,
        _store: &'a mut Store,
        status: Result<(), ()>,
    ) -> ComponentFuture<'a, Result<(), ComponentError>> {
        Box::pin(async move { self.exit(if status.is_ok() { 0 } else { 1 }) })
    }
}

impl cli_stdin::Host for WasiHost {
    fn get_stdin(&self, _store: &mut Store) -> Result<WasiIoStreamsInputStream, ComponentError> {
        Ok(self.input_stream_from_stdin())
    }
}

impl cli_stdin::HostAsync for WasiHost {
    fn get_stdin<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<WasiIoStreamsInputStream, ComponentError>> {
        Box::pin(async move { Ok(self.input_stream_from_stdin()) })
    }
}

impl cli_stdout::Host for WasiHost {
    fn get_stdout(&self, _store: &mut Store) -> Result<WasiIoStreamsOutputStream, ComponentError> {
        Ok(self.output_stream(OutputStreamKind::Stdout))
    }
}

impl cli_stdout::HostAsync for WasiHost {
    fn get_stdout<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<WasiIoStreamsOutputStream, ComponentError>> {
        Box::pin(async move { Ok(self.output_stream(OutputStreamKind::Stdout)) })
    }
}

impl cli_stderr::Host for WasiHost {
    fn get_stderr(&self, _store: &mut Store) -> Result<WasiIoStreamsOutputStream, ComponentError> {
        Ok(self.output_stream(OutputStreamKind::Stderr))
    }
}

impl cli_stderr::HostAsync for WasiHost {
    fn get_stderr<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<WasiIoStreamsOutputStream, ComponentError>> {
        Box::pin(async move { Ok(self.output_stream(OutputStreamKind::Stderr)) })
    }
}

impl cli_terminal_input::Host for WasiHost {}
impl cli_terminal_input::HostAsync for WasiHost {}
impl cli_terminal_output::Host for WasiHost {}
impl cli_terminal_output::HostAsync for WasiHost {}

impl cli_terminal_stdin::Host for WasiHost {
    fn get_terminal_stdin(
        &self,
        _store: &mut Store,
    ) -> Result<Option<WasiCliTerminalInputTerminalInput>, ComponentError> {
        Ok(None)
    }
}

impl cli_terminal_stdin::HostAsync for WasiHost {
    fn get_terminal_stdin<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Option<WasiCliTerminalInputTerminalInput>, ComponentError>>
    {
        Box::pin(async move { Ok(None) })
    }
}

impl cli_terminal_stdout::Host for WasiHost {
    fn get_terminal_stdout(
        &self,
        _store: &mut Store,
    ) -> Result<Option<WasiCliTerminalOutputTerminalOutput>, ComponentError> {
        Ok(None)
    }
}

impl cli_terminal_stdout::HostAsync for WasiHost {
    fn get_terminal_stdout<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Option<WasiCliTerminalOutputTerminalOutput>, ComponentError>>
    {
        Box::pin(async move { Ok(None) })
    }
}

impl cli_terminal_stderr::Host for WasiHost {
    fn get_terminal_stderr(
        &self,
        _store: &mut Store,
    ) -> Result<Option<WasiCliTerminalOutputTerminalOutput>, ComponentError> {
        Ok(None)
    }
}

impl cli_terminal_stderr::HostAsync for WasiHost {
    fn get_terminal_stderr<'a>(
        &'a self,
        _store: &'a mut Store,
    ) -> ComponentFuture<'a, Result<Option<WasiCliTerminalOutputTerminalOutput>, ComponentError>>
    {
        Box::pin(async move { Ok(None) })
    }
}
