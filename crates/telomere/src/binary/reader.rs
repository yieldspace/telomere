use std::io::{self, Read};

/// A trait that defines methods for reading binary data.
pub trait BinaryReader {
    /// Reads a slice of bytes into the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to a buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// * `io::Result<usize>` - The number of bytes read, or an error if the read operation fails.
    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize>;

    /// Reads a fixed number of bytes into an array.
    ///
    /// # Type Parameters
    ///
    /// * `N` - The number of bytes to read.
    ///
    /// # Returns
    ///
    /// * `io::Result<(usize, [u8; N])>` - A tuple containing the number of bytes read and the array of read bytes, or an error if the read operation fails.
    fn read<const N: usize>(&mut self) -> io::Result<(usize, [u8; N])>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        let len = self.read_slice(&mut buf)?;
        Ok((len, buf))
    }

    /// Reads exactly `N` bytes into an array.
    ///
    /// # Type Parameters
    ///
    /// * `N` - The number of bytes to read.
    ///
    /// # Returns
    ///
    /// * `io::Result<[u8; N]>` - An array of read bytes, or an error if the read operation fails.
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized;

    /// Reads a single byte and returns it as an `Option<u8>`.
    ///
    /// # Returns
    ///
    /// * `io::Result<Option<u8>>` - An `Option` containing the read byte, or `None` if no byte was read, or an error if the read operation fails.
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

    /// Reads exactly one byte and returns it.
    ///
    /// # Returns
    ///
    /// * `io::Result<u8>` - The read byte, or an error if the read operation fails.
    fn read_exact_one(&mut self) -> io::Result<u8>
    where
        Self: Sized,
    {
        Ok(self.read_exact::<1>()?[0])
    }

    /// Returns the number of bytes read so far.
    ///
    /// # Returns
    ///
    /// * `usize` - The number of bytes read.
    fn read_count(&self) -> usize
    where
        Self: Sized;

    /// A type that represents a limited view of the reader.
    ///
    /// # Type Parameters
    ///
    /// * `'b` - The lifetime of the limited view.
    type Take<'b>: BinaryReader + 'b
    where
        Self: 'b;

    /// Creates a limited view of the reader with the specified byte limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - The maximum number of bytes that can be read from the limited view.
    ///
    /// # Returns
    ///
    /// * `Self::Take<'_>` - A limited view of the reader.
    fn take(&mut self, limit: usize) -> Self::Take<'_>;
}

/// A struct that implements `BinaryReader` for any type that implements `Read`.
pub struct IoReadBinaryReader<R: Read> {
    read: R,
    count: usize,
}

impl<R: Read> BinaryReader for IoReadBinaryReader<R> {
    /// Reads a slice of bytes into the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to a buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// * `io::Result<usize>` - The number of bytes read, or an error if the read operation fails.
    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.read.read(buf)?;
        self.count += bytes_read;
        Ok(bytes_read)
    }

    /// Reads exactly `N` bytes into an array.
    ///
    /// # Type Parameters
    ///
    /// * `N` - The number of bytes to read.
    ///
    /// # Returns
    ///
    /// * `io::Result<[u8; N]>` - An array of read bytes, or an error if the read operation fails.
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized,
    {
        let mut buf = [0u8; N];
        self.read.read_exact(&mut buf)?;
        self.count += N;
        Ok(buf)
    }

    /// Returns the number of bytes read so far.
    ///
    /// # Returns
    ///
    /// * `usize` - The number of bytes read.
    fn read_count(&self) -> usize
    where
        Self: Sized,
    {
        self.count
    }

    /// A type that represents a limited view of the reader.
    ///
    /// # Type Parameters
    ///
    /// * `'b` - The lifetime of the limited view.
    type Take<'b>
        = LimitingBinaryReader<'b, IoReadBinaryReader<R>>
    where
        Self: 'b;

    /// Creates a limited view of the reader with the specified byte limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - The maximum number of bytes that can be read from the limited view.
    ///
    /// # Returns
    ///
    /// * `Self::Take<'_>` - A limited view of the reader.
    fn take(&mut self, limit: usize) -> Self::Take<'_> {
        LimitingBinaryReader::new(self, self.read_count() + limit)
    }
}

