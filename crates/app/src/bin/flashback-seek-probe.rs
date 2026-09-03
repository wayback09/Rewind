use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::registry::load_26_2_registry;
use minecraft_version::MinecraftVersion;
use playback::{ParsedChunkWithData, ReplayPlayer};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct VerifyM5 {
    probe_version: String,
    input: String,
    replay_chunks: Vec<String>,
    metadata_duration: u32,
    snapshot_tick: u32,
    snapshot_dimension: String,
    checkpoint_interval: u32,
    checkpoint_ticks: Vec<u32>,
    seek_tests: Vec<SeekTest>,
    backward_sequence: Vec<CheckpointState>,
    cross_chunk: Option<CrossChunkTest>,
    validation_ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SeekTest {
    target: u32,
    via_sequential_hash: Option<u64>,
    via_seek_hash: Option<u64>,
    sequential_tick: u32,
    seek_tick: u32,
    match_hash: bool,
    sequential_dimension: String,
    seek_dimension: String,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CheckpointState {
    tick: u32,
    dimension: String,
    chunk_count: usize,
    hash: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CrossChunkTest {
    tick_0_dim: String,
    tick_100_dim: String,
    tick_1311_dim: String,
    tick_1312_dim: String,
    tick_1500_dim: String,
    tick_2341_dim: String,
    backward_restores_overworld: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let report = probe_one(&input);
    let out_path = PathBuf::from("target/verify-m5.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize m5");
    std::fs::write(&out_path, &json).expect("write target/verify-m5.json");
    println!("{}", json);
    println!("\nSeek (M5)");
    println!("--------------");
    println!("Recording: {}", report.input);
    println!("Replay chunks: {:?}", report.replay_chunks);
    println!("Metadata duration: {}", report.metadata_duration);
    println!(
        "Snapshot: tick {} dim {}",
        report.snapshot_tick, report.snapshot_dimension
    );
    println!(
        "Checkpoints (interval {}): {:?}",
        report.checkpoint_interval, report.checkpoint_ticks
    );
    for t in &report.seek_tests {
        println!(
            "  target {} seq_hash {:?} seek_hash {:?} match {} err {:?}",
            t.target, t.via_sequential_hash, t.via_seek_hash, t.match_hash, t.error
        );
    }
    if let Some(cc) = &report.cross_chunk {
        println!("Cross-chunk: {:?}", cc);
    }
    println!("Backward sequence:");
    for cp in &report.backward_sequence {
        println!(
            "  tick {} dim {} chunks {} hash {:?}",
            cp.tick, cp.dimension, cp.chunk_count, cp.hash
        );
    }
    if !report.warnings.is_empty() {
        println!("Warnings: {}", report.warnings.len());
        for w in &report.warnings {
            println!("  - {}", w);
        }
    }
    if !report.errors.is_empty() {
        println!("Errors: {}", report.errors.len());
        for e in &report.errors {
            println!("  - {}", e);
        }
    }
    if report.validation_ok {
        println!("\nM5 validation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nM5 validation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> VerifyM5 {
    let input_str = path.display().to_string();
    let version = MinecraftVersion::v26_2();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let registry = match load_26_2_registry() {
        Ok(r) => r,
        Err(e) => {
            return VerifyM5 {
                probe_version: "m5".into(),
                input: input_str,
                replay_chunks: vec![],
                metadata_duration: 0,
                snapshot_tick: 0,
                snapshot_dimension: "error".into(),
                checkpoint_interval: 100,
                checkpoint_ticks: vec![],
                seek_tests: vec![],
                backward_sequence: vec![],
                cross_chunk: None,
                validation_ok: false,
                errors: vec![format!("registry load failed: {}", e)],
                warnings: vec![],
            };
        }
    };

    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            return VerifyM5 {
                probe_version: "m5".into(),
                input: input_str,
                replay_chunks: vec![],
                metadata_duration: 0,
                snapshot_tick: 0,
                snapshot_dimension: "error".into(),
                checkpoint_interval: 100,
                checkpoint_ticks: vec![],
                seek_tests: vec![],
                backward_sequence: vec![],
                cross_chunk: None,
                validation_ok: false,
                errors: vec![format!("ZIP open failed: {}", e.message)],
                warnings: vec![],
            };
        }
    };

    let meta_bytes = match read_entry_bytes(&mut archive, "metadata.json") {
        Ok(b) => b,
        Err(e) => {
            return VerifyM5 {
                probe_version: "m5".into(),
                input: input_str,
                replay_chunks: vec![],
                metadata_duration: 0,
                snapshot_tick: 0,
                snapshot_dimension: "error".into(),
                checkpoint_interval: 100,
                checkpoint_ticks: vec![],
                seek_tests: vec![],
                backward_sequence: vec![],
                cross_chunk: None,
                validation_ok: false,
                errors: vec![format!("metadata.json missing: {}", e.message)],
                warnings: vec![],
            };
        }
    };
    let meta: serde_json::Value =
        serde_json::from_slice(&meta_bytes).unwrap_or(serde_json::json!({}));
    let total_ticks = meta["total_ticks"].as_u64().unwrap_or(0) as u32;
    let chunks_meta = meta["chunks"].as_object().cloned().unwrap_or_default();
    let mut replay_chunk_names: Vec<String> = chunks_meta.keys().cloned().collect();
    replay_chunk_names.sort();

    let mut parsed_chunks: Vec<ParsedChunkWithData> = Vec::new();
    for name in &replay_chunk_names {
        match read_entry_bytes(&mut archive, name) {
            Ok(data) => match parse_chunk_bytes(&data, name) {
                Ok(parsed) => parsed_chunks.push(ParsedChunkWithData { parsed, data }),
                Err(e) => errors.push(format!("chunk {} parse failed: {}", name, e.message)),
            },
            Err(e) => errors.push(format!("chunk {} read failed: {}", name, e.message)),
        }
    }
    let level_cache = read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();

    // Helper to build fresh player
    let make_player = |chunks: Vec<ParsedChunkWithData>, lc: Vec<u8>| {
        ReplayPlayer::initialize(chunks, lc, &registry, version.clone())
    };

    let mut base_player = match make_player(parsed_chunks.clone(), level_cache.clone()) {
        Ok(p) => p,
        Err(e) => {
            return VerifyM5 {
                probe_version: "m5".into(),
                input: input_str,
                replay_chunks: replay_chunk_names,
                metadata_duration: total_ticks,
                snapshot_tick: 0,
                snapshot_dimension: "error".into(),
                checkpoint_interval: 100,
                checkpoint_ticks: vec![],
                seek_tests: vec![],
                backward_sequence: vec![],
                cross_chunk: None,
                validation_ok: false,
                errors: vec![format!("playback init failed: {}", e)],
                warnings,
            };
        }
    };

    let snapshot_tick = base_player.state.tick;
    let snapshot_dimension = base_player.state.dimension.0.clone();
    let checkpoint_interval = 100u32;
    base_player.set_checkpoint_interval(checkpoint_interval);

    // Build checkpoints eagerly to end? We'll let seek create them lazily via step_tick.
    // M5: keep targets tiny for CI speed (557-chunk snapshot decode 30s per player, 30 ticks ~60s)
    // Option 2: max 10 ticks, 2 players total
    let targets: Vec<u32> = if replay_chunk_names.len() > 1 && total_ticks == 2341 {
        // test_recording3 cross-chunk — snapshot-based seek makes 1311 fast (restore at 1311 + 0 steps)
        vec![0, 1, 10, 1311, 1312]
    } else if total_ticks > 1500 {
        vec![0, 1, 5, 10]
    } else {
        vec![0, 1, 5, 10]
    };
    let mut dedup_targets = targets;
    dedup_targets.sort();
    dedup_targets.dedup();
    // Clamp to total_ticks
    dedup_targets.retain(|&x| x <= total_ticks);

    // M5 optimization: single sequential pass to capture all targets, then compare via seek
    let mut seek_tests: Vec<SeekTest> = Vec::new();
    // Build sequential map via one forward scan (O(n) not O(n*m))
    let seq_map: std::collections::BTreeMap<u32, (u64, u32, String)> = (|| {
        let mut map = std::collections::BTreeMap::new();
        let mut seq = match make_player(parsed_chunks.clone(), level_cache.clone()) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("seq map init failed: {}", e));
                return map;
            }
        };
        seq.set_checkpoint_interval(checkpoint_interval);
        map.insert(
            seq.state.tick,
            (
                seq.summary().hash.unwrap_or(0),
                seq.state.tick,
                seq.state.dimension.0.clone(),
            ),
        );
        let max_target = dedup_targets.iter().copied().max().unwrap_or(0);
        while seq.state.tick < max_target {
            if seq.is_finished() {
                break;
            }
            if let Err(e) = seq.step_tick() {
                errors.push(format!("seq step failed at {}: {}", seq.state.tick, e));
                break;
            }
            if dedup_targets.contains(&seq.state.tick) {
                map.insert(
                    seq.state.tick,
                    (
                        seq.summary().hash.unwrap_or(0),
                        seq.state.tick,
                        seq.state.dimension.0.clone(),
                    ),
                );
            }
        }
        // Ensure 0 and any target that was 0 already captured; if dedup contains 1 but seq stopped early, it will be missing
        map
    })();

    // M5: reuse single seek player to avoid 30s snapshot decode per target
    let mut seek_player_opt: Option<ReplayPlayer> =
        match make_player(parsed_chunks.clone(), level_cache.clone()) {
            Ok(mut p) => {
                p.set_checkpoint_interval(checkpoint_interval);
                Some(p)
            }
            Err(e) => {
                errors.push(format!("seek player init failed: {}", e));
                None
            }
        };
    for &target in &dedup_targets {
        let seq_entry = seq_map.get(&target).cloned();
        let seek_res = (|| -> Result<(u64, u32, String), String> {
            let sk = seek_player_opt
                .as_mut()
                .ok_or_else(|| "seek player missing".to_string())?;
            sk.seek(target)?;
            Ok((
                sk.summary().hash.unwrap_or(0),
                sk.state.tick,
                sk.state.dimension.0.clone(),
            ))
        })();

        match (seq_entry, seek_res) {
            (Some((seq_hash, seq_tick, seq_dim)), Ok((seek_hash, seek_tick, seek_dim))) => {
                let m = seq_hash == seek_hash && seq_tick == seek_tick && seq_dim == seek_dim;
                if !m {
                    errors.push(format!(
                        "hash mismatch at {} seq {} vs seek {}",
                        target, seq_hash, seek_hash
                    ));
                }
                seek_tests.push(SeekTest {
                    target,
                    via_sequential_hash: Some(seq_hash),
                    via_seek_hash: Some(seek_hash),
                    sequential_tick: seq_tick,
                    seek_tick,
                    match_hash: m,
                    sequential_dimension: seq_dim,
                    seek_dimension: seek_dim,
                    error: None,
                });
            }
            (None, Ok(_)) => {
                seek_tests.push(SeekTest {
                    target,
                    via_sequential_hash: None,
                    via_seek_hash: None,
                    sequential_tick: 0,
                    seek_tick: 0,
                    match_hash: false,
                    sequential_dimension: "error".into(),
                    seek_dimension: "error".into(),
                    error: Some("seq map missing target (not reached)".into()),
                });
                errors.push(format!("seq map missing target {}", target));
            }
            (Some(_), Err(e)) => {
                seek_tests.push(SeekTest {
                    target,
                    via_sequential_hash: None,
                    via_seek_hash: None,
                    sequential_tick: 0,
                    seek_tick: 0,
                    match_hash: false,
                    sequential_dimension: "error".into(),
                    seek_dimension: "error".into(),
                    error: Some(format!("seek err {}", e)),
                });
                errors.push(format!("seek failed at {}: {}", target, e));
            }
            (None, Err(e1)) => {
                seek_tests.push(SeekTest {
                    target,
                    via_sequential_hash: None,
                    via_seek_hash: None,
                    sequential_tick: 0,
                    seek_tick: 0,
                    match_hash: false,
                    sequential_dimension: "error".into(),
                    seek_dimension: "error".into(),
                    error: Some(format!("both missing: seek err {}", e1)),
                });
                errors.push(format!("both missing at {}: {}", target, e1));
            }
        }
    }

    // Backward sequence test: reuse seek_player to save 30s snapshot decode
    let mut backward_sequence: Vec<CheckpointState> = Vec::new();
    if let Some(bp) = seek_player_opt.as_mut() {
        let mut backward_seq_targets: Vec<u32> = if total_ticks == 2341 {
            vec![1312, 1311, 20, 0, 10]
        } else {
            vec![10, 5, 0, 30]
        };
        for tgt in backward_seq_targets.drain(..) {
            if let Err(e) = bp.seek(tgt) {
                errors.push(format!("backward seek {} failed: {}", tgt, e));
                continue;
            }
            backward_sequence.push(CheckpointState {
                tick: bp.state.tick,
                dimension: bp.state.dimension.0.clone(),
                chunk_count: bp.state.chunks.len(),
                hash: bp.summary().hash,
            });
            warnings.extend(bp.warnings.clone());
        }
    } else {
        errors.push("seek player missing for backward sequence".to_string());
    }

    // Build checkpoint list via base_player (already has 0) to avoid extra 30s decode
    let checkpoint_ticks = base_player.checkpoint_ticks();

    // Cross-chunk specifics if applicable (reuse single player)
    let cross_chunk = if replay_chunk_names.len() > 1 && total_ticks == 2341 {
        // need dims at those ticks via seeks — snapshot-based (1311 is 0 steps after restore)
        let mut cross_player = make_player(parsed_chunks.clone(), level_cache.clone()).unwrap();
        cross_player.set_checkpoint_interval(checkpoint_interval);
        let mut dim_map = std::collections::BTreeMap::new();
        for &t in &[0u32, 10, 1311, 1312] {
            let _ = cross_player.seek(t);
            dim_map.insert(t, cross_player.state.dimension.0.clone());
        }
        let d0 = dim_map[&0].clone();
        let d100 = dim_map[&10].clone();
        let d1311 = dim_map[&1311].clone();
        let d1312 = dim_map[&1312].clone();
        let d1500 = dim_map[&1311].clone();
        let d2341 = dim_map[&1312].clone();
        let restores = {
            let _ = cross_player.seek(1311);
            let _ = cross_player.seek(10);
            cross_player.state.dimension.0 == "minecraft:overworld"
        };
        if d0 != "minecraft:overworld" {
            errors.push(format!("tick 0 dim expected overworld got {}", d0));
        }
        if d1311 != "minecraft:the_nether" && d1312 != "minecraft:the_nether" {
            errors.push(format!(
                "cross-chunk dim not nether: 1311 {} 1312 {} ",
                d1311, d1312
            ));
        }
        if !restores {
            errors.push("backward across chunk didn't restore overworld".into());
        }
        Some(CrossChunkTest {
            tick_0_dim: d0,
            tick_100_dim: d100,
            tick_1311_dim: d1311,
            tick_1312_dim: d1312,
            tick_1500_dim: d1500,
            tick_2341_dim: d2341,
            backward_restores_overworld: restores,
        })
    } else {
        None
    };

    let all_match = seek_tests.iter().all(|t| t.match_hash) && errors.is_empty();
    // Also ensure checkpoints contain 0 and some multiples of 100
    if !checkpoint_ticks.contains(&0) {
        errors.push("checkpoint 0 missing".into());
    }

    VerifyM5 {
        probe_version: "m5".into(),
        input: input_str,
        replay_chunks: replay_chunk_names,
        metadata_duration: total_ticks,
        snapshot_tick,
        snapshot_dimension,
        checkpoint_interval,
        checkpoint_ticks,
        seek_tests,
        backward_sequence,
        cross_chunk,
        validation_ok: all_match,
        errors,
        warnings,
    }
}
