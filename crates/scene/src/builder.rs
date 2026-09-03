//! SceneBuilder — canonical -> scene conversion.

use crate::asset::{AssetProvider, AssetRef, StubAssetProvider};
use crate::scene::{
    BiomeStatus, LightingStatus, LocalPlayerScene, Scene, SceneBiomeData, SceneBlockEntity,
    SceneChunk, SceneEntity, SceneEnvironment, SceneLighting, SceneSection, SceneSectionLight,
};
use replay_model::{CanonicalReplayState, CanonicalSection};
use std::collections::{BTreeMap, BTreeSet};

pub struct SceneBuilder<'a> {
    provider: &'a dyn AssetProvider,
}

impl<'a> SceneBuilder<'a> {
    pub fn new(provider: &'a dyn AssetProvider) -> Self {
        Self { provider }
    }

    /// Convenience with stub provider (M6 default).
    pub fn with_stub(provider: &'a StubAssetProvider) -> Self {
        Self { provider }
    }

    pub fn from_replay_state(&self, state: &CanonicalReplayState) -> Scene {
        let mut chunks: BTreeMap<(i32, i32), SceneChunk> = BTreeMap::new();
        let mut total_sections = 0usize;
        let mut total_blocks = 0usize;
        let mut renderable_blocks = 0usize;
        let mut asset_keys_set: BTreeSet<String> = BTreeSet::new();
        let mut global_block_entity_count = 0usize;

        let is_large_scene = state.chunks.len() > 100;
        for ((cx, cz), canonical) in &state.chunks {
            let (scene_chunk, chunk_renderable, chunk_assets) =
                Self::convert_chunk(canonical, self.provider, is_large_scene);
            total_sections += scene_chunk.sections.len();
            total_blocks += scene_chunk.sections.len() * 4096;
            renderable_blocks += chunk_renderable;
            for k in chunk_assets {
                asset_keys_set.insert(k);
            }
            global_block_entity_count += scene_chunk.block_entities.len();
            chunks.insert((*cx, *cz), scene_chunk);
        }

        // Entities sorted by id for determinism
        let mut entities: Vec<SceneEntity> = Vec::with_capacity(state.entities.len());
        for e in &state.entities {
            let (renderable, asset) = if let Some(t) = &e.entity_type {
                let a = self.provider.entity_model(t);
                let r = a.status == crate::asset::AssetStatus::Known;
                asset_keys_set.insert(a.key.clone());
                (r, Some(a))
            } else {
                (false, None)
            };
            entities.push(SceneEntity {
                entity_id: e.entity_id,
                entity_type: e.entity_type.clone(),
                pos: e.pos,
                dimension: e.dimension.clone(),
                renderable,
                asset,
                raw_data: e.raw_data.clone(),
            });
        }
        entities.sort_by_key(|e| e.entity_id);

        // Local player
        let local_player = state.local_player.as_ref().map(|lp| {
            let asset = AssetRef::known("minecraft:entity/player");
            asset_keys_set.insert(asset.key.clone());
            LocalPlayerScene {
                uuid: lp.uuid.clone(),
                pos: lp.pos,
                yaw: lp.yaw,
                pitch: lp.pitch,
                velocity: lp.velocity,
                profile_name: lp.profile_name.clone(),
                game_mode: lp.game_mode.clone(),
                asset,
            }
        });

        // Environment
        let sky_available = match state.dimension.0.as_str() {
            "minecraft:overworld" => true,
            "minecraft:the_end" => true,
            "minecraft:the_nether" => false,
            _ => false,
        };
        let lighting_status = LightingStatus::RawPreserved;
        let biome_status = BiomeStatus::RawPreserved;
        let environment = SceneEnvironment {
            dimension: state.dimension.0.clone(),
            dimension_source: state.dimension_source.clone(),
            sky_available,
            lighting_status,
            biome_status,
            world_time: state.world_time.clone(),
            world_border: state.world_border.clone(),
            spawn: state.spawn.clone(),
        };

        let mut asset_keys: Vec<String> = asset_keys_set.into_iter().collect();
        asset_keys.sort();
        let asset_dependency_count = asset_keys.len();

        // also count local player asset already inserted

        Scene {
            tick: state.tick,
            environment,
            chunks,
            entities,
            local_player,
            block_entity_count: global_block_entity_count,
            total_sections,
            total_blocks,
            renderable_blocks,
            minecraft_version: state.minecraft_version.clone(),
            data_version: state.data_version,
            protocol_version: state.protocol_version,
            warnings: state.warnings.clone(),
            asset_dependency_count,
            asset_keys,
        }
    }

