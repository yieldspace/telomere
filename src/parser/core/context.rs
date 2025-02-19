use crate::binary::BinaryReader;

pub struct ParseContext {
    pub reader: Box<dyn BinaryReader>,
}

impl ParseContext {
    pub fn new(reader: Box<dyn BinaryReader>) -> Self {
        Self { reader }
    }
}
