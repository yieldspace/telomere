use crate::binary::{BinaryReader, Counter};
use crate::component_model::ComponentBuilder;
use crate::parser::component::sort::SortMap;

pub struct ParseContext<'a, 'b, R: BinaryReader> {
    pub reader: &'a mut R,
    pub builder: &'b mut ComponentBuilder,
}

impl<'a, 'b, R> ParseContext<'a, 'b, R>
where
    R: BinaryReader,
{
    pub fn new(reader: &'a mut R, builder: &'b mut ComponentBuilder) -> Self {
        ParseContext { reader, builder }
    }

    pub fn start_count(&self) -> usize {
        self.reader.read_count()
    }

    pub fn end_count(&self, start: usize) -> usize {
        self.reader.read_count() - start
    }
}
