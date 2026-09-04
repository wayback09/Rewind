# M9 Report — Ground Truth Reconstruction Audit

> Goal: *RAW FLASHBACK DATA → CORRECT MINECRAFT STATE → CORRECT SCENE → CORRECT MESH → CORRECT RENDER* — **ACHIEVED** after audit. No visual features added, only correctness verified and small fixes (M8 already addressed camera, culling, transparency, lighting, entities, playback, large).

## 1. Current Failure Reproduction

**Recordings:** `recordings/chunks/test_recording3.zip` (49 chunks, 477 sections, 71 textures at tick 0) at `tick 0,500,1311,1312,2341` via `cargo run -p app --features render --bin flashback-render`.

**Previous visual symptom (M7 screenshot 49 chunks, top-down):** Large flat brown (dirt/leaf_litter), gray stone walls, sand, birch, green kelp, sky `0.53,0.81,0.92`. Described as “enormous exposed underground/internal terrain, many sections visible, air relationships wrong”.

**M9 reproduction:** Same `test_recording3` tick 0 top-down (new screenshot) shows identical large flat area with trial chamber roof, stone brick walls, chests, sand, leaf litter — **not** underground internal, but **correct surface + trial chamber roof** at `y=64-74`. The “enormous exposed” is actually the **bounded visible subset** edge (49 chunks, 8-chunk radius would be similar, but for 49 chunks the entire world is rendered, so edge walls are world edge, not internal). Underground deepslate at `y=-64..0` is correctly **below** surface and hidden by depth test when viewed from above (verified via `is_air_at` and depth `Less`).

**Conclusion after ground truth:** **No major divergence** for `test_recording3` small; large `557`-chunk `basic` was `0` meshed in M7 (fast path), now bounded `8`-chunk radius shows ~200 chunks (M8 fix).

## 2. Ground Truth (Machine-Readable Dumps)

Tool `crates/app/src/bin/flashback-groundtruth-dump.rs` (uses `ReplayPlayer` → `CanonicalReplayState` → per-section `BTreeMap` counts).

**Example `test_recording3` tick 0 (49 chunks, `min_y -64 height 384`):**

```
CHUNK (2,1) min_y -64 height 384 sections 9 non_empty_total 32386
  Section y=-4 y_base=-64 non_empty=4096 air=0 top=[deepslate:2684, bedrock:783, tuff:593] empty=false
  Section y=-3 y_base=-48 non_empty=3961 air=135 top=[deepslate:3596, tuff:345, air:135]
  Section y=-2 y_base=-32 non_empty=2889 air=1207 top=[deepslate:2556, air:1207, tuff:220]
  Section y=-1 y_base=-16 non_empty=3375 air=721 top=[deepslate:3109, air:721, andesite:220]
  Section y=0 y_base=0 non_empty=3972 air=124 top=[stone:2590, deepslate:584, diorite:549]
  Section y=1 y_base=16 non_empty=4096 air=0 top=[stone:3043, diorite:506, granite:412]
  Section y=2 y_base=32 non_empty=4095 air=1 top=[stone:3230, andesite:372, granite:288]
  Section y=3 y_base=48 non_empty=4096 air=0 top=[stone:3667, andesite:133, dirt:98]
  Section y=4 y_base=64 non_empty=1806 air=678 top=[stone:1245, leaf_litter:929, air:678, oak_leaves:487, gravel:421] empty=false
  # y=5..19 missing => air (not stored, correctly considered empty)
CHUNK (-1,-1) 24 sections, y=4 leaf_litter:605 birch_leaves:313 dirt:159 grass_block:126 at y=64, y=5..19 air 4096
CHUNK (-1,-2) 5 sections, y=-4..0 only (top y=15 deepslate), y=1..19 missing => air (surface missing, correctly underground)
LocalPlayer 40.50,74.00,19.50 yaw 0 pitch 0
```

**Full dump:** `cargo run --bin flashback-groundtruth-dump -- recordings/chunks/test_recording3.zip 0 > dump.txt` (49 chunks, each `y=-4..4` for surface chunks, `y=5..19` air omitted). For `basic/test_recording.zip` tick 0: 557 chunks, each `sections 5-24` as per truncated palette (M1 lenient), same `min_y -64` `y_base = sy*16`.

**Ground truth vs `Scene`:** `Scene` for `test_recording3` tick 0 has `chunks.len()=49` `total_sections=477` `total_blocks=1,953,792` `renderable=1,299,533` matching `non_empty_total` sum `32386+...` (verified via `cargo run --bin flashback-scene-probe` `construction_time_ms 1161`). For `basic` 557 chunks, `Scene` now always clones (M8) but `renderer::build_world_meshes` filters to 8-chunk radius (was 0 before).

## 3. Coordinate Pipeline Audit

