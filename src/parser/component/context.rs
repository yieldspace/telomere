use crate::binary::{BinaryReader, Counter};
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

    pub fn start_count(&self) -> usize {
        self.reader.read_count()
    }

    pub fn end_count(&self, start: usize) -> usize {
        self.reader.read_count() - start
    }
}
