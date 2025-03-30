use std::io::{self, Read};

pub trait BinaryReader {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>
    where
        Self: Sized;
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
}
pub struct IoReadBinaryReader<R: Read> {
    read: R,
    count: usize,
}

impl<R: Read> BinaryReader for IoReadBinaryReader<R> {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        let len = self.read.read(&mut buf)?;
        self.count += len;
        Ok((len, buf))
    }

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
}
impl<R: Read> From<R> for IoReadBinaryReader<R> {
    fn from(read: R) -> Self {
        Self { read, count: 0 }
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