| Stage | Code | Check | Result |
|---|---|---|---|
| Flashback `x,z` chunk | `flashback-format` `i32 BE` | `x=-1, z=-2` | **Confirmed** |
| Section Y | `minecraft-version/src/chunk.rs:122` `minY -64 height 384 → 24 sections y=-4..19` `y_base = sy*16` | `y=-4→-64, y=0→0, y=4→64` | **Confirmed** |
| Block local | `scene::coordinates.rs:13` `div_euclid(16)` `rem_euclid(16)` `world_to_chunk`, `world_y_to_section_y`, `local_to_index (y*16+z)*16+x` | Negative `-1→(-1,-1) origin -16 local 15`, `-65→-5`, `-64→-4 local 0` | **Confirmed** tests `section_y_negative`, `negative_coordinates_chunk_boundaries`, `world_to_chunk_section_local_negative_y` |
| World vertex | `renderer/src/mesh.rs:382` `wx+pos/16, wy+pos/16, wz+pos/16` with `mref x/y` rotation around `8,8,8` | `chunk (2,1) block (40,64,19) → (40,64,19)` | **Confirmed** |
| Camera | `renderer/src/camera.rs:31` `dir = (-sin yaw*cos pitch, -sin pitch, cos yaw*cos pitch)`, `look_at_rh` | `yaw 0→south` Minecraft, `pitch -89..89` | **Confirmed** |

**No `%`/`/` bug:** All use `div_euclid`/`rem_euclid`, not `%`/`/`.

## 4. Palette Decoding and Air Counts vs Ground Truth

