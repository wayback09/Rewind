//! Asset / model abstraction — renderer-independent.
//! Scene contains AssetRef, not GPU handles.

use replay_model::CanonicalBlockState;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetStatus {
    /// Model known and resolvable (e.g., vanilla block model exists)
    Known,
    /// Asset not available in current provider (missing file, not downloaded)
    Unavailable,
    /// Asset type explicitly unsupported in this provider
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRef {
    /// Stable lookup key, e.g., `minecraft:block/stone` or `minecraft:entity/zombie`
    pub key: String,
    pub status: AssetStatus,
    /// Optional note why unavailable/unsupported
    pub note: Option<String>,
}

impl AssetRef {
    pub fn known(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            status: AssetStatus::Known,
            note: None,
        }
    }
    pub fn unavailable(key: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            status: AssetStatus::Unavailable,
            note: Some(note.into()),
        }
    }
    pub fn unsupported(key: impl Into<String>, note: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            status: AssetStatus::Unsupported,
            note: Some(note.into()),
        }
    }
}

/// Renderer-facing asset provider — behind a trait so Scene stays independent of
/// filesystem layout, .minecraft location, network, etc.
pub trait AssetProvider: Send + Sync {
    /// Resolve a block state's model.
    fn block_model(&self, state: &CanonicalBlockState) -> AssetRef;
    /// Resolve an entity type's model.
    fn entity_model(&self, entity_type: &str) -> AssetRef;
    /// Resolve a block entity type's model.
    fn block_entity_model(&self, type_name: &str) -> AssetRef;
    /// Resolve a texture (unused in M6, stubbed).
    fn texture(&self, key: &str) -> AssetRef {
        AssetRef::unsupported(key, "texture resolution not implemented in M6")
    }
}

/// Stub provider for M6 — never requires .minecraft, never hits disk/network.
/// Distinguishes known vs unsupported via simple heuristics, without loading assets.
#[derive(Debug, Clone, Default)]
pub struct StubAssetProvider;

impl AssetProvider for StubAssetProvider {
    fn block_model(&self, state: &CanonicalBlockState) -> AssetRef {
        // In M6 we just map name to key `minecraft:block/<name>`; no filesystem check.
        // Treat air as known but non-renderable; unknown names as unavailable.
        if state.name == "minecraft:air" {
            AssetRef {
                key: "minecraft:block/air".into(),
                status: AssetStatus::Known,
                note: Some("air — no geometry".into()),
            }
        } else if state.name.starts_with("minecraft:") {
            AssetRef::known(format!(
                "minecraft:block/{}",
                state.name.trim_start_matches("minecraft:")
            ))
        } else {
            AssetRef::unavailable(state.name.clone(), "non-minecraft namespace")
        }
    }

    fn entity_model(&self, entity_type: &str) -> AssetRef {
        if entity_type.starts_with("minecraft:") {
            AssetRef::known(format!(
                "minecraft:entity/{}",
                entity_type.trim_start_matches("minecraft:")
            ))
        } else if entity_type.starts_with("unknown") || entity_type.is_empty() {
            AssetRef::unavailable(entity_type.to_string(), "unknown entity type")
        } else {
            AssetRef::known(format!("minecraft:entity/{}", entity_type))
        }
    }

    fn block_entity_model(&self, type_name: &str) -> AssetRef {
        if type_name.contains("block_entity_type_") {
            AssetRef::unsupported(
                type_name.to_string(),
                "numeric block entity type — no symbolic model",
            )
        } else if type_name.starts_with("minecraft:") {
            AssetRef::known(format!(
                "minecraft:block_entity/{}",
                type_name.trim_start_matches("minecraft:")
            ))
        } else {
            AssetRef::unavailable(type_name.to_string(), "unknown block entity type")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use replay_model::CanonicalBlockState;
    use std::collections::BTreeMap;

    #[test]
    fn stub_block_model_air_and_stone() {
        let p = StubAssetProvider;
        let air = CanonicalBlockState {
            name: "minecraft:air".into(),
            properties: BTreeMap::new(),
        };
        let a = p.block_model(&air);
        assert_eq!(a.status, AssetStatus::Known);
        let stone = CanonicalBlockState {
            name: "minecraft:stone".into(),
            properties: BTreeMap::new(),
        };
        let s = p.block_model(&stone);
        assert_eq!(s.key, "minecraft:block/stone");
        assert_eq!(s.status, AssetStatus::Known);
    }

    #[test]
    fn stub_block_entity_numeric_unsupported() {
        let p = StubAssetProvider;
        let r = p.block_entity_model("minecraft:block_entity_type_1");
        assert_eq!(r.status, AssetStatus::Unsupported);
    }
}
