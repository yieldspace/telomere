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
}
pub struct IoReadBinaryReader<R: Read> {
    read: R,
}

impl<R: Read> BinaryReader for IoReadBinaryReader<R> {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        let len = self.read.read(&mut buf)?;
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
}
pub fn new_binary_reader<R: Read>(source: R) -> IoReadBinaryReader<R> {
    IoReadBinaryReader { read: source }
}
