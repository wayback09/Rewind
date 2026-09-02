use crate::error::{FormatError, Result};

/// Read a Minecraft VarInt (signed 32-bit, 7 bits per byte, MSB continuation).
/// Returns (value, bytes_consumed).
/// Mirrors FriendlyByteBuf.readVarInt().
pub fn read_varint(data: &[u8], offset: usize) -> Result<(i32, usize)> {
    let mut num_read: usize = 0;
    let mut result: i32 = 0;
    let mut shift: u32 = 0;

    loop {
        if offset + num_read >= data.len() {
            return Err(FormatError::new(format!(
                "VarInt truncated: need byte {} but data ends at {}",
                num_read,
                data.len()
            ))
            .with_offset(offset + num_read)
            .with_context(format!("reading VarInt at offset {}", offset)));
        }
        let byte = data[offset + num_read];
        let value = (byte & 0x7F) as i32;
        result |= value << shift;

        num_read += 1;
        if num_read > 5 {
            return Err(FormatError::new("VarInt too big (exceeds 5 bytes)")
                .with_offset(offset)
                .with_context("VarInt must fit in 32 bits"));
        }
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
    }
    Ok((result, num_read))
}

/// Read big-endian i32 at offset.
pub fn read_be_i32(data: &[u8], offset: usize) -> Result<i32> {
    if offset + 4 > data.len() {
        return Err(FormatError::new(format!(
            "BE i32 truncated: offset {} +4 > len {}",
            offset,
            data.len()
        ))
        .with_offset(offset));
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    Ok(i32::from_be_bytes(bytes))
}

/// Read big-endian u32 at offset.
pub fn read_be_u32(data: &[u8], offset: usize) -> Result<u32> {
    if offset + 4 > data.len() {
        return Err(FormatError::new(format!(
            "BE u32 truncated: offset {} +4 > len {}",
            offset,
            data.len()
        ))
        .with_offset(offset));
    }
    let bytes = [
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ];
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_zero() {
        let (v, n) = read_varint(&[0x00], 0).unwrap();
        assert_eq!(v, 0);
        assert_eq!(n, 1);
    }

    #[test]
    fn varint_one_byte_max() {
        let (v, n) = read_varint(&[0x09], 0).unwrap();
        assert_eq!(v, 9);
        assert_eq!(n, 1);
    }

    #[test]
    fn varint_two_bytes() {
        // 300 = 0xAC 0x02
        let (v, n) = read_varint(&[0xAC, 0x02], 0).unwrap();
        assert_eq!(v, 300);
        assert_eq!(n, 2);
    }

    #[test]
    fn be_i32() {
        let data = [0x00, 0x02, 0xF3, 0xBF];
        assert_eq!(read_be_i32(&data, 0).unwrap(), 193_471);
        // LE would be different
        let le = i32::from_le_bytes(data);
        assert_ne!(le, 193_471);
    }

    #[test]
    fn magic_be() {
        let data = [0xD7, 0x80, 0xE8, 0x84];
        assert_eq!(read_be_u32(&data, 0).unwrap(), 0xD780E884);
        assert_eq!(read_be_i32(&data, 0).unwrap() as u32, 0xD780E884);
    }
}