impl<R: Read> From<R> for IoReadBinaryReader<R> {
    /// Creates a new `IoReadBinaryReader` from a type that implements `Read`.
    ///
    /// # Arguments
    ///
    /// * `read` - The reader to wrap.
    ///
    /// # Returns
    ///
    /// * `Self` - A new `IoReadBinaryReader`.
    fn from(read: R) -> Self {
        Self { read, count: 0 }
    }
}

/// A struct that implements `BinaryReader` with a byte limit.
pub struct LimitingBinaryReader<'a, R: BinaryReader> {
    reader: &'a mut R,
    limit: usize,
}

impl<'a, R: BinaryReader> LimitingBinaryReader<'a, R> {
    /// Creates a new `LimitingBinaryReader` with the specified byte limit.
    ///
    /// # Arguments
    ///
    /// * `reader` - The underlying reader.
    /// * `limit` - The maximum number of bytes that can be read.
    ///
    /// # Returns
    ///
    /// * `Self` - A new `LimitingBinaryReader`.
    fn new(reader: &'a mut R, limit: usize) -> Self {
        Self { reader, limit }
    }
}

impl<R: BinaryReader> BinaryReader for LimitingBinaryReader<'_, R> {
    /// Reads a slice of bytes into the provided buffer.
    ///
    /// # Arguments
    ///
    /// * `buf` - A mutable reference to a buffer where the read bytes will be stored.
    ///
    /// # Returns
    ///
    /// * `io::Result<usize>` - The number of bytes read, or an error if the read operation fails.
    fn read_slice(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining: usize = self.limit.saturating_sub(self.read_count());
        if remaining == 0 {
            return Ok(0);
        }
        let len = remaining.min(buf.len());

        let buf = &mut buf[..len];
        self.reader.read_slice(buf)
    }

    /// Reads exactly `N` bytes into an array.
    ///
    /// # Type Parameters
    ///
    /// * `N` - The number of bytes to read.
    ///
    /// # Returns
    ///
    /// * `io::Result<[u8; N]>` - An array of read bytes, or an error if the read operation fails.
    fn read_exact<const N: usize>(&mut self) -> io::Result<[u8; N]>
    where
        Self: Sized,
    {
        let remaining: usize = self.limit.saturating_sub(self.read_count());
        if remaining < N {
            io::Result::Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "read limit exceeded",
            ))?
        }
        self.reader.read_exact::<N>()
    }

    /// Returns the number of bytes read so far.
    ///
    /// # Returns
    ///
    /// * `usize` - The number of bytes read.
    fn read_count(&self) -> usize
    where
        Self: Sized,
    {
        self.reader.read_count()
    }

    /// A type that represents a limited view of the reader.
    ///
    /// # Type Parameters
    ///
    /// * `'b` - The lifetime of the limited view.
    type Take<'b>
        = LimitingBinaryReader<'b, R>
    where
        R: 'b,
        Self: 'b;

    /// Creates a limited view of the reader with the specified byte limit.
    ///
    /// # Arguments
    ///
    /// * `limit` - The maximum number of bytes that can be read from the limited view.
    ///
    /// # Returns
    ///
    /// * `Self::Take<'_>` - A limited view of the reader.
    fn take(&mut self, limit: usize) -> Self::Take<'_> {
        let limit = match self.read_count().checked_add(limit) {
            Some(limit) => limit.min(self.limit),
            None => self.limit,
        };
        LimitingBinaryReader::new(self.reader, limit)
    }
}

