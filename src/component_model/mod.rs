mod canon;
mod compiled;
mod component;
mod core;
mod func;
mod idx;
mod instance;
mod sort;
mod types;

pub use canon::*;
pub use compiled::{CompiledState, Relation};
pub use component::*;
pub use core::*;
pub use func::*;
pub use idx::*;
pub use instance::*;
pub use sort::*;
pub use types::*;

pub type ExportName = String;
pub type ImportName = String;

#[derive(Debug)]
pub struct InlineExport {
    pub name: String,
    pub sort: SortWithIdx,
}
