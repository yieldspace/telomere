use crate::binary::BinaryReader;
use crate::parser::component::error::ComponentParseError;
use crate::parser::component::ParseContext;
use crate::parser::core::parse_u32;

pub struct VecParser<'ctx, 'a, 'b, T, R, F>
where
    R: BinaryReader,
    F: for<'r> FnMut(&'r mut ParseContext<'a, 'b, R>) -> Result<(usize, T), ComponentParseError>,
{
    size: usize,
    total_read: usize,
    generator: F,
    context_ref: &'ctx mut ParseContext<'a, 'b, R>,
}

impl<'ctx, 'a, 'b, T, R, F> VecParser<'ctx, 'a, 'b, T, R, F>
where
    R: BinaryReader,
    F: for<'r> FnMut(&'r mut ParseContext<'a, 'b, R>) -> Result<(usize, T), ComponentParseError>,
{
    pub fn new(ctx: &'ctx mut ParseContext<'a, 'b, R>, generator: F) -> Result<Self, ComponentParseError> {
        let (len_len, len) = parse_u32(ctx.reader)?;
        Ok(Self {
            size: len as usize,
            total_read: len_len,
            generator,
            context_ref: ctx,
        })
    }
}

impl<'ctx, 'a, 'b, T, R, F> Iterator for VecParser<'ctx, 'a, 'b, T, R, F>
where
    R: BinaryReader,
    F: for<'r> FnMut(&'r mut ParseContext<'a, 'b, R>) -> Result<(usize, T), ComponentParseError>,
{
    type Item = Result<(usize, T), ComponentParseError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.size > 0 {
            // 呼び出し時に self のミュータブル借用のライフタイムが自動的に適用されるため問題なく動作
            let result = (self.generator)(self.context_ref);
            self.size -= 1;
            if let Ok((read, _)) = &result {
                self.total_read += read;
            }
            Some(result)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::component_model::ComponentBuilder;

    #[test]
    fn vecparser_initializes_correctly() {
        use super::*;
        use crate::binary::IoReadBinaryReader;
        use std::io::Cursor;

        let data = [0x04, 0xf, 0xf, 0xf, 0xf]; // u32 length = 4
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut builder = ComponentBuilder::new();
        let mut ctx = ParseContext { reader: &mut reader, builder: &mut builder };

        let mut parser = VecParser::new(&mut ctx, |ctx| {
            Ok((1, ctx.reader.read_exact_one()?))
        }).unwrap();
        assert_eq!(parser.size, 4);
        for data in &mut parser {
            let (size, value) = data.unwrap();
            assert_eq!(value, 0xf);
        }
        assert_eq!(parser.total_read, 5);
    }

    #[test]
    fn vecparser_iterates_correctly() {
        use super::*;
        use crate::binary::IoReadBinaryReader;
        use std::io::Cursor;

        let data = [0x02, 0x00, 0x00]; // u32 length = 2
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut builder = ComponentBuilder::new();
        let mut ctx = ParseContext { reader: &mut reader, builder: &mut builder };

        let mut parser = VecParser::new(&mut ctx, |ctx| Ok((1, 42))).unwrap();
        let results: Vec<_> = parser.collect();

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|res| res.as_ref().unwrap().1 == 42));
    }

    #[test]
    fn vecparser_handles_empty_vector() {
        use super::*;
        use crate::binary::IoReadBinaryReader;
        use std::io::Cursor;

        let data = [0x00, 0x00, 0x00, 0x00]; // u32 length = 0
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut builder = ComponentBuilder::new();
        let mut ctx = ParseContext { reader: &mut reader, builder: &mut builder };

        let mut parser = VecParser::new(&mut ctx, |ctx| Ok((1, 42))).unwrap();
        assert!(parser.next().is_none());
    }

    #[test]
    fn vecparser_propagates_generator_error() {
        use super::*;
        use crate::binary::IoReadBinaryReader;
        use std::io::Cursor;

        let data = [0x01, 0x00]; // u32 length = 1
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut builder = ComponentBuilder::new();
        let mut ctx = ParseContext { reader: &mut reader, builder: &mut builder };

        let mut parser = VecParser::new(&mut ctx, |_| Err(ComponentParseError::InvalidVersion([0, 0]))).unwrap();
        let result: Result<(usize, ()), ComponentParseError> = parser.next().unwrap();

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ComponentParseError::InvalidVersion([0, 0])));
    }
}
