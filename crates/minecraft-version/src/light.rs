//! M12: Minecraft lightData decoding (26.2 `ClientboundLevelChunkWithLight` tail).
//!
//! Layout (CONFIRMED against `recordings/chunks/test_recording3.zip`, 58
//! chunks: exact byte consumption, mask popcount == array counts, surface
//! columns reading sky 15/14/0, torch cells reading block 14):
//! ```text
//! BitSet skyYMask, BitSet blockYMask, BitSet emptySkyYMask, BitSet emptyBlockYMask,
//! VarInt skyCount, skyCount x (VarInt(2048) + 2048 nibble bytes),
//! VarInt blockCount, blockCount x (VarInt(2048) + 2048 nibble bytes)
//! ```
//! - BitSet = VarInt(n_longs) + n_longs x BE i64, LSB-first bit numbering.
//! - Light bit `i` <-> section_y = `light_base + i`, where
//!   `light_base = min_y / 16 - 1` (-5 for minY -64: 26 light sections).
//! - Array `k` belongs to the k-th set bit (rank order).
//! - Within an array, `idx = (ly * 16 + lz) * 16 + lx` (same convention as
//!   blocks), even idx = LOW nibble first.
//! - Bit set in a mask but empty-mask semantics: a section with no array is
//!   treated as all-zero by the renderer (with a documented above-terrain
//!   sky rule); the decoder itself only splits what is present.
//!
//! The decoder is STRICT: any structural inconsistency (truncation, wrong
//! array length, count != popcount, trailing bytes) yields `None`, and the
//! caller keeps `preserved_raw` + per-section `None` (documented fallback).
//! In the primary validation recording this happens for 2/58 chunks, where
//! the block-entity loop broke early and the blob starts mid-NBT.

use flashback_format::varint::read_varint;

/// One light section's nibble arrays (2048 bytes = 4096 x 4 bits).
#[derive(Debug, Clone)]
pub struct SectionNibbles {
    pub section_y: i32,
    pub sky: Option<[u8; 2048]>,
    pub block: Option<[u8; 2048]>,
}

/// Decoded chunk lighting keyed by section_y (chunk sections only; the
/// below-minY light section has no block data and is dropped).
#[derive(Debug, Clone)]
pub struct ChunkLightData {
    /// `light_base` used: `min_y / 16 - 1`.
    pub light_base: i32,
    pub sections: Vec<SectionNibbles>,
}

/// Read one nibble: `idx=(ly*16+lz)*16+lx`, low nibble first.
pub fn nibble_at(arr: &[u8; 2048], lx: usize, ly: usize, lz: usize) -> u8 {
    debug_assert!(lx < 16 && ly < 16 && lz < 16);
    let idx = (ly * 16 + lz) * 16 + lx;
    let b = arr[idx / 2];
    if idx % 2 == 0 {
        b & 0x0F
    } else {
        b >> 4
    }
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
        b.copy_from_slice(&data[*off..*off + 8]);
        longs.push(u64::from_be_bytes(b));
        *off += 8;
    }
    Some(longs)
}

fn bit_is_set(longs: &[u64], i: usize) -> bool {
    longs
        .get(i / 64)
        .map(|w| (w >> (i % 64)) & 1 == 1)
        .unwrap_or(false)
}

fn popcount_26(longs: &[u64]) -> usize {
    (0..26).filter(|&i| bit_is_set(longs, i)).count()
}

fn read_arrays(data: &[u8], off: &mut usize) -> Option<Vec<[u8; 2048]>> {
    let (count, k) = read_varint(data, *off).ok()?;
    if count < 0 || count > 64 {
        return None;
    }
    *off += k;
    let mut out = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (len, k) = read_varint(data, *off).ok()?;
        *off += k;
        if len != 2048 || *off + 2048 > data.len() {
            return None;
        }
        let mut arr = [0u8; 2048];
        arr.copy_from_slice(&data[*off..*off + 2048]);
        out.push(arr);
        *off += 2048;
    }
    Some(out)
}

