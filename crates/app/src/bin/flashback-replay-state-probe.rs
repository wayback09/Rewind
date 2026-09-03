use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::{
    registry::load_26_2_registry, snapshot::decode_snapshot_with_data, MinecraftVersion,
};
use replay_model::CanonicalReplayState;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct VerifyM3 {
    probe_version: String,
    input: String,
    replay_chunk: String,
    minecraft_version: String,
    data_version: i32,
    protocol_version: i32,
    snapshot_size: usize,
    snapshot_action_count: usize,
    action_table: Vec<String>,
    initial_tick: u32,
    current_dimension: String,
    dimension_source: String,
    canonical_chunk_count: usize,
    canonical_block_state_count: usize,
    block_entity_count: usize,
    entity_count: usize,
    local_player_present: bool,
    local_player_pos: Option<[f64; 3]>,
    world_time_status: String,
    world_time: Option<serde_json::Value>,
    border_status: String,
    border: Option<serde_json::Value>,
    spawn_status: String,
    spawn: Option<serde_json::Value>,
    player_metadata_status: String,
    unknown_actions: Vec<UnknownActionReport>,
    warnings: Vec<String>,
    validation_ok: bool,
    errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UnknownActionReport {
    identifier: String,
    local_id: i32,
    payload_len: usize,
    payload_prefix_hex: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let report = probe_one(&input);
    let out_path = PathBuf::from("target/verify-m3.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize m3");
    std::fs::write(&out_path, &json).expect("write target/verify-m3.json");
    println!("{}", json);
    // Human summary
    println!("\nReplayState (snapshot)");
    println!("----------------------");
    println!("Recording: {}", report.input);
    println!("Replay chunk: {}", report.replay_chunk);
    println!(
        "Snapshot size: {} ({} actions)",
        report.snapshot_size, report.snapshot_action_count
    );
    println!("Initial tick: {}", report.initial_tick);
    println!(
        "Dimension: {} ({})",
        report.current_dimension, report.dimension_source
    );
    println!(
        "Canonical chunks: {} ({} block states)",
        report.canonical_chunk_count, report.canonical_block_state_count
    );
    println!("Block entities: {}", report.block_entity_count);
    println!("Entities: {}", report.entity_count);
    println!(
        "Local player: {} {:?}",
        report.local_player_present, report.local_player_pos
    );
    println!(
        "World time: {} {:?}",
        report.world_time_status, report.world_time
    );
    println!("Border: {} {:?}", report.border_status, report.border);
    println!("Spawn: {} {:?}", report.spawn_status, report.spawn);
    println!("Unknown actions: {}", report.unknown_actions.len());
    for ua in &report.unknown_actions {
        println!(
            "  {} (local_id {}, len {}) prefix {}",
            ua.identifier, ua.local_id, ua.payload_len, ua.payload_prefix_hex
        );
    }
    if !report.warnings.is_empty() {
        println!("Warnings:");
        for w in &report.warnings {
            println!("  - {}", w);
        }
    }
    if report.validation_ok {
        println!("\nM3 validation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nM3 validation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> VerifyM3 {
    let input_str = path.display().to_string();
    let version = MinecraftVersion::v26_2();
    let mut errors: Vec<String> = Vec::new();

    let registry = match load_26_2_registry() {
        Ok(r) => r,
        Err(e) => {
            return VerifyM3 {
                probe_version: "m3".to_string(),
                input: input_str,
                replay_chunk: "unknown".to_string(),
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                snapshot_size: 0,
                snapshot_action_count: 0,
                action_table: vec![],
                initial_tick: 0,
                current_dimension: "minecraft:overworld".to_string(),
                dimension_source: "fallback".to_string(),
                canonical_chunk_count: 0,
                canonical_block_state_count: 0,
                block_entity_count: 0,
                entity_count: 0,
                local_player_present: false,
                local_player_pos: None,
                world_time_status: "unavailable".to_string(),
                world_time: None,
                border_status: "unavailable".to_string(),
                border: None,
                spawn_status: "unavailable".to_string(),
                spawn: None,
                player_metadata_status: "unavailable".to_string(),
                unknown_actions: vec![],
                warnings: vec![format!("registry load failed: {}", e)],
                validation_ok: false,
                errors: vec![format!("registry load failed: {}", e)],
            };
        }
    };

    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            return VerifyM3 {
                probe_version: "m3".to_string(),
                input: input_str,
                replay_chunk: "unknown".to_string(),
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                snapshot_size: 0,
                snapshot_action_count: 0,
                action_table: vec![],
                initial_tick: 0,
                current_dimension: "minecraft:overworld".to_string(),
                dimension_source: "fallback".to_string(),
                canonical_chunk_count: 0,
                canonical_block_state_count: 0,
                block_entity_count: 0,
                entity_count: 0,
                local_player_present: false,
                local_player_pos: None,
                world_time_status: "unavailable".to_string(),
                world_time: None,
                border_status: "unavailable".to_string(),
                border: None,
                spawn_status: "unavailable".to_string(),
                spawn: None,
                player_metadata_status: "unavailable".to_string(),
                unknown_actions: vec![],
                warnings: vec![format!("ZIP open failed: {}", e.message)],
                validation_ok: false,
                errors: vec![format!("ZIP open failed: {}", e.message)],
            };
        }
    };

    // Find first replay chunk (c0.flashback) — for M3 we use the first chunk's snapshot
    let chunk_names: Vec<String> = {
        let mut names = Vec::new();
        for i in 0.. {
            let name = format!("c{}.flashback", i);
            if archive.by_name(&name).is_ok() {
                names.push(name);
            } else {
                break;
            }
        }
        // Fallback via find_chunk_names
        if names.is_empty() {
            names = flashback_format::zip_container::find_chunk_names(&mut archive);
        }
        names
    };

    if chunk_names.is_empty() {
        return VerifyM3 {
            probe_version: "m3".to_string(),
            input: input_str,
            replay_chunk: "unknown".to_string(),
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            snapshot_size: 0,
            snapshot_action_count: 0,
            action_table: vec![],
            initial_tick: 0,
            current_dimension: "minecraft:overworld".to_string(),
            dimension_source: "fallback".to_string(),
            canonical_chunk_count: 0,
            canonical_block_state_count: 0,
            block_entity_count: 0,
            entity_count: 0,
            local_player_present: false,
            local_player_pos: None,
            world_time_status: "unavailable".to_string(),
            world_time: None,
            border_status: "unavailable".to_string(),
            border: None,
            spawn_status: "unavailable".to_string(),
            spawn: None,
            player_metadata_status: "unavailable".to_string(),
            unknown_actions: vec![],
            warnings: vec!["no replay chunks found".to_string()],
            validation_ok: false,
            errors: vec!["no replay chunks found".to_string()],
        };
    }

    let replay_chunk_name = chunk_names[0].clone();
    let chunk_data = match read_entry_bytes(&mut archive, &replay_chunk_name) {
        Ok(b) => b,
        Err(e) => {
            return VerifyM3 {
                probe_version: "m3".to_string(),
                input: input_str,
                replay_chunk: replay_chunk_name,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                snapshot_size: 0,
                snapshot_action_count: 0,
                action_table: vec![],
                initial_tick: 0,
                current_dimension: "minecraft:overworld".to_string(),
                dimension_source: "fallback".to_string(),
                canonical_chunk_count: 0,
                canonical_block_state_count: 0,
                block_entity_count: 0,
                entity_count: 0,
                local_player_present: false,
                local_player_pos: None,
                world_time_status: "unavailable".to_string(),
                world_time: None,
                border_status: "unavailable".to_string(),
                border: None,
                spawn_status: "unavailable".to_string(),
                spawn: None,
                player_metadata_status: "unavailable".to_string(),
                unknown_actions: vec![],
                warnings: vec![format!("chunk read failed: {}", e.message)],
                validation_ok: false,
                errors: vec![format!("chunk read failed: {}", e.message)],
            };
        }
    };

    let parsed = match parse_chunk_bytes(&chunk_data, &replay_chunk_name) {
        Ok(p) => p,
        Err(e) => {
            return VerifyM3 {
                probe_version: "m3".to_string(),
                input: input_str,
                replay_chunk: replay_chunk_name,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                snapshot_size: 0,
                snapshot_action_count: 0,
                action_table: vec![],
                initial_tick: 0,
                current_dimension: "minecraft:overworld".to_string(),
                dimension_source: "fallback".to_string(),
                canonical_chunk_count: 0,
                canonical_block_state_count: 0,
                block_entity_count: 0,
                entity_count: 0,
                local_player_present: false,
                local_player_pos: None,
                world_time_status: "unavailable".to_string(),
                world_time: None,
                border_status: "unavailable".to_string(),
                border: None,
                spawn_status: "unavailable".to_string(),
                spawn: None,
                player_metadata_status: "unavailable".to_string(),
                unknown_actions: vec![],
                warnings: vec![format!("chunk parse failed: {}", e.message)],
                validation_ok: false,
                errors: vec![format!("chunk parse failed: {}", e.message)],
            };
        }
    };

    // Read level_chunk_caches/0 for chunk decoding
    let level_cache_bytes =
        read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();

    // Decode snapshot
    let snapshot_result = decode_snapshot_with_data(
        &parsed,
        &chunk_data,
        &level_cache_bytes,
        &registry,
        &version,
    );

    match snapshot_result {
        Ok(decoded) => {
            let state = decoded.state;
            let unknown_reports: Vec<UnknownActionReport> = decoded
                .unknown_actions
                .iter()
                .map(|ua| UnknownActionReport {
                    identifier: ua.identifier.clone(),
                    local_id: ua.local_id,
                    payload_len: ua.payload_len,
                    payload_prefix_hex: ua.payload_prefix_hex.clone(),
                })
                .collect();

            let has_local_player = state.local_player.is_some();
            let local_pos = state.local_player.as_ref().map(|lp| lp.pos);

            // Check for unresolved block state IDs: should be 0 for M2's palette, but for M3 we ensure snapshot's chunks have no unresolved
            let unresolved_empty = unknown_reports
                .iter()
                .filter(|ua| ua.identifier.contains("level_chunk_cached"))
                .count()
                == 0
                || state.chunks.values().all(|c| {
                    c.sections
                        .iter()
                        .all(|s| s.block_states.iter().all(|bs| !bs.name.is_empty()))
                });

            let validation_ok = !state.chunks.is_empty()
                && has_local_player
                && state
                    .chunks
                    .values()
                    .all(|c| c.sections.iter().all(|s| s.block_states.len() == 4096))
                && state.dimension.0.starts_with("minecraft:")
                && unresolved_empty;

            let mut all_warnings = decoded.warnings.clone();
            if !validation_ok {
                all_warnings.push(
                    "M3 validation: missing required state (dimension/chunks/local_player)"
                        .to_string(),
                );
            }

            VerifyM3 {
                probe_version: "m3".to_string(),
                input: input_str,
                replay_chunk: replay_chunk_name,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                snapshot_size: state.snapshot_size,
                snapshot_action_count: state.snapshot_action_count,
                action_table: parsed.action_table.clone(),
                initial_tick: state.tick,
                current_dimension: state.dimension.0.clone(),
                dimension_source: state.dimension_source.clone(),
                canonical_chunk_count: state.chunks.len(),
                canonical_block_state_count: state
                    .chunks
                    .values()
                    .map(|c| c.sections.len() * 4096)
                    .sum(),
                block_entity_count: state.block_entity_count,
                entity_count: state.entities.len(),
                local_player_present: has_local_player,
                local_player_pos: local_pos,
                world_time_status: state
                    .world_time
                    .as_ref()
                    .map(|wt| wt.raw_status.clone())
                    .unwrap_or_else(|| "unavailable".to_string()),
                world_time: state
                    .world_time
                    .as_ref()
                    .map(|wt| serde_json::to_value(wt).unwrap()),
                border_status: state
                    .world_border
                    .as_ref()
                    .map(|wb| wb.raw_status.clone())
                    .unwrap_or_else(|| "unavailable".to_string()),
                border: state
                    .world_border
                    .as_ref()
                    .map(|wb| serde_json::to_value(wb).unwrap()),
                spawn_status: state
                    .spawn
                    .as_ref()
                    .map(|s| s.raw_status.clone())
                    .unwrap_or_else(|| "unavailable".to_string()),
                spawn: state
                    .spawn
                    .as_ref()
                    .map(|s| serde_json::to_value(s).unwrap()),
                player_metadata_status: if state.player_metadata.is_some() {
                    "present".to_string()
                } else {
                    "unavailable".to_string()
                },
                unknown_actions: unknown_reports,
                warnings: all_warnings,
                validation_ok,
                errors,
            }
        }
        Err(e) => VerifyM3 {
            probe_version: "m3".to_string(),
            input: input_str,
            replay_chunk: replay_chunk_name,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            snapshot_size: parsed.snapshot_size as usize,
            snapshot_action_count: parsed.snapshot_tlvs.len(),
            action_table: parsed.action_table.clone(),
            initial_tick: 0,
            current_dimension: "minecraft:overworld".to_string(),
            dimension_source: "error".to_string(),
            canonical_chunk_count: 0,
            canonical_block_state_count: 0,
            block_entity_count: 0,
            entity_count: 0,
            local_player_present: false,
            local_player_pos: None,
            world_time_status: "error".to_string(),
            world_time: None,
            border_status: "error".to_string(),
            border: None,
            spawn_status: "error".to_string(),
            spawn: None,
            player_metadata_status: "error".to_string(),
            unknown_actions: vec![],
            warnings: vec![format!("snapshot decode failed: {}", e)],
            validation_ok: false,
            errors: vec![format!("snapshot decode failed: {}", e)],
        },
    }
}
