use proc_macro2::TokenStream;
use quote::format_ident;
use quote::quote;
use quote::IdentFragment;
use quote::ToTokens;
use syn::bracketed;
use syn::parse::Parse;
use syn::parse_macro_input;
use syn::ExprClosure;
use syn::Ident;

struct DefineSimdOperationInput {
    handler: Ident,
    operation_name: Ident,
    target_types: Vec<Ident>,
    closure: ExprClosure,
}
impl Parse for DefineSimdOperationInput {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        let handler: Ident = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let operation_name: Ident = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let content;
        let _ = bracketed!(content in input);
        let target_types = content
            .parse_terminated(Ident::parse, syn::Token![,])?
            .into_iter()
            .collect();
        input.parse::<syn::Token![,]>()?;

        let closure = ExprClosure::parse(input)?;
        Ok(Self {
            handler,
            closure,
            operation_name,
            target_types,
        })
    }
}
fn generate_vm_unary_op(
    handler: Ident,
    target: &[Ident],
    op_name: impl IdentFragment,
    closure: impl ToTokens,
) -> TokenStream {
    let mut mangled_names = vec![];
    for target in target.iter() {
        let mangled = format_ident!("{}_{}", target, op_name);
        mangled_names.push((mangled, target));
    }
    let mut stream = TokenStream::new();
    for (mangled, target) in mangled_names {
        stream.extend( quote! {
          pub unsafe fn #mangled(tail_code: *const Instr, ctx: &mut ExecuteContext) -> VMResult<()> {
              #handler::<#target>(tail_code, ctx, #closure)
          }
        });
    }
    stream
}

#[proc_macro]
pub fn define_simd_operation(stream: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let input = parse_macro_input!(stream as DefineSimdOperationInput);
    let code = generate_vm_unary_op(
        input.handler,
        &input.target_types,
        input.operation_name,
        input.closure,
    );
    code.into()
}
