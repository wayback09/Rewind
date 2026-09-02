use crate::error::{FormatError, Result};
use crate::varint::{read_be_i32, read_varint};

#[derive(Debug, Clone)]
pub struct Tlv {
    pub local_id: i32,
    pub payload_size: i32,
    pub header_offset: usize,
    pub header_len: usize,
    pub payload_offset: usize,
    pub total_len: usize,
}

impl Tlv {
    pub fn payload_range(&self) -> std::ops::Range<usize> {
        self.payload_offset..self.payload_offset + self.payload_size as usize
    }
}

/// Parse a single TLV at offset. Returns Tlv and next offset (payload end).
pub fn parse_one_tlv(data: &[u8], offset: usize) -> Result<(Tlv, usize)> {
    let start = offset;
    let (local_id, varint_len) = read_varint(data, offset).map_err(|e| {
        FormatError::new(format!("failed to read TLV action id: {}", e.message))
            .with_offset(offset)
            .with_context(format!("TLV header at {}", offset))
    })?;
    let size_offset = offset + varint_len;
    let payload_size = read_be_i32(data, size_offset).map_err(|e| {
        FormatError::new(format!("failed to read TLV payload size: {}", e.message))
            .with_offset(size_offset)
            .with_context(format!("TLV id {} at {}", local_id, offset))
    })?;
    if payload_size < 0 {
        return Err(FormatError::new(format!(
            "TLV payload size negative: {} for id {}",
            payload_size, local_id
        ))
        .with_offset(size_offset)
        .with_context(format!("TLV at offset {}", offset)));
    }
    let header_len = varint_len + 4;
    let payload_offset = size_offset + 4;
    let payload_size_usize = payload_size as usize;
    if payload_offset + payload_size_usize > data.len() {
        return Err(FormatError::new(format!(
            "TLV payload truncated: need {} bytes at {} but data len {} (id {} size {})",
            payload_size_usize,
            payload_offset,
            data.len(),
            local_id,
            payload_size
        ))
        .with_offset(offset)
        .with_context(format!(
            "TLV header_len {} payload_size {}",
            header_len, payload_size
        )));
    }
    let total_len = header_len + payload_size_usize;
    let next = payload_offset + payload_size_usize;
    Ok((
        Tlv {
            local_id,
            payload_size,
            header_offset: start,
            header_len,
            payload_offset,
            total_len,
        },
        next,
    ))
}

/// Walk all TLVs from start..end and return counts and validation.
/// Expects that data[start..end] consists of exactly concatenated TLVs with no leftover.
pub fn walk_tlvs(data: &[u8], start: usize, end: usize) -> Result<Vec<Tlv>> {
    if start > end || end > data.len() {
        return Err(FormatError::new(format!(
            "walk range invalid: start {} end {} len {}",
            start,
            end,
            data.len()
        ))
        .with_offset(start));
    }
    let mut tlvs = Vec::new();
    let mut offset = start;
    while offset < end {
        let (tlv, next) = parse_one_tlv(data, offset)?;
        if next > end {
            return Err(FormatError::new(format!(
                "TLV at {} overruns walk end {} (next {})",
                offset, end, next
            ))
            .with_offset(offset));
        }
        tlvs.push(tlv);
        offset = next;
    }
    if offset != end {
        return Err(FormatError::new(format!(
            "TLV walk leftover: offset {} != end {}",
            offset, end
        ))
        .with_offset(offset));
    }
    Ok(tlvs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tlv_zero_payload() {
        // id 1 (next_tick), size 0
        let data = [0x01, 0x00, 0x00, 0x00, 0x00];
        let (tlv, next) = parse_one_tlv(&data, 0).unwrap();
        assert_eq!(tlv.local_id, 1);
        assert_eq!(tlv.payload_size, 0);
        assert_eq!(next, 5);
    }

    #[test]
    fn tlv_walk_two() {
        // two TLVs: id1 size0, id2 size4 with payload 0x01020304
        let data = [
            0x01, 0x00, 0x00, 0x00, 0x00, // first
            0x02, 0x00, 0x00, 0x00, 0x04, 0x01, 0x02, 0x03, 0x04, // second
        ];
        let tlvs = walk_tlvs(&data, 0, data.len()).unwrap();
        assert_eq!(tlvs.len(), 2);
        assert_eq!(tlvs[0].local_id, 1);
        assert_eq!(tlvs[1].local_id, 2);
        assert_eq!(tlvs[1].payload_size, 4);
    }
}
