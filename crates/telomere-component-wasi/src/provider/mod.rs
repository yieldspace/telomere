mod cli;
mod clocks;
mod common;
mod filesystem;
mod io;
mod random;
mod sockets;

#[cfg(test)]
mod tests;

use crate::state::WasiState;
use std::rc::Rc;
use telomere_component::{ComponentError, ComponentLinker};

#[derive(Clone)]
struct WasiHost {
    state: WasiState,
}

impl WasiHost {
    fn new(state: WasiState) -> Self {
        Self { state }
    }
}

/// Installs synchronous WASI 0.2 host functions into a component linker.
///
/// The supplied state is shared with the registered host implementation, so
/// callers can inspect captured output and exit status after component calls.
/// Arguments, environment, preopened directories, and inherited process I/O
/// require explicit WasiStateBuilder configuration. Default clock and secure
/// random providers remain available without those settings.
pub fn add_to_linker_sync(
    linker: &mut ComponentLinker,
    state: WasiState,
) -> Result<(), ComponentError> {
    let host = Rc::new(WasiHost::new(state));
    register_sync(linker, host);
    Ok(())
}

/// Installs asynchronous WASI 0.2 host functions into a component linker.
///
/// This registers the async trait variants generated from the bundled WIT. It
/// shares the same process-data configuration and default clock/random provider
/// behavior as add_to_linker_sync.
pub fn add_to_linker_async(
    linker: &mut ComponentLinker,
    state: WasiState,
) -> Result<(), ComponentError> {
    let host = Rc::new(WasiHost::new(state));
    register_async(linker, host);
    Ok(())
}

fn register_sync(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    cli::add_to_linker_sync(linker, Rc::clone(&host));
    clocks::add_to_linker_sync(linker, Rc::clone(&host));
    filesystem::add_to_linker_sync(linker, Rc::clone(&host));
    io::add_to_linker_sync(linker, Rc::clone(&host));
    random::add_to_linker_sync(linker, Rc::clone(&host));
    sockets::add_to_linker_sync(linker, host);
}

fn register_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    cli::add_to_linker_async(linker, Rc::clone(&host));
    clocks::add_to_linker_async(linker, Rc::clone(&host));
    filesystem::add_to_linker_async(linker, Rc::clone(&host));
    io::add_to_linker_async(linker, Rc::clone(&host));
    random::add_to_linker_async(linker, Rc::clone(&host));
    sockets::add_to_linker_async(linker, host);
}
