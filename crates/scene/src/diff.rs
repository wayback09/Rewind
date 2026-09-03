//! Lightweight Scene diff — deterministic, simple.

use crate::scene::Scene;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChunkDiff {
    Added { x: i32, z: i32 },
    Removed { x: i32, z: i32 },
    Changed { x: i32, z: i32 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityDiff {
    Added { entity_id: i32 },
    Removed { entity_id: i32 },
    Changed { entity_id: i32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneDiff {
    pub from_tick: u32,
    pub to_tick: u32,
    pub chunk_diffs: Vec<ChunkDiff>,
    pub entity_diffs: Vec<EntityDiff>,
    pub local_player_changed: bool,
    pub environment_changed: bool,
    pub is_empty: bool,
}

pub fn diff(a: &Scene, b: &Scene) -> SceneDiff {
    use std::collections::BTreeSet;
    let mut chunk_diffs = Vec::new();
    let a_keys: BTreeSet<_> = a.chunks.keys().cloned().collect();
    let b_keys: BTreeSet<_> = b.chunks.keys().cloned().collect();
    for k in a_keys.union(&b_keys) {
        let in_a = a.chunks.contains_key(k);
        let in_b = b.chunks.contains_key(k);
        if in_a && !in_b {
            chunk_diffs.push(ChunkDiff::Removed { x: k.0, z: k.1 });
        } else if !in_a && in_b {
            chunk_diffs.push(ChunkDiff::Added { x: k.0, z: k.1 });
        } else {
            // both present — check section non_empty or fingerprint quickly
            let ca = &a.chunks[k];
            let cb = &b.chunks[k];
            if ca.non_empty_count != cb.non_empty_count || ca.sections.len() != cb.sections.len() {
                chunk_diffs.push(ChunkDiff::Changed { x: k.0, z: k.1 });
            } else {
                // shallow check first block name of first section
                let mut changed = false;
                for (sa, sb) in ca.sections.iter().zip(cb.sections.iter()) {
                    if sa.has_renderable != sb.has_renderable || sa.is_empty != sb.is_empty {
                        changed = true;
                        break;
                    }
                    if sa.blocks.first().map(|b| &b.name) != sb.blocks.first().map(|b| &b.name) {
                        changed = true;
                        break;
                    }
                    if sa.non_empty_block_count != sb.non_empty_block_count {
                        changed = true;
                        break;
                    }
                }
                if changed {
                    chunk_diffs.push(ChunkDiff::Changed { x: k.0, z: k.1 });
                }
            }
        }
    }
    // Entities — by id
    let mut entity_diffs = Vec::new();
    let a_ids: BTreeSet<_> = a.entities.iter().map(|e| e.entity_id).collect();
    let b_ids: BTreeSet<_> = b.entities.iter().map(|e| e.entity_id).collect();
    for id in a_ids.union(&b_ids) {
        let in_a = a_ids.contains(id);
        let in_b = b_ids.contains(id);
        if in_a && !in_b {
            entity_diffs.push(EntityDiff::Removed { entity_id: *id });
        } else if !in_a && in_b {
            entity_diffs.push(EntityDiff::Added { entity_id: *id });
        } else {
            let ea = a.entities.iter().find(|e| e.entity_id == *id).unwrap();
            let eb = b.entities.iter().find(|e| e.entity_id == *id).unwrap();
            if ea.pos != eb.pos
                || ea.velocity != eb.velocity
                || ea.entity_type != eb.entity_type
                || ea.dimension != eb.dimension
            {
                entity_diffs.push(EntityDiff::Changed { entity_id: *id });
            }
        }
    }

    let local_player_changed = match (&a.local_player, &b.local_player) {
        (None, None) => false,
        (Some(_), None) | (None, Some(_)) => true,
        (Some(pa), Some(pb)) => {
            pa.pos != pb.pos || pa.yaw != pb.yaw || pa.pitch != pb.pitch || pa.uuid != pb.uuid
        }
    };
    let environment_changed = a.environment.dimension != b.environment.dimension;

    let is_empty = chunk_diffs.is_empty()
        && entity_diffs.is_empty()
        && !local_player_changed
        && !environment_changed;

    SceneDiff {
        from_tick: a.tick,
        to_tick: b.tick,
        chunk_diffs,
        entity_diffs,
        local_player_changed,
        environment_changed,
        is_empty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::StubAssetProvider;
    use crate::builder::SceneBuilder;
    use replay_model::{CanonicalReplayState, Dimension};
    use std::collections::BTreeMap;

    #[test]
    fn diff_empty_on_same_tick() {
        let state = CanonicalReplayState {
            tick: 0,
            dimension: Dimension::overworld(),
            dimension_source: "test".into(),
            chunks: BTreeMap::new(),
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
        let p = StubAssetProvider;
        let b = SceneBuilder::new(&p);
        let s = b.from_replay_state(&state);
        let d = diff(&s, &s);
        assert!(d.is_empty);
        assert!(!d.environment_changed);
    }

    #[test]
    fn diff_detects_dimension_change() {
        let mk = |dim: &str, tick: u32| {
            let state = CanonicalReplayState {
                tick,
                dimension: Dimension(dim.into()),
                dimension_source: "test".into(),
                chunks: BTreeMap::new(),
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
            let p = StubAssetProvider;
            let b = SceneBuilder::new(&p);
            b.from_replay_state(&state)
        };
        let a = mk("minecraft:overworld", 0);
        let b = mk("minecraft:the_nether", 1311);
        let d = diff(&a, &b);
        assert!(d.environment_changed);
        assert!(!d.is_empty);
    }
}