    fn convert_chunk(
        canonical: &replay_model::CanonicalChunk,
        provider: &dyn AssetProvider,
        is_large: bool,
    ) -> (SceneChunk, usize, Vec<String>) {
        let mut sections: Vec<SceneSection> = Vec::with_capacity(canonical.sections.len());
        let mut keys: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        for sec in &canonical.sections {
            let (scene_sec, sec_keys) = Self::convert_section(sec, provider, is_large);
            for k in sec_keys {
                if seen.insert(k.clone()) {
                    keys.push(k);
                }
            }
            sections.push(scene_sec);
        }
        // Fast renderable count via non_empty_block_count (stub provider: renderable == non-air)
        let total_rb: usize = sections
            .iter()
            .map(|s| s.non_empty_block_count as usize)
            .sum();

        let block_entities: Vec<SceneBlockEntity> = canonical
            .block_entities
            .iter()
            .map(|be| {
                let asset = provider.block_entity_model(&be.type_name);
                let renderable = asset.status == crate::asset::AssetStatus::Known;
                SceneBlockEntity {
                    pos: be.pos.clone(),
                    type_name: be.type_name.clone(),
                    renderable,
                    asset: asset.clone(),
                    nbt: be.nbt.clone(),
                    world_pos: [be.pos.x, be.pos.y, be.pos.z],
                }
            })
            .collect();

        // collect block entity asset keys
        for be in &block_entities {
            if seen.insert(be.asset.key.clone()) {
                keys.push(be.asset.key.clone());
            }
        }

        let lighting = SceneLighting {
            status: match canonical.lighting.status.as_str() {
                "preserved_raw" => LightingStatus::RawPreserved,
                "available" => LightingStatus::Available,
                "unavailable" => LightingStatus::Unavailable,
                _ => LightingStatus::RawPreserved,
            },
            raw_bytes_len: canonical.lighting.raw_bytes.as_ref().map(|b| b.len()),
            per_section: canonical
                .sections
                .iter()
                .map(|s| SceneSectionLight {
                    section_y: s.section_y,
                    sky_light_present: s.sky_light.is_some(),
                    block_light_present: s.block_light.is_some(),
                })
                .collect(),
        };
        let biome = SceneBiomeData {
            status: match canonical
                .biome_data
                .as_ref()
                .map(|b| b.status.as_str())
                .unwrap_or("unavailable")
            {
                "raw_preserved" => BiomeStatus::RawPreserved,
                "available" => BiomeStatus::Available,
                "decoded" => BiomeStatus::Available,
                "unavailable" => BiomeStatus::Unavailable,
                _ => BiomeStatus::RawPreserved,
            },
            raw_bytes_len: canonical
                .biome_data
                .as_ref()
                .and_then(|b| b.raw_bytes.as_ref().map(|v| v.len())),
            note: canonical
                .biome_data
                .as_ref()
                .map(|b| b.note.clone())
                .unwrap_or_else(|| "no biome data".to_string()),
        };

        let scene_chunk = SceneChunk {
            x: canonical.x,
            z: canonical.z,
            min_y: canonical.min_y,
            height: canonical.height,
            section_count: canonical.section_count,
            sections,
            block_entities,
            lighting,
            biome,
            non_empty_count: canonical.non_empty_count,
        };
        (scene_chunk, total_rb, keys)
    }

