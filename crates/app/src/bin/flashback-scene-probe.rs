use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::{registry::load_26_2_registry, MinecraftVersion};
use playback::{ParsedChunkWithData, ReplayPlayer};
use scene::{fingerprint, SceneBuilder, StubAssetProvider};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Serialize, Deserialize)]
struct VerifyM6 {
    probe_version: String,
    input: String,
    minecraft_version: String,
    data_version: i32,
    protocol_version: i32,
    total_ticks: u32,
    replay_chunks: Vec<String>,
    scenes: Vec<SceneSummary>,
    seek_invariants: Vec<SeekInvariant>,
    cross_chunk: Option<CrossChunk>,
    construction_time_ms: u64,
    validation_ok: bool,
    errors: Vec<String>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SceneSummary {
    tick: u32,
    dimension: String,
    dimension_source: String,
    chunk_count: usize,
    section_count: usize,
    total_blocks: usize,
    renderable_blocks: usize,
    block_entity_count: usize,
    entity_count: usize,
    local_player_present: bool,
    local_player_pos: Option<[f64; 3]>,
    environment_status: String,
    lighting_status: String,
    biome_status: String,
    asset_dependency_count: usize,
    asset_keys_sample: Vec<String>,
    fingerprint: u64,
    scene_build_ms: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct SeekInvariant {
    tick: u32,
    sequential_fingerprint: Option<u64>,
    seek_fingerprint: Option<u64>,
    match_: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CrossChunk {
    tick_0_dim: String,
    tick_1311_dim: String,
    tick_1312_dim: String,
    tick_final_dim: String,
    dimension_changed: bool,
    fingerprint_match_1311: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let report = probe_one(&input);
    let out_path = PathBuf::from("target/verify-m6.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize m6");
    // per-recording
    let stem = input
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");
    let parent_name = input
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let per_path = PathBuf::from(format!("target/verify-m6-{}_{}.json", parent_name, stem));
    let _ = std::fs::write(&per_path, &json);
    std::fs::write(&out_path, &json).expect("write target/verify-m6.json");
    println!("{}", json);
    println!("\nScene (M6)");
    println!("--------------");
    println!("Recording: {}", report.input);
    println!("Total ticks: {}", report.total_ticks);
    for s in &report.scenes {
        println!(
            " tick {} dim {} chunks {} sections {} blocks {} renderable {} fp {}",
            s.tick,
            s.dimension,
            s.chunk_count,
            s.section_count,
            s.total_blocks,
            s.renderable_blocks,
            s.fingerprint
        );
    }
    for inv in &report.seek_invariants {
        println!(
            " seek invariant tick {} seq {:?} seek {:?} match {} err {:?}",
            inv.tick, inv.sequential_fingerprint, inv.seek_fingerprint, inv.match_, inv.error
        );
    }
    if let Some(cc) = &report.cross_chunk {
        println!("Cross-chunk: {:?}", cc);
    }
    if !report.warnings.is_empty() {
        for w in &report.warnings {
            println!(" warn {}", w);
        }
    }
    if !report.errors.is_empty() {
        for e in &report.errors {
            println!(" err {}", e);
        }
    }
    println!("construction_time_ms: {}", report.construction_time_ms);
    if report.validation_ok {
        println!("\nM6 validation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nM6 validation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> VerifyM6 {
    let input_str = path.display().to_string();
    let version = MinecraftVersion::v26_2();
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let registry = match load_26_2_registry() {
        Ok(r) => r,
        Err(e) => {
            return VerifyM6 {
                probe_version: "m6".into(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                total_ticks: 0,
                replay_chunks: vec![],
                scenes: vec![],
                seek_invariants: vec![],
                cross_chunk: None,
                construction_time_ms: 0,
                validation_ok: false,
                errors: vec![format!("registry: {}", e)],
                warnings: vec![],
            }
        }
    };
    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            return VerifyM6 {
                probe_version: "m6".into(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                total_ticks: 0,
                replay_chunks: vec![],
                scenes: vec![],
                seek_invariants: vec![],
                cross_chunk: None,
                construction_time_ms: 0,
                validation_ok: false,
                errors: vec![format!("zip: {}", e.message)],
                warnings: vec![],
            }
        }
    };
    let meta_bytes = match read_entry_bytes(&mut archive, "metadata.json") {
        Ok(b) => b,
        Err(e) => {
            return VerifyM6 {
                probe_version: "m6".into(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                total_ticks: 0,
                replay_chunks: vec![],
                scenes: vec![],
                seek_invariants: vec![],
                cross_chunk: None,
                construction_time_ms: 0,
                validation_ok: false,
                errors: vec![format!("metadata: {}", e.message)],
                warnings: vec![],
            }
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
                Err(e) => errors.push(format!("chunk {} parse: {}", name, e.message)),
            },
            Err(e) => errors.push(format!("chunk {} read: {}", name, e.message)),
        }
    }
    let level_cache = read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();

    // Determine tick targets: spec says tick 0, tick 100, final tick; plus for test_recording3 tick 1311,1312
    let mut targets: Vec<u32> = vec![0];
    if total_ticks >= 1 {
        targets.push(1);
    }
    if total_ticks >= 100 {
        targets.push(100);
    }
    if total_ticks >= 500 {
        targets.push(500);
    }
    if replay_chunk_names.len() > 1 && total_ticks >= 1311 {
        targets.push(1311);
    }
    if replay_chunk_names.len() > 1 && total_ticks >= 1312 {
        targets.push(1312);
    }
    if total_ticks > 0 {
        targets.push(total_ticks);
    }
    targets.sort();
    targets.dedup();
    targets.retain(|&t| t <= total_ticks);

    // Helper to build player fresh
    let make_player = |chunks: Vec<ParsedChunkWithData>, lc: Vec<u8>| {
        ReplayPlayer::initialize(chunks, lc, &registry, version.clone())
    };

    let provider = StubAssetProvider;
    let builder = SceneBuilder::new(&provider);

    let mut scenes: Vec<SceneSummary> = Vec::new();
    let mut seek_invariants: Vec<SeekInvariant> = Vec::new();
    let mut construction_time_ms: u64 = 0;

    // Build scenes via sequential (actually via seek from fresh player — deterministic single path, but we treat as sequential baseline via one forward scan)
    // Optimized: single sequential scan to capture fingerprints, then seek player to compare.
    // For scene invariant we need both paths: sequential playback vs seek. We'll capture via two players per target to prove determinism.
    // Since we want to avoid 30s * N snapshot decodes, reuse players where possible but ensure fresh for determinism proof.

    // Sequential map
    let seq_map: std::collections::BTreeMap<u32, (u64, SceneSummary)> = (|| {
        let mut map = std::collections::BTreeMap::new();
        let mut seq = match make_player(parsed_chunks.clone(), level_cache.clone()) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("seq init: {}", e));
                return map;
            }
        };
        seq.set_checkpoint_interval(100);
        let t0 = Instant::now();
        let scene0 = builder.from_replay_state(&seq.state);
        let fp0 = fingerprint(&scene0);
        let ms0 = t0.elapsed().as_millis() as u64;
        construction_time_ms += ms0;
        let summary0 = summarize(&scene0, fp0, ms0);
        map.insert(seq.state.tick, (fp0, summary0));
        let max_target = *targets.iter().max().unwrap_or(&0);
        while seq.state.tick < max_target {
            if seq.is_finished() {
                break;
            }
            if let Err(e) = seq.step_tick() {
                errors.push(format!("seq step at {}: {}", seq.state.tick, e));
                break;
            }
            if targets.contains(&seq.state.tick) {
                let t = Instant::now();
                let scene = builder.from_replay_state(&seq.state);
                let fp = fingerprint(&scene);
                let ms = t.elapsed().as_millis() as u64;
                let summary = summarize(&scene, fp, ms);
                map.insert(seq.state.tick, (fp, summary));
            }
        }
        map
    })();

    // For seek invariant, use fresh player per target to prove equivalence (or reuse single to save time but still compare)
    let mut seek_player = match make_player(parsed_chunks.clone(), level_cache.clone()) {
        Ok(p) => p,
        Err(e) => {
            errors.push(format!("seek init: {}", e));
            return VerifyM6 {
                probe_version: "m6".into(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                total_ticks,
                replay_chunks: replay_chunk_names,
                scenes: vec![],
                seek_invariants: vec![],
                cross_chunk: None,
                construction_time_ms,
                validation_ok: false,
                errors,
                warnings,
            };
        }
    };
    seek_player.set_checkpoint_interval(100);

    for &t in &targets {
        let seq_entry = seq_map.get(&t).cloned();
        // seek
        let seek_res: Result<u64, String> = (|| {
            seek_player.seek(t).map_err(|e| e.to_string())?;
            let s = Instant::now();
            let scene = builder.from_replay_state(&seek_player.state);
            let fp = fingerprint(&scene);
            let _ms = s.elapsed().as_millis() as u64;
            Ok(fp)
        })();
        match (seq_entry, seek_res) {
            (Some((seq_fp, seq_summary)), Ok(seek_fp)) => {
                let m = seq_fp == seek_fp;
                if !m {
                    errors.push(format!(
                        "fingerprint mismatch at {} seq {} vs seek {}",
                        t, seq_fp, seek_fp
                    ));
                }
                // push to scenes list from seq (avoid duplicate)
                if !scenes.iter().any(|s| s.tick == t) {
                    scenes.push(seq_summary);
                }
                seek_invariants.push(SeekInvariant {
                    tick: t,
                    sequential_fingerprint: Some(seq_fp),
                    seek_fingerprint: Some(seek_fp),
                    match_: m,
                    error: None,
                });
            }
            (Some(_), Err(e)) => {
                seek_invariants.push(SeekInvariant {
                    tick: t,
                    sequential_fingerprint: None,
                    seek_fingerprint: None,
                    match_: false,
                    error: Some(format!("seek err {}", e)),
                });
                errors.push(format!("seek failed at {}: {}", t, e));
            }
            (None, Ok(seek_fp)) => {
                // seq didn't reach (should not happen), still report seek
                let s = Instant::now();
                let scene = builder.from_replay_state(&seek_player.state);
                let fp = fingerprint(&scene);
                let ms = s.elapsed().as_millis() as u64;
                let summary = summarize(&scene, fp, ms);
                scenes.push(summary);
                seek_invariants.push(SeekInvariant {
                    tick: t,
                    sequential_fingerprint: None,
                    seek_fingerprint: Some(seek_fp),
                    match_: false,
                    error: Some("seq missing".into()),
                });
            }
            (None, Err(e)) => {
                seek_invariants.push(SeekInvariant {
                    tick: t,
                    sequential_fingerprint: None,
                    seek_fingerprint: None,
                    match_: false,
                    error: Some(format!("both missing seek err {}", e)),
                });
                errors.push(format!("both missing at {}", t));
            }
        }
    }
    scenes.sort_by_key(|s| s.tick);

    // Cross-chunk specifics
    let cross_chunk = if replay_chunk_names.len() > 1 && total_ticks == 2341 {
        let d0 = scenes
            .iter()
            .find(|s| s.tick == 0)
            .map(|s| s.dimension.clone())
            .unwrap_or_default();
        let d1311 = scenes
            .iter()
            .find(|s| s.tick == 1311)
            .map(|s| s.dimension.clone())
            .unwrap_or_default();
        let d1312 = scenes
            .iter()
            .find(|s| s.tick == 1312)
            .map(|s| s.dimension.clone())
            .unwrap_or_default();
        let dfinal = scenes
            .iter()
            .find(|s| s.tick == total_ticks)
            .map(|s| s.dimension.clone())
            .unwrap_or_default();
        let dim_changed = d0 != d1311;
        let fp_match = seek_invariants
            .iter()
            .find(|i| i.tick == 1311)
            .map(|i| i.match_)
            .unwrap_or(false);
        if d0 != "minecraft:overworld" {
            errors.push(format!("tick 0 dim expected overworld got {}", d0));
        }
        if d1311 != "minecraft:the_nether" && d1312 != "minecraft:the_nether" {
            errors.push(format!("1311/1312 not nether: {} {}", d1311, d1312));
        }
        Some(CrossChunk {
            tick_0_dim: d0,
            tick_1311_dim: d1311,
            tick_1312_dim: d1312,
            tick_final_dim: dfinal,
            dimension_changed: dim_changed,
            fingerprint_match_1311: fp_match,
        })
    } else {
        None
    };

    // Collect warnings from last player
    warnings.extend(seek_player.warnings.clone());

    let all_match = seek_invariants.iter().all(|i| i.match_) && errors.is_empty();
    VerifyM6 {
        probe_version: "m6".into(),
        input: input_str,
        minecraft_version: version.version.clone(),
        data_version: version.data_version,
        protocol_version: version.protocol_version,
        total_ticks,
        replay_chunks: replay_chunk_names,
        scenes,
        seek_invariants,
        cross_chunk,
        construction_time_ms,
        validation_ok: all_match,
        errors,
        warnings,
    }
}

fn summarize(scene: &scene::Scene, fp: u64, build_ms: u64) -> SceneSummary {
    SceneSummary {
        tick: scene.tick,
        dimension: scene.environment.dimension.clone(),
        dimension_source: scene.environment.dimension_source.clone(),
        chunk_count: scene.chunk_count(),
        section_count: scene.total_sections,
        total_blocks: scene.total_blocks,
        renderable_blocks: scene.renderable_blocks,
        block_entity_count: scene.block_entity_count,
        entity_count: scene.entities.len(),
        local_player_present: scene.local_player.is_some(),
        local_player_pos: scene.local_player.as_ref().map(|lp| lp.pos),
        environment_status: format!("{:?}", scene.environment.lighting_status),
        lighting_status: format!(
            "{:?}",
            scene
                .chunks
                .values()
                .next()
                .map(|c| c.lighting.status.clone())
                .unwrap_or(scene::LightingStatus::Unavailable)
        ),
        biome_status: format!(
            "{:?}",
            scene
                .chunks
                .values()
                .next()
                .map(|c| c.biome.status.clone())
                .unwrap_or(scene::BiomeStatus::Unavailable)
        ),
        asset_dependency_count: scene.asset_dependency_count,
        asset_keys_sample: scene.asset_keys.iter().take(10).cloned().collect(),
        fingerprint: fp,
        scene_build_ms: build_ms,
    }
}
