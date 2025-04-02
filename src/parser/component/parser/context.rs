use crate::binary::BinaryReader;
pub struct ParseContext<'a, R: BinaryReader> {
    pub reader: &'a mut R,
}

impl<'a, R> ParseContext<'a, R>
where
    R: BinaryReader,
{
    pub fn new(reader: &'a mut R) -> Self {
        ParseContext { reader }
    }
}
