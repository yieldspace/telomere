//! WASI 0.2.6-targeting host provider for telomere's Component Model runtime.
//!
//! This crate targets the bundled WASI 0.2.6 WIT packages and registers their
//! `cli`, `io`, `clocks`, `random`, `filesystem`, and `sockets` bindings in a
//! component linker. Function-level coverage is intentionally narrower than
//! the bundled WIT surface; consult the support matrix for coverage details,
//! including partial filesystem support and currently unimplemented sockets.
//!
//! Process-derived settings - arguments, environment, preopened directories,
//! and standard I/O inheritance - are configured through [`WasiState`]. The
//! provider supplies default wall and monotonic clocks plus secure random values.
//! Install its interfaces with [`add_to_linker_sync`] or [`add_to_linker_async`].
//! The generated [`bindings`] module is the typed WIT-facing API for callers
//! that need to invoke or implement these interfaces directly.
//!
//! # Example
//!
//! This runs the bundled WASI command component with explicit process-derived
//! guest settings while retaining the provider's default clocks and secure random.
//! The small executor uses only the standard library and is
//! the same style used by the full standalone example in
//! `examples/minimal-embedder/`.
//!
//! ```
//! use telomere_component::{ComponentEngine, ComponentLinker, Store};
//! use telomere_component_wasi::{add_to_linker_sync, bindings, WasiState};
//! # use std::{future::Future, sync::Arc, task::{Context, Poll, Wake, Waker}};
//! # struct ThreadWaker(std::thread::Thread);
//! # impl Wake for ThreadWaker {
//! #     fn wake(self: Arc<Self>) { self.0.unpark(); }
//! #     fn wake_by_ref(self: &Arc<Self>) { self.0.unpark(); }
//! # }
//! # fn block_on<F: Future>(future: F) -> F::Output {
//! #     let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
//! #     let mut context = Context::from_waker(&waker);
//! #     let mut future = std::pin::pin!(future);
//! #     loop {
//! #         match future.as_mut().poll(&mut context) {
//! #             Poll::Ready(output) => return output,
//! #             Poll::Pending => std::thread::park(),
//! #         }
//! #     }
//! # }
//! # fn run() -> Result<(), Box<dyn std::error::Error>> {
//! let bytes = std::fs::read(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/../../examples/wasi-component-args.wasm",
//! ))?;
//! let engine = ComponentEngine::new();
//! let program = engine.compile(&bytes)?;
//! let state = WasiState::builder().args(["guest", "one"]).build();
//! let mut linker = ComponentLinker::new();
//! add_to_linker_sync(&mut linker, state.clone())?;
//! let store = Store::new();
//! let instance = block_on(engine.instantiate(&program, &store, &linker))?;
//! let outcome = block_on(bindings::Exports::new(instance).wasi_cli_run().run(&store))?;
//! assert!(outcome.is_ok());
//! assert_eq!(state.exit_code(), None);
//! # Ok(())
//! # }
//! # run().unwrap();
//! ```

#![warn(missing_docs)]

mod provider;
mod state;

/// Adds asynchronous WASI host implementations to a component linker.
#[doc(inline)]
pub use provider::add_to_linker_async;
/// Adds synchronous WASI host implementations to a component linker.
#[doc(inline)]
pub use provider::add_to_linker_sync;
#[doc(inline)]
pub use state::{WasiState, WasiStateBuilder};

/// The WASI WIT revision targeted by this crate's bundled bindings.
///
/// This constant does not claim complete function-level support; consult the
/// support matrix for the implemented coverage of each interface.
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
