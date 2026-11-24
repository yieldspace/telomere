mod cli;
mod clocks;
mod common;
mod filesystem;
mod io;
mod random;

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
    register_resource_drops(linker, host);
}

fn register_async(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    cli::add_to_linker_async(linker, Rc::clone(&host));
    clocks::add_to_linker_async(linker, Rc::clone(&host));
    filesystem::add_to_linker_async(linker, Rc::clone(&host));
    io::add_to_linker_async(linker, Rc::clone(&host));
    random::add_to_linker_async(linker, Rc::clone(&host));
    register_resource_drops(linker, host);
}

/// Registers destructors for the resource types this provider hands out, so
/// a guest's `canon resource.drop` releases the provider state behind the
/// handle instead of trapping.
///
/// Every handle is allocated from one counter in [WasiState] and lives in
/// exactly one table, so one release operation serves every resource type.
/// The terminal-input/terminal-output resources are omitted: the provider
/// never issues handles for them. Sockets resources are omitted for a
/// stronger reason: the provider does not register the `wasi:sockets`
/// interfaces at all, so no guest can hold a sockets handle through this
/// provider (see docs/core/sockets-scope.md).
fn register_resource_drops(linker: &mut ComponentLinker, host: Rc<WasiHost>) {
    const RESOURCE_TYPES: &[(&str, &str)] = &[
        ("wasi:io/error@0.2.6", "error"),
        ("wasi:io/poll@0.2.6", "pollable"),
        ("wasi:io/streams@0.2.6", "input-stream"),
        ("wasi:io/streams@0.2.6", "output-stream"),
        ("wasi:filesystem/types@0.2.6", "descriptor"),
        ("wasi:filesystem/types@0.2.6", "directory-entry-stream"),
    ];
    for &(instance, resource) in RESOURCE_TYPES {
        let host = Rc::clone(&host);
        linker.register_import_resource_drop(instance, resource, move |_store, handle| {
            if host.state.drop_handle(handle) {
                Ok(())
            } else {
                Err(ComponentError::Trap(format!(
                    "unknown wasi resource handle {handle}"
                )))
            }
        });
    }
}
