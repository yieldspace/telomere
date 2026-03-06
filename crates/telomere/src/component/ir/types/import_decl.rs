use crate::component::ir::{ExternDesc, ImportName};

pub struct ImportDecl {
    pub name: ImportName,
    pub desc: ExternDesc,
}

impl ImportDecl {
    pub(crate) fn new(name: ImportName, desc: ExternDesc) -> ImportDecl {
        ImportDecl { name, desc }
    }
}
