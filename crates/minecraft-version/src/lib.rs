//! minecraft-version — version-specific decoding for Minecraft 26.2 (data 4903, proto 776).
//!
//! Responsibilities (per architecture-v2.md):
//! - Holds `MinecraftVersion` identifier
//! - Owns `CanonicalBlockState` representation
//! - Owns `BlockStateRegistry` abstraction and 26.2 implementation (external IdMap from local jar)
//! - Owns palette / `PalettedContainer` decoding (bits, palette, BitStorage) — flashback-format is agnostic
//!
//! flashback-format → raw bytes → minecraft-version → CanonicalBlockState

pub mod block_entity_registry;
pub mod chunk;
pub mod palette;
pub mod registry;
pub mod snapshot;

use serde::{Deserialize, Serialize};

/// Minecraft version identifier — must match `metadata.json` exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MinecraftVersion {
    pub version: String,
    pub data_version: i32,
    pub protocol_version: i32,
}

impl MinecraftVersion {
    pub const V26_2: Self = Self {
        version: String::new(), // placeholder, use `v26_2()` for owned
        data_version: 4903,
        protocol_version: 776,
    };

    pub fn v26_2() -> Self {
        Self {
            version: "26.2".to_string(),
            data_version: 4903,
            protocol_version: 776,
        }
    }

    pub fn known() -> Self {
        Self::v26_2()
    }
}

// Canonical types are owned by replay-model; re-export for convenience.
// This keeps the architectural boundary: minecraft-version decodes, replay-model owns canonical.
pub use replay_model::CanonicalBlockState;

/// Abstraction for resolving numeric BlockState IDs.
pub trait BlockStateRegistry: Send + Sync {
    fn get(&self, id: u32) -> Option<&CanonicalBlockState>;
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn version(&self) -> &MinecraftVersion;
    fn contains(&self, id: u32) -> bool {
        self.get(id).is_some()
    }
}

pub use chunk::{decode_canonical_chunk, ChunkDecodeError};
pub use palette::{decode_chunk_packet, SectionPaletteInfo, SectionPalettes};
pub use registry::{load_26_2_registry, RegistryError, RegistrySource};
pub use snapshot::{decode_snapshot, decode_snapshot_with_data, SnapshotDecode};
