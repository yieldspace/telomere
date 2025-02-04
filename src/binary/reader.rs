use std::fs::File;
use std::io;
use std::io::{BufReader, Read};

pub trait BinaryReader {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>;
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>;
    fn read_exact_one(&mut self) -> io::Result<u8>;
}
