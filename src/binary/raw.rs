use std::io;
use std::io::{Cursor, Read};
use crate::binary::BinaryReader;

pub struct RawBinaryReader<const SIZE: usize> {
    cursor: Cursor<[u8; SIZE]>,
}

impl<const SIZE: usize> RawBinaryReader<SIZE> {
    pub fn new(binary: [u8; SIZE]) -> Self {
        Self {
            cursor: Cursor::new(binary),
        }
    }
}

impl<const SIZE: usize> BinaryReader for RawBinaryReader<SIZE> {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])> {
        let mut buf = [0u8; N];
        let r = self.cursor.read(&mut buf)?;
        Ok((r, buf))
    }

    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.cursor.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_one(&mut self) -> io::Result<Option<u8>> {
        let (len, buf) = self.read::<1>()?;
        match len {
            0 => Ok(None),
            _ => Ok(Some(buf[0]))
        }
    }

    fn read_exact_one(&mut self) -> io::Result<u8> {
        let buf = self.read_exact::<1>()?;
        Ok(buf[0])
    }
}

#[cfg(test)]
mod tests {
    use crate::binary::BinaryReader;
    use crate::binary::raw::RawBinaryReader;

    #[test]
    fn test_reader() {
        let mut reader = RawBinaryReader::new([1, 2, 3, 4, 5, 6]);
        println!("{:?}", reader.cursor);
        reader.read_exact_one().unwrap();
        println!("{:?}", reader.cursor);
    }
}
