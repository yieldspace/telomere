pub mod preview3;
mod provider;
mod state;
mod substrate;

pub use provider::{add_to_linker_async, add_to_linker_sync};
pub use state::{WasiState, WasiStateBuilder};

pub const WASI_VERSION: &str = "0.2.6";
pub const WASI_PREVIEW3_CLI_VERSION: &str = "0.3.0-rc-2026-03-15";
pub const WASI_PREVIEW3_CLOCKS_VERSION: &str = "0.3.0-rc-2026-03-15";
pub const WASI_PREVIEW3_RANDOM_VERSION: &str = "0.3.0-rc-2026-03-15";
pub const WASI_PREVIEW3_FILESYSTEM_VERSION: &str = "0.3.0-rc-2026-03-15";
pub const WASI_PREVIEW3_SOCKETS_VERSION: &str = "0.3.0-rc-2026-03-15";
pub const WASI_PREVIEW3_IO_COMPAT_VERSION: &str = "0.2.8";
pub const WASI_PREVIEW3_WIT_SNAPSHOT: &str =
    "WebAssembly/WASI main proposals/cli|clocks|filesystem|random|sockets wit-0.3.0-draft vendored in wit-preview3; wit/ remains WebAssembly/WASI v0.2.6 wasip2 for compatibility bindgen";

telomere_component_bindgen::bindgen!({
    path: "wit/cli",
    deps: [
        "wit/io",
        "wit/clocks",
        "wit/random",
        "wit/filesystem",
        "wit/sockets"
    ],
    world: "wasi:cli/command@0.2.6",
    module: "bindings",
    host_mode: "both",
    strip_interface_version: true
});

include!(concat!(env!("OUT_DIR"), "/generated_bindgen.rs"));
