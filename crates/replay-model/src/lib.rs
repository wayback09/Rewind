//! replay-model — future canonical replay representation.
//!
//! M0: only types justified by current research are created.
//! Do not prematurely implement the complete model.
//! Keep this minimal until palette / registry decoding is proven.

pub mod chunk;

pub use chunk::{
    BiomeSectionData, BlockEntity, BlockPos, CanonicalBlockState, CanonicalChunk, CanonicalSection,
    HeightmapData, LightingData, SectionLight,
};

use serde::{Deserialize, Serialize};

/// Minimal validated replay summary — just enough to prove M0 understands the container.
/// This is NOT the full CanonicalReplay from synthesis docs; that will be expanded later.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedReplaySummary {
    pub uuid: String,
    pub name: String,
    pub version_string: Option<String>,
    pub data_version: Option<i32>,
    pub protocol_version: Option<i32>,
    pub total_ticks: Option<i32>,
    pub chunks: Vec<ChunkSummary>,
    pub total_cache_entries: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkSummary {
    pub file_name: String,
    pub duration: i32,
    pub tick_count: usize,
    pub snapshot_tlvs: usize,
    pub replay_tlvs: usize,
}
