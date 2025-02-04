use std::fs::File;
use std::io;
use std::io::{BufReader, Read};
use crate::binary::reader::BinaryReader;

pub struct FileBinaryReader {
    reader: BufReader<File>
}

impl FileBinaryReader {
    pub fn new(file: File) -> FileBinaryReader {
        Self {
            reader: BufReader::new(file)
        }
    }
}

impl BinaryReader for FileBinaryReader {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])> {
        let mut buf = [0u8; N];
        let r = self.reader.read(&mut buf)?;
        Ok((r, buf))
    }

    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        let mut buf = [0u8; N];
        self.reader.read_exact(&mut buf)?;
        Ok(buf)
    }

    fn read_exact_one(&mut self) -> io::Result<u8> {
        let buf = self.read_exact::<1>()?;
        Ok(buf[0])
    }
}
