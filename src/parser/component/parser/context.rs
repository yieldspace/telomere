use crate::binary::BinaryReader;
use crate::parser::component::sort::SortMap;

pub struct ParseContext<'a, 'b, R: BinaryReader> {
    pub reader: &'a mut R,
    pub sort: SortMap<'b>,
}

impl<'a, 'b, R> ParseContext<'a, 'b, R>
where
    R: BinaryReader,
{
    pub fn new(reader: &'a mut R, sort: SortMap<'b>) -> Self {
        ParseContext { reader, sort }
    }
}
