use crate::binary::BinaryReader;
use crate::parser::core::WasmParserError;

pub type Result<R> = std::result::Result<R, WasmParserError>;

pub(crate) struct Leb128Parser<'a, R: BinaryReader> {
    reader: &'a mut R,
}
macro_rules! parse_leb128impl {
    ($name: ident, $t: ident, $is_signed: expr) => {
        pub fn $name(&mut self, bit_size: usize) -> Result<(usize, $t)> {
            let mut result: $t = 0;
            let mut read_bytes: usize = 0;
            let mut byte: u8;

            loop {
                byte = self.reader.read_exact_one()?;
                // Extract the lower 7 bits and shift them into the result.
                result |= ((byte & 0x7F) as $t) << (read_bytes * 7);
                read_bytes += 1;

                // If the most significant bit is not set, this is the final byte.
                if byte & 0x80 == 0 {
                    break;
                }

                // Check if the shift amount exceeds or equals the specified bit size.
                if (read_bytes * 7) >= bit_size {
                    return Err(WasmParserError::InvalidLeb128Encoding);
                }
            }

            // For signed numbers, perform sign extension if the sign bit (0x40) is set.
            if $is_signed && (read_bytes * 7) < bit_size && (byte & 0x40) != 0 {
                result |= (!0 as $t) << read_bytes * 7;
            }

            Ok((read_bytes, result))
        }
    };
    ($name: ident, $t: ident) => {
        parse_leb128impl!($name, $t, false);
    };
}
impl<'a, R: BinaryReader> Leb128Parser<'a, R> {
    pub fn new(reader: &'a mut R) -> Self {
        Self { reader }
    }
    parse_leb128impl!(parse_u32, u32);
    parse_leb128impl!(parse_i32, i32, true);
    parse_leb128impl!(parse_i64, i64, true);
}
