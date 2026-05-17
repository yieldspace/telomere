#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsmError {
    InvalidRegister,
    InvalidImmediate,
    BranchOutOfRange,
}

impl From<AsmError> for () {
    fn from(error: AsmError) -> Self {
        if std::env::var_os("TELOMERE_JIT_TRACE_COMPILE").is_some() {
            eprintln!("[telomere-jit] assembler error: {error:?}");
        }
    }
}

pub type AsmResult<T = ()> = Result<T, AsmError>;
