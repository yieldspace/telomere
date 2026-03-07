use crate::ir::{ExportName, ExternDesc};

pub struct ExportDecl {
    pub name: ExportName,
    pub desc: ExternDesc,
}
