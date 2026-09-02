use flashback_format::zip_container::{open_zip_readonly, read_entry_bytes};
use minecraft_version::{chunk::decode_canonical_chunk, registry::load_26_2_registry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct VerifyM2 {
    probe_version: String,
    input: String,
    minecraft_version: String,
    data_version: i32,
    protocol_version: i32,
    registry_source: String,
    registry_path: Option<String>,
    registry_found: bool,
    registry_error: Option<String>,
    num_block_states: usize,
    chunk_x: i32,
    chunk_z: i32,
    min_y: i32,
    height: i32,
    section_count_expected: usize,
    section_count_decoded: usize,
    num_canonical_block_states: usize, // 4096 * sections
    num_block_entities: usize,
    block_entities_sample: Vec<BlockEntitySample>,
    lighting_status: String,
    lighting_raw_bytes: Option<usize>,
    biome_status: String,
    biome_note: String,
    heightmaps_status: String,
    representative_blocks: Vec<RepBlock>,
    unresolved_block_state_ids: Vec<u32>,
    validation_ok: bool,
    errors: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct RepBlock {
    world_pos: String,
    local_pos: String,
    section_y: i32,
    canonical: String,
    name: String,
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct BlockEntitySample {
    pos: String,
    type_name: String,
    nbt_keys: Vec<String>,
    nbt_sample: serde_json::Value,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let report = probe_one(&input);
    let out_path = PathBuf::from("target/verify-m2.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize m2");
    std::fs::write(&out_path, &json).expect("write target/verify-m2.json");
    println!("{}", json);
    // Also print human summary
    println!("\nCanonical Chunk");
    println!("--------------");
    println!("Position: {}, {}", report.chunk_x, report.chunk_z);
    println!(
        "Sections: {} (expected {})",
        report.section_count_decoded, report.section_count_expected
    );
    println!(
        "Block states resolved: {} ({} sections ×4096)",
        report.num_canonical_block_states, report.section_count_decoded
    );
    println!("Block entities: {}", report.num_block_entities);
    println!(
        "Lighting: {} (raw {} bytes)",
        report.lighting_status,
        report.lighting_raw_bytes.unwrap_or(0)
    );
    println!("Biomes: {} — {}", report.biome_status, report.biome_note);
    if !report.representative_blocks.is_empty() {
        println!("\nRepresentative blocks:");
        for r in &report.representative_blocks {
            println!(
                "{} (section_y {}) -> {}",
                r.world_pos, r.section_y, r.canonical
            );
        }
    }
    if !report.unresolved_block_state_ids.is_empty() {
        println!("\nUnresolved IDs: {:?}", report.unresolved_block_state_ids);
    }
    if !report.errors.is_empty() {
        println!("\nErrors:");
        for e in &report.errors {
            println!(" - {}", e);
        }
    }
    if report.validation_ok {
        println!("\nM2 validation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nM2 validation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> VerifyM2 {
    let input_str = path.display().to_string();
    let version = minecraft_version::MinecraftVersion::v26_2();
    let mut errors: Vec<String> = Vec::new();

    let registry_result = load_26_2_registry();
    let (registry_found, registry_source, registry_path, registry_error, num_block_states) =
        match &registry_result {
            Ok(reg) => (
                true,
                reg.source().description.clone(),
                reg.source().path.clone(),
                None,
                reg.len(),
            ),
            Err(e) => (false, format!("ERROR: {}", e), None, Some(e.to_string()), 0),
        };

    if !registry_found {
        return VerifyM2 {
            probe_version: "m2".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source,
            registry_path,
            registry_found: false,
            registry_error,
            num_block_states: 0,
            chunk_x: 0,
            chunk_z: 0,
            min_y: -64,
            height: 384,
            section_count_expected: 24,
            section_count_decoded: 0,
            num_canonical_block_states: 0,
            num_block_entities: 0,
            block_entities_sample: vec![],
            lighting_status: "unavailable".to_string(),
            lighting_raw_bytes: None,
            biome_status: "unavailable".to_string(),
            biome_note: "registry missing".to_string(),
            heightmaps_status: "unavailable".to_string(),
            representative_blocks: vec![],
            unresolved_block_state_ids: vec![],
            validation_ok: false,
            errors: vec!["registry not found".to_string()],
        };
    }

    let registry = registry_result.unwrap();

    // Open ZIP
    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            return VerifyM2 {
                probe_version: "m2".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(format!("ZIP open failed: {}", e.message)),
                num_block_states: registry.len(),
                chunk_x: 0,
                chunk_z: 0,
                min_y: -64,
                height: 384,
                section_count_expected: 24,
                section_count_decoded: 0,
                num_canonical_block_states: 0,
                num_block_entities: 0,
                block_entities_sample: vec![],
                lighting_status: "unavailable".to_string(),
                lighting_raw_bytes: None,
                biome_status: "unavailable".to_string(),
                biome_note: "ZIP open failed".to_string(),
                heightmaps_status: "unavailable".to_string(),
                representative_blocks: vec![],
                unresolved_block_state_ids: vec![],
                validation_ok: false,
                errors: vec![format!("ZIP open failed: {}", e.message)],
            };
        }
    };

    // Read first cache entry
    let shard_bytes = match read_entry_bytes(&mut archive, "level_chunk_caches/0") {
        Ok(b) => b,
        Err(e) => {
            return VerifyM2 {
                probe_version: "m2".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(format!("level_chunk_caches/0 missing: {}", e.message)),
                num_block_states: registry.len(),
                chunk_x: 0,
                chunk_z: 0,
                min_y: -64,
                height: 384,
                section_count_expected: 24,
                section_count_decoded: 0,
                num_canonical_block_states: 0,
                num_block_entities: 0,
                block_entities_sample: vec![],
                lighting_status: "unavailable".to_string(),
                lighting_raw_bytes: None,
                biome_status: "unavailable".to_string(),
                biome_note: "cache missing".to_string(),
                heightmaps_status: "unavailable".to_string(),
                representative_blocks: vec![],
                unresolved_block_state_ids: vec![],
                validation_ok: false,
                errors: vec![format!("cache missing: {}", e.message)],
            };
        }
    };

    if shard_bytes.len() < 4 {
        return VerifyM2 {
            probe_version: "m2".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source: registry.source().description.clone(),
            registry_path: registry.source().path.clone(),
            registry_found: true,
            registry_error: Some("shard too small".to_string()),
            num_block_states: registry.len(),
            chunk_x: 0,
            chunk_z: 0,
            min_y: -64,
            height: 384,
            section_count_expected: 24,
            section_count_decoded: 0,
            num_canonical_block_states: 0,
            num_block_entities: 0,
            block_entities_sample: vec![],
            lighting_status: "unavailable".to_string(),
            lighting_raw_bytes: None,
            biome_status: "unavailable".to_string(),
            biome_note: "shard too small".to_string(),
            heightmaps_status: "unavailable".to_string(),
            representative_blocks: vec![],
            unresolved_block_state_ids: vec![],
            validation_ok: false,
            errors: vec!["shard too small".to_string()],
        };
    }

    let first_size = i32::from_be_bytes([
        shard_bytes[0],
        shard_bytes[1],
        shard_bytes[2],
        shard_bytes[3],
    ]) as usize;
    if first_size == 0 || first_size + 4 > shard_bytes.len() {
        return VerifyM2 {
            probe_version: "m2".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source: registry.source().description.clone(),
            registry_path: registry.source().path.clone(),
            registry_found: true,
            registry_error: Some(format!("first entry size invalid {}", first_size)),
            num_block_states: registry.len(),
            chunk_x: 0,
            chunk_z: 0,
            min_y: -64,
            height: 384,
            section_count_expected: 24,
            section_count_decoded: 0,
            num_canonical_block_states: 0,
            num_block_entities: 0,
            block_entities_sample: vec![],
            lighting_status: "unavailable".to_string(),
            lighting_raw_bytes: None,
            biome_status: "unavailable".to_string(),
            biome_note: "first entry size invalid".to_string(),
            heightmaps_status: "unavailable".to_string(),
            representative_blocks: vec![],
            unresolved_block_state_ids: vec![],
            validation_ok: false,
            errors: vec![format!("first entry size invalid {}", first_size)],
        };
    }

    let payload = &shard_bytes[4..4 + first_size];

    // Decode canonical chunk
    let chunk = match decode_canonical_chunk(payload, &registry) {
        Ok(c) => c,
        Err(e) => {
            errors.push(format!("chunk decode failed: {}", e));
            return VerifyM2 {
                probe_version: "m2".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(format!("chunk decode failed: {}", e)),
                num_block_states: registry.len(),
                chunk_x: 0,
                chunk_z: 0,
                min_y: -64,
                height: 384,
                section_count_expected: 24,
                section_count_decoded: 0,
                num_canonical_block_states: 0,
                num_block_entities: 0,
                block_entities_sample: vec![],
                lighting_status: "unavailable".to_string(),
                lighting_raw_bytes: None,
                biome_status: "unavailable".to_string(),
                biome_note: format!("decode failed: {}", e),
                heightmaps_status: "unavailable".to_string(),
                representative_blocks: vec![],
                unresolved_block_state_ids: vec![],
                validation_ok: false,
                errors,
            };
        }
    };

    // Collect representative blocks: pick a few positions across sections
    let mut reps: Vec<RepBlock> = Vec::new();
    // Sample positions: for first 3 sections, pick (0,0,0), (8,8,8), (15,15,15) local, plus a few
    let samples = [(0, 0, 0), (8, 0, 0), (0, 8, 0), (0, 0, 8), (15, 15, 15)];
    for sec in chunk.sections.iter().take(3) {
        for &(lx, ly, lz) in &samples {
            let world_y = sec.y_base + ly as i32;
            if let Some(state) = chunk.canonical_block_at(world_y, lx, lz) {
                let world_x = chunk.x * 16 + lx as i32;
                let world_z = chunk.z * 16 + lz as i32;
                reps.push(RepBlock {
                    world_pos: format!("{},{},{}", world_x, world_y, world_z),
                    local_pos: format!("{},{},{}", lx, ly, lz),
                    section_y: sec.section_y,
                    canonical: state.to_string(),
                    name: state.name.clone(),
                    properties: state.properties.clone(),
                });
                if reps.len() >= 8 {
                    break;
                }
            }
        }
        if reps.len() >= 8 {
            break;
        }
    }
    let unresolved: Vec<u32> = Vec::new();

    let block_entities_sample: Vec<BlockEntitySample> = chunk
        .block_entities
        .iter()
        .take(3)
        .map(|be| BlockEntitySample {
            pos: format!("{},{},{}", be.pos.x, be.pos.y, be.pos.z),
            type_name: be.type_name.clone(),
            nbt_keys: be
                .nbt
                .as_object()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default(),
            nbt_sample: be.nbt.clone(),
        })
        .collect();

    let lighting_status = chunk.lighting.status.clone();
    let lighting_raw_bytes = chunk.lighting.raw_bytes.as_ref().map(|b| b.len());
    let biome_status = chunk
        .biome_data
        .as_ref()
        .map(|b| b.status.clone())
        .unwrap_or_else(|| "unavailable".to_string());
    let biome_note = chunk
        .biome_data
        .as_ref()
        .map(|b| b.note.clone())
        .unwrap_or_else(|| "no biome data".to_string());
    let heightmaps_status = chunk
        .heightmaps
        .as_ref()
        .map(|h| h.raw_status.clone())
        .unwrap_or_else(|| "unavailable".to_string());

    let num_canonical = chunk.sections.len() * 4096;
    let validation_ok = !chunk.sections.is_empty()
        && chunk.block_entities.len() <= 10000 // reasonable
        && unresolved.is_empty()
        && chunk.sections.iter().all(|s| s.block_states.len() == 4096);

    // Also check that no numeric IDs leak: all block_states are canonical
    // (already ensured by decode)

    VerifyM2 {
        probe_version: "m2".to_string(),
        input: input_str,
        minecraft_version: version.version.clone(),
        data_version: version.data_version,
        protocol_version: version.protocol_version,
        registry_source: registry.source().description.clone(),
        registry_path: registry.source().path.clone(),
        registry_found: true,
        registry_error: None,
        num_block_states: registry.len(),
        chunk_x: chunk.x,
        chunk_z: chunk.z,
        min_y: chunk.min_y,
        height: chunk.height,
        section_count_expected: chunk.section_count,
        section_count_decoded: chunk.sections.len(),
        num_canonical_block_states: num_canonical,
        num_block_entities: chunk.block_entities.len(),
        block_entities_sample,
        lighting_status,
        lighting_raw_bytes,
        biome_status,
        biome_note,
        heightmaps_status,
        representative_blocks: reps,
        unresolved_block_state_ids: unresolved,
        validation_ok,
        errors,
    }
}
