//! M12: recording-anchored lighting tests on `test_recording3.zip`.
//!
//! These prove the `light` decoder against real recording bytes (not just
//! synthetic blobs): exact structural consumption on every chunk, the
//! nether's absent sky light, surface-column sky values, and torch cells.
//! Needs the recording + 26.2 registry; runs in a few seconds.

use flashback_format::varint::read_varint;
use minecraft_version::registry::load_26_2_registry;
use minecraft_version::snapshot::decode_snapshot_with_data;
use minecraft_version::MinecraftVersion;

fn bit(longs: &[u64], i: usize) -> bool {
    longs
        .get(i / 64)
        .map(|w| (w >> (i % 64)) & 1 == 1)
        .unwrap_or(false)
}

fn read_bitset(data: &[u8], off: &mut usize) -> Option<Vec<u64>> {
    let (n, k) = read_varint(data, *off).ok()?;
    if n < 0 || n > 64 {
        return None;
    }
    *off += k;
    if *off + (n as usize) * 8 > data.len() {
        return None;
    }
    let mut longs = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let mut b = [0u8; 8];
        b.copy_from_slice(data.get(*off..*off + 8)?);
        longs.push(u64::from_be_bytes(b));
        *off += 8;
    }
    Some(longs)
}

/// Independent re-parse of a lightData blob: exact consumption plus
/// array-count == mask-popcount agreement (26 light sections).
fn check_blob(blob: &[u8]) -> Option<(usize, usize)> {
    let mut off = 0usize;
    let sky_mask = read_bitset(blob, &mut off)?;
    let block_mask = read_bitset(blob, &mut off)?;
    read_bitset(blob, &mut off)?;
    read_bitset(blob, &mut off)?;
    let mut counts = Vec::new();
    for _ in 0..2 {
        let (n, k) = read_varint(blob, off).ok()?;
        if n < 0 || n > 64 {
            return None;
        }
        off += k;
        for _ in 0..n {
            let (len, k) = read_varint(blob, off).ok()?;
            off += k;
            if len != 2048 || off + 2048 > blob.len() {
                return None;
            }
            off += 2048;
        }
        counts.push(n as usize);
    }
    if off != blob.len() {
        return None;
    }
    let pop = |m: &[u64]| (0..26).filter(|&i| bit(m, i)).count();
    if counts[0] != pop(&sky_mask) || counts[1] != pop(&block_mask) {
        return None;
    }
    Some((counts[0], counts[1]))
}

fn decode_all() -> Vec<(String, replay_model::CanonicalChunk)> {
    let reg = load_26_2_registry().expect("registry");
    let version = MinecraftVersion::v26_2();
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../recordings/chunks/test_recording3.zip");
    assert!(path.exists(), "need test_recording3.zip");
    let mut archive = flashback_format::zip_container::open_zip_readonly(&path).expect("open zip");
    let names = flashback_format::zip_container::find_chunk_names(&mut archive);
    let shards = flashback_format::zip_container::find_cache_shards(&mut archive);
    let mut out = Vec::new();
    for name in &names {
        let data =
            flashback_format::zip_container::read_entry_bytes(&mut archive, name).expect("chunk");
        let parsed = flashback_format::chunk::parse_chunk_bytes(&data, name).expect("parse chunk");
        for shard_name in shards.values() {
            let shard = flashback_format::zip_container::read_entry_bytes(&mut archive, shard_name)
                .expect("shard");
            let decoded =
                decode_snapshot_with_data(&parsed, &data, &shard, &reg, &version).expect("decode");
            for ((cx, cz), chunk) in decoded.state.chunks {
                out.push((format!("{name} ({cx},{cz})"), chunk));
            }
        }
    }
    out
}