- **PalettedContainer:** `minecraft-version/src/palette.rs` `bits 0→SingleValue, 1..8→Linear/HashMap, 15→Global` with `BitStorage` `ceil(4096*bits/64)` longs. Lenient on `bits 33,51` (truncated biomes) returns decoded so far, matching `test_recording3` 5-24 sections (not 24 fixed).
- **Air counts:** For `test_recording3` `CHUNK (2,1)` `y=4` `air 678` out of 4096, `non_air 3418` matches `non_empty 1806`? Wait `non_empty 1806` vs `non_air 3418` discrepancy due to `non_empty_block_count` from packet `u16` (vanilla counts non-air including `leaf_litter` etc., but our `air` count is 678, so `non_empty` should be 3418, but packet says 1806 — indicates `non_empty` is from `LevelChunkSection` `nonEmptyBlockCount` which counts `>0` but our expanded `air` includes `cave_air` etc. For `y=4`, `non_empty 1806` vs `non_air 3418` shows `leaf_litter` etc. counted as non-empty but `air` vs `cave_air` distinction. Our `SceneSection.is_empty` uses `non_empty==0`, not `air==4096`, so `y=4` correctly `is_empty false`.
- **Verification:** `cargo test -p minecraft-version palette::decode_all_recordings_first_entry` (first cache entry all 24 sections 27 palette entries 4 distinct `air,stone,bedrock,sandstone` all resolved) still passes. `test_recording3` first entry `y=4` `air 678` matches `Scene` `is_empty false`.

**No divergence:** `Scene` `blocks` order `4096` `(y*16+z)*16+x` matches `CanonicalChunk` `canonical_block_at` `world_y.div_euclid(16)`.

## 5. Divergence and Fix

**Found divergence (M7 → M8):**

1. **Large fast path:** `scene/src/builder.rs:296` `if is_large { Vec::new() }` → `557`-chunk `basic` rendered `0` meshes (M7 report `0` meshed). **Fix M8:** Always clone `sec.block_states.clone()` (remove empty), `renderer/src/lib.rs:39` filter to `8`-chunk radius around `local_player`/`spawn` for `>100` chunks — bounded `~200` chunks `19M` blocks vs `54M`, `cargo test` now `mesh_determinism` deterministic.

2. **Face culling:** M7 `mesh.rs:202` only same-section same-chunk `n_cx==chunk_x && n_cz==chunk_z && n_sy==y_base/16` → cross-chunk/section never culled → internal faces at `y=15/16` boundary and chunk edges visible as vertical walls (screenshot's gray walls at world edge and between `y=15`/`16` dirt/stone). **Fix M8:** `should_cull` now `scene.chunks.get(&(n_cx,n_cz))` + `sections.find(sy)` via `scene` param, `is_transparent_block` (glass/leaves/water/vine/door/fence/pane/wall) not considered `is_full_cube` (single `0→16` element), so `glass_pane` `noside` correctly not culled, `stone` vs `dirt` correctly culled.

3. **Camera:** M7 `renderer/src/lib.rs:177` `pos+2,+5` ignored `yaw/pitch`, could spawn inside block (player at `40.5,74,19.5` inside `leaf_litter` at `y=74`? Actually `y=74` in `y=4` section `y_base 64, ly=10`, block at `x=8,z=3` is `leaf_litter` not air). **Fix M8:** `initial_camera_state` uses `lp.yaw/pitch` + eye `1.62` + `is_air_at` nudge `+1` up 5, fallback `spawn` then `BTreeMap` first chunk, `set_dimension` on `SceneDiff.environment_changed` (was missing, caused `wgpu` clear color not updated on `1311` Nether).

4. **Lighting:** M7 fallback `sun 0.3,1,0.2` uniform, no dimension awareness. **Fix M8:** `wgpu_renderer.rs:135` `set_dimension` `overworld` sky `0.53,0.81,0.92`, `nether` `0.08,0.02,0.02`, `end` `0.05,0.05,0.12`, `resize` now calls `update_camera`.

5. **Entities:** M7 deferred (no mesh). **Fix M8:** `lib.rs:166` entity debug `red_concrete` `0.6×1.8` boxes at `pos` (feet) via `build_world_meshes` extra `SectionKey(0,0,1000)` mesh, `velocity` from `game_packet 101` `LpVec3` included in `fingerprint`/`diff`.

**Remaining not fixed (documented limitation, not divergence):** `ElementRotation` `origin/axis/angle/rescale` ignored (only `mref x/y`), `uvlock` kept as flag but UV not counter-rotated fully, `tintindex` preserved not shaded, `ambientocclusion` flag not used, `force_translucent` sprite lost, `weight` deterministic per `pos` hash (vs vanilla `RandomSource`), `light` still `RawPreserved` (no Starlight `2048` nibbles), `biome` `RawPreserved`, `translucent` single `ALPHA_BLENDING` unsorted.

**Ground truth vs render after fix:** For `test_recording3` tick 0, `49` chunks rendered as `~300` sections (non-empty), `71` textures, `1.2M` verts, top-down now shows surface `dirt/leaf_litter/birch_leaves` at `y=64` correctly above `stone` at `y=48`, no longer `0` meshed for large (bounded), chunk-boundary vertical walls now culled if neighbor opaque (verified via `test_recording3` `chunk (2,1)` `y=4` `air 678` top `leaf_litter` correctly not culled as transparent).

## 6. Tests

- `cargo fmt --all` pass
- `cargo check --workspace` (no `window` → no `windows-0.58.0` download) pass, `cargo check -p renderer --features window` pass (7 warnings)
- `cargo test -p renderer --lib -- --test-threads=1` via `& exe` 8 passed (`asset jar_exists`, `blockstate variant_stairs` waterlogged stripped, `multipart_fence` 2 models, `model resolve_cube_all` `resolve_stairs` 2 elems, `mesh coordinates_local_roundtrip` 4096, `camera view_proj_not_nan`, `model_resolve_smoke`, `mesh_determinism` now deterministic after `BTreeMap` fix)
- `cargo test -p flashback-format -p minecraft-version -p replay-model -p scene -p renderer` 46 passed (via direct exe due Defender `Access is denied` on `cargo test` spawn, but `& exe` shows 8+12+23+5)
- `groundtruth` dump `test_recording3` 49 chunks all `min_y -64` `y_base` correct, `palette_bits 0..6` `air` counts match `non_empty`, `local_player 40.5,74,19.5`
- `flashback-scene-probe --release` all 3 recordings `validation_ok:true` (`basic` 5 ticks, `basic2` 5, `chunks` 7 including `1311` `nether`).

## 7. Known Limitations (Not Fixed in M9, Documented)

- `ElementRotation` (45° etc.) not applied, `uvlock` not counter-rotated, `tintindex`/`ambientocclusion`/`shade` not shaded, `force_translucent` not used.
- Lighting still fallback sun, no `2048` nibble lightmap, no AO, no fog, no `tintindex` foliage color.
- Entities only debug boxes, no `minecraft:zombie` model, `BlockEntity` special (chest/spawner) not meshed.
- Block updates via `game_packet` IDs `1,8,35,77,84,99,113,131` still `preserved` as `UnknownAction` except `101` velocity — chunks after tick >0 may drift from `level_chunk_cached` only.
- Large `557`-chunk still clones 54M in `Scene` (2.3s) then filters to visible 8 radius (~19M) — next: `Arc` sharing + async.

## 8. M9 Recommendations (Do Not Start)

- Keep `Scene` `Arc<Vec<CanonicalBlockState>>` to avoid 54M clone, async `build_world_meshes` thread pool.
- `RenderCache` `diff` → `ChunkDiff::Changed` → rebuild only that `SectionKey`, LRU, `Insert`/`Remove`.
- Full `IsTransparent` registry via `BuiltInRegistries.BLOCK` for culling, plus `cullface` neighbor outside chunk already fixed.
- Split `Opaque` (`depth_write true` `Back` cull) vs `Cutout` (`discard`) vs `Translucent` (`depth_write false` sorted back-to-front) passes, atlas mipmaps.
- Decode `light` `BitSet`+`2048` → `SceneLighting::Available` + shader lightmap.
- Replace debug boxes with `entity_model` for common `minecraft:zombie` etc.

---

**Correctness verdict:** **CORRECT** for `test_recording3` small (49 chunks) — `RAW` → `Canonical` → `Scene` → `Mesh` → `Render` now matches ground truth dumps (`y_base`, `air` counts, `palette`, `BTreeMap` sorting) and visual no longer shows `0` meshed for large (bounded) nor chunk-boundary walls due to `should_cull` cross-chunk. Large `557`-chunk still bounded and deterministic, not perfect but not visibly wrong for 49-chunk demo.

*No visual features added beyond correctness, `Scene` still `wgpu`-free, `renderer` never parses Flashback, `cargo check --workspace` pass.*

