# Rewind

Standalone desktop viewer and editor for **Moulberry's Flashback** `.flashback` recordings — designed to work without Minecraft running.

Rewind reads Flashback recordings directly, validates their binary structure, decodes Minecraft-specific data, and converts it into a version-independent canonical replay model.

The long-term goal is a standalone replay viewer/editor capable of reconstructing and editing Flashback recordings without requiring the Minecraft client.

> **Status:** M0, M1, M2, M3, M4, M5, and **M6 (scene representation)** complete — renderer-independent Scene validated, seek→scene == sequential→scene.  
> Rendering (M7) and the desktop UI come later.

## Features

### M0 — Flashback container and recording format

Rewind can read and validate the Flashback recording container:

- ZIP/DEFLATE container
- `metadata.json`
- Replay chunk files
- Flashback replay magic `0xD780E884`
- Dynamic per-chunk action tables
- VarInt identifiers
- Snapshot framing
- TLV action records
- Level chunk cache entries
- Tick boundary validation
- Recording duration validation
- Cross-checking `next_tick` counts against metadata

Action IDs are resolved from the recording's action table rather than hard-coded.

For example, optional actions can shift the IDs of core actions such as `next_tick`.

### M1 — Minecraft registry and palette decoding

Rewind can decode Minecraft 26.2 block-state palettes using the external Minecraft block-state registry.

Current validated version:

- **Minecraft:** 26.2
- **Data version:** 4903
- **Protocol:** 776
- **Block states:** 32,366

Numeric Minecraft block-state IDs are resolved into canonical states such as:

```text
minecraft:air
minecraft:stone
minecraft:bedrock
minecraft:sandstone
minecraft:oak_stairs[facing=north,half=top,shape=straight,waterlogged=true]
```

The registry is derived from the Minecraft 26.2 client JAR and stored as a compact indexed registry.

Rewind supports the Minecraft palette modes required by the current recordings, including:

- Single-value palettes
- Indirect palettes
- Global/direct palette
- Minecraft bit storage

### M2 — Canonical chunk reconstruction

Rewind now converts decoded Minecraft chunk data into a version-independent CanonicalChunk.

### M3 — Canonical replay state (snapshot)

Rewind now reconstructs the initial replay state from a Flashback replay chunk snapshot:

- Dynamic action-table resolution (no hard-coded IDs)
- Snapshot action dispatch via `flashback:action/*` identifiers
- Dimension (`minecraft:overworld` / `the_nether` / `the_end`) from login packet
- Chunks via `level_chunk_cached` → `level_chunk_caches/0` → `CanonicalChunk` (M2)
- Local player via `create_local_player` (UUID, pos, yaw/pitch, velocity)
- World border / time / spawn where decoded (raw preserved otherwise)
- Player metadata / scoreboard raw preserved
- Unknown actions preserved with `local_id`, `payload_len`, `payload_prefix_hex` for diagnostics
- Initial tick 0, before replay deltas

Rewind now converts decoded Minecraft chunk data into a version-independent CanonicalChunk.

A canonical chunk contains:

- Chunk coordinates
- Sections
- 4,096 canonical block states per section
- Canonical block-state names and properties
- Block entities
- Block entity NBT
- Heightmaps
- Raw lighting data
- Raw biome data
- Section metadata
- Non-empty block counts

The important architectural boundary is:

```text
Flashback recording
        │
        ▼
 flashback-format
        │
        │ raw recording structures
        ▼
 minecraft-version
        │
        │ Minecraft-specific decoding
        ▼
   CanonicalChunk
        │
        ▼
    replay-model
        │
        ▼
 future renderer / editor
```

`replay-model` does not know what a PalettedContainer, BitStorage, or Minecraft registry ID is.

This allows the eventual renderer and editor to operate on canonical data rather than directly on Minecraft's network representation.

## Current limitations

Rewind is still early-stage.

M4 (tick-by-tick playback) and M5 (snapshot-based seeking, forward/backward, checkpoints) are validated.

The following are intentionally not complete yet:

- 3D rendering
- Minecraft model loading
- Texture loading
- Lighting reconstruction
- Full biome canonicalization
- Complete block-entity semantics
- Entity reconstruction
- Editing
- Camera/keyframe tools
- Timeline UI
- Desktop GUI

Some Minecraft data is currently preserved in raw form when its exact representation has not yet been fully validated.

Rewind prefers preserving unknown data over inventing an incorrect interpretation.

## Requirements

- Rust 1.88+ with Cargo.

- Minecraft 26.2

  A Minecraft 26.2 client JAR is currently used to derive the block-state registry:

  ```
  %APPDATA%\.minecraft\versions\26.2\26.2.jar
  ```

  The generated registry is already included in the repository, so most users do not need to regenerate it.

