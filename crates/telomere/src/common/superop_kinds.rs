#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum I32ScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    And = 3,
    Or = 4,
    Xor = 5,
    Shl = 6,
    ShrS = 7,
    ShrU = 8,
    DivS = 9,
    DivU = 10,
    RemS = 11,
    RemU = 12,
}

impl I32ScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::And,
            4 => Self::Or,
            5 => Self::Xor,
            6 => Self::Shl,
            7 => Self::ShrS,
            8 => Self::ShrU,
            9 => Self::DivS,
            10 => Self::DivU,
            11 => Self::RemS,
            12 => Self::RemU,
            _ => unreachable!("invalid I32ScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum I64ScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    And = 3,
    Or = 4,
    Xor = 5,
    Shl = 6,
    ShrS = 7,
    ShrU = 8,
    DivS = 9,
    DivU = 10,
    RemS = 11,
    RemU = 12,
}

impl I64ScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::And,
            4 => Self::Or,
            5 => Self::Xor,
            6 => Self::Shl,
            7 => Self::ShrS,
            8 => Self::ShrU,
            9 => Self::DivS,
            10 => Self::DivU,
            11 => Self::RemS,
            12 => Self::RemU,
            _ => unreachable!("invalid I64ScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatScalarKind {
    Add = 0,
    Sub = 1,
    Mul = 2,
    Div = 3,
}

impl FloatScalarKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Add,
            1 => Self::Sub,
            2 => Self::Mul,
            3 => Self::Div,
            _ => unreachable!("invalid FloatScalarKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntCompareKind {
    Eq = 0,
    Ne = 1,
    LtS = 2,
    LtU = 3,
    GtS = 4,
    GtU = 5,
    LeS = 6,
    LeU = 7,
    GeS = 8,
    GeU = 9,
}

impl IntCompareKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::LtS,
            3 => Self::LtU,
            4 => Self::GtS,
            5 => Self::GtU,
            6 => Self::LeS,
            7 => Self::LeU,
            8 => Self::GeS,
            9 => Self::GeU,
            _ => unreachable!("invalid IntCompareKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FloatCompareKind {
    Eq = 0,
    Ne = 1,
    Lt = 2,
    Gt = 3,
    Le = 4,
    Ge = 5,
}

impl FloatCompareKind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::Eq,
            1 => Self::Ne,
            2 => Self::Lt,
            3 => Self::Gt,
            4 => Self::Le,
            5 => Self::Ge,
            _ => unreachable!("invalid FloatCompareKind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load4Kind {
    I32 = 0,
    I32Load8S = 1,
    I32Load8U = 2,
    I32Load16S = 3,
    I32Load16U = 4,
    F32 = 5,
}

impl Load4Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I32,
            1 => Self::I32Load8S,
            2 => Self::I32Load8U,
            3 => Self::I32Load16S,
            4 => Self::I32Load16U,
            5 => Self::F32,
            _ => unreachable!("invalid Load4Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Load8Kind {
    I64 = 0,
    I64Load8S = 1,
    I64Load8U = 2,
    I64Load16S = 3,
    I64Load16U = 4,
    I64Load32S = 5,
    I64Load32U = 6,
    F64 = 7,
}

impl Load8Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I64,
            1 => Self::I64Load8S,
            2 => Self::I64Load8U,
            3 => Self::I64Load16S,
            4 => Self::I64Load16U,
            5 => Self::I64Load32S,
            6 => Self::I64Load32U,
            7 => Self::F64,
            _ => unreachable!("invalid Load8Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store4Kind {
    I32 = 0,
    I32Store8 = 1,
    I32Store16 = 2,
    F32 = 3,
}

impl Store4Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I32,
            1 => Self::I32Store8,
            2 => Self::I32Store16,
            3 => Self::F32,
            _ => unreachable!("invalid Store4Kind: {raw}"),
        }
    }
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Store8Kind {
    I64 = 0,
    I64Store8 = 1,
    I64Store16 = 2,
    I64Store32 = 3,
    F64 = 4,
}

impl Store8Kind {
    #[inline(always)]
    pub(crate) fn from_raw(raw: u32) -> Self {
        match raw {
            0 => Self::I64,
            1 => Self::I64Store8,
            2 => Self::I64Store16,
            3 => Self::I64Store32,
            4 => Self::F64,
            _ => unreachable!("invalid Store8Kind: {raw}"),
        }
    }
}
