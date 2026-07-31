//! Proc macro that generates telomere component bindings from WIT.
//!
//! The [`bindgen!`] macro parses a WIT world at macro-expansion time and emits
//! the host trait definitions, linker registration helpers, and typed export
//! wrappers that `telomere-component` needs. It is the compile-time half of the
//! WASI provider; nothing from this crate is present at runtime.

mod generator;

use proc_macro::TokenStream;

#[proc_macro]
pub fn bindgen(input: TokenStream) -> TokenStream {
    match syn::parse::<generator::BindgenInput>(input).and_then(generator::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
