use crate::error::{FormatError, Result};
use crate::identifier::read_identifier;
use crate::tlv::{walk_tlvs, Tlv};
use crate::varint::{read_be_i32, read_be_u32, read_varint};

pub const MAGIC: u32 = 0xD780E884;
pub const MAGIC_BYTES: [u8; 4] = [0xD7, 0x80, 0xE8, 0x84];

#[derive(Debug, Clone)]
pub struct ParsedChunk {
    pub file_name: String,
    pub total_bytes: usize,
    pub magic: u32,
    pub action_count: i32,
    pub action_table: Vec<String>, // index = local_id
    pub snapshot_size: i32,
    pub snapshot_offset: usize,
    pub actions_offset: usize,
    pub snapshot_tlvs: Vec<Tlv>,
    pub replay_tlvs: Vec<Tlv>,
}

impl ParsedChunk {
    /// Resolve local_id -> identifier string, or None if out of bounds.
    pub fn resolve(&self, local_id: i32) -> Option<&str> {
        if local_id < 0 {
            return None;
        }
        let idx = local_id as usize;
        self.action_table.get(idx).map(|s| s.as_str())
    }

    /// Find local_id for a given identifier, or None if not present.
    pub fn find_id(&self, identifier: &str) -> Option<i32> {
        self.action_table
            .iter()
            .position(|s| s == identifier)
            .map(|i| i as i32)
    }

    /// Count TLVs whose identifier matches the given string.
    pub fn count_by_identifier(&self, identifier: &str) -> usize {
        if let Some(target_id) = self.find_id(identifier) {
            self.snapshot_tlvs
                .iter()
                .chain(self.replay_tlvs.iter())
                .filter(|t| t.local_id == target_id)
                .count()
        } else {
            0
        }
    }

    pub fn count_replay_by_identifier(&self, identifier: &str) -> usize {
        if let Some(target_id) = self.find_id(identifier) {
            self.replay_tlvs
                .iter()
                .filter(|t| t.local_id == target_id)
                .count()
        } else {
            0
        }
    }

    pub fn count_snapshot_by_identifier(&self, identifier: &str) -> usize {
        if let Some(target_id) = self.find_id(identifier) {
            self.snapshot_tlvs
                .iter()
                .filter(|t| t.local_id == target_id)
                .count()
        } else {
            0
        }
    }
}

