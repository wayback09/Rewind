use flashback_format::{
    cache::parse_cache_shard,
    chunk::{parse_chunk_bytes, MAGIC},
    metadata::parse_metadata,
    zip_container::{
        find_cache_shards, find_chunk_names, list_entries, open_zip_readonly, read_entry_bytes,
    },
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
struct ProbeReport {
    probe_version: String,
    input: String,
    file_size: u64,
    zip_entries: Vec<ZipEntryReport>,
    zip_ok: bool,
    metadata: Option<serde_json::Value>,
    metadata_parsed: Option<MetadataReport>,
    metadata_validation_ok: bool,
    metadata_issues: Vec<String>,
    chunks: Vec<ChunkReport>,
    chunk_caches: Vec<CacheReport>,
    total_cache_entries: usize,
    global_tick_count: usize,
    global_duration_sum: i32,
    global_tick_vs_metadata_ok: bool,
    errors: Vec<ErrorReport>,
    validation_ok: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ZipEntryReport {
    name: String,
    file_size: u64,
    compressed_size: u64,
    compression_method: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct MetadataReport {
    uuid: String,
    name: String,
    version_string: Option<String>,
    data_version: Option<i32>,
    protocol_version: Option<i32>,
    total_ticks: Option<i32>,
    chunks: BTreeMap<String, ChunkMetaReport>,
    markers: Option<BTreeMap<String, serde_json::Value>>,
    custom_namespaces: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChunkMetaReport {
    duration: i32,
    force_play_snapshot: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChunkReport {
    file_name: String,
    file_size: u64,
    compressed_size: u64,
    magic: String,
    magic_ok: bool,
    big_endian_ok: bool,
    action_table_count: usize,
    action_table: Vec<String>,
    action_table_ok: bool,
    identifier_encoding_ok: bool,
    snapshot_size: i32,
    snapshot_size_ok: bool,
    snapshot_offset: usize,
    actions_offset: usize,
    snapshot_boundaries_ok: bool,
    sentinel_ok: bool,
    snapshot_tlvs: usize,
    replay_tlvs: usize,
    total_tlvs: usize,
    tlv_ok: bool,
    next_tick_id_dynamic: Option<i32>,
    next_tick_identifier: String,
    tick_count_via_dynamic_table: usize,
    tick_count_ok: bool,
    duration_vs_ticks_ok: bool,
    per_identifier_counts_snapshot: BTreeMap<String, usize>,
    per_identifier_counts_replay: BTreeMap<String, usize>,
    per_identifier_counts_total: BTreeMap<String, usize>,
    issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheReport {
    shard_name: String,
    shard_index: u32,
    entries: usize,
    total_bytes: usize,
    first_entry_sizes: Vec<i32>,
    structure_ok: bool,
    issues: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ErrorReport {
    message: String,
    offset: Option<usize>,
    context: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let input_path = if args.len() > 1 {
        PathBuf::from(&args[1])
    } else {
        eprintln!("Usage: {} <recording.zip>", args[0]);
        eprintln!("No input given, probing all recordings under recordings/");
        // fallback: probe all known recordings and emit aggregated report?
        // For M0 we require single file; exit with error.
        std::process::exit(1);
    };

    let report = probe_one(&input_path);
    let validation_ok = report.validation_ok;
    // Ensure target dir exists
    let out_path = PathBuf::from("target/verify-m0.json");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let json = serde_json::to_string_pretty(&report).expect("serialize report");
    std::fs::write(&out_path, json).expect("write target/verify-m0.json");
    println!("{}", serde_json::to_string_pretty(&report).unwrap());
    if validation_ok {
        println!("\nValidation OK → {}", out_path.display());
        std::process::exit(0);
    } else {
        eprintln!("\nValidation FAILED → {}", out_path.display());
        std::process::exit(2);
    }
}

fn probe_one(path: &Path) -> ProbeReport {
    let input_str = path.display().to_string();
    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    let mut errors: Vec<ErrorReport> = Vec::new();
    let mut zip_entries: Vec<ZipEntryReport> = Vec::new();
    let mut metadata_value: Option<serde_json::Value> = None;
    let mut metadata_report: Option<MetadataReport> = None;
    let mut metadata_issues: Vec<String> = Vec::new();
    let mut metadata_validation_ok = false;
    let mut chunk_reports: Vec<ChunkReport> = Vec::new();
    let mut cache_reports: Vec<CacheReport> = Vec::new();
    let mut total_cache_entries: usize = 0;
    let mut global_tick_count: usize = 0;
    let mut global_duration_sum: i32 = 0;
    let mut zip_ok = true;

    // Open ZIP read-only (no modification)
    let mut archive = match open_zip_readonly(path) {
        Ok(a) => a,
        Err(e) => {
            errors.push(ErrorReport {
                message: format!("failed to open ZIP: {}", e.message),
                offset: e.offset,
                context: e.context,
            });
            return ProbeReport {
                probe_version: "m0".to_string(),
                input: input_str,
                file_size,
                zip_entries,
                zip_ok: false,
                metadata: None,
                metadata_parsed: None,
                metadata_validation_ok: false,
                metadata_issues: vec!["ZIP open failed".to_string()],
                chunks: vec![],
                chunk_caches: vec![],
                total_cache_entries: 0,
                global_tick_count: 0,
                global_duration_sum: 0,
                global_tick_vs_metadata_ok: false,
                errors,
                validation_ok: false,
            };
        }
    };

    // List entries for report (deterministic sort by name already but ensure)
    let entries = list_entries(&mut archive);
    for e in &entries {
        zip_entries.push(ZipEntryReport {
            name: e.name.clone(),
            file_size: e.file_size,
            compressed_size: e.compressed_size,
            compression_method: e.compression_method.clone(),
        });
    }
    zip_entries.sort_by(|a, b| a.name.cmp(&b.name));

    // Verify expected entries compress method is DEFLATED (method 8)
    // The zip crate reports as "Deflated" string
    for e in &entries {
        if e.compression_method != "Deflated" && !e.is_dir {
            // Not fatal but note
            errors.push(ErrorReport {
                message: format!(
                    "ZIP entry '{}' uses compression {:?} expected Deflated",
                    e.name, e.compression_method
                ),
                offset: None,
                context: None,
            });
        }
    }

    // Locate and parse metadata.json
    let metadata_bytes = match read_entry_bytes(&mut archive, "metadata.json") {
        Ok(b) => b,
        Err(e) => {
            errors.push(ErrorReport {
                message: format!("metadata.json missing or unreadable: {}", e.message),
                offset: e.offset,
                context: e.context,
            });
            zip_ok = false;
            Vec::new()
        }
    };

    let mut metadata_total_ticks: Option<i32> = None;
    let mut metadata_chunks: BTreeMap<String, ChunkMetaReport> = BTreeMap::new();

    if !metadata_bytes.is_empty() {
        metadata_value = serde_json::from_slice::<serde_json::Value>(&metadata_bytes).ok();
        match parse_metadata(&metadata_bytes) {
            Ok(m) => {
                metadata_total_ticks = m.total_ticks;
                global_duration_sum = m.total_duration();
                // Validate
                let issues = m.validate();
                metadata_issues = issues.clone();
                metadata_validation_ok = issues.is_empty();
                // Build report
                let mut chunks_map = BTreeMap::new();
                for (k, v) in &m.chunks {
                    chunks_map.insert(
                        k.clone(),
                        ChunkMetaReport {
                            duration: v.duration,
                            force_play_snapshot: v.forcePlaySnapshot,
                        },
                    );
                }
                metadata_chunks = chunks_map.clone();
                metadata_report = Some(MetadataReport {
                    uuid: m.uuid.clone(),
                    name: m.name.clone(),
                    version_string: m.version_string.clone(),
                    data_version: m.data_version,
                    protocol_version: m.protocol_version,
                    total_ticks: m.total_ticks,
                    chunks: chunks_map,
                    markers: m.markers.map(|mm| {
                        let mut out = BTreeMap::new();
                        for (k, v) in mm {
                            out.insert(k, serde_json::to_value(v).unwrap());
                        }
                        out
                    }),
                    custom_namespaces: m.customNamespacesForRegistries,
                });
                if !metadata_validation_ok {
                    for iss in &metadata_issues {
                        errors.push(ErrorReport {
                            message: format!("metadata validation: {}", iss),
                            offset: None,
                            context: None,
                        });
                    }
                }
            }
            Err(e) => {
                errors.push(ErrorReport {
                    message: format!("metadata.json parse failed: {}", e),
                    offset: None,
                    context: None,
                });
                zip_ok = false;
            }
        }
    }

    // Locate replay chunk files c*.flashback
    let chunk_names = find_chunk_names(&mut archive);
    if chunk_names.is_empty() {
        errors.push(ErrorReport {
            message: "no c*.flashback chunk files found".to_string(),
            offset: None,
            context: None,
        });
        zip_ok = false;
    }

    // Also collect chunk file_sizes/compressed_sizes for reporting
    let chunk_file_info: HashMap<String, (u64, u64)> = {
        let mut m = HashMap::new();
        for e in &entries {
            if e.name.ends_with(".flashback") {
                m.insert(e.name.clone(), (e.file_size, e.compressed_size));
            }
        }
        m
    };

    // Parse each chunk
    for chunk_name in &chunk_names {
        let chunk_bytes = match read_entry_bytes(&mut archive, chunk_name) {
            Ok(b) => b,
            Err(e) => {
                errors.push(ErrorReport {
                    message: format!("failed to read chunk '{}': {}", chunk_name, e.message),
                    offset: e.offset,
                    context: e.context,
                });
                continue;
            }
        };
        let (fs, cs) = chunk_file_info
            .get(chunk_name)
            .copied()
            .unwrap_or((chunk_bytes.len() as u64, 0));
        match parse_chunk_and_report(
            &chunk_bytes,
            chunk_name,
            fs,
            cs,
            metadata_chunks.get(chunk_name).map(|c| c.duration),
        ) {
            Ok((report, tick_count)) => {
                global_tick_count += tick_count;
                if !report.tlv_ok || !report.duration_vs_ticks_ok {
                    // already counted as issues
                }
                chunk_reports.push(report);
            }
            Err(e) => {
                errors.push(ErrorReport {
                    message: format!("chunk '{}' parse error: {}", chunk_name, e.message),
                    offset: e.offset,
                    context: e.context,
                });
                // Still push a minimal failed report
                chunk_reports.push(ChunkReport {
                    file_name: chunk_name.clone(),
                    file_size: fs,
                    compressed_size: cs,
                    magic: format!("0x{:08X}", 0),
                    magic_ok: false,
                    big_endian_ok: false,
                    action_table_count: 0,
                    action_table: vec![],
                    action_table_ok: false,
                    identifier_encoding_ok: false,
                    snapshot_size: -1,
                    snapshot_size_ok: false,
                    snapshot_offset: 0,
                    actions_offset: 0,
                    snapshot_boundaries_ok: false,
                    sentinel_ok: false,
                    snapshot_tlvs: 0,
                    replay_tlvs: 0,
                    total_tlvs: 0,
                    tlv_ok: false,
                    next_tick_id_dynamic: None,
                    next_tick_identifier: "flashback:action/next_tick".to_string(),
                    tick_count_via_dynamic_table: 0,
                    tick_count_ok: false,
                    duration_vs_ticks_ok: false,
                    per_identifier_counts_snapshot: BTreeMap::new(),
                    per_identifier_counts_replay: BTreeMap::new(),
                    per_identifier_counts_total: BTreeMap::new(),
                    issues: vec![e.message.clone()],
                });
            }
        }
    }

    // Sort chunks deterministically by file_name (c0, c1...)
    chunk_reports.sort_by(|a, b| a.file_name.cmp(&b.file_name));

    // Parse level_chunk_caches shards
    let cache_shards = find_cache_shards(&mut archive);
    if cache_shards.is_empty() {
        // Not necessarily error? Research says legacy single file but current uses shards.
        // For M0, expect at least shard 0 if any chunks referenced level_chunk_cached.
        // We'll note but not fail if no level chunks? But all known recordings have it.
        errors.push(ErrorReport {
            message: "no level_chunk_caches/* shards found".to_string(),
            offset: None,
            context: None,
        });
    }
    for (idx, shard_name) in &cache_shards {
        let shard_bytes = match read_entry_bytes(&mut archive, shard_name) {
            Ok(b) => b,
            Err(e) => {
                errors.push(ErrorReport {
                    message: format!("failed to read cache shard '{}': {}", shard_name, e.message),
                    offset: e.offset,
                    context: e.context,
                });
                cache_reports.push(CacheReport {
                    shard_name: shard_name.clone(),
                    shard_index: *idx,
                    entries: 0,
                    total_bytes: 0,
                    first_entry_sizes: vec![],
                    structure_ok: false,
                    issues: vec![e.message],
                });
                continue;
            }
        };
        match parse_cache_shard(&shard_bytes, *idx) {
            Ok(info) => {
                total_cache_entries += info.entries;
                cache_reports.push(CacheReport {
                    shard_name: shard_name.clone(),
                    shard_index: *idx,
                    entries: info.entries,
                    total_bytes: info.total_bytes,
                    first_entry_sizes: info.first_entry_sizes,
                    structure_ok: true,
                    issues: vec![],
                });
            }
            Err(e) => {
                errors.push(ErrorReport {
                    message: format!(
                        "cache shard '{}' structure invalid: {}",
                        shard_name, e.message
                    ),
                    offset: e.offset,
                    context: e.context.clone(),
                });
                cache_reports.push(CacheReport {
                    shard_name: shard_name.clone(),
                    shard_index: *idx,
                    entries: 0,
                    total_bytes: shard_bytes.len(),
                    first_entry_sizes: vec![],
                    structure_ok: false,
                    issues: vec![e.message],
                });
            }
        }
    }
    cache_reports.sort_by(|a, b| a.shard_index.cmp(&b.shard_index));

    // Global tick vs metadata check
    let global_tick_vs_metadata_ok = if let Some(total) = metadata_total_ticks {
        total as usize == global_tick_count
    } else {
        // If no metadata total, we can't verify; consider false but not fatal?
        false
    };
    if !global_tick_vs_metadata_ok {
        if let Some(total) = metadata_total_ticks {
            errors.push(ErrorReport {
                message: format!(
                    "global tick count {} != metadata total_ticks {} (sum durations {})",
                    global_tick_count, total, global_duration_sum
                ),
                offset: None,
                context: None,
            });
        }
    }

    let validation_ok = errors.is_empty()
        && zip_ok
        && metadata_validation_ok
        && chunk_reports
            .iter()
            .all(|c| c.tlv_ok && c.duration_vs_ticks_ok)
        && cache_reports.iter().all(|c| c.structure_ok)
        && global_tick_vs_metadata_ok;

    ProbeReport {
        probe_version: "m0".to_string(),
        input: input_str,
        file_size,
        zip_entries: zip_entries,
        zip_ok,
        metadata: metadata_value,
        metadata_parsed: metadata_report,
        metadata_validation_ok,
        metadata_issues,
        chunks: chunk_reports,
        chunk_caches: cache_reports,
        total_cache_entries,
        global_tick_count,
        global_duration_sum,
        global_tick_vs_metadata_ok,
        errors,
        validation_ok,
    }
}

fn parse_chunk_and_report(
    data: &[u8],
    file_name: &str,
    file_size: u64,
    compressed_size: u64,
    expected_duration: Option<i32>,
) -> Result<(ChunkReport, usize), flashback_format::error::FormatError> {
    let parsed = parse_chunk_bytes(data, file_name)?;

    // Validate big-endian: LE would be nonsensical huge negative
    let snapshot_size_be = parsed.snapshot_size;
    let be_bytes = &data[parsed.snapshot_offset - 4..parsed.snapshot_offset];
    let le_as_i32 = i32::from_le_bytes([be_bytes[0], be_bytes[1], be_bytes[2], be_bytes[3]]);
    let big_endian_ok = snapshot_size_be >= 0
        && snapshot_size_be < 10_000_000
        && le_as_i32 != snapshot_size_be
        && (le_as_i32 < 0 || le_as_i32 > 10_000_000); // LE would be absurd

    let magic_ok = parsed.magic == MAGIC;
    let action_table_ok = parsed.action_count as usize == parsed.action_table.len();
    let identifier_encoding_ok = parsed
        .action_table
        .iter()
        .all(|s| s.contains(':') && s.starts_with("flashback:action/"));
    let snapshot_size_ok = snapshot_size_be >= 0 && (snapshot_size_be as usize) < data.len();
    let snapshot_boundaries_ok = parsed.snapshot_offset + snapshot_size_be as usize
        == parsed.actions_offset
        && parsed.actions_offset <= data.len();
    let sentinel_ok = snapshot_size_be as u32 != 0xDEADBEEF;

    // Resolve next_tick dynamically via table (NEVER hard-code)
    let next_tick_id = parsed.find_id("flashback:action/next_tick");
    let next_tick_identifier = "flashback:action/next_tick".to_string();
    let tick_count = if let Some(id) = next_tick_id {
        parsed
            .replay_tlvs
            .iter()
            .filter(|t| t.local_id == id)
            .count()
        // Note: spec says snapshot never contains next_tick; but we also check
        // Also count snapshot just in case but expect 0
    } else {
        0
    };
    let tick_count_ok = next_tick_id.is_some();
    let duration_vs_ticks_ok = if let Some(dur) = expected_duration {
        tick_count == dur as usize
    } else {
        // No metadata duration, just check at least that snapshot has 0 next_tick
        let snap_next = if let Some(id) = next_tick_id {
            parsed
                .snapshot_tlvs
                .iter()
                .filter(|t| t.local_id == id)
                .count()
        } else {
            0
        };
        snap_next == 0
    };

    // Count per-identifier
    let mut snap_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut replay_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_counts: BTreeMap<String, usize> = BTreeMap::new();

    for tlv in &parsed.snapshot_tlvs {
        if let Some(ident) = parsed.resolve(tlv.local_id) {
            *snap_counts.entry(ident.to_string()).or_insert(0) += 1;
            *total_counts.entry(ident.to_string()).or_insert(0) += 1;
        }
    }
    for tlv in &parsed.replay_tlvs {
        if let Some(ident) = parsed.resolve(tlv.local_id) {
            *replay_counts.entry(ident.to_string()).or_insert(0) += 1;
            *total_counts.entry(ident.to_string()).or_insert(0) += 1;
        }
    }

    // TLV ok if we got here without error and counts match header boundaries (already validated)
    let tlv_ok = true;

    // Verify that all NextTick payloads are size 0 (as per spec)
    let mut issues: Vec<String> = Vec::new();
    if let Some(id) = next_tick_id {
        for tlv in parsed.replay_tlvs.iter().filter(|t| t.local_id == id) {
            if tlv.payload_size != 0 {
                issues.push(format!(
                    "NextTick at offset {} has non-zero payload size {}",
                    tlv.header_offset, tlv.payload_size
                ));
            }
        }
        for tlv in parsed.snapshot_tlvs.iter().filter(|t| t.local_id == id) {
            if tlv.payload_size != 0 {
                issues.push(format!(
                    "Snapshot NextTick at offset {} has non-zero payload size {} (should not be in snapshot)",
                    tlv.header_offset, tlv.payload_size
                ));
            }
        }
    } else {
        issues.push("next_tick identifier not found in action table".to_string());
    }

    // Check that voice-chat dynamic shift is handled: if table[0] is voice, next_tick should be 1
    // This is not an error, just evidence we resolved dynamically
    if parsed.action_table.first().map(|s| s.as_str())
        == Some("flashback:action/simple_voice_chat_sound_optional")
        && next_tick_id != Some(1)
    {
        issues.push(format!(
            "unexpected next_tick id {} when voice-chat at 0 (expected 1)",
            next_tick_id.unwrap_or(-1)
        ));
    }

    let report = ChunkReport {
        file_name: file_name.to_string(),
        file_size,
        compressed_size,
        magic: format!("0x{:08X}", parsed.magic),
        magic_ok,
        big_endian_ok,
        action_table_count: parsed.action_table.len(),
        action_table: parsed.action_table.clone(),
        action_table_ok,
        identifier_encoding_ok,
        snapshot_size: parsed.snapshot_size,
        snapshot_size_ok,
        snapshot_offset: parsed.snapshot_offset,
        actions_offset: parsed.actions_offset,
        snapshot_boundaries_ok,
        sentinel_ok,
        snapshot_tlvs: parsed.snapshot_tlvs.len(),
        replay_tlvs: parsed.replay_tlvs.len(),
        total_tlvs: parsed.snapshot_tlvs.len() + parsed.replay_tlvs.len(),
        tlv_ok,
        next_tick_id_dynamic: next_tick_id,
        next_tick_identifier,
        tick_count_via_dynamic_table: tick_count,
        tick_count_ok,
        duration_vs_ticks_ok,
        per_identifier_counts_snapshot: snap_counts,
        per_identifier_counts_replay: replay_counts,
        per_identifier_counts_total: total_counts,
        issues,
    };

    Ok((report, tick_count))
}
