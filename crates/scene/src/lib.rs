//! scene — M6 renderer-independent scene representation.
//!
//! Dependency chain: flashback-format -> minecraft-version -> replay-model -> playback -> scene -> (future renderer)
//! Scene consumes CanonicalReplayState, never Flashback bytes or Minecraft packets.

pub mod asset;
pub mod builder;
pub mod coordinates;
pub mod diff;
pub mod fingerprint;
pub mod scene;

pub use asset::{AssetProvider, AssetRef, AssetStatus, StubAssetProvider};
pub use builder::SceneBuilder;
pub use coordinates::{
    chunk_origin, index_to_local, local_to_index, local_to_world, section_y_to_y_base,
    world_to_chunk, world_to_chunk_section_local, world_to_local, world_y_to_section_y,
};
pub use diff::{diff, ChunkDiff, EntityDiff, SceneDiff};
pub use fingerprint::fingerprint;
pub use scene::{
    BiomeStatus, BlockRenderRef, LightingStatus, LocalPlayerScene, Scene, SceneBiomeData,
    SceneBlockEntity, SceneChunk, SceneEntity, SceneEnvironment, SceneLighting, SceneSection,
};