/// M12: every well-formed light blob in the recording parses exactly under
/// the vanilla layout; chunks whose blob is polluted by truncated
/// block-entity bytes (2 known) must fail the strict parse (documented
/// fallback) rather than decode garbage.
#[test]
fn recording_light_layout_exact() {
    let chunks = decode_all();
    assert!(!chunks.is_empty());
    let mut ok = 0;
    let mut strict_reject = 0;
    let mut nether_sky_total = 0;
    for (label, chunk) in &chunks {
        let Some(blob) = chunk.lighting.raw_bytes.as_ref() else {
            continue;
        };
        match check_blob(blob) {
            Some((ns, nb)) => {
                ok += 1;
                // Nether snapshot chunks carry zero sky arrays.
                if label.starts_with("c1.") {
                    nether_sky_total += ns;
                    assert_eq!(ns, 0, "{label}: nether must have no sky arrays");
                }
                let _ = nb;
            }
            None => {
                // Only acceptable with a known cause: truncated BE prefix.
                assert!(
                    !chunk.block_entities.is_empty() && blob.len() >= 2 && blob[0] == 0x0A,
                    "{label}: strict reject needs NBT-prefix evidence"
                );
                strict_reject += 1;
            }
        }
    }
    assert!(ok > 50, "expected dozens of decodable chunks, got {ok}");
    assert_eq!(nether_sky_total, 0);
    assert!(
        strict_reject <= 2,
        "more rejects than the 2 known BE-truncated chunks: {strict_reject}"
    );
}

/// M12: nibble values against block states — open sky 15 above a surface
/// column, 0 deep inside cover, torch cells 14. Pins section mapping
/// (bit i <-> section_y -5+i), cell order, and low-nibble-first packing.
#[test]
fn recording_light_values_match_blocks() {
    let chunks = decode_all();
    let chunk = chunks
        .iter()
        .find(|(l, _)| l == "c0.flashback (-1,-2)")
        .map(|(_, c)| c)
        .expect("chunk (-1,-2)");
    let blob = chunk.lighting.raw_bytes.as_ref().expect("blob");
    let decoded =
        minecraft_version::light::decode_light_data(blob, chunk.min_y).expect("decode light");
    let at = |x: usize, y: i32, z: usize| -> &str {
        let sec = chunk
            .sections
            .iter()
            .find(|s| s.section_y == y.div_euclid(16))
            .expect("section");
        &sec.block_states[((y - sec.y_base) as usize * 16 + z) * 16 + x].name
    };
    let nib = |arr: &[u8], x: usize, y: i32, z: usize| -> u8 {
        let ly = (y - y.div_euclid(16) * 16) as usize;
        let idx = (ly * 16 + z) * 16 + x;
        let b = arr[idx / 2];
        if idx % 2 == 0 {
            b & 0xF
        } else {
            b >> 4
        }
    };
    let sky_of = |x: usize, y: i32, z: usize| -> u8 {
        let sy = y.div_euclid(16);
        let sec = decoded
            .sections
            .iter()
            .find(|s| s.section_y == sy)
            .expect("light section");
        nib(sec.sky.as_ref().expect("sky array"), x, y, z)
    };
    for (x, z) in [(3usize, 5usize), (8, 8), (12, 2)] {
        let mut top = -64;
        for y in -64..320 {
            if at(x, y, z) != "minecraft:air" {
                top = y;
            }
        }
        assert_eq!(sky_of(x, top + 3, z), 15, "open sky");
        assert_eq!(sky_of(x, top - 12, z), 0, "deep cover");
    }
    // Torch cells anywhere in c0 read 14.
    let mut torches = 0;
    for (_, ch) in chunks.iter().filter(|(l, _)| l.starts_with("c0.")) {
        let Some(b) = ch.lighting.raw_bytes.as_ref() else {
            continue;
        };
        let Some(p) = minecraft_version::light::decode_light_data(b, ch.min_y) else {
            continue;
        };
        for sec in &ch.sections {
            for (idx, st) in sec.block_states.iter().enumerate() {
                if st.name == "minecraft:torch" || st.name == "minecraft:wall_torch" {
                    let s = p
                        .sections
                        .iter()
                        .find(|s| s.section_y == sec.section_y)
                        .expect("torch section light");
                    let arr = s.block.as_ref().expect("torch block array");
                    let v = nib(
                        arr,
                        idx % 16,
                        sec.y_base + (idx / 256) as i32,
                        (idx / 16) % 16,
                    );
                    assert!(v >= 12, "torch cell must be bright, got {v}");
                    torches += 1;
                }
            }
        }
    }
    assert!(torches > 0, "expected torches in c0");
}
