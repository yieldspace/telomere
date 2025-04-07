use std::io::{self, Read};

pub trait BinaryReader {
    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        let len = self.read_slice(&mut buf)?;
        Ok((len, buf))
    }
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized;
    fn read_one(&mut self) -> io::Result<Option<u8>>
    where
        Self: Sized,
    {
        match self.read::<1>()? {
            (0, _value) => Ok(None),
            (1, value) => Ok(Some(value[0])),
            _ => unreachable!(),
        }
    }
    fn read_exact_one(&mut self) -> io::Result<u8>
    where
        Self: Sized,
    {
        Ok(self.read_exact::<1>()?[0])
    }

    fn read_count(&self) -> usize
    where
        Self: Sized;
    type Take<'b>: BinaryReader + 'b
    where
        Self: 'b;
    fn take(&mut self, limit: usize) -> Self::Take<'_>;
}

pub struct IoReadBinaryReader<R: Read> {
    read: R,
    count: usize,
}

impl<R: Read> BinaryReader for IoReadBinaryReader<R> {
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        self.read.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_count(&self) -> usize
    where
        Self: Sized,
    {
        self.count
    }

    type Take<'b>
        = LimitingBinaryReader<'b, IoReadBinaryReader<R>>
    where
        Self: 'b;

    fn take(&mut self, limit: usize) -> Self::Take<'_> {
        LimitingBinaryReader::new(self, self.read_count() + limit)
    }

    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.read.read(buf)?;
        self.count += bytes_read;
        Ok(bytes_read)
    }
}
impl<R: Read> From<R> for IoReadBinaryReader<R> {
    fn from(read: R) -> Self {
        Self { read, count: 0 }
    }
}

pub struct LimitingBinaryReader<'a, R: BinaryReader> {
    reader: &'a mut R,
    limit: usize,
}
impl<'a, R: BinaryReader> LimitingBinaryReader<'a, R> {
    fn new(reader: &'a mut R, limit: usize) -> Self {
        Self { reader, limit }
    }
}
impl<R: BinaryReader> BinaryReader for LimitingBinaryReader<'_, R> {
    fn read_count(&self) -> usize
    where
        Self: Sized,
    {
        self.reader.read_count()
    }

    type Take<'b>
        = LimitingBinaryReader<'b, R>
    where
        R: 'b,
        Self: 'b;
    fn take(&mut self, limit: usize) -> Self::Take<'_> {
        let limit = self.read_count() + limit;
        LimitingBinaryReader::new(self.reader, limit)
    }

    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining: usize = self.limit.saturating_sub(self.read_count());
        if remaining == 0 && !buf.is_empty() {
            io::Result::Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read limit exceeded",
            ))?
        }
        let len = remaining.min(buf.len());

        let buf = &mut buf[..len];
        self.reader.read_slice(buf)
    }

    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized,
    {
        let remaining: usize = self.limit.saturating_sub(self.read_count());
        if remaining < N {
            io::Result::Err(io::Error::new(io::ErrorKind::UnexpectedEof, "take error"))?
        }
        self.reader.read_exact()
    }
}
#[macro_export]
macro_rules! with_count {
    ($reader:expr, $b:block) => {{
        let start_count = $reader.read_count();
        let result = $b;
        let end_count = $reader.read_count();
        let count = end_count - start_count;
        (count, result)
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_count() {
        use super::*;
        use std::io::Cursor;

        let data = [1, 2, 3, 4, 5];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        let (count, _) = with_count!(reader, {
            let _ = reader.read::<2>().unwrap();
            1
        });
        assert_eq!(count, 2);
    }
}
