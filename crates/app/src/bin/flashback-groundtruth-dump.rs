use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::{registry::load_26_2_registry, MinecraftVersion};
use playback::{ParsedChunkWithData, ReplayPlayer};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("recordings/chunks/test_recording3.zip");
    let tick: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let reg = load_26_2_registry().expect("registry");
    let ver = MinecraftVersion::v26_2();
    let mut archive = open_zip_readonly(std::path::Path::new(path)).expect("open");
    let meta_bytes = read_entry_bytes(&mut archive, "metadata.json").unwrap();
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).unwrap();
    let total = meta["total_ticks"].as_u64().unwrap_or(0);
    println!("Recording: {} total_ticks {} seek {}", path, total, tick);
    let chunks_meta = meta["chunks"].as_object().cloned().unwrap_or_default();
    let mut names: Vec<String> = chunks_meta.keys().cloned().collect();
    names.sort();
    let mut parsed_chunks = Vec::new();
    for name in &names {
        let data = read_entry_bytes(&mut archive, name).unwrap();
        let parsed = parse_chunk_bytes(&data, name).unwrap();
        parsed_chunks.push(ParsedChunkWithData { parsed, data });
    }
    let lc = read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();
    let mut player = ReplayPlayer::initialize(parsed_chunks, lc, &reg, ver).unwrap();
    if tick > 0 {
        player.seek(tick).unwrap();
    }
    println!(
        "Tick {} dim {} chunks {} entities {}",
        player.state.tick,
        player.state.dimension.0,
        player.state.chunks.len(),
        player.state.entities.len()
    );
    // Dump per chunk
    let mut sorted: Vec<_> = player.state.chunks.iter().collect();
    sorted.sort_by_key(|((x, z), _)| (*x, *z));
    for ((cx, cz), chunk) in &sorted {
        println!(
            "\nCHUNK ({},{}) min_y {} height {} sections {} non_empty_total {}",
            cx,
            cz,
            chunk.min_y,
            chunk.height,
            chunk.sections.len(),
            chunk.non_empty_count
        );
        for sec in &chunk.sections {
            let mut counts: BTreeMap<String, usize> = BTreeMap::new();
            for st in &sec.block_states {
                *counts.entry(st.name.clone()).or_insert(0) += 1;
            }
            let air = counts.get("minecraft:air").cloned().unwrap_or(0);
            let non_air = 4096 - air;
            // Top 5
            let mut top: Vec<_> = counts.iter().collect();
            top.sort_by(|a, b| b.1.cmp(a.1));
            let top_str: String = top
                .iter()
                .take(5)
                .map(|(k, v)| format!("{}:{}", k.trim_start_matches("minecraft:"), v))
                .collect::<Vec<_>>()
                .join(", ");
            println!("  Section y={} y_base={} non_empty={} palette_bits={} palette_size={} air={} non_air={} top=[{}] empty={} renderable={} lighting_sky={} block={} entities={}",
                sec.section_y, sec.y_base, sec.non_empty_block_count, sec.palette_bits, sec.palette_size, air, non_air, top_str, sec.block_states.iter().all(|s| s.name=="minecraft:air"), sec.block_states.iter().any(|s| s.name!="minecraft:air"), sec.sky_light.is_some(), sec.block_light.is_some(), sec.block_entities.len());
        }
        println!(
            "  BlockEntities: {} {:?}",
            chunk.block_entities.len(),
            chunk
                .block_entities
                .iter()
                .map(|be| format!("{}@{}, {}, {}", be.type_name, be.pos.x, be.pos.y, be.pos.z))
                .collect::<Vec<_>>()
                .join("; ")
        );
        // Heightmap sample
        if let Some(hm) = &chunk.heightmaps {
            for (k, v) in &hm.heightmaps {
                println!("  Heightmap {} len {}", k, v.len());
            }
        }
        // Dump all chunks (no limit for M9 audit)
    }
    // Also dump local player and entities
    if let Some(lp) = &player.state.local_player {
        println!(
            "\nLocalPlayer pos {:.2},{:.2},{:.2} yaw {:.1} pitch {:.1} vel {:?}",
            lp.pos[0], lp.pos[1], lp.pos[2], lp.yaw, lp.pitch, lp.velocity
        );
    }
    println!("\nEntities: {}", player.state.entities.len());
    for e in player.state.entities.iter().take(5) {
        println!(
            "  id={} type={:?} pos={:?} vel={:?} dim={:?}",
            e.entity_id, e.entity_type, e.pos, e.velocity, e.dimension
        );
    }
}
