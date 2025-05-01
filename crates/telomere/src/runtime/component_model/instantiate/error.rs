use thiserror::Error;

#[derive(Error, Debug)]
pub enum InstantiateError {
    #[error("root-level {0} imports are not supported")]
    UnsupportedToplevelImportError(String),
    #[error("import not found: {0}")]
    ImportNotFound(String),
    #[error("import type mismatch: {0} = {1} (expected {2})")]
    ImportTypeMismatch(String, String, String),
    #[error("export not found: {0}")]
    ExportNotFound(String),
    #[error("export type mismatch: {0} = {1} (expected {2})")]
    ExportTypeMismatch(String, String, String),
    #[error("VMError: {0}")]
    CoreVMError(String),
}