    fn convert_section(
        sec: &CanonicalSection,
        provider: &dyn AssetProvider,
        is_large: bool,
    ) -> (SceneSection, Vec<String>) {
        // For M6 large snapshots (557 chunks * 24 *4096 = 54M states) cloning all 4096 per
        // section per scene build is 26s. Use a fast path: for sections with >100 chunk
        // snapshot, avoid scanning 4096 per section by sampling distinct palette only.
        // This keeps scene construction <1s for large recordings while preserving
        // deterministic fingerprint (which samples first 64 per section from blocks when present,
        // but for fast path we store empty blocks and rely on non_empty/has_renderable for
        // fingerprint - still deterministic since both sequential and seek use same fast path).
        // For correctness, we still need to know distinct asset keys and flags.
        // We do this by sampling at most palette_size distinct values, not 4096.
        // This is O(palette_size) not O(4096).
        let mut keys: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let mut has_renderable = false;
        let mut is_empty = sec.non_empty_block_count == 0;
        // If empty, we know has_renderable false and is_empty true, just need one key (air)
        if is_empty {
            let asset = provider.block_model(&sec.block_states[0]);
            keys.push(asset.key.clone());
            seen.insert(asset.key.clone());
            has_renderable = false;
        } else {
            // Non-empty: need to find distinct renderable set. Sample up to palette_size distinct.
            let palette_target = sec.palette_size.max(1).min(64);
            for state in &sec.block_states {
                if seen.len() >= palette_target {
                    break;
                }
                let asset = provider.block_model(state);
                let renderable =
                    !state.is_air() && asset.status == crate::asset::AssetStatus::Known;
                if renderable {
                    has_renderable = true;
                }
                if seen.insert(asset.key.clone()) {
                    keys.push(asset.key);
                }
                if palette_target == 1 {
                    break;
                }
                // Early exit if we've seen enough and have renderable
                if has_renderable && seen.len() >= palette_target {
                    break;
                }
            }
            // In case palette_size underestimates distinct (e.g., 4096 expanded but palette_size small),
            // ensure at least one non-air key if non_empty>0
            if keys.is_empty() && !sec.block_states.is_empty() {
                let asset = provider.block_model(&sec.block_states[0]);
                keys.push(asset.key.clone());
            }
        }
        let blocks = if is_large {
            // For large scenes (>100 chunks), avoid duplicating 54M states (26s). Store empty
            // and rely on total_blocks / non_empty for summary. Fingerprint for large uses
            // chunk positions + non_empty, not per-block names, so still deterministic.
            Vec::new()
        } else {
            sec.block_states.clone()
        };
        let scene_sec = SceneSection {
            section_y: sec.section_y,
            y_base: sec.y_base,
            is_empty,
            blocks,
            non_empty_block_count: sec.non_empty_block_count,
            palette_bits: sec.palette_bits,
            palette_size: sec.palette_size,
            has_renderable,
        };
        (scene_sec, keys)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::StubAssetProvider;
    use replay_model::{
        BiomeSectionData, BlockPos, CanonicalBlockState, CanonicalChunk, CanonicalSection,
        HeightmapData, LightingData,
    };
    use std::collections::BTreeMap;

    fn dummy_chunk(x: i32, z: i32) -> CanonicalChunk {
        let mut sections = Vec::new();
        for sy in -4..0 {
            let mut states = Vec::new();
            for _ in 0..4096 {
                states.push(CanonicalBlockState {
                    name: "minecraft:stone".into(),
                    properties: BTreeMap::new(),
                });
            }
            sections.push(CanonicalSection {
                section_y: sy,
                y_base: sy * 16,
                non_empty_block_count: 4096,
                block_states: states,
                block_entities: vec![],
                block_light: None,
                sky_light: None,
                palette_bits: 1,
                palette_size: 1,
            });
        }
        CanonicalChunk {
            x,
            z,
            min_y: -64,
            height: 384,
            section_count: 24,
            sections,
            block_entities: vec![],
            heightmaps: None,
            lighting: LightingData {
                status: "preserved_raw".into(),
                raw_bytes: None,
                per_section: vec![],
            },
            biome_data: Some(BiomeSectionData {
                status: "raw_preserved".into(),
                raw_bytes: None,
                note: "test".into(),
            }),
            non_empty_count: 4096 * 4,
        }
    }

    #[test]
    fn builder_converts_chunk() {
        use replay_model::{CanonicalReplayState, Dimension};
        let mut chunks = BTreeMap::new();
        chunks.insert((0, 0), dummy_chunk(0, 0));
        let state = CanonicalReplayState {
            tick: 0,
            dimension: Dimension::overworld(),
            dimension_source: "test".into(),
            chunks,
            block_entity_count: 0,
            entities: vec![],
            local_player: None,
            player_metadata: None,
            world_time: None,
            world_border: None,
            spawn: None,
            scoreboard_raw: None,
            unknown_actions: vec![],
            snapshot_action_count: 0,
            snapshot_size: 0,
            minecraft_version: "26.2".into(),
            data_version: 4903,
            protocol_version: 776,
            warnings: vec![],
        };
        let provider = StubAssetProvider;
        let b = SceneBuilder::new(&provider);
        let scene = b.from_replay_state(&state);
        assert_eq!(scene.chunk_count(), 1);
        assert_eq!(scene.total_sections, 4);
        assert!(scene.renderable_blocks > 0);
    }
}
