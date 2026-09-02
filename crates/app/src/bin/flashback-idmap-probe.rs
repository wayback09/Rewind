use flashback_format::zip_container::{open_zip_readonly, read_entry_bytes};
use minecraft_version::{
    palette::decode_chunk_packet, registry::load_26_2_registry, BlockStateRegistry,
    MinecraftVersion,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct VerifyM1 {
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
    num_sections_inspected: usize,
    num_palette_entries_inspected: usize,
    num_successfully_resolved: usize,
    num_unresolved: usize,
    unresolved_ids: Vec<u32>,
    representative_resolved: Vec<Representative>,
    all_resolved_distinct: BTreeSet<u32>,
    palette_per_section: Vec<SectionReport>,
    validation_ok: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct Representative {
    palette_entry_global_id: u32,
    canonical: String,
    name: String,
    properties: BTreeMap<String, String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SectionReport {
    section_index: usize,
    non_empty_block_count: u16,
    bits: u8,
    palette_global_ids: Vec<u32>,
    palette_resolved_names: Vec<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        std::process::exit(1);
    }
    let input = PathBuf::from(&args[1]);
    let report = probe_one(&input);
    let out_path = PathBuf::from("target/verify-m1.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize m1");
    std::fs::write(&out_path, &json).expect("write target/verify-m1.json");
    println!("{}", json);
    if report.validation_ok {
        println!("\nM1 validation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nM1 validation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> VerifyM1 {
    let input_str = path.display().to_string();
    let version = MinecraftVersion::v26_2();

    // Try to load registry
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
        // Produce diagnostic
        return VerifyM1 {
            probe_version: "m1".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source: registry_source.clone(),
            registry_path,
            registry_found: false,
            registry_error,
            num_block_states: 0,
            num_sections_inspected: 0,
            num_palette_entries_inspected: 0,
            num_successfully_resolved: 0,
            num_unresolved: 0,
            unresolved_ids: vec![],
            representative_resolved: vec![],
            all_resolved_distinct: BTreeSet::new(),
            palette_per_section: vec![],
            validation_ok: false,
        };
    }

    let registry = registry_result.unwrap();

    // Open ZIP and read first cache entry
    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            let err_str = format!("failed to open ZIP: {}", e.message);
            return VerifyM1 {
                probe_version: "m1".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(err_str.clone()),
                num_block_states: registry.len(),
                num_sections_inspected: 0,
                num_palette_entries_inspected: 0,
                num_successfully_resolved: 0,
                num_unresolved: 0,
                unresolved_ids: vec![],
                representative_resolved: vec![],
                all_resolved_distinct: BTreeSet::new(),
                palette_per_section: vec![],
                validation_ok: false,
            };
        }
    };

    // Read level_chunk_caches/0
    let shard_bytes = match read_entry_bytes(&mut archive, "level_chunk_caches/0") {
        Ok(b) => b,
        Err(e) => {
            return VerifyM1 {
                probe_version: "m1".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(format!("level_chunk_caches/0 missing: {}", e.message)),
                num_block_states: registry.len(),
                num_sections_inspected: 0,
                num_palette_entries_inspected: 0,
                num_successfully_resolved: 0,
                num_unresolved: 0,
                unresolved_ids: vec![],
                representative_resolved: vec![],
                all_resolved_distinct: BTreeSet::new(),
                palette_per_section: vec![],
                validation_ok: false,
            };
        }
    };

    // Parse first entry: BE size + payload
    if shard_bytes.len() < 4 {
        return VerifyM1 {
            probe_version: "m1".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source: registry.source().description.clone(),
            registry_path: registry.source().path.clone(),
            registry_found: true,
            registry_error: Some("level_chunk_caches/0 too small".to_string()),
            num_block_states: registry.len(),
            num_sections_inspected: 0,
            num_palette_entries_inspected: 0,
            num_successfully_resolved: 0,
            num_unresolved: 0,
            unresolved_ids: vec![],
            representative_resolved: vec![],
            all_resolved_distinct: BTreeSet::new(),
            palette_per_section: vec![],
            validation_ok: false,
        };
    }
    let first_size = i32::from_be_bytes([
        shard_bytes[0],
        shard_bytes[1],
        shard_bytes[2],
        shard_bytes[3],
    ]);
    if first_size <= 0 || (first_size as usize) + 4 > shard_bytes.len() {
        return VerifyM1 {
            probe_version: "m1".to_string(),
            input: input_str,
            minecraft_version: version.version.clone(),
            data_version: version.data_version,
            protocol_version: version.protocol_version,
            registry_source: registry.source().description.clone(),
            registry_path: registry.source().path.clone(),
            registry_found: true,
            registry_error: Some(format!("first cache entry size invalid {}", first_size)),
            num_block_states: registry.len(),
            num_sections_inspected: 0,
            num_palette_entries_inspected: 0,
            num_successfully_resolved: 0,
            num_unresolved: 0,
            unresolved_ids: vec![],
            representative_resolved: vec![],
            all_resolved_distinct: BTreeSet::new(),
            palette_per_section: vec![],
            validation_ok: false,
        };
    }
    let payload = &shard_bytes[4..4 + first_size as usize];

    // Decode chunk packet
    let palettes = match decode_chunk_packet(payload) {
        Ok(p) => p,
        Err(e) => {
            return VerifyM1 {
                probe_version: "m1".to_string(),
                input: input_str,
                minecraft_version: version.version.clone(),
                data_version: version.data_version,
                protocol_version: version.protocol_version,
                registry_source: registry.source().description.clone(),
                registry_path: registry.source().path.clone(),
                registry_found: true,
                registry_error: Some(format!("palette decode failed: {}", e)),
                num_block_states: registry.len(),
                num_sections_inspected: 0,
                num_palette_entries_inspected: 0,
                num_successfully_resolved: 0,
                num_unresolved: 0,
                unresolved_ids: vec![],
                representative_resolved: vec![],
                all_resolved_distinct: BTreeSet::new(),
                palette_per_section: vec![],
                validation_ok: false,
            };
        }
    };

    let mut palette_per_section: Vec<SectionReport> = Vec::new();
    let mut all_ids: BTreeSet<u32> = BTreeSet::new();
    let mut representative: Vec<Representative> = Vec::new();
    let mut unresolved: Vec<u32> = Vec::new();
    let mut num_inspected: usize = 0;
    let mut num_resolved: usize = 0;

    for sec in &palettes.sections {
        let mut resolved_names: Vec<String> = Vec::new();
        for &gid in &sec.palette {
            all_ids.insert(gid);
            num_inspected += 1;
            if let Some(state) = registry.get(gid) {
                num_resolved += 1;
                resolved_names.push(state.to_string());
                // Add to representative if not already present (first 10 distinct)
                if representative.len() < 10
                    && !representative
                        .iter()
                        .any(|r| r.palette_entry_global_id == gid)
                {
                    representative.push(Representative {
                        palette_entry_global_id: gid,
                        canonical: state.to_string(),
                        name: state.name.clone(),
                        properties: state.properties.clone(),
                    });
                }
            } else {
                unresolved.push(gid);
                resolved_names.push(format!("UNRESOLVED({})", gid));
            }
        }
        // For bits==0 case, palette has 1 entry, we already handled
        // For direct (bits>8, palette empty), we should note that we inspected 0 palette entries but could try to hint
        palette_per_section.push(SectionReport {
            section_index: sec.section_index,
            non_empty_block_count: sec.non_empty_block_count,
            bits: sec.bits,
            palette_global_ids: sec.palette.clone(),
            palette_resolved_names: resolved_names,
        });
    }

    // Also ensure we have at least air and stone resolved for validation
    let has_air = all_ids.contains(&0) && registry.get(0).is_some();
    let has_stone = all_ids.contains(&1) && registry.get(1).is_some();

    // For flat void chunk, first section should be air only (bits 0, palette [0])
    // For mixed, we should have at least 2 distinct
    let validation_ok = registry_found
        && num_inspected > 0
        && num_resolved == num_inspected
        && unresolved.is_empty()
        && has_air;

    // If no palette entries inspected but we have direct palettes, we should still be ok if we can handle direct?
    // For now, require at least one palette entry

    VerifyM1 {
        probe_version: "m1".to_string(),
        input: input_str,
        minecraft_version: version.version.clone(),
        data_version: version.data_version,
        protocol_version: version.protocol_version,
        registry_source: registry.source().description.clone(),
        registry_path: registry.source().path.clone(),
        registry_found: true,
        registry_error: None,
        num_block_states: registry.len(),
        num_sections_inspected: palettes.sections.len(),
        num_palette_entries_inspected: num_inspected,
        num_successfully_resolved: num_resolved,
        num_unresolved: unresolved.len(),
        unresolved_ids: unresolved,
        representative_resolved: representative,
        all_resolved_distinct: all_ids,
        palette_per_section,
        validation_ok,
    }
}
