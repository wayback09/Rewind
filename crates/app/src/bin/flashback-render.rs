//! flashback-render — load recording, seek to tick, build Scene, launch wgpu renderer.

use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::{registry::load_26_2_registry, MinecraftVersion};
use playback::{ParsedChunkWithData, ReplayPlayer};
use scene::{SceneBuilder, StubAssetProvider};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip> [tick]", args[0]);
        eprintln!(
            "Example: {} recordings/chunks/test_recording3.zip 0",
            args[0]
        );
        std::process::exit(1);
    }
    let path = PathBuf::from(&args[1]);
    let tick: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

    let registry = load_26_2_registry().expect("26.2 registry");
    let version = MinecraftVersion::v26_2();
    let mut archive = open_zip_readonly(&path).expect("open zip");
    let meta_bytes = read_entry_bytes(&mut archive, "metadata.json").expect("metadata");
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).unwrap();
    let total_ticks = meta["total_ticks"].as_u64().unwrap_or(0) as u32;
    println!(
        "Recording: {} ({} ticks, seek {})",
        path.display(),
        total_ticks,
        tick
    );

    let chunks_meta = meta["chunks"].as_object().cloned().unwrap_or_default();
    let mut names: Vec<String> = chunks_meta.keys().cloned().collect();
    names.sort();
    let mut parsed_chunks = Vec::new();
    for name in &names {
        let data = read_entry_bytes(&mut archive, name).expect("chunk");
        let parsed = parse_chunk_bytes(&data, name).expect("parse");
        parsed_chunks.push(ParsedChunkWithData { parsed, data });
    }
    let level_cache = read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();

    let mut player =
        ReplayPlayer::initialize(parsed_chunks, level_cache, &registry, version).expect("init");
    if tick > 0 {
        println!("Seeking to {}...", tick);
        player.seek(tick).expect("seek");
    }
    println!(
        "Tick {} dim {} chunks {} entities {}",
        player.state.tick,
        player.state.dimension.0,
        player.state.chunks.len(),
        player.state.entities.len()
    );

    let provider = StubAssetProvider;
    let builder = SceneBuilder::new(&provider);
    let t0 = std::time::Instant::now();
    let scene = builder.from_replay_state(&player.state);
    let build_ms = t0.elapsed().as_millis();
    println!(
        "Scene built in {}ms: {} chunks, {} sections, {} blocks",
        build_ms,
        scene.chunk_count(),
        scene.total_sections,
        scene.total_blocks
    );

    // Launch renderer (blocking)
    renderer::run_blocking(scene);
}
