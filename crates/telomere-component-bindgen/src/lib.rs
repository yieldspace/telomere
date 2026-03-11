mod generator;

use proc_macro::TokenStream;

#[proc_macro]
pub fn bindgen(input: TokenStream) -> TokenStream {
    match syn::parse::<generator::BindgenInput>(input).and_then(generator::expand) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}
