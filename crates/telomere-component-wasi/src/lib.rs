extern crate telomere_component;

mod provider;
mod state;

pub use provider::{add_to_linker_async, add_to_linker_sync};
pub use state::{WasiState, WasiStateBuilder};

pub const WASI_VERSION: &str = "0.2.6";

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
