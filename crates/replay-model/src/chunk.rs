use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version-independent block position.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockPos {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Faithful canonical block state — name + all properties as strings (Mojang lower-case).
/// Example: `minecraft:oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=false]`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CanonicalBlockState {
    pub name: String,
    pub properties: BTreeMap<String, String>,
}

impl CanonicalBlockState {
    pub fn is_air(&self) -> bool {
        self.name == "minecraft:air"
    }
}

impl std::fmt::Display for CanonicalBlockState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.properties.is_empty() {
            write!(f, "{}", self.name)
        } else {
            write!(f, "{}[", self.name)?;
            let mut first = true;
            for (k, v) in &self.properties {
                if !first {
                    write!(f, ",")?;
                }
                write!(f, "{}={}", k, v)?;
                first = false;
            }
            write!(f, "]")?;
            Ok(())
        }
    }
}

/// Block entity with canonical type and preserved NBT.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockEntity {
    /// World block position (absolute, e.g., x=-112, y=62, z=16)
    pub pos: BlockPos,
    /// Packed XZ and Y as stored in packet (for debugging)
    pub packed_xz: u8,
    pub y: i32,
    /// Canonical block entity type, e.g., `minecraft:spawner`, `minecraft:chest`
    pub type_name: String,
    /// Preserved NBT tag as JSON (via `serde_json::Value` from `CompoundTag`).
    /// For spawners, contains `SpawnData: {entity:{id:"minecraft:zombie"}}` etc.
    pub nbt: serde_json::Value,
}

/// Per-section canonical data — 16×16×16 = 4096 block states.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalSection {
    /// Section Y index (e.g., -4 for y=-64..-49, 0 for y=0..15, 19 for y=304..319). For 26.2, minY -64, height 384 → 24 sections (-4..19).
    pub section_y: i32,
    /// World Y base for this section (section_y * 16)
    pub y_base: i32,
    /// Non-empty block count as stored (u16, 0..4096)
    pub non_empty_block_count: u16,
    /// 4096 canonical block states in palette-expanded order.
    /// Index `idx = (y * 16 + z) * 16 + x` where x,y,z are 0..15 local.
    pub block_states: Vec<CanonicalBlockState>,
    /// Block entities that lie within this section (y in [y_base, y_base+15]).
    pub block_entities: Vec<BlockEntity>,
    /// Block light nibbles (2048 bytes, 4096 entries × 4 bits) if present, else None.
    /// For 26.2, light is `BitSet` + `byte[2048]` per section, but we preserve raw bytes.
    pub block_light: Option<Vec<u8>>,
    /// Sky light nibbles (2048 bytes) if present.
    pub sky_light: Option<Vec<u8>>,
    /// Raw palette bits for debugging (0,1..8,15)
    pub palette_bits: u8,
    /// Whether this section was `SingleValue` (air) or indirect/direct.
    pub palette_size: usize,
}

/// Lighting data for the whole chunk — raw preservation, explicitly version-adapter-owned if uncertain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingData {
    /// Raw sky/block light as preserved bytes, or unavailable.
    /// For 26.2, the packet's `lightData` is `BitSet` + `byte[2048]` per section, but exact representation is not confidently established for all cases (vanilla vs Starlight).
    /// We preserve raw bytes and mark status.
    pub status: String, // "preserved_raw" or "unavailable" or "decoded"
    /// Raw light bytes as `Vec<u8>` if preserved, else None.
    pub raw_bytes: Option<Vec<u8>>,
    /// Per-section light data if decoded (optional).
    pub per_section: Vec<SectionLight>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionLight {
    pub section_y: i32,
    pub sky_light: Option<Vec<u8>>,
    pub block_light: Option<Vec<u8>>,
}

/// Heightmap data — raw or decoded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeightmapData {
    pub heightmaps: BTreeMap<String, Vec<u64>>,
    pub raw_status: String,
}

/// Canonical chunk — version-independent, no numeric BlockState IDs, no PalettedContainer, no BitStorage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalChunk {
    /// Chunk X (e.g., -7)
    pub x: i32,
    /// Chunk Z (e.g., 1)
    pub z: i32,
    /// World min Y for this chunk (e.g., -64 for 26.2)
    pub min_y: i32,
    /// World height (e.g., 384)
    pub height: i32,
    /// Number of sections (e.g., 24)
    pub section_count: usize,
    /// Sections in Y order (section_y = min_y/16 + idx)
    pub sections: Vec<CanonicalSection>,
    /// All block entities in the chunk (global list, also per-section)
    pub block_entities: Vec<BlockEntity>,
    /// Heightmaps (if decoded)
    pub heightmaps: Option<HeightmapData>,
    /// Lighting (raw preservation)
    pub lighting: LightingData,
    /// Biome data — not yet canonicalized, raw placeholder.
    pub biome_data: Option<BiomeSectionData>,
    /// Number of non-air blocks (derived)
    pub non_empty_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BiomeSectionData {
    pub status: String, // "raw_preserved" or "unavailable" or "decoded"
    pub raw_bytes: Option<Vec<u8>>,
    pub note: String,
}

impl CanonicalChunk {
    /// Get canonical block state at local coordinates (0..15) within section.
    /// `y` is world Y (e.g., 62), `x,z` local 0..15, `section_y` derived.
    pub fn canonical_block_at(
        &self,
        world_y: i32,
        local_x: usize,
        local_z: usize,
    ) -> Option<&CanonicalBlockState> {
        let section_y = world_y.div_euclid(16);
        let section = self.sections.iter().find(|s| s.section_y == section_y)?;
        let local_y = (world_y - section.y_base) as usize;
        if local_x >= 16 || local_y >= 16 || local_z >= 16 {
            return None;
        }
        let idx = (local_y * 16 + local_z) * 16 + local_x;
        section.block_states.get(idx)
    }

    /// Count of sections that were successfully decoded (non-empty palette expanded).
    pub fn decoded_section_count(&self) -> usize {
        self.sections.len()
    }
}
