#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmError {
    InvalidRegister,
    InvalidImmediate,
    BranchOutOfRange,
}

impl From<AsmError> for () {
    fn from(_: AsmError) -> Self {}
}

pub type AsmResult<T = ()> = Result<T, AsmError>;
