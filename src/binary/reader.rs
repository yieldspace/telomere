use std::io;

pub trait BinaryReader {
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])> where Self: Sized;
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]> where Self: Sized;
    fn read_one(&mut self) -> io::Result<Option<u8>> where Self: Sized;
    fn read_exact_one(&mut self) -> io::Result<u8> where Self: Sized;
}
