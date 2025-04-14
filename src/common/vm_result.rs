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
    Unlinkable,
    InvalidOperand,
}
#[macro_export]
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
            VMResult::Unlinkable => return VMResult::Unlinkable,
            VMResult::InvalidOperand => return VMResult::InvalidOperand,
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
        match self {
            VMResult::Success(v) => v,
            VMResult::Unreachable => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Unreachable",)
            }
            VMResult::StackOverflow => {
                panic!("called `VMResult::unwrap()` on an `Err` value: StackOverflow",)
            }
            VMResult::MemoryIndexOutOfRange => {
                panic!("called `VMResult::unwrap()` on an `Err` value: MemoryIndexOutOfRange",)
            }
            VMResult::TableIndexOutOfRange => {
                panic!("called `VMResult::unwrap()` on an `Err` value: TableIndexOutOfRange",)
            }
            VMResult::CallIndirectInvalidType => {
                panic!("called `VMResult::unwrap()` on an `Err` value: CallIndirectInvalidType",)
            }
            VMResult::TableUninitialized => {
                panic!("called `VMResult::unwrap()` on an `Err` value: TableUninitialized")
            }
            VMResult::Unlinkable => {
                panic!("called `VMResult::unwrap()` on an `Err` value: Unlinkable")
            }
            VMResult::InvalidOperand => {
                panic!("called `VMResult::unwrap()` on an `Err` value: InvalidOperand")
            }
        }
    }
    pub fn is_err(&self) -> bool {
        !matches!(self, VMResult::Success(_))
    }
}