- Java

  A compatible JDK is required only when regenerating the registry from the Minecraft client JAR.

  The current development environment uses Microsoft JDK 25.

## Quick start

Clone the repository and build the workspace:

```powershell
cargo fmt --all
cargo check --workspace
cargo test --workspace -- --test-threads=1
```

### M0 — Validate a Flashback recording

```powershell
cargo run --bin flashback-probe -- recordings/basic/test_recording.zip
```

Validation output is written to:

```
target/verify-m0.json
```

### M1 — Resolve Minecraft block-state IDs

```powershell
cargo run --bin flashback-idmap-probe -- recordings/basic/test_recording.zip
```

Example resolved states:

```
85  -> minecraft:bedrock
1   -> minecraft:stone
578 -> minecraft:sandstone
0   -> minecraft:air
```

Output:

```
target/verify-m1.json
```

### M2 — Build a canonical chunk

```powershell
cargo run --bin flashback-canonical-probe -- recordings/basic/test_recording.zip
cargo run --bin flashback-replay-state-probe -- recordings/basic/test_recording.zip
```

### M4 — Tick-by-tick playback

```powershell
cargo run --bin flashback-playback-probe -- recordings/basic/test_recording.zip
cargo run --bin flashback-playback-probe -- recordings/chunks/test_recording3.zip
```

Output: `target/verify-m4.json` — final tick matches `metadata.json`, dimension, local player, checkpoints.

### M5 — Snapshot-based seeking (random access)

```powershell
cargo run --release --bin flashback-seek-probe -- recordings/basic/test_recording.zip
cargo run --release --bin flashback-seek-probe -- recordings/chunks/test_recording3.zip
```

M5 reuses the single `step_tick` apply path. Forward seeks replay forward; backward seeks restore the nearest file snapshot (`c0` at 0, `c1` at 1311) via `snapshot_cache` then linear replay. Validation checks `seek(N) == sequential(N)` (FNV hash over dimension/chunks/entities) and cross-chunk dimension restores.

Output: `target/verify-m5.json` — `validation_ok: true` when all seek targets match sequential. Note: basic recording snapshot decode (557 chunks, 54M states) costs ~30s per player, so the probe is heavy (30s per `ReplayPlayer::initialize`) — use `--release` and expect >60s.

Probe targets are tiny for CI (`[0,1,5,10]` and `1311/1312`) to keep runtime under ~120s in release.

### M6 — Renderer-independent scene representation

```powershell
cargo run --release --bin flashback-scene-probe -- recordings/basic/test_recording.zip
cargo run --release --bin flashback-scene-probe -- recordings/chunks/test_recording3.zip
```

M6 builds `Scene` from `CanonicalReplayState` (no Flashback/Minecraft parsing). Exposes `SceneChunk/Section` (x,z, section_y, y_base, blocks, lighting/biome `RawPreserved`), `SceneEntity`/`LocalPlayerScene` (`minecraft:zombie` not numeric), `SceneEnvironment` (dimension/sky/time/border), `AssetProvider` trait (`StubAssetProvider` → `Known/Unavailable/Unsupported`), `fingerprint` (FNV over sorted chunks/entities), `diff` (ChunkAdded/Changed etc.), and coordinate helpers (`world→chunk/section/local`, `idx=(y*16+z)*16+x`). Validates `seek(T)→scene` fingerprint `==` sequential `→scene` for `T=[0,1,100,500,1311,1312,final]` and Overworld→Nether (1311) transition.

Output: `target/verify-m6.json` (and per-recording `target/verify-m6-*.json`) — `validation_ok: true`, `construction_time_ms` ~1-2s per scene (large 557-chunk fast path avoids 54M clones). Probe covers all three recordings (916t, 2242t, 2341t).

Example:

```
Position: -7,1
Sections: 24
Block states: 98304
Block entities: 0
Lighting: preserved_raw
Biomes: raw_preserved
```

Output:

```
target/verify-m2.json
target/verify-m3.json
```

## Registry generation

The repository contains:

```
crates/minecraft-version/data/26.2-blocks-array.json
```

This is a compact indexed representation of the Minecraft 26.2 block-state registry.

The source Minecraft report contains:

- 32366 block states
- IDs 0..32365
- 0: air
- 1: stone
- 85: bedrock
- 3907: oak_stairs

The registry is indexed by the original Minecraft global block-state ID.

If the generated registry needs to be reproduced, Minecraft's data generator can create `reports/blocks.json` from the 26.2 client JAR.

## Workspace

