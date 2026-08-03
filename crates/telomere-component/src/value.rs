/// A dynamically represented Component Model value.
///
/// Use this type when a component signature is only known at runtime. For a
/// compile-time checked interface, prefer generated bindings or
/// [`crate::ComponentInstance::get_typed_func`].
#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    /// A Component Model `bool`.
    Bool(bool),
    /// An unsigned 8-bit integer.
    U8(u8),
    /// A signed 8-bit integer.
    S8(i8),
    /// An unsigned 16-bit integer.
    U16(u16),
    /// A signed 16-bit integer.
    S16(i16),
    /// An unsigned 32-bit integer.
    U32(u32),
    /// A signed 32-bit integer.
    S32(i32),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// A signed 64-bit integer.
    S64(i64),
    /// A core WebAssembly `i32`, retained for canonical ABI interoperability.
    I32(i32),
    /// A core WebAssembly `i64`, retained for canonical ABI interoperability.
    I64(i64),
    /// A 32-bit IEEE floating-point number.
    F32(f32),
    /// A 64-bit IEEE floating-point number.
    F64(f64),
    /// A Unicode scalar value.
    Char(char),
    /// A UTF-8 string.
    String(String),
    /// A homogeneous Component Model list.
    List(Vec<ComponentValue>),
    /// A record represented by ordered field-name/value pairs.
    Record(Vec<(String, ComponentValue)>),
    /// A positional tuple.
    Tuple(Vec<ComponentValue>),
    /// A discriminated union case and its optional payload.
    Variant {
        /// The selected case label.
        case: String,
        /// The payload for cases that carry one.
        value: Option<Box<ComponentValue>>,
    },
    /// A payload-free enum case label.
    Enum(String),
    /// The labels selected in a flags value.
    Flags(Vec<String>),
    /// An optional value, or `None` for the absent case.
    Option(Option<Box<ComponentValue>>),
    /// A success or error payload. Exactly one payload is normally present.
    Result {
        /// The payload of a successful result, when one exists.
        ok: Option<Box<ComponentValue>>,
        /// The payload of an error result, when one exists.
        err: Option<Box<ComponentValue>>,
    },
    /// An owned resource handle.
    Own(u32),
    /// A borrowed resource handle.
    Borrow(u32),
}

impl ComponentValue {
    /// Returns the value when it is a [`Self::Bool`].
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns a lossless 32-bit representation for unsigned and resource values.
    ///
    /// This accepts `u8`, `u16`, `u32`, owned-resource, and borrowed-resource
    /// variants; signed and wider values return `None`.
    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U8(v) => Some(u32::from(*v)),
            Self::U16(v) => Some(u32::from(*v)),
            Self::U32(v) => Some(*v),
            Self::Own(v) => Some(*v),
            Self::Borrow(v) => Some(*v),
            _ => None,
        }
    }

    /// Returns a lossless 32-bit representation for signed integer values.
    ///
    /// This accepts `s8`, `s16`, `s32`, and core `i32` variants.
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::S8(v) => Some(i32::from(*v)),
            Self::S16(v) => Some(i32::from(*v)),
            Self::I32(v) => Some(*v),
            Self::S32(v) => Some(*v),
            _ => None,
        }
    }

    /// Borrows the UTF-8 contents when the value is a [`Self::String`].
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }
}