/// A macro that calculates the number of bytes read within a block of code.
///
/// # Arguments
///
/// * `$reader` - The reader to use.
/// * `$b` - The block of code to execute.
///
/// # Returns
///
/// * `(usize, T)` - A tuple containing the number of bytes read and the result of the block.
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

    #[test]
    fn read_slice_within_limit() {
        use super::*;
        use std::io::Cursor;

        let data = [1, 2, 3, 4, 5];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut limiting_reader = LimitingBinaryReader::new(&mut reader, 3);

        let mut buf = [0u8; 2];
        let bytes_read = limiting_reader.read_slice(&mut buf).unwrap();
        assert_eq!(bytes_read, 2);
        assert_eq!(buf, [1, 2]);
    }

    #[test]
    fn read_exact_within_limit() {
        use super::*;
        use std::io::Cursor;

        let data = [1, 2, 3, 4, 5];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut limiting_reader = LimitingBinaryReader::new(&mut reader, 3);

        let buf = limiting_reader.read_exact::<2>().unwrap();
        assert_eq!(buf, [1, 2]);
    }

    #[test]
    fn read_exact_exceeding_limit() {
        use super::*;
        use std::io::Cursor;

        let data = [1, 2, 3, 4, 5];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));
        let mut limiting_reader = LimitingBinaryReader::new(&mut reader, 3);

        let result = limiting_reader.read_exact::<4>();
        assert!(result.is_err());
    }

    #[test]
    fn nested_take_read_slice_stops_at_parent_limit() {
        use super::*;
        use std::io::Cursor;

        let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        {
            let mut parent = reader.take(10);
            let mut child = parent.take(1000);
            let mut buf = [0xff; 11];

            assert_eq!(child.read_slice(&mut buf).unwrap(), 10);
            assert_eq!(&buf[..10], &data[..10]);
            assert_eq!(buf[10], 0xff);
        }

        assert_eq!(reader.read_count(), 10);
    }

    #[test]
    fn nested_take_read_exact_does_not_cross_parent_limit() {
        use super::*;
        use std::io::{Cursor, ErrorKind};

        let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        {
            let mut parent = reader.take(10);
            let mut child = parent.take(1000);

            let error = child.read_exact::<11>().unwrap_err();
            assert_eq!(error.kind(), ErrorKind::UnexpectedEof);
        }

        assert_eq!(reader.read_count(), 0);
    }

    #[test]
    fn nested_take_preserves_ancestor_limit_at_three_levels() {
        use super::*;
        use std::io::Cursor;

        let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        {
            let mut parent = reader.take(10);
            let mut child = parent.take(1000);
            let mut grandchild = child.take(1000);
            let mut buf = [0; 11];

            assert_eq!(grandchild.read_slice(&mut buf).unwrap(), 10);
            assert_eq!(&buf[..10], &data[..10]);
        }

        assert_eq!(reader.read_count(), 10);
    }

    #[test]
    fn nested_take_preserves_smaller_child_limit() {
        use super::*;
        use std::io::{Cursor, ErrorKind};

        let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        {
            let mut parent = reader.take(10);
            let mut child = parent.take(4);
            let mut buf = [0; 10];

            assert_eq!(child.read_slice(&mut buf).unwrap(), 4);
            assert_eq!(&buf[..4], &data[..4]);
            assert_eq!(
                child.read_exact::<1>().unwrap_err().kind(),
                ErrorKind::UnexpectedEof
            );
        }

        assert_eq!(reader.read_count(), 4);
    }

    #[test]
    fn nested_take_clamps_overflowing_child_limit() {
        use super::*;
        use std::io::Cursor;

        let data = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        let mut reader = IoReadBinaryReader::from(Cursor::new(data));

        {
            let mut parent = reader.take(10);
            let mut child = parent.take(usize::MAX);
            let mut buf = [0; 11];

            assert_eq!(child.read_slice(&mut buf).unwrap(), 10);
            assert_eq!(&buf[..10], &data[..10]);
        }

        assert_eq!(reader.read_count(), 10);
    }
}
