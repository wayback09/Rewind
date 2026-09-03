use crate::chunk::{BlockEntity, CanonicalBlockState, CanonicalChunk};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Version-independent dimension, e.g., `minecraft:overworld`
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dimension(pub String);

impl Dimension {
    pub fn overworld() -> Self {
        Self("minecraft:overworld".to_string())
    }
    pub fn the_nether() -> Self {
        Self("minecraft:the_nether".to_string())
    }
    pub fn the_end() -> Self {
        Self("minecraft:the_end".to_string())
    }
}

impl std::fmt::Display for Dimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Local player as created by `create_local_player` snapshot action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalPlayer {
    pub uuid: String,  // UUID string
    pub pos: [f64; 3], // x,y,z
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Option<[f64; 3]>,
    pub game_mode: Option<String>, // e.g., "survival", or numeric id as string
    pub profile_name: Option<String>,
    pub raw_payload_len: usize, // for diagnostics
}

/// Minimal entity — only as far as snapshot evidence supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalEntity {
    pub entity_id: i32,
    pub entity_type: Option<String>, // e.g., `minecraft:zombie` if known
    pub pos: Option<[f64; 3]>,
    #[serde(default)]
    pub velocity: Option<[f64; 3]>,
    pub dimension: Option<String>,
    pub raw_data: Option<serde_json::Value>, // preserved raw where decoding not complete
}

/// Player metadata (from PlayerInfoUpdate etc.) — raw for now.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerMetadata {
    pub entries: Vec<serde_json::Value>, // raw
}

/// World time as decoded from SetTime or ClockManager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldTime {
    pub game_time: Option<i64>,
    pub day_time: Option<i64>,
    pub raw_status: String, // "decoded" or "raw_preserved"
    pub raw_payload_len: Option<usize>,
}

/// World border as decoded from InitializeBorder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldBorder {
    pub center_x: Option<f64>,
    pub center_z: Option<f64>,
    pub size: Option<f64>,
    pub lerp_target: Option<f64>,
    pub raw_status: String,
    pub raw_payload_len: Option<usize>,
}

/// Spawn information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnInfo {
    pub pos: Option<[i32; 3]>, // x,y,z
    pub angle: Option<f32>,
    pub raw_status: String,
    pub raw_payload_len: Option<usize>,
}

/// Unknown/unsupported snapshot action preserved for diagnostics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnknownAction {
    pub identifier: String,
    pub local_id: i32,
    pub payload_len: usize,
    pub payload_prefix_hex: String, // first 16 bytes hex for debugging
}

/// Canonical replay state immediately after snapshot, before replay deltas.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalReplayState {
    pub tick: u32, // initial tick, always 0 for snapshot
    pub dimension: Dimension,
    pub dimension_source: String, // e.g., "login_packet" or "fallback_overworld"
    pub chunks: BTreeMap<(i32, i32), CanonicalChunk>,
    pub block_entity_count: usize, // total across chunks
    pub entities: Vec<CanonicalEntity>,
    pub local_player: Option<LocalPlayer>,
    pub player_metadata: Option<PlayerMetadata>,
    pub world_time: Option<WorldTime>,
    pub world_border: Option<WorldBorder>,
    pub spawn: Option<SpawnInfo>,
    pub scoreboard_raw: Option<Vec<serde_json::Value>>, // preserved
    pub unknown_actions: Vec<UnknownAction>,
    pub snapshot_action_count: usize,
    pub snapshot_size: usize,
    pub minecraft_version: String,
    pub data_version: i32,
    pub protocol_version: i32,
    // Diagnostics
    pub warnings: Vec<String>,
}

impl CanonicalReplayState {
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }
    pub fn canonical_block_state_count(&self) -> usize {
        self.chunks.values().map(|c| c.sections.len() * 4096).sum()
    }
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }
    pub fn has_local_player(&self) -> bool {
        self.local_player.is_some()
    }
}
