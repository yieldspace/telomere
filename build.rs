use proc_macro2::TokenStream;
use quote::{format_ident, quote, IdentFragment, ToTokens};
use syn::Ident;

fn generate_vm_unary_op(
    target: &[Ident],
    op_name: impl IdentFragment,
    closure: impl ToTokens,
) -> impl ToTokens {
    let mut name_type_pair = quote! {};
    for target in target[..(target.len() - 1)].iter() {
        let mangled = format_ident!("{}_{}", target, op_name);
        name_type_pair.extend(quote! {
          (#mangled,#target),
        });
    }
    let target = target.last().unwrap();
    let mangled = format_ident!("{}_{}", target, op_name);
    name_type_pair.extend(quote! {
      (#mangled,#target)
    });
    let code = quote::quote! {
      impl_unary_op!([#name_type_pair], #closure);
    };
    code
}
macro_rules! define_simd_operation {
    ($name: ident,[$($target: ident),*], $closure: expr) => {
        (format_ident!("{}", stringify!($name)),vec![$(format_ident!("{}",stringify!($target))),*] ,$closure)
    };
}
fn unary_name_op_pairs() -> Vec<(Ident, Vec<Ident>, TokenStream)> {
    vec![
        define_simd_operation!(add, [i8x16, i32x4, i64x2], quote! { |a, b| a + b }),
        define_simd_operation!(sub, [i8x16, i32x4], quote! { |a, b| a - b }),
        define_simd_operation!(mul, [f32x4, i32x4], quote! { |a, b| a * b }),
        define_simd_operation!(div, [f32x4], quote! { |a, b| a / b }),
        define_simd_operation!(swizzle, [i8x16], quote! { |a, b| a.swizzle(b) }),
        define_simd_operation!(min, [i8x16, u8x16, f32x4], quote! { |a, b| a.min(b) }), // FIXME: nan behaviour
        define_simd_operation!(max, [i8x16, u8x16, f32x4], quote! { |a, b| a.max(b) }), // FIXME: nan behaviour
        define_simd_operation!(pmin, [f32x4], quote! { |a, b| a.max(b) }),
        define_simd_operation!(pmax, [f32x4], quote! { |a, b| a.max(b) }),
    ]
}
fn generate_vm_op() -> String {
    let mut unary_ops = quote! {};
    for (name, types, closure) in unary_name_op_pairs() {
        let op = generate_vm_unary_op(&types, name, closure);
        unary_ops.extend(op.into_token_stream());
    }
    let code = quote::quote! {
      #unary_ops
      impl_binary_op!([(f32x4_abs, f32x4), (i32x4_abs, i32x4)], |a| a.abs());
    };
    code.to_string()
}
fn write_to_file(file_path: &str, content: &str) {
    let mut file = std::fs::File::create(file_path).expect("Unable to create file");
    use std::io::Write;
    file.write_all(content.as_bytes())
        .expect("Unable to write data");
}
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    write_to_file("src/runtime/vm/simd_generated.rs", &generate_vm_op());
}
