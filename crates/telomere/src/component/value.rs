#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    Bool(bool),
    U8(u8),
    S8(i8),
    U16(u16),
    S16(i16),
    U32(u32),
    S32(i32),
    U64(u64),
    S64(i64),
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    Char(char),
    String(String),
    List(Vec<ComponentValue>),
    Record(Vec<(String, ComponentValue)>),
    Tuple(Vec<ComponentValue>),
    Variant {
        case: String,
        value: Option<Box<ComponentValue>>,
    },
    Enum(String),
    Flags(Vec<String>),
    Option(Option<Box<ComponentValue>>),
    Result {
        ok: Option<Box<ComponentValue>>,
        err: Option<Box<ComponentValue>>,
    },
    Own(u32),
    Borrow(u32),
}

impl ComponentValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(v) => Some(*v),
            Self::Own(v) => Some(*v),
            Self::Borrow(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(v) => Some(*v),
            Self::S32(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }
}
