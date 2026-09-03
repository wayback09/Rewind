//! Scene data model — version-independent, renderer-facing.

use crate::asset::AssetRef;
use replay_model::{BlockPos, CanonicalBlockState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Lighting / biome status

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingStatus {
    Available,
    RawPreserved,
    Unavailable,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BiomeStatus {
    Available,
    RawPreserved,
    Unavailable,
    Unsupported,
}

// ---------------------------------------------------------------------------
// Per-block render reference

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockRenderRef {
    /// Canonical block identity (never a numeric id).
    pub state: CanonicalBlockState,
    /// Asset lookup key derived via AssetProvider.
    pub model: AssetRef,
    /// Whether this block contributes geometry (false for air).
    pub renderable: bool,
}

// ---------------------------------------------------------------------------
// Chunks / sections

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSection {
    pub section_y: i32,
    pub y_base: i32,
    /// True if palette was single air or all air after expand.
    pub is_empty: bool,
    /// 4096 states in canonical idx order (ly*16+lz)*16+lx.
    pub blocks: Vec<CanonicalBlockState>,
    pub non_empty_block_count: u16,
    pub palette_bits: u8,
    pub palette_size: usize,
    /// Does this section contain any non-air renderable block?
    pub has_renderable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneLighting {
    pub status: LightingStatus,
    pub raw_bytes_len: Option<usize>,
    pub per_section: Vec<SceneSectionLight>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneSectionLight {
    pub section_y: i32,
    pub sky_light_present: bool,
    pub block_light_present: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneBiomeData {
    pub status: BiomeStatus,
    pub raw_bytes_len: Option<usize>,
    pub note: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneBlockEntity {
    pub pos: BlockPos,
    pub type_name: String,
    /// Whether a model exists for this type (via provider)
    pub renderable: bool,
    pub asset: AssetRef,
    /// Preserved NBT as JSON
    pub nbt: serde_json::Value,
    /// World position helper (same as pos)
    pub world_pos: [i32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneChunk {
    pub x: i32,
    pub z: i32,
    pub min_y: i32,
    pub height: i32,
    pub section_count: usize,
    pub sections: Vec<SceneSection>,
    pub block_entities: Vec<SceneBlockEntity>,
    pub lighting: SceneLighting,
    pub biome: SceneBiomeData,
    pub non_empty_count: usize,
}

impl SceneChunk {
    /// Retrieve block state at world Y and local 0..15 x,z within this chunk.
    pub fn block_at(
        &self,
        world_y: i32,
        local_x: usize,
        local_z: usize,
    ) -> Option<&CanonicalBlockState> {
        let sy = world_y.div_euclid(16);
        let sec = self.sections.iter().find(|s| s.section_y == sy)?;
        let ly = (world_y - sec.y_base) as usize;
        if local_x >= 16 || ly >= 16 || local_z >= 16 {
            return None;
        }
        let idx = (ly * 16 + local_z) * 16 + local_x;
        sec.blocks.get(idx)
    }
}

impl SceneSection {
    /// Check if world coordinate belongs to this section and return local idx.
    pub fn contains_world_y(&self, world_y: i32) -> bool {
        world_y >= self.y_base && world_y < self.y_base + 16
    }
    pub fn block_at_local(&self, lx: usize, ly: usize, lz: usize) -> Option<&CanonicalBlockState> {
        if lx >= 16 || ly >= 16 || lz >= 16 {
            return None;
        }
        let idx = (ly * 16 + lz) * 16 + lx;
        self.blocks.get(idx)
    }
}

// ---------------------------------------------------------------------------
// Entities

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneEntity {
    pub entity_id: i32,
    pub entity_type: Option<String>,
    pub pos: Option<[f64; 3]>,
    pub velocity: Option<[f64; 3]>,
    pub dimension: Option<String>,
    pub renderable: bool,
    pub asset: Option<AssetRef>,
    pub raw_data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPlayerScene {
    pub uuid: String,
    pub pos: [f64; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Option<[f64; 3]>,
    pub profile_name: Option<String>,
    pub game_mode: Option<String>,
    pub asset: AssetRef,
}

// ---------------------------------------------------------------------------
// Environment

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneEnvironment {
    pub dimension: String,
    pub dimension_source: String,
    pub sky_available: bool,
    /// Whether lighting is available in scene sense (preserved_raw still counts as unavailable for rendering)
    pub lighting_status: LightingStatus,
    pub biome_status: BiomeStatus,
    pub world_time: Option<replay_model::WorldTime>,
    pub world_border: Option<replay_model::WorldBorder>,
    pub spawn: Option<replay_model::SpawnInfo>,
}

// ---------------------------------------------------------------------------
// Top-level Scene — immutable snapshot

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub tick: u32,
    pub environment: SceneEnvironment,
    /// Chunks sorted by (x,z) — BTreeMap for determinism.
    pub chunks: BTreeMap<(i32, i32), SceneChunk>,
    pub entities: Vec<SceneEntity>, // sorted by entity_id
    pub local_player: Option<LocalPlayerScene>,
    pub block_entity_count: usize,
    pub total_sections: usize,
    pub total_blocks: usize, // sections * 4096
    pub renderable_blocks: usize,
    pub minecraft_version: String,
    pub data_version: i32,
    pub protocol_version: i32,
    pub warnings: Vec<String>,
    /// Asset dependency count (unique keys)
    pub asset_dependency_count: usize,
    /// Sorted unique asset keys
    pub asset_keys: Vec<String>,
}

impl Scene {
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
    pub fn section_count(&self) -> usize {
        self.total_sections
    }
}
