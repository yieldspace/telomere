use crate::binary::BinaryReader;
use crate::parser::leb128::Leb128Parser;
use crate::WasmParserError;
use tracing::trace;

pub type Result<R> = std::result::Result<R, WasmParserError>;

pub fn parse_u32<R: BinaryReader>(reader: &mut R) -> Result<(usize, u32)> {
    Leb128Parser::new(reader).parse_u32(std::mem::size_of::<u32>() * 8)
}

pub fn parse_i32<R: BinaryReader>(reader: &mut R) -> Result<(usize, i32)> {
    Leb128Parser::new(reader).parse_i32(std::mem::size_of::<i32>() * 8)
}

pub fn parse_i64<R: BinaryReader>(reader: &mut R) -> Result<(usize, i64)> {
    Leb128Parser::new(reader).parse_i64(std::mem::size_of::<i64>() * 8)
}

pub fn parse_f32<R: BinaryReader>(reader: &mut R) -> Result<(usize, f32)> {
    let v = reader.read_exact::<4>()?;
    Ok((4, f32::from_le_bytes(v)))
}

pub fn parse_f64<R: BinaryReader>(reader: &mut R) -> Result<(usize, f64)> {
    let v = reader.read_exact::<8>()?;
    Ok((8, f64::from_le_bytes(v)))
}

pub fn parse_vec<A, R: BinaryReader, F, G, V, E>(
    env: &mut A,
    reader: F,
    mut f: G,
) -> std::result::Result<(usize, Vec<V>), E>
where
    E: From<WasmParserError>,
    F: for<'b> FnOnce(&'b mut A) -> &'b mut R,
    G: for<'b> FnMut(&'b mut A) -> std::result::Result<(usize, V), E>,
{
    let mut read_bytes = 0;
    let r = reader(env);
    let (len_len, len) = parse_u32(r)?;
    trace!("parse_vec: {len_len} {len}");
    read_bytes += len_len;
    let mut result = Vec::new();
    for _i in 0..len {
        let (len, v) = f(env)?;
        result.push(v);
        read_bytes += len;
    }
    Ok((read_bytes, result))
}

pub fn parse_byte<R: BinaryReader>(reader: &mut R) -> Result<(usize, u8)> {
    Ok((1, reader.read_exact_one()?))
}

pub fn parse_name<R: BinaryReader>(reader: &mut R) -> Result<(usize, String)> {
    let (len, name) = parse_vec(reader, |v| v, parse_byte)?;
    Ok((
        len,
        String::from_utf8(name).map_err(|_| WasmParserError::InvalidNameEncoding)?,
    ))
}