pub fn parse_chunk_bytes(data: &[u8], file_name: &str) -> Result<ParsedChunk> {
    if data.len() < 4 {
        return Err(FormatError::new(format!(
            "chunk {} too small for MAGIC: len {}",
            file_name,
            data.len()
        ))
        .with_offset(0));
    }
    let magic = read_be_u32(data, 0)?;
    if magic != MAGIC {
        return Err(FormatError::new(format!(
            "chunk {} MAGIC mismatch: expected 0x{:08X} got 0x{:08X} (bytes {:02X?})",
            file_name,
            MAGIC,
            magic,
            &data[0..4]
        ))
        .with_offset(0)
        .with_context(format!(
            "big-endian check: LE would be 0x{:08X}",
            u32::from_le_bytes([data[0], data[1], data[2], data[3]])
        )));
    }

    // action count at offset 4
    let (action_count, varint_len) = read_varint(data, 4).map_err(|e| {
        FormatError::new(format!("failed to read action count: {}", e.message))
            .with_offset(4)
            .with_context(format!("chunk {}", file_name))
    })?;
    if action_count < 0 {
        return Err(
            FormatError::new(format!("action count negative: {}", action_count)).with_offset(4),
        );
    }
    if action_count > 1024 {
        return Err(
            FormatError::new(format!("action count absurdly large: {}", action_count))
                .with_offset(4),
        );
    }

    let mut offset = 4 + varint_len;
    let mut action_table = Vec::with_capacity(action_count as usize);
    for i in 0..action_count {
        let (ident, consumed) = read_identifier(data, offset).map_err(|e| {
            FormatError::new(format!(
                "failed to read action table entry {}: {}",
                i, e.message
            ))
            .with_offset(offset)
            .with_context(format!("table index {}", i))
        })?;
        // Validate identifier shape contains colon and flashback prefix ideally
        if !ident.contains(':') {
            return Err(FormatError::new(format!(
                "identifier missing colon: '{}' at index {}",
                ident, i
            ))
            .with_offset(offset));
        }
        action_table.push(ident);
        offset += consumed;
    }

    // snapshotSize BE i32
    if offset + 4 > data.len() {
        return Err(FormatError::new(format!(
            "chunk {} truncated before snapshotSize at offset {} len {}",
            file_name,
            offset,
            data.len()
        ))
        .with_offset(offset));
    }
    let snapshot_size = read_be_i32(data, offset)?;
    if snapshot_size < 0 {
        return Err(FormatError::new(format!(
            "snapshotSize negative {} (0x{:08X}) — maybe DEADBEEF sentinel leaked?",
            snapshot_size, snapshot_size as u32
        ))
        .with_offset(offset)
        .with_context(format!(
            "snapshotSize bytes {:02X?}",
            &data[offset..offset + 4]
        )));
    }
    // Sentinel check: if writer had not overwritten DEADBEEF
    if snapshot_size as u32 == 0xDEADBEEF {
        return Err(
            FormatError::new("snapshotSize is DEADBEEF sentinel — snapshot not closed")
                .with_offset(offset),
        );
    }

    let snapshot_offset = offset + 4;
    let actions_offset = snapshot_offset + snapshot_size as usize;

    if snapshot_offset > data.len() {
        return Err(FormatError::new(format!(
            "snapshot_offset {} > len {}",
            snapshot_offset,
            data.len()
        ))
        .with_offset(snapshot_offset));
    }
    if actions_offset > data.len() {
        return Err(FormatError::new(format!(
            "actions_offset {} (snapshot_offset {} + snapshot_size {}) > len {} — snapshotSize truncated",
            actions_offset, snapshot_offset, snapshot_size, data.len()
        ))
        .with_offset(snapshot_offset)
        .with_context(format!(
            "snapshot_size {} payload end {} vs len {}",
            snapshot_size, actions_offset, data.len()
        )));
    }

    // Walk snapshot TLVs
    let snapshot_tlvs = walk_tlvs(data, snapshot_offset, actions_offset).map_err(|e| {
        FormatError::new(format!("snapshot TLV walk failed: {}", e.message))
            .with_offset(e.offset.unwrap_or(snapshot_offset))
            .with_context(format!(
                "snapshot range {}..{} size {}",
                snapshot_offset, actions_offset, snapshot_size
            ))
    })?;

    // Walk replay TLVs
    let replay_tlvs = walk_tlvs(data, actions_offset, data.len()).map_err(|e| {
        FormatError::new(format!("replay TLV walk failed: {}", e.message))
            .with_offset(e.offset.unwrap_or(actions_offset))
            .with_context(format!(
                "replay range {}..{} len {}",
                actions_offset,
                data.len(),
                data.len()
            ))
    })?;

    // Verify that all action ids are within table bounds (or warn)
    for tlv in snapshot_tlvs.iter().chain(replay_tlvs.iter()) {
        if tlv.local_id < 0 || (tlv.local_id as usize) >= action_table.len() {
            return Err(FormatError::new(format!(
                "TLV at offset {} has out-of-bounds local_id {} (table size {})",
                tlv.header_offset,
                tlv.local_id,
                action_table.len()
            ))
            .with_offset(tlv.header_offset)
            .with_context(format!("identifier table len {}", action_table.len())));
        }
    }

    Ok(ParsedChunk {
        file_name: file_name.to_string(),
        total_bytes: data.len(),
        magic,
        action_count,
        action_table,
        snapshot_size,
        snapshot_offset,
        actions_offset,
        snapshot_tlvs,
        replay_tlvs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_validation() {
        let mut data = Vec::new();
        data.extend_from_slice(&MAGIC_BYTES);
        data.push(0x00); // action count 0
        data.extend_from_slice(&0i32.to_be_bytes()); // snapshot size 0
        let parsed = parse_chunk_bytes(&data, "c0.flashback").unwrap();
        assert_eq!(parsed.magic, MAGIC);
        assert_eq!(parsed.action_count, 0);
        assert_eq!(parsed.snapshot_size, 0);
    }

    #[test]
    fn rejects_wrong_magic() {
        let data = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let err = parse_chunk_bytes(&data, "c0.flashback").unwrap_err();
        assert!(err.message.contains("MAGIC mismatch"));
    }

    #[test]
    fn rejects_deadbeef() {
        let mut data = Vec::new();
        data.extend_from_slice(&MAGIC_BYTES);
        data.push(0x01);
        data.push(0x03); // len 3 for "a:b"
        data.extend_from_slice(b"a:b");
        data.extend_from_slice(&0xDEADBEEFu32.to_be_bytes());
        let err = parse_chunk_bytes(&data, "c0.flashback").unwrap_err();
        eprintln!("err: {}", err.message);
        assert!(
            err.message.contains("DEADBEEF") || err.message.contains("negative"),
            "msg: {}",
            err.message
        );
    }
}
