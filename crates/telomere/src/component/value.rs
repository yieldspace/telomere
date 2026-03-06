#[derive(Clone, Debug, PartialEq)]
pub enum ComponentValue {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl ComponentValue {
    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::I32(v) => Some(*v),
            _ => None,
        }
    }
}
