use crate::ir::{ExportName, ExternDesc};

// Retained conservatively; declaration IR is not materialized by the current decoder.
#[allow(dead_code)]
pub struct ExportDecl {
    pub name: ExportName,
    pub desc: ExternDesc,
}