```
Rewind/
├── Cargo.toml
│
├── crates/
│   ├── flashback-format/
│   │   └── # Flashback ZIP/container format
│   │
│   ├── minecraft-version/
│   │   ├── data/
│   │   │   └── 26.2-blocks-array.json
│   │   └── # Minecraft 26.2 decoding
│   │
│   ├── replay-model/
│   │   └── # Version-independent canonical replay data
│   │
│   ├── playback/
│   │   └── # M4/M5 tick-by-tick + seeking (snapshot cache, checkpoints)
│   │
│   ├── scene/
│   │   └── # M6 renderer-independent Scene (coordinates, builder, fingerprint, diff, asset)
│   │
│   └── app/
│       └── src/bin/
│           ├── flashback-probe.rs
│           ├── flashback-idmap-probe.rs
│           ├── flashback-canonical-probe.rs
│           ├── flashback-replay-state-probe.rs
│           ├── flashback-playback-probe.rs
│           ├── flashback-seek-probe.rs
│           └── flashback-scene-probe.rs
│
└── recordings/
    └── # Local test recordings
```

The core separation is intentional:

```
flashback-format
    knows Flashback

minecraft-version
    knows Minecraft

replay-model
    knows neither
```

## Validation

Rewind has been validated against real Flashback recordings.

- **test_recording.zip**
  - 916 ticks
  - 24 sections decoded
  - 98,304 canonical block states
  - 0 unresolved block-state IDs
  - 0 block entities
- **test_recording_2.zip**
  - 2242 ticks
  - First section decoded
  - 4,096 canonical block states
  - 9 block entities
  - Block entity NBT preserved

  This recording currently exposes an unresolved later-section decoding edge case.

- **test_recording3.zip**
  - 2341 ticks
  - 9 sections decoded
  - 36,864 canonical block states
  - 0 unresolved block-state IDs

  This recording currently exposes a later-section decoding edge case.

These failures are retained as validation targets rather than being hidden by weakening the decoder.

## Architecture

Rewind is being built in layers.

```
┌───────────────────────────────┐
│         Desktop UI            │
├───────────────────────────────┤
│       Timeline / Editor       │
├───────────────────────────────┤
│        Scene Evaluation       │
├───────────────────────────────┤
│       Canonical Replay        │
├───────────────────────────────┤
│      Minecraft Version        │
├───────────────────────────────┤
│       Flashback Format        │
└───────────────────────────────┘
```

The version-specific layer is deliberately isolated so that Minecraft protocol and registry details do not leak into the canonical replay representation.

The eventual goal is to support the same general principle for more Minecraft versions:

```
Minecraft recording
       │
       ▼
version adapter
       │
       ▼
canonical replay model
       │
       ▼
version-independent renderer/editor
```

## Roadmap

- **M0 — Format archaeology**
  - Flashback ZIP parsing
  - Replay chunk framing
  - Dynamic action tables
  - Snapshot/TLV parsing
  - Chunk-cache parsing
  - Tick validation
- **M1 — Minecraft decoding**
  - Minecraft 26.2 registry
  - Block-state ID resolution
  - PalettedContainer decoding
  - BitStorage expansion
  - Real-recording validation
- **M2 — Canonical chunks**
  - CanonicalBlockState
  - CanonicalChunk
  - Canonical sections
  - Block entity preservation
  - Heightmap decoding
  - Raw lighting preservation
  - Raw biome preservation
  - Real-recording validation
- **M3 — Replay state**
  - Decode replay snapshots into canonical state
  - Construct initial world state
  - Track dimensions
  - Track chunks
  - Track entities
  - Track local player
  - Track world time
  - Track scoreboard/border/spawn state
  - Preserve unknown actions safely
- **M4 — Playback**
  - Apply replay actions to canonical state
  - Advance state tick-by-tick
  - Reconstruct state at arbitrary ticks
  - Handle dimension changes
  - Validate playback against recordings
- **M5 — Seeking**
  - Snapshot-based seeking
  - Forward replay
  - Backward seeking
  - State checkpoints
  - Random-access validation
- **M6 — Scene representation**
  - Renderable world representation
  - Entity scene representation
  - Camera representation
  - Lighting representation
  - Asset abstraction
- **M7 — Rendering**
  - GPU renderer
  - Minecraft block models
  - Textures
  - Entities
  - Lighting
  - Transparency
  - Chunk meshing
- **M8 — Desktop editor**
  - Timeline
  - Camera/keyframes
  - Playback controls
  - World navigation
  - Recording inspector
  - Editing tools
  - Export

## Philosophy

Rewind is being developed from the recording bytes outward.

When documentation, source-code assumptions, and actual recordings disagree, real recording data is treated as the final authority.

The project also deliberately avoids pretending that an unknown format is understood.

When a representation is not yet validated, Rewind preserves it rather than silently converting it into an incorrect abstraction.

## License

MIT OR Apache-2.0
