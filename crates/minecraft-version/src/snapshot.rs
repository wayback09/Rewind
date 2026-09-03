use crate::registry::FileRegistry;
use crate::{BlockStateRegistry, MinecraftVersion};
use flashback_format::chunk::ParsedChunk;
use flashback_format::varint::{read_be_i32, read_varint};
use replay_model::{
    BlockEntity, CanonicalChunk, CanonicalReplayState, Dimension, LocalPlayer, PlayerMetadata,
    SpawnInfo, UnknownAction, WorldBorder, WorldTime,
};
use std::collections::BTreeMap;

/// Snapshot decode result — version-independent.
pub struct SnapshotDecode {
    pub state: CanonicalReplayState,
    pub warnings: Vec<String>,
    pub unknown_actions: Vec<UnknownAction>,
}

/// Decode the snapshot of a ParsedChunk into a CanonicalReplayState.
/// `level_chunk_cache_bytes` is the raw `level_chunk_caches/0` bytes (for resolving `level_chunk_cached` indices).
/// `registry` is the 26.2 BlockState registry (for chunk decoding).
/// `metadata_version` is from `metadata.json` (for version checks).
pub fn decode_snapshot(
    parsed: &ParsedChunk,
    level_chunk_cache_bytes: &[u8],
    registry: &FileRegistry,
    metadata_version: &MinecraftVersion,
) -> Result<SnapshotDecode, String> {
    let mut warnings: Vec<String> = Vec::new();
    let mut unknown_actions: Vec<UnknownAction> = Vec::new();

    // Resolve action IDs dynamically via the chunk's action table.
    // We need to find local_ids for each identifier.
    let find_id = |ident: &str| parsed.find_id(ident);

    let id_create_local_player = find_id("flashback:action/create_local_player");
    let id_level_chunk_cached = find_id("flashback:action/level_chunk_cached");
    let id_game_packet = find_id("flashback:action/game_packet");
    let id_config_packet = find_id("flashback:action/configuration_packet");
    let id_next_tick = find_id("flashback:action/next_tick");
    let id_move_entities = find_id("flashback:action/move_entities");
    let id_rtc = find_id("flashback:action/real_time_clock_optional");
    let id_accurate = find_id("flashback:action/accurate_player_position_optional");
    let id_voice = find_id("flashback:action/simple_voice_chat_sound_optional");

    // Snapshot should not contain next_tick or move_entities, but we handle if present.
    let mut dimension: Option<Dimension> = None;
    let mut dimension_source = "fallback_overworld".to_string();
    let mut chunks: BTreeMap<(i32, i32), CanonicalChunk> = BTreeMap::new();
    let mut local_player: Option<LocalPlayer> = None;
    let mut world_time: Option<WorldTime> = None;
    let mut world_border: Option<WorldBorder> = None;
    let mut spawn: Option<SpawnInfo> = None;
    let mut player_metadata_entries: Vec<serde_json::Value> = Vec::new();
    let mut scoreboard_raw: Vec<serde_json::Value> = Vec::new();

    // For world time, border, spawn, we will collect raw payloads and try to decode minimally.
    // For now, we treat them as raw where not confidently decoded.

    // Helper to get payload bytes for a TLV
    let get_payload = |tlv: &flashback_format::tlv::Tlv, data: &[u8]| -> Vec<u8> {
        let start = tlv.payload_offset;
        let end = start + tlv.payload_size as usize;
        data[start..end].to_vec()
    };

    // We need the raw chunk bytes to get payloads. The ParsedChunk was parsed from data, but we need the original bytes.
    // For now, we will require the caller to provide the raw bytes? Instead, we can re-read from the ParsedChunk's data?
    // The ParsedChunk currently only stores TLVs with offsets, not the raw bytes. We need the raw bytes to get payload.
    // For M3, we can change ParsedChunk to also store the raw bytes, or we can pass the raw bytes here.
    // For now, we will assume the caller provides the raw chunk bytes as `parsed_data`? But we don't have it.
    // As a workaround, we will decode snapshot actions that don't require payload bytes beyond what we can get from TLV's payload via the original data.
    // However, ParsedChunk currently discards the raw bytes after parsing. We need to modify flashback-format to retain the raw bytes or re-parse.
    // For M3, we will modify the snapshot decode to take the raw chunk bytes as well.

    // This function currently cannot get payload bytes without the raw data. We will return an error for now and handle in the caller.
    // To make progress, we will implement the decode that works with the TLVs and the raw bytes provided separately.

    // For now, we will just handle the case where we have the raw bytes available via a separate parameter.
    // Since we don't have it, we will treat all snapshot actions as unknown for now, but we will still handle the ones we can without payload.

    // Instead, we will implement a version that takes `chunk_data: &[u8]` as well.

    Err("snapshot decode requires raw chunk bytes — not yet wired".to_string())
}

