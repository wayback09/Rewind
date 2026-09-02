use crate::error::{FormatError, Result};
use crate::varint::read_be_i32;

#[derive(Debug, Clone)]
pub struct CacheShardInfo {
    pub shard_index: u32,
    pub file_name: String,
    pub entries: usize,
    pub total_bytes: usize,
    pub first_entry_sizes: Vec<i32>,
}

#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub size: i32,
    pub offset: usize,
    pub payload_offset: usize,
}

/// Parse a chunk cache shard: [BE i32 size][payload] repeated.
/// Returns vector of entries and validates no leftover bytes.
pub fn parse_cache_shard(data: &[u8], shard_index: u32) -> Result<CacheShardInfo> {
    let mut offset: usize = 0;
    let mut entries: usize = 0;
    let mut first_sizes = Vec::new();
    while offset < data.len() {
        if offset + 4 > data.len() {
            return Err(FormatError::new(format!(
                "cache shard {} truncated at offset {}: need 4 bytes for size but len {}",
                shard_index,
                offset,
                data.len()
            ))
            .with_offset(offset));
        }
        let size = read_be_i32(data, offset)?;
        if size < 0 {
            return Err(FormatError::new(format!(
                "cache shard {} entry {} has negative size {}",
                shard_index, entries, size
            ))
            .with_offset(offset));
        }
        let size_usize = size as usize;
        let payload_offset = offset + 4;
        if payload_offset + size_usize > data.len() {
            return Err(FormatError::new(format!(
                "cache shard {} entry {} truncated: size {} at {} needs {} but len {}",
                shard_index,
                entries,
                size,
                offset,
                payload_offset + size_usize,
                data.len()
            ))
            .with_offset(offset));
        }
        if first_sizes.len() < 3 {
            first_sizes.push(size);
        }
        offset = payload_offset + size_usize;
        entries += 1;
    }
    if offset != data.len() {
        return Err(FormatError::new(format!(
            "cache shard {} leftover: offset {} != len {}",
            shard_index,
            offset,
            data.len()
        ))
        .with_offset(offset));
    }
    Ok(CacheShardInfo {
        shard_index,
        file_name: format!("level_chunk_caches/{}", shard_index),
        entries,
        total_bytes: data.len(),
        first_entry_sizes: first_sizes,
    })
}

/// Collect entries with their offsets for deeper validation if needed.
pub fn collect_entries(data: &[u8]) -> Result<Vec<CacheEntry>> {
    let mut offset = 0usize;
    let mut out = Vec::new();
    while offset < data.len() {
        let size = read_be_i32(data, offset)?;
        if size < 0 {
            return Err(
                FormatError::new(format!("negative cache entry size {}", size)).with_offset(offset),
            );
        }
        let payload_offset = offset + 4;
        let size_usize = size as usize;
        if payload_offset + size_usize > data.len() {
            return Err(FormatError::new(format!(
                "cache entry truncated at {} size {} len {}",
                offset,
                size,
                data.len()
            ))
            .with_offset(offset));
        }
        out.push(CacheEntry {
            size,
            offset,
            payload_offset,
        });
        offset = payload_offset + size_usize;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_cache() {
        let data: &[u8] = &[];
        let info = parse_cache_shard(data, 0).unwrap();
        assert_eq!(info.entries, 0);
    }

    #[test]
    fn single_entry() {
        let mut data = Vec::new();
        data.extend_from_slice(&4i32.to_be_bytes());
        data.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let info = parse_cache_shard(&data, 0).unwrap();
        assert_eq!(info.entries, 1);
        assert_eq!(info.first_entry_sizes, vec![4]);
    }

    #[test]
    fn two_entries() {
        let mut data = Vec::new();
        data.extend_from_slice(&2i32.to_be_bytes());
        data.extend_from_slice(&[0x01, 0x02]);
        data.extend_from_slice(&3i32.to_be_bytes());
        data.extend_from_slice(&[0x03, 0x04, 0x05]);
        let info = parse_cache_shard(&data, 0).unwrap();
        assert_eq!(info.entries, 2);
    }
}
