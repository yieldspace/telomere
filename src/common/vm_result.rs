#[derive(Debug)]
#[must_use]
pub enum VMResult<V> {
    Success(V),
    Unreachable,
    StackOverflow,
    MemoryIndexOutOfRange,
    TableIndexOutOfRange,
    CallIndirectInvalidType,
    TableUninitialized,
}

macro_rules! vm_try {
    ($expr: expr) => {
        match $expr {
            VMResult::Success(v) => v,
            VMResult::Unreachable => return VMResult::Unreachable,
            VMResult::StackOverflow => return VMResult::StackOverflow,
            VMResult::MemoryIndexOutOfRange => return VMResult::MemoryIndexOutOfRange,
            VMResult::TableIndexOutOfRange => return VMResult::TableIndexOutOfRange,
            VMResult::CallIndirectInvalidType => return VMResult::CallIndirectInvalidType,
            VMResult::TableUninitialized => return VMResult::TableUninitialized,
        }
    };
}
impl<V> VMResult<V> {
    pub fn from_option(opt: Option<V>, err: impl FnOnce() -> VMResult<V>) -> VMResult<V> {
        match opt {
            Some(v) => VMResult::Success(v),
            None => err(),
        }
    }
    pub fn unwrap(self) -> V {
        if let VMResult::Success(v) = self {
            return v;
        }
        panic!()
    }
    pub fn is_err(&self) -> bool {
        !matches!(self, VMResult::Success(_))
    }
}