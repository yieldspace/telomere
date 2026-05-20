mod cli;
mod clocks;
mod common;
mod filesystem;
mod io;
pub(crate) mod preview3;
mod random;
mod sockets;

#[cfg(test)]
mod tests;

use crate::state::WasiState;
use crate::substrate::WasiSubstrate;
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

    pub(crate) fn substrate(&self) -> WasiSubstrate {
        WasiSubstrate::new(self.state.clone())
    }
}

pub fn add_to_linker_sync(
    linker: &mut ComponentLinker,
    state: WasiState,
) -> Result<(), ComponentError> {
    let host = Rc::new(WasiHost::new(state));
    register_sync(linker, host);
    Ok(())
}

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