/// Strictly decode a preserved `lightData` blob. `min_y` is the chunk's
/// world minimum Y (e.g., -64 for 26.2 overworld/nether).
pub fn decode_light_data(raw: &[u8], min_y: i32) -> Option<ChunkLightData> {
    let mut off = 0usize;
    let sky_mask = read_bitset(raw, &mut off)?;
    let block_mask = read_bitset(raw, &mut off)?;
    let _empty_sky = read_bitset(raw, &mut off)?;
    let _empty_block = read_bitset(raw, &mut off)?;
    let sky_arrays = read_arrays(raw, &mut off)?;
    let block_arrays = read_arrays(raw, &mut off)?;
    if off != raw.len() {
        return None;
    }
    // Array counts must match mask popcounts over the 26 light sections.
    if sky_arrays.len() != popcount_26(&sky_mask) || block_arrays.len() != popcount_26(&block_mask)
    {
        return None;
    }
    let light_base = min_y.div_euclid(16) - 1;
    let rank = |mask: &[u64], i: usize| -> Option<usize> {
        if !bit_is_set(mask, i) {
            return None;
        }
        Some((0..i).filter(|&j| bit_is_set(mask, j)).count())
    };
    let mut sections = Vec::new();
    // Chunk sections run section_y = min_y/16 .. min_y/16+section_count-1;
    // cover the full light range that overlaps blocks (base+1 .. base+26).
    for i in 1..26usize {
        let section_y = light_base + i as i32;
        let sky = rank(&sky_mask, i).map(|r| sky_arrays[r]);
        let block = rank(&block_mask, i).map(|r| block_arrays[r]);
        if sky.is_some() || block.is_some() {
            sections.push(SectionNibbles {
                section_y,
                sky,
                block,
            });
        }
    }
    Some(ChunkLightData {
        light_base,
        sections,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitset_bytes(bits: &[usize]) -> Vec<u8> {
        let mut longs = [0u64; 1];
        for &i in bits {
            longs[i / 64] |= 1u64 << (i % 64);
        }
        let mut out = vec![1u8]; // VarInt(1)
        out.extend_from_slice(&longs[0].to_be_bytes());
        out
    }

    fn array_bytes(fill: [u8; 2048]) -> Vec<u8> {
        // VarInt(2048) = 0x80 0x10
        let mut out = vec![0x80u8, 0x10];
        out.extend_from_slice(&fill);
        out
    }

    /// M12: nibble order — even idx reads the LOW 4 bits first.
    #[test]
    fn nibble_order_low_first() {
        let mut arr = [0u8; 2048];
        arr[0] = 0xA5; // idx0 -> 0x5, idx1 -> 0xA
        arr[7] = 0xF0; // idx14 -> 0x0, idx15 -> 0xF
        assert_eq!(nibble_at(&arr, 0, 0, 0), 0x5);
        assert_eq!(nibble_at(&arr, 1, 0, 0), 0xA);
        assert_eq!(nibble_at(&arr, 14, 0, 0), 0x0);
        assert_eq!(nibble_at(&arr, 15, 0, 0), 0xF);
        // idx=(ly*16+lz)*16+lx: x=15,y=15,z=15 -> idx 4095 (last byte,
        // odd -> HIGH nibble); x=14 -> idx 4094 (even -> LOW nibble).
        let mut arr2 = [0u8; 2048];
        arr2[2047] = 0x1B;
        assert_eq!(nibble_at(&arr2, 15, 15, 15), 0x1);
        assert_eq!(nibble_at(&arr2, 14, 15, 15), 0xB);
    }

    fn blob_with(
        sky_bits: &[usize],
        sky: &[[u8; 2048]],
        blk_bits: &[usize],
        blk: &[[u8; 2048]],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&bitset_bytes(sky_bits));
        out.extend_from_slice(&bitset_bytes(blk_bits));
        out.extend_from_slice(&bitset_bytes(&[])); // empty sky
        out.extend_from_slice(&bitset_bytes(&[])); // empty block
        out.push(sky.len() as u8); // VarInt count (small)
        for a in sky {
            out.extend_from_slice(&array_bytes(*a));
        }
        out.push(blk.len() as u8);
        for a in blk {
            out.extend_from_slice(&array_bytes(*a));
        }
        out
    }

    /// M12: known nibble values 0/1/5/10/15 decode for sky and block.
    #[test]
    fn known_nibble_values_sky_and_block() {
        // sky array for light bit 10 (section_y 5), block array for bit 6.
        let mut sky = [0u8; 2048];
        sky[0] = 0xF0; // idx0 sky=0, idx1 sky=15
        sky[1] = 0xA5; // idx2 sky=5, idx3 sky=10
        sky[2] = 0x11; // idx4,idx5 sky=1
        let mut blk = [0u8; 2048];
        blk[0] = 0xF0;
        blk[1] = 0xA5;
        blk[2] = 0x11;
        let blob = blob_with(&[10], &[sky], &[6], &[blk]);
        let d = decode_light_data(&blob, -64).expect("decode");
        assert_eq!(d.light_base, -5);
        let s5 = d.sections.iter().find(|s| s.section_y == 5).expect("s5");
        let sky = s5.sky.expect("sky");
        assert_eq!(nibble_at(&sky, 0, 0, 0), 0);
        assert_eq!(nibble_at(&sky, 1, 0, 0), 15);
        assert_eq!(nibble_at(&sky, 2, 0, 0), 5);
        assert_eq!(nibble_at(&sky, 3, 0, 0), 10);
        assert_eq!(nibble_at(&sky, 4, 0, 0), 1);
        let s1 = d.sections.iter().find(|s| s.section_y == 1).expect("s1");
        let blk = s1.block.expect("block");
        assert_eq!(nibble_at(&blk, 0, 0, 0), 0);
        assert_eq!(nibble_at(&blk, 1, 0, 0), 15);
        assert_eq!(nibble_at(&blk, 2, 0, 0), 5);
        assert_eq!(nibble_at(&blk, 3, 0, 0), 10);
        assert_eq!(nibble_at(&blk, 4, 0, 0), 1);
        // Section with no arrays is absent from the split.
        assert!(d
            .sections
            .iter()
            .all(|s| s.section_y == 5 || s.section_y == 1));
    }

    /// M12: strictness — trailing bytes, count/popcount mismatch, and
    /// truncated arrays must all decode to None (documented fallback).
    #[test]
    fn strict_rejects_malformed_blobs() {
        let sky = [0xFFu8; 2048];
        let good = blob_with(&[10], &[sky], &[], &[]);
        assert!(decode_light_data(&good, -64).is_some());
        // Trailing byte.
        let mut trailing = good.clone();
        trailing.push(0);
        assert!(decode_light_data(&trailing, -64).is_none());
        // Mask claims 2 bits but only 1 array sent.
        let mismatch = blob_with(&[10, 11], &[sky], &[], &[]);
        assert!(decode_light_data(&mismatch, -64).is_none());
        // Truncated array.
        let mut trunc = good.clone();
        trunc.truncate(trunc.len() - 100);
        assert!(decode_light_data(&trunc, -64).is_none());
        // Garbage (mid-NBT, like the 2 affected recording chunks).
        assert!(decode_light_data(&[0x0A, 0x0A, 0x00, 0x09, 0x53], -64).is_none());
        assert!(decode_light_data(&[], -64).is_none());
    }
}
