// Unsafe policy (#132, docs/unsafe-policy.md): this reader
// consumes unvalidated input and currently has no unsafe
// sites; the deny keeps it that way unless a site is added
// under the policy's SAFETY-comment rules.
#![deny(unsafe_op_in_unsafe_fn)]

mod reader;

pub use reader::BinaryReader;
pub use reader::IoReadBinaryReader;
pub use reader::LimitingBinaryReader;
