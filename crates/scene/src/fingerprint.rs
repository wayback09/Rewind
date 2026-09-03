//! Deterministic scene fingerprint (FNV-1a over sorted canonical content).

use crate::scene::Scene;

pub fn fingerprint(scene: &Scene) -> u64 {
    let mut h: u64 = 14695981039346656037;
    let mut fnv = |bytes: &[u8]| {
        for b in bytes {
            h ^= *b as u64;
            h = h.wrapping_mul(1099511628211);
        }
    };
    fnv(&scene.tick.to_le_bytes());
    fnv(scene.environment.dimension.as_bytes());
    fnv(&(scene.chunks.len() as u64).to_le_bytes());
    fnv(&(scene.block_entity_count as u64).to_le_bytes());
    fnv(&(scene.total_sections as u64).to_le_bytes());
    // Chunks sorted already (BTreeMap)
    for ((x, z), chunk) in &scene.chunks {
        fnv(&x.to_le_bytes());
        fnv(&z.to_le_bytes());
        fnv(&(chunk.non_empty_count as u64).to_le_bytes());
        for sec in &chunk.sections {
            fnv(&sec.section_y.to_le_bytes());
            // sample first 64 blocks to keep fast but still detect changes; plus non-empty count
            fnv(&sec.non_empty_block_count.to_le_bytes());
            for st in sec.blocks.iter().take(64) {
                fnv(st.name.as_bytes());
                for (k, v) in &st.properties {
                    fnv(k.as_bytes());
                    fnv(v.as_bytes());
                }
            }
            // include has_renderable/is_empty
            fnv(&[sec.has_renderable as u8, sec.is_empty as u8]);
        }
        for be in &chunk.block_entities {
            fnv(be.type_name.as_bytes());
            fnv(&be.pos.x.to_le_bytes());
            fnv(&be.pos.y.to_le_bytes());
            fnv(&be.pos.z.to_le_bytes());
        }
    }
    // Entities sorted by id
    fnv(&(scene.entities.len() as u64).to_le_bytes());
    for e in &scene.entities {
        fnv(&e.entity_id.to_le_bytes());
        if let Some(t) = &e.entity_type {
            fnv(t.as_bytes());
        }
        if let Some(p) = e.pos {
            fnv(&p[0].to_le_bytes());
            fnv(&p[1].to_le_bytes());
            fnv(&p[2].to_le_bytes());
        }
    }
    if let Some(lp) = &scene.local_player {
        fnv(lp.uuid.as_bytes());
        fnv(&lp.pos[0].to_le_bytes());
        fnv(&lp.pos[1].to_le_bytes());
        fnv(&lp.pos[2].to_le_bytes());
        fnv(&lp.yaw.to_le_bytes());
        fnv(&lp.pitch.to_le_bytes());
    }
    fnv(&(scene.asset_dependency_count as u64).to_le_bytes());
    for k in &scene.asset_keys {
        fnv(k.as_bytes());
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asset::StubAssetProvider;
    use crate::builder::SceneBuilder;
    use replay_model::{CanonicalReplayState, Dimension};
    use std::collections::BTreeMap;

    #[test]
    fn fingerprint_deterministic() {
        let state = CanonicalReplayState {
            tick: 5,
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
        let s1 = b.from_replay_state(&state);
        let s2 = b.from_replay_state(&state);
        assert_eq!(fingerprint(&s1), fingerprint(&s2));
    }
}
