use crate::component::ir::{ExportName, ExternDesc};

pub struct ExportDecl {
    pub name: ExportName,
    pub desc: ExternDesc,
}
