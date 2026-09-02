use crate::error::{FormatError, Result};
use crate::varint::read_varint;

/// Read an Identifier encoded as VarInt length + UTF-8 bytes.
/// Mirrors FriendlyByteBuf.writeUtf / writeIdentifier (VarInt length + bytes).
pub fn read_identifier(data: &[u8], offset: usize) -> Result<(String, usize)> {
    let (len, varint_len) = read_varint(data, offset)?;
    if len < 0 {
        return Err(
            FormatError::new(format!("Identifier length negative: {}", len))
                .with_offset(offset)
                .with_context("Identifier VarInt length must be >=0"),
        );
    }
    let len = len as usize;
    let start = offset + varint_len;
    let end = start + len;
    if end > data.len() {
        return Err(FormatError::new(format!(
            "Identifier truncated: need {} bytes at {} but len {}",
            len,
            start,
            data.len()
        ))
        .with_offset(offset)
        .with_context(format!("Identifier expected {} bytes", len)));
    }
    let bytes = &data[start..end];
    let s = std::str::from_utf8(bytes).map_err(|e| {
        FormatError::new(format!("Identifier not valid UTF-8: {}", e))
            .with_offset(start)
            .with_context(format!(
                "Identifier bytes: {:02X?}",
                &bytes[..bytes.len().min(32)]
            ))
    })?;
    Ok((s.to_string(), varint_len + len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifier_simple() {
        // "flashback:action/next_tick" len 26 = 0x1A
        let mut data = vec![0x1A];
        data.extend_from_slice(b"flashback:action/next_tick");
        let (s, n) = read_identifier(&data, 0).unwrap();
        assert_eq!(s, "flashback:action/next_tick");
        assert_eq!(n, 1 + 26);
    }

    #[test]
    fn identifier_voice() {
        let name = "flashback:action/simple_voice_chat_sound_optional";
        let len = name.len() as u8;
        let mut data = vec![len];
        data.extend_from_slice(name.as_bytes());
        let (s, n) = read_identifier(&data, 0).unwrap();
        assert_eq!(s, name);
        assert_eq!(n, 1 + name.len());
    }
}
