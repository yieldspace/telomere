//! Proc macro that generates telomere component bindings from WIT.
//!
//! The [`bindgen!`] macro parses a WIT world at macro-expansion time and emits
//! the host trait definitions, linker registration helpers, and typed export
//! wrappers that `telomere-component` needs. It is the compile-time half of the
//! WASI provider; nothing from this crate is present at runtime.

mod generator;

use proc_macro::TokenStream;

#[proc_macro]
/// Generates typed Component Model bindings from a WIT world.
///
/// The macro emits Rust representations of WIT types, host traits, linker
/// registration helpers, and typed export wrappers. Use inline WIT for a small
/// self-contained interface or path plus deps for a package on disk.
///
/// # Example
///
/// This component imports a custom math interface and exports a function that
/// forwards to it. The WAT is hidden only to keep the interface and host wiring
/// readable; it is compiled and run as part of the doctest.
///
/// ```
/// use std::rc::Rc;
/// use telomere_component::{ComponentEngine, ComponentError, ComponentLinker, Store};
/// use telomere_component_bindgen::bindgen;
///
/// bindgen!({
///     inline: r#"
///         package ex:demo;
///
///         world demo {
///             import math: interface {
///                 double: func(value: u32) -> u32;
///             }
///             export run: func(value: u32) -> u32;
///         }
///     "#,
///     world: "demo",
///     module: "bindings"
/// });
///
/// struct MathHost;
///
/// impl bindings::imports::math::Host for MathHost {
///     fn double(&self, _store: &Store, value: u32) -> Result<u32, ComponentError> {
///         Ok(value * 2)
///     }
/// }
///
/// # use std::{future::Future, sync::Arc, task::{Context, Poll, Wake, Waker}};
/// # struct ThreadWaker(std::thread::Thread);
/// # impl Wake for ThreadWaker {
/// #     fn wake(self: Arc<Self>) { self.0.unpark(); }
/// #     fn wake_by_ref(self: &Arc<Self>) { self.0.unpark(); }
/// # }
/// # fn block_on<F: Future>(future: F) -> F::Output {
/// #     let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
/// #     let mut context = Context::from_waker(&waker);
/// #     let mut future = std::pin::pin!(future);
/// #     loop {
/// #         match future.as_mut().poll(&mut context) {
/// #             Poll::Ready(output) => return output,
/// #             Poll::Pending => std::thread::park(),
/// #         }
/// #     }
/// # }
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// # let bytes = wat::parse_str(r#"
/// # (component
/// #   (type $math-t (func (param "value" u32) (result u32)))
/// #   (type $math-i (instance (export "double" (func (type $math-t)))))
/// #   (import "math" (instance $math (type $math-i)))
/// #   (alias export $math "double" (func $double))
/// #   (core func $double-lower (canon lower (func $double)))
/// #   (core module $guest-m
/// #     (import "" "double" (func $double (param i32) (result i32)))
/// #     (func (export "run") (param i32) (result i32)
/// #       local.get 0
/// #       call $double)
/// #   )
/// #   (core instance $guest-core
/// #     (instantiate $guest-m
/// #       (with "" (instance (export "double" (func $double-lower))))
/// #     )
/// #   )
/// #   (type $run-t (func (param "value" u32) (result u32)))
/// #   (func (export "run") (type $run-t)
/// #     (canon lift (core func $guest-core "run")))
/// # )
/// # "#)?;
/// let engine = ComponentEngine::new();
/// let program = engine.compile(&bytes)?;
/// let mut linker = ComponentLinker::new();
/// bindings::imports::math::add_to_linker(&mut linker, Rc::new(MathHost));
/// let store = Store::new();
/// let instance = block_on(engine.instantiate(&program, &store, &linker))?;
/// let result = block_on(bindings::Exports::new(instance).run(&store, 21))?;
/// assert_eq!(result, 42);
/// # Ok(())
/// # }
/// # run().unwrap();
/// ```
pub fn bindgen(input: TokenStream) -> TokenStream {
    match syn::parse::<generator::BindgenInput>(input).and_then(generator::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
