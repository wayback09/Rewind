//! flashback-format — ZIP container, metadata, replay chunk, TLV, and chunk-cache parsing.
//!
//! This crate is Minecraft-agnostic: it validates container framing and TLV structure
//! without decoding Minecraft packet codecs. It never hard-codes action IDs; the per-chunk
//! action table is the source of truth.

pub mod cache;
pub mod chunk;
pub mod error;
pub mod identifier;
pub mod metadata;
pub mod tlv;
pub mod varint;
pub mod zip_container;

pub use cache::{parse_cache_shard, CacheShardInfo};
pub use chunk::{parse_chunk_bytes, ParsedChunk, MAGIC};
pub use error::{FormatError, Result};
pub use metadata::{parse_metadata, ChunkMeta, FlashbackMetadata};
pub use tlv::{parse_one_tlv, walk_tlvs, Tlv};
pub use varint::{read_be_i32, read_be_u32, read_varint};

#[cfg(test)]
mod real_recordings_tests {
    use crate::{
        cache::parse_cache_shard, chunk::parse_chunk_bytes, metadata::parse_metadata,
        zip_container::*,
    };
    use std::path::PathBuf;

    fn recordings_root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../recordings")
    }

    fn recording_paths() -> Vec<PathBuf> {
        let root = recordings_root();
        let mut out = Vec::new();
        let candidates = [
            "basic/test_recording.zip",
            "basic/test_recording_2.zip",
            "chunks/test_recording3.zip",
            "entities/test_recording3.zip",
        ];
        for c in candidates {
            let p = root.join(c);
            if p.exists() {
                out.push(p);
            }
        }
        out
    }

    #[test]
    fn all_recordings_exist() {
        let paths = recording_paths();
        assert!(
            !paths.is_empty(),
            "no recordings found under {:?}",
            recordings_root()
        );
        assert!(paths.len() >= 3, "expected >=3 recordings, got {:?}", paths);
    }

    #[test]
    fn metadata_invariants() {
        for path in recording_paths() {
            let mut archive =
                open_zip_readonly(&path).unwrap_or_else(|e| panic!("open {:?}: {}", path, e));
            let meta_bytes =
                read_entry_bytes(&mut archive, "metadata.json").expect("metadata.json");
            let meta = parse_metadata(&meta_bytes).expect("parse metadata");
            assert!(
                meta.validate().is_empty(),
                "metadata validate failed for {:?}: {:?}",
                path,
                meta.validate()
            );
            if let Some(total) = meta.total_ticks {
                assert_eq!(
                    total,
                    meta.total_duration(),
                    "total_ticks mismatch for {:?}",
                    path
                );
            }
            for k in meta.chunks.keys() {
                assert!(
                    k.starts_with('c') && k.ends_with(".flashback"),
                    "chunk key {}",
                    k
                );
            }
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            if name == "test_recording.zip" {
                assert_eq!(meta.total_ticks, Some(916));
            } else if name == "test_recording_2.zip" {
                assert_eq!(meta.total_ticks, Some(2242));
            } else if name == "test_recording3.zip" {
                assert_eq!(meta.total_ticks, Some(2341));
                assert_eq!(meta.chunks.len(), 2);
            }
        }
    }

    #[test]
    fn chunk_header_and_tlv_invariants() {
        for path in recording_paths() {
            let mut archive =
                open_zip_readonly(&path).unwrap_or_else(|e| panic!("open {:?}: {}", path, e));
            let chunk_names = find_chunk_names(&mut archive);
            assert!(!chunk_names.is_empty(), "no chunks for {:?}", path);
            for chunk_name in chunk_names {
                let chunk_bytes = read_entry_bytes(&mut archive, &chunk_name).expect("read chunk");
                let parsed = parse_chunk_bytes(&chunk_bytes, &chunk_name).unwrap_or_else(|e| {
                    panic!("parse chunk {:?} {}: {}", path.display(), chunk_name, e)
                });
                assert_eq!(parsed.magic, 0xD780E884, "MAGIC for {:?}", path);
                assert_eq!(parsed.action_table.len(), 9, "action count for {:?}", path);
                assert_eq!(
                    parsed.action_table[0], "flashback:action/simple_voice_chat_sound_optional",
                    "voice-chat at 0 for {:?}",
                    path
                );
                assert_eq!(
                    parsed.action_table[1], "flashback:action/next_tick",
                    "next_tick at 1 for {:?}",
                    path
                );
                let next_id = parsed
                    .find_id("flashback:action/next_tick")
                    .expect("next_tick id");
                assert_eq!(next_id, 1);
                assert_eq!(
                    parsed.snapshot_offset + parsed.snapshot_size as usize,
                    parsed.actions_offset,
                    "snapshot boundaries for {:?} {}",
                    path.display(),
                    chunk_name
                );
                let tick_count = parsed
                    .replay_tlvs
                    .iter()
                    .filter(|t| t.local_id == next_id)
                    .count();
                let snap_ticks = parsed
                    .snapshot_tlvs
                    .iter()
                    .filter(|t| t.local_id == next_id)
                    .count();
                assert_eq!(
                    snap_ticks,
                    0,
                    "snapshot should have 0 next_tick for {:?} {}",
                    path.display(),
                    chunk_name
                );
                for tlv in parsed.replay_tlvs.iter().filter(|t| t.local_id == next_id) {
                    assert_eq!(
                        tlv.payload_size, 0,
                        "NextTick payload size 0 for {:?}",
                        path
                    );
                }
                assert!(tick_count > 0, "tick_count >0 for {:?}", path);
            }
        }
    }

    #[test]
    fn duration_vs_tick_count_matches_metadata() {
        for path in recording_paths() {
            let mut archive =
                open_zip_readonly(&path).unwrap_or_else(|e| panic!("open {:?}: {}", path, e));
            let meta_bytes = read_entry_bytes(&mut archive, "metadata.json").unwrap();
            let meta = parse_metadata(&meta_bytes).unwrap();
            let chunk_names = find_chunk_names(&mut archive);
            for chunk_name in chunk_names {
                let expected_duration = meta
                    .chunks
                    .get(&chunk_name)
                    .expect("chunk in metadata")
                    .duration as usize;
                let chunk_bytes = read_entry_bytes(&mut archive, &chunk_name).unwrap();
                let parsed = parse_chunk_bytes(&chunk_bytes, &chunk_name).unwrap();
                let next_id = parsed.find_id("flashback:action/next_tick").unwrap();
                let tick_count = parsed
                    .replay_tlvs
                    .iter()
                    .filter(|t| t.local_id == next_id)
                    .count();
                assert_eq!(
                    tick_count,
                    expected_duration,
                    "tick count {} vs duration {} for {:?} {}",
                    tick_count,
                    expected_duration,
                    path.display(),
                    chunk_name
                );
            }
            let total: usize = {
                let mut sum = 0;
                for (chunk_name, chunk_meta) in &meta.chunks {
                    let chunk_bytes = read_entry_bytes(&mut archive, chunk_name).unwrap();
                    let parsed = parse_chunk_bytes(&chunk_bytes, chunk_name).unwrap();
                    let next_id = parsed.find_id("flashback:action/next_tick").unwrap();
                    let c = parsed
                        .replay_tlvs
                        .iter()
                        .filter(|t| t.local_id == next_id)
                        .count();
                    assert_eq!(c, chunk_meta.duration as usize);
                    sum += c;
                }
                sum
            };
            if let Some(total_ticks) = meta.total_ticks {
                assert_eq!(total, total_ticks as usize, "global ticks for {:?}", path);
            }
        }
    }

    #[test]
    fn cache_structure_invariants() {
        for path in recording_paths() {
            let mut archive =
                open_zip_readonly(&path).unwrap_or_else(|e| panic!("open {:?}: {}", path, e));
            let shards = find_cache_shards(&mut archive);
            assert!(!shards.is_empty(), "no cache shards for {:?}", path);
            assert_eq!(shards.len(), 1, "expected 1 shard for {:?}", path);
            assert!(shards.contains_key(&0), "shard 0 missing for {:?}", path);
            for (idx, shard_name) in shards {
                let shard_bytes = read_entry_bytes(&mut archive, &shard_name).unwrap();
                let info = parse_cache_shard(&shard_bytes, idx).expect("parse cache");
                assert!(info.entries > 0, "cache entries >0 for {:?}", path);
                for &sz in &info.first_entry_sizes {
                    assert!(
                        sz > 0 && sz < 200_000,
                        "first entry size {} for {:?}",
                        sz,
                        path
                    );
                }
                assert_eq!(info.total_bytes, shard_bytes.len());
            }
        }
    }

    #[test]
    fn snapshot_contents_invariants() {
        for path in recording_paths() {
            let mut archive = open_zip_readonly(&path).unwrap();
            let chunk_names = find_chunk_names(&mut archive);
            for chunk_name in chunk_names {
                let chunk_bytes = read_entry_bytes(&mut archive, &chunk_name).unwrap();
                let parsed = parse_chunk_bytes(&chunk_bytes, &chunk_name).unwrap();
                let next_id = parsed.find_id("flashback:action/next_tick").unwrap();
                let move_id = parsed.find_id("flashback:action/move_entities").unwrap();
                let snap_next = parsed
                    .snapshot_tlvs
                    .iter()
                    .filter(|t| t.local_id == next_id)
                    .count();
                let snap_move = parsed
                    .snapshot_tlvs
                    .iter()
                    .filter(|t| t.local_id == move_id)
                    .count();
                assert_eq!(
                    snap_next,
                    0,
                    "snapshot next_tick 0 for {:?} {}",
                    path.display(),
                    chunk_name
                );
                assert_eq!(
                    snap_move,
                    0,
                    "snapshot move_entities 0 for {:?} {}",
                    path.display(),
                    chunk_name
                );
                let config_id = parsed
                    .find_id("flashback:action/configuration_packet")
                    .unwrap();
                let snap_config = parsed
                    .snapshot_tlvs
                    .iter()
                    .filter(|t| t.local_id == config_id)
                    .count();
                let replay_config = parsed
                    .replay_tlvs
                    .iter()
                    .filter(|t| t.local_id == config_id)
                    .count();
                assert_eq!(
                    snap_config,
                    32,
                    "snapshot config 32 for {:?} {}",
                    path.display(),
                    chunk_name
                );
                assert_eq!(
                    replay_config,
                    0,
                    "replay config 0 for {:?} {}",
                    path.display(),
                    chunk_name
                );
                let create_id = parsed
                    .find_id("flashback:action/create_local_player")
                    .unwrap();
                let snap_create = parsed
                    .snapshot_tlvs
                    .iter()
                    .filter(|t| t.local_id == create_id)
                    .count();
                let replay_create = parsed
                    .replay_tlvs
                    .iter()
                    .filter(|t| t.local_id == create_id)
                    .count();
                assert_eq!(
                    snap_create,
                    1,
                    "snapshot create 1 for {:?} {}",
                    path.display(),
                    chunk_name
                );
                assert_eq!(
                    replay_create,
                    0,
                    "replay create 0 for {:?} {}",
                    path.display(),
                    chunk_name
                );
            }
        }
    }
}
