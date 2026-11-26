// Unsafe policy (#132, docs/unsafe-policy.md): the parser and
// optimizer read unvalidated input, so this subtree is held
// to the strictest stage of the ratchet: every unsafe
// operation inside an `unsafe fn` must be scoped in an
// explicit inner `unsafe` block with a SAFETY comment.
#![deny(unsafe_op_in_unsafe_fn)]

pub(crate) mod core;
pub(crate) mod leb128;