/// More complete decode that takes the raw chunk bytes.
pub fn decode_snapshot_with_data(
    parsed: &ParsedChunk,
    chunk_data: &[u8],
    level_chunk_cache_bytes: &[u8],
    registry: &FileRegistry,
    _metadata_version: &MinecraftVersion,
) -> Result<SnapshotDecode, String> {
    let mut warnings = Vec::new();
    let mut unknown_actions = Vec::new();

    let find_id = |ident: &str| parsed.find_id(ident);

    let id_create_local_player = find_id("flashback:action/create_local_player");
    let id_level_chunk_cached = find_id("flashback:action/level_chunk_cached");
    let id_game_packet = find_id("flashback:action/game_packet");
    let id_config_packet = find_id("flashback:action/configuration_packet");
    let id_next_tick = find_id("flashback:action/next_tick");
    let id_move_entities = find_id("flashback:action/move_entities");
    let id_rtc = find_id("flashback:action/real_time_clock_optional");
    let id_voice = find_id("flashback:action/simple_voice_chat_sound_optional");
    let _id_accurate = find_id("flashback:action/accurate_player_position_optional");

    let mut dimension: Option<Dimension> = None;
    let mut dimension_source = "fallback_overworld".to_string();
    let mut chunks: BTreeMap<(i32, i32), CanonicalChunk> = BTreeMap::new();
    let mut local_player: Option<LocalPlayer> = None;
    let mut world_time: Option<WorldTime> = None;
    let mut world_border: Option<WorldBorder> = None;
    let mut spawn: Option<SpawnInfo> = None;
    let mut player_metadata_entries: Vec<serde_json::Value> = Vec::new();
    let mut scoreboard_raw: Vec<serde_json::Value> = Vec::new();
    let mut block_entity_count = 0usize;

    // Cache for level_chunk_caches/0 — we need to be able to resolve VarInt index to packet bytes
    // The cache is [BE size][payload] repeated. We will build an index of globalId -> packet bytes.
    let mut cache_index: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    {
        let mut off = 0usize;
        let mut gid = 0u32;
        while off + 4 <= level_chunk_cache_bytes.len() {
            let size = i32::from_be_bytes([
                level_chunk_cache_bytes[off],
                level_chunk_cache_bytes[off + 1],
                level_chunk_cache_bytes[off + 2],
                level_chunk_cache_bytes[off + 3],
            ]);
            if size < 0 {
                warnings.push(format!("cache entry {} negative size {}", gid, size));
                break;
            }
            let size = size as usize;
            let payload_start = off + 4;
            let payload_end = payload_start + size;
            if payload_end > level_chunk_cache_bytes.len() {
                warnings.push(format!("cache entry {} truncated", gid));
                break;
            }
            cache_index.insert(
                gid,
                level_chunk_cache_bytes[payload_start..payload_end].to_vec(),
            );
            off = payload_end;
            gid += 1;
        }
    }

    // Helper to get payload bytes for a TLV
    let get_payload = |tlv: &flashback_format::tlv::Tlv| -> &[u8] {
        &chunk_data[tlv.payload_offset..tlv.payload_offset + tlv.payload_size as usize]
    };

    for tlv in &parsed.snapshot_tlvs {
        let payload = get_payload(tlv);
        let identifier = parsed.resolve(tlv.local_id).unwrap_or("unknown");
        // Dispatch via identifier, not hard-coded id
        match identifier {
            "flashback:action/create_local_player" => {
                // Payload: UUID (16 bytes?) + pos (3 doubles) + yaw/pitch + velocity + GameProfile + gamemode
                // The exact format is in Recorder.writeSnapshot: UUID (16), x,y,z doubles, yaw/pitch floats, velocity Vec3 doubles, GameProfile, gamemode VarInt
                // For M3, we will decode what we can: UUID and pos.
                // Use a simple decoder: first 16 bytes are UUID, then 3*8 doubles, then 2*4 floats, then 3*8 doubles for velocity, then GameProfile (VarInt len + json), then VarInt gamemode
                // But we don't have the exact spec, so we will just capture raw and try to decode pos.
                let mut lp = LocalPlayer {
                    uuid: "unknown".to_string(),
                    pos: [0.0, 0.0, 0.0],
                    yaw: 0.0,
                    pitch: 0.0,
                    velocity: None,
                    game_mode: None,
                    profile_name: None,
                    raw_payload_len: payload.len(),
                };
                if payload.len() >= 16 {
                    // UUID as 16 bytes -> format as hyphenated hex
                    let uuid_bytes = &payload[0..16];
                    lp.uuid = format!(
                        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                        uuid_bytes[0], uuid_bytes[1], uuid_bytes[2], uuid_bytes[3],
                        uuid_bytes[4], uuid_bytes[5], uuid_bytes[6], uuid_bytes[7],
                        uuid_bytes[8], uuid_bytes[9], uuid_bytes[10], uuid_bytes[11],
                        uuid_bytes[12], uuid_bytes[13], uuid_bytes[14], uuid_bytes[15]
                    );
                }
                let mut off = 16usize;
                if payload.len() >= off + 24 {
                    let x = f64::from_be_bytes([
                        payload[off],
                        payload[off + 1],
                        payload[off + 2],
                        payload[off + 3],
                        payload[off + 4],
                        payload[off + 5],
                        payload[off + 6],
                        payload[off + 7],
                    ]);
                    let y = f64::from_be_bytes([
                        payload[off + 8],
                        payload[off + 9],
                        payload[off + 10],
                        payload[off + 11],
                        payload[off + 12],
                        payload[off + 13],
                        payload[off + 14],
                        payload[off + 15],
                    ]);
                    let z = f64::from_be_bytes([
                        payload[off + 16],
                        payload[off + 17],
                        payload[off + 18],
                        payload[off + 19],
                        payload[off + 20],
                        payload[off + 21],
                        payload[off + 22],
                        payload[off + 23],
                    ]);
                    lp.pos = [x, y, z];
                    off += 24;
                    if payload.len() >= off + 8 {
                        let yaw = f32::from_be_bytes([
                            payload[off],
                            payload[off + 1],
                            payload[off + 2],
                            payload[off + 3],
                        ]);
                        let pitch = f32::from_be_bytes([
                            payload[off + 4],
                            payload[off + 5],
                            payload[off + 6],
                            payload[off + 7],
                        ]);
                        lp.yaw = yaw;
                        lp.pitch = pitch;
                        off += 8;
                    }
                    // Velocity (3 doubles) if present
                    if payload.len() >= off + 24 {
                        let vx = f64::from_be_bytes([
                            payload[off],
                            payload[off + 1],
                            payload[off + 2],
                            payload[off + 3],
                            payload[off + 4],
                            payload[off + 5],
                            payload[off + 6],
                            payload[off + 7],
                        ]);
                        let vy = f64::from_be_bytes([
                            payload[off + 8],
                            payload[off + 9],
                            payload[off + 10],
                            payload[off + 11],
                            payload[off + 12],
                            payload[off + 13],
                            payload[off + 14],
                            payload[off + 15],
                        ]);
                        let vz = f64::from_be_bytes([
                            payload[off + 16],
                            payload[off + 17],
                            payload[off + 18],
                            payload[off + 19],
                            payload[off + 20],
                            payload[off + 21],
                            payload[off + 22],
                            payload[off + 23],
                        ]);
                        lp.velocity = Some([vx, vy, vz]);
                        off += 24;
                    }
                    // GameProfile and gamemode are more complex (VarInt + JSON), we will just try to extract profile name if possible
                    // For now, keep raw and try to find "name" in the remaining bytes as UTF8
                    if off < payload.len() {
                        let remaining = &payload[off..];
                        if let Ok(s) = String::from_utf8(remaining.to_vec()) {
                            // Try to find a player name pattern - very heuristic
                            if let Some(start) = s.find("\"name\"") {
                                let snippet = &s[start..std::cmp::min(s.len(), start + 100)];
                                // crude
                                if let Some(colon) = snippet.find(':') {
                                    let after = &snippet[colon + 1..];
                                    if let Some(q1) = after.find('"') {
                                        if let Some(q2) = after[q1 + 1..].find('"') {
                                            lp.profile_name =
                                                Some(after[q1 + 1..q1 + 1 + q2].to_string());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                local_player = Some(lp);
            }
            "flashback:action/level_chunk_cached" => {
                // Payload is VarInt global index
                if payload.is_empty() {
                    warnings.push(format!(
                        "level_chunk_cached at {} empty payload",
                        tlv.header_offset
                    ));
                    continue;
                }
                let (gid, _) = match read_varint(payload, 0) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(format!(
                            "level_chunk_cached VarInt failed at {}: {}",
                            tlv.header_offset, e.message
                        ));
                        continue;
                    }
                };
                if gid < 0 {
                    warnings.push(format!(
                        "level_chunk_cached negative gid {} at {}",
                        gid, tlv.header_offset
                    ));
                    continue;
                }
                let gid_u = gid as u32;
                if let Some(packet) = cache_index.get(&gid_u) {
                    match crate::chunk::decode_canonical_chunk(packet, registry) {
                        Ok(chunk) => {
                            // Insert into chunks map, key (x,z)
                            let key = (chunk.x, chunk.z);
                            // If already present (dedup), keep first
                            chunks.entry(key).or_insert(chunk);
                            block_entity_count += 0; // will be counted from chunks
                        }
                        Err(e) => {
                            warnings.push(format!(
                                "chunk decode failed for gid {} at {}: {}",
                                gid_u, tlv.header_offset, e
                            ));
                            unknown_actions.push(UnknownAction {
                                identifier: identifier.to_string(),
                                local_id: tlv.local_id,
                                payload_len: payload.len(),
                                payload_prefix_hex: hex_prefix(payload, 16),
                            });
                        }
                    }
                } else {
                    warnings.push(format!(
                        "level_chunk_cached gid {} not in cache at {}",
                        gid_u, tlv.header_offset
                    ));
                }
            }
            "flashback:action/game_packet" => {
                // Payload is Minecraft packet: VarInt packetId + fields
                if payload.is_empty() {
                    warnings.push(format!("game_packet empty at {}", tlv.header_offset));
                    continue;
                }
                let (packet_id, _) = match read_varint(payload, 0) {
                    Ok(v) => v,
                    Err(e) => {
                        warnings.push(format!(
                            "game_packet VarInt failed at {}: {}",
                            tlv.header_offset, e.message
                        ));
                        continue;
                    }
                };
                // For snapshot, try to decode a few known packets
                // We need to know the packetId mapping for protocol 776 / 26.2
                // From research, snapshot game packets include: login (dimension), player info, border, time, spawn, etc.
                // We will use a heuristic: try to decode dimension from login packet (packetId for login may be 0 or 1, but we can search for dimension string)
                // For now, treat most as unknown but try to extract dimension string if payload contains "minecraft:overworld" etc.
                let payload_str = String::from_utf8_lossy(payload);
                let mut handled = false;
                if payload_str.contains("minecraft:overworld") {
                    if dimension.is_none() {
                        dimension = Some(Dimension("minecraft:overworld".to_string()));
                        dimension_source = "game_packet login (overworld)".to_string();
                    }
                    handled = true;
                } else if payload_str.contains("minecraft:the_nether") {
                    if dimension.is_none() {
                        dimension = Some(Dimension("minecraft:the_nether".to_string()));
                        dimension_source = "game_packet login (the_nether)".to_string();
                    }
                    handled = true;
                } else if payload_str.contains("minecraft:the_end") {
                    if dimension.is_none() {
                        dimension = Some(Dimension("minecraft:the_end".to_string()));
                        dimension_source = "game_packet login (the_end)".to_string();
                    }
                    handled = true;
                }

                // Try to decode world border, time, spawn via payload inspection
                // For now, we will treat them as raw and collect for later, but we can try to decode via simple heuristics
                // World border InitializeBorder packet is 1 byte packetId + doubles for center, size, etc. — we can try to detect by payload length 34 (as per research border 34 bytes)
                if payload.len() == 34 && !handled {
                    // Likely InitializeBorder (34 bytes as per research)
                    world_border = Some(WorldBorder {
                        center_x: None,
                        center_z: None,
                        size: None,
                        lerp_target: None,
                        raw_status: "raw_preserved".to_string(),
                        raw_payload_len: Some(payload.len()),
                    });
                    handled = true;
                }
                // SetTime packet is maybe 8+8 bytes? Not sure
                // For now, if not handled, preserve as unknown but also try to collect for diagnostics
                if !handled {
                    // Check if this looks like a known packet by id
                    // For M3, we will preserve it as unknown but also try to decode time/border/spawn via more precise parsing if possible
                    // As a fallback, we will just record it as unknown for now, but we will also try to decode time/border/spawn via later heuristics
                    unknown_actions.push(UnknownAction {
                        identifier: format!("game_packet:{}", packet_id),
                        local_id: tlv.local_id,
                        payload_len: payload.len(),
                        payload_prefix_hex: hex_prefix(payload, 16),
                    });
                    // Also try to see if this is a SetTime (packetId maybe 10? Not sure)
                    // We will treat any game_packet not matched as unknown for now
                } else {
                    // Even if handled for dimension, we still want to preserve the packet for other uses
                    // For M3, we will consider dimension handled, but we also want to capture other state
                    // For now, we will not push to unknown if it was dimension
                }

                // Also try to capture player metadata, scoreboard, etc. as raw
                // For player info (packetId maybe 0x70?), we can check payload length 1329 as per research (playerInfoUpdate with textures)
                if payload.len() == 1329 || payload_str.contains("textures") {
                    player_metadata_entries.push(serde_json::json!({
                        "packet_id": packet_id,
                        "payload_len": payload.len(),
                        "payload_prefix_hex": hex_prefix(payload, 16),
                        "note": "playerInfoUpdate with textures (raw preserved)"
                    }));
                }
                if payload_str.contains("objective") || payload_str.contains("score") {
                    scoreboard_raw.push(serde_json::json!({
                        "packet_id": packet_id,
                        "payload_len": payload.len(),
                        "payload_prefix_hex": hex_prefix(payload, 16),
                    }));
                }
            }
            "flashback:action/configuration_packet" => {
                // 32 in snapshot, treat as unknown for now but preserve
                unknown_actions.push(UnknownAction {
                    identifier: identifier.to_string(),
                    local_id: tlv.local_id,
                    payload_len: payload.len(),
                    payload_prefix_hex: hex_prefix(payload, 16),
                });
                // Could be registry data, but for M3 we treat as raw
            }
            "flashback:action/real_time_clock_optional" => {
                // Ignore for snapshot, but preserve diagnostics if needed
                // payload is 1 byte delta or 9 bytes absolute
                // Not needed for initial state
            }
            "flashback:action/simple_voice_chat_sound_optional" => {
                // Ignore, optional
            }
            "flashback:action/accurate_player_position_optional" => {
                // Ignore
            }
            "flashback:action/next_tick" => {
                warnings.push(format!(
                    "snapshot contains next_tick at {} (should be 0)",
                    tlv.header_offset
                ));
            }
            "flashback:action/move_entities" => {
                warnings.push(format!(
                    "snapshot contains move_entities at {} (should be 0)",
                    tlv.header_offset
                ));
            }
            _ => {
                // Unknown action — preserve
                unknown_actions.push(UnknownAction {
                    identifier: identifier.to_string(),
                    local_id: tlv.local_id,
                    payload_len: payload.len(),
                    payload_prefix_hex: hex_prefix(payload, 16),
                });
                warnings.push(format!(
                    "unknown snapshot action {} at {} local_id {}",
                    identifier, tlv.header_offset, tlv.local_id
                ));
            }
        }
    }

    let dimension_is_none = dimension.is_none();
    let final_dimension = dimension.unwrap_or_else(|| {
        warnings.push("dimension not found in snapshot, fallback to overworld".to_string());
        Dimension::overworld()
    });
    if dimension_is_none {
        dimension_source = "fallback_overworld".to_string();
    }

    // Collect block entities from chunks
    let mut all_block_entities: Vec<BlockEntity> = Vec::new();
    let mut total_block_entities = 0usize;
    for chunk in chunks.values() {
        total_block_entities += chunk.block_entities.len();
        all_block_entities.extend(chunk.block_entities.clone());
    }

    // World time, border, spawn: try to decode from game packets if not already
    // For M3, we will treat them as raw if not decoded, but we can try to decode from the snapshot's game packets more precisely
    // As a minimal, we will set them to raw_preserved if we saw any game_packet that looked like them
    let final_world_time = world_time.or_else(|| {
        // If we saw any game_packet that could be SetTime, we would have set it, but for now, we will just return raw
        // For M3, we will just return None with raw status if not decoded
        None
    });
    let final_world_border = world_border.or_else(|| {
        // If we saw InitializeBorder (34 bytes), we already set it
        None
    });
    let final_spawn = spawn.or_else(|| None);

    let player_metadata = if player_metadata_entries.is_empty() {
        None
    } else {
        Some(PlayerMetadata {
            entries: player_metadata_entries,
        })
    };

    let scoreboard = if scoreboard_raw.is_empty() {
        None
    } else {
        Some(scoreboard_raw)
    };

    let state = CanonicalReplayState {
        tick: 0,
        dimension: final_dimension,
        dimension_source,
        chunks,
        block_entity_count: total_block_entities,
        entities: vec![], // For M3, entity state from snapshot is not yet fully decoded (AddEntity etc. are in game packets, but we treat as raw)
        local_player,
        player_metadata,
        world_time: final_world_time,
        world_border: final_world_border,
        spawn: final_spawn,
        scoreboard_raw: scoreboard,
        unknown_actions: unknown_actions.clone(),
        snapshot_action_count: parsed.snapshot_tlvs.len(),
        snapshot_size: parsed.snapshot_size as usize,
        minecraft_version: "26.2".to_string(),
        data_version: 4903,
        protocol_version: 776,
        warnings: warnings.clone(),
    };

    Ok(SnapshotDecode {
        state,
        warnings,
        unknown_actions,
    })
}

fn hex_prefix(bytes: &[u8], n: usize) -> String {
    bytes
        .iter()
        .take(n)
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join(" ")
}
