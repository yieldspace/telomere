use crate::provider;
use crate::state::WasiState;
use telomere_component::{ComponentError, ComponentLinker};

pub const CLI_VERSION: &str = crate::WASI_PREVIEW3_CLI_VERSION;
pub const CLOCKS_VERSION: &str = crate::WASI_PREVIEW3_CLOCKS_VERSION;
pub const RANDOM_VERSION: &str = crate::WASI_PREVIEW3_RANDOM_VERSION;
pub const FILESYSTEM_VERSION: &str = crate::WASI_PREVIEW3_FILESYSTEM_VERSION;
pub const SOCKETS_VERSION: &str = crate::WASI_PREVIEW3_SOCKETS_VERSION;
pub const IO_COMPAT_VERSION: &str = crate::WASI_PREVIEW3_IO_COMPAT_VERSION;
pub const WIT_SNAPSHOT: &str = crate::WASI_PREVIEW3_WIT_SNAPSHOT;

pub fn add_to_linker_async(
    linker: &mut ComponentLinker,
    state: WasiState,
) -> Result<(), ComponentError> {
    provider::preview3::add_to_linker_async(linker, state)
}

pub fn unsupported_interface_error(interface: &str) -> ComponentError {
    provider::preview3::unsupported_interface_error(interface)
}
