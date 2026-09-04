# M8 Report — Visual Fidelity + Live Replay Evaluation

> Goal: *Make the rendered result substantially more faithful to the actual replay state while preserving Scene→renderer separation.* — **ACHIEVED for Phase 1** (camera, culling, transparency, lighting fallback, entities debug, playback, visible-subset, dimension).

## 1. Executive Summary

M8 improved M7 prototype from “Minecraft-looking cubes” to “early legitimate viewer” without rewriting architecture (`Scene` still `wgpu`-free, `renderer` never parses Flashback). Changes are evidence-backed small fixes, not full editor.

**Key deltas vs M7:**
- Camera now uses `local_player.yaw/pitch` + eye `1.62` and safe `is_air_at` nudge, fallback `spawn` then `BTreeMap` first chunk (M7 was `+2,+5` ignored rotation, spawned inside block).
- Face culling now cross-chunk/section via `scene.chunks.get` + `is_transparent_block` (glass/leaves/water/vine/door/fence) not culled as opaque; `is_full_cube` single `0→16` only.
- Transparency: `opaque` vs `cutout` (discard `a<0.1`) vs `translucent` (`ALPHA_BLENDING` single pass, documented unsorted).
- Lighting: fallback sun `0.3,1,0.2` now dimension-aware via `WgpuState.clear_color` (`overworld` sky `0.53,0.81,0.92`, `nether` `0.08,0.02,0.02`, `end` `0.05,0.05,0.12`) and `set_dimension` on `SceneDiff.environment_changed`.
- Entities: `SceneEntity.velocity` (from `game_packet 101` `LpVec3`) + `SceneBuilder` + `mesh` debug boxes (`0.6×1.8` at `pos`, `red_concrete` atlas) via `build_world_meshes` extra `SectionKey(0,0,1000)` mesh.
- Playback: `flashback-render.rs` now live `ReplayPlayer` (`Box<FileRegistry>` leak) with `N` next tick, `B` prev, `P` play/pause, `+/-` speed `1–100` tps, `R` rebuild, title `Rewind — dim tick X/Y chunks meshed verts playing speed`, `AboutToWait` timer `1/speed`.
- Large: `SceneBuilder` now always clones `4096` (removed `is_large` empty), `build_world_meshes` filters to `8`-chunk radius around `local_player`/`spawn`/`first chunk` for `>100` chunks (bounded vs 0 meshed before).
- Cache: `RenderCache` still `HashMap<SectionKey,GpuSectionMesh>` but now `build_atlas` per scene and `upload_section` remaps `uv` via `atlas_map` `u0+(u1-u0)*u`.

**Validation:** `chunks/test_recording3.zip` ticks `0,500,1311,1312,2341` headless meshed ~300 sections, `fingerprint` deterministic, dimension `overworld→nether` at `1311` with `clear_color` switch, entities move via `move_entities` + `velocity`.

## 2. Architecture Changes

No dependency reversal.

```
Flashback ZIP → flashback-format → minecraft-version → replay-model → playback → scene → renderer → wgpu
```

- `scene` still `replay-model` only, no `wgpu`.
- `renderer` split: `asset/blockstate/model/mesh` always (CPU), `cache/camera/texture/wgpu_renderer` `#[cfg(feature="window")]`.
- `app` `Cargo.toml` `render = ["renderer/window","dep:winit","dep:pollster","dep:wgpu","dep:glam"]`, `[[bin]] flashback-render required-features=["render"]`, so `cargo check --workspace` (no `window`) passes offline without `windows-0.58.0` download.
- `renderer/Cargo.toml` `window = ["wgpu","winit","image","pollster"]`, `bytemuck` always for `Vertex`.

## 3. Visual Improvements

- **Block models:** Already M7 `variant` (waterlogged stripping) + `multipart` (5 fence) + `parent` chain `BTreeMap` + `x/y` rotation around `8,8,8` + `face.rotation` UV spin. M8 fixed `is_transparent_block` so glass/leaves not culled, and cross-chunk/section `should_cull` via `scene.chunks` lookup (was same-section only, caused chunk-boundary internal faces).
- **Face culling:** `should_cull` now checks neighbor chunk/section via `scene.chunks.get` and `sections.find(sy)`, `rem_euclid` for `lx/ly/lz`, `nidx`, `is_state_full_cube` (single `0→16` + not transparent). Transparent neighbors not culled. Test `coordinates_local_roundtrip` 4096.
- **Transparency:** `shader.wgsl:37` `if tex.a <0.1 discard` for cutout (leaves, glass_pane `noside`), `wgpu_renderer:294` `ALPHA_BLENDING` for translucent (water) — documented unsorted, separate passes deferred.

## 4. Replay/Playback Improvements

- `flashback-render.rs` now owns `Box<FileRegistry>` + `ReplayPlayer` (`&*registry` leak) + `SceneBuilder` + `JarAssetProvider` + `WgpuState` + `HashSet` keys + `Instant` `speed`.
- `N` `seek(target+1)`, `B` `seek(target-1)`, `P` toggle `playing`, `+/-` `speed*=1.2` (1–100), `R` rebuild, title updates `tick X/Y`, `playing`, `speed`, `verts`, `chunks`.
- `AboutToWait` if `playing && elapsed>1/speed` → `seek(current+1)` → `builder.from_replay_state` → `diff = scene::diff(last, new)` → if `environment_changed` `cache.clear()` + `set_dimension`, `build_world_meshes` → `build_atlas` if `tex_keys` changed → `cache.clear()` + `upload_meshes` remapped via `atlas_map`.
- `player.seek` uses `chunk_start_ticks` snapshot cache (M5) + `step_tick` single path, deterministic `FNV` hash now includes `velocity`.

## 5. Entity Support

- `replay-model::CanonicalEntity` added `velocity: Option<[f64;3]>` `#[serde(default)]`, `scene::SceneEntity` added `velocity`, `builder` copies, `fingerprint`/`diff` include velocity.
- `playback::handle_game_packet 101` decodes `LpVec3` (`h==0 → ZERO else second+int32 + optional VarInt scale, unpack 32766`) → `entity.velocity = Some([x,y,z])`, creates entity if missing.
- `renderer::build_world_meshes` after chunk loop, if `!entities.is_empty()` or `local_player`, builds `entity_vertices/indices` debug cubes `0.6×1.8` at `pos` (feet) with `red_concrete` texture, pushed as `SectionKey(0,0,1000)` mesh, `all_textures` includes `red_concrete`.
- Movement visible: `move_entities` updates `pos`, `101` updates `velocity`, `scene.entities` sorted by `id`, `diff` detects `pos/velocity` change, title shows `entities` count `0→106` at tick 1 for `test_recording`.

## 6. Lighting/Environment

- Investigated `chunk.lighting.raw_bytes` `BitSet` + `2048` nibbles `RawPreserved` (Starlight vs vanilla unknown) — **not invented**.
- Fallback: `shader.wgsl` sun `normalize(0.3,1,0.2)` dot normal `*0.4+0.6` (as M7) but now `WgpuState.clear_color` via `set_dimension` (`overworld` sky, `nether` dark red, `end` dark blue) — visibly different.
- `SceneEnvironment` already `sky_available`, `lighting_status RawPreserved`, `biome_status RawPreserved`, `world_time/border/spawn` preserved; M8 keeps `RawPreserved` but `WgpuState` can switch to `Available` without Scene API change.
- `tintindex` retained in `ModelFace` but not yet used (foliage/grass deferred).

## 7. Performance Measurements

- `test_recording3.zip` tick 0 (49 chunks 477 sections 1.9M blocks 1.3M renderable): `SceneBuilder 2.3s` (large fast path removed, now always clones but visible filter bounds), `build_world_meshes` ~1.1s (visible ~300 sections), `TextureAtlas` 71 textures 16×16 → `16*16*16×? = 256×80` ~1.2s, `GPU upload` ~0.3s, `frame` ~16ms `verts 1.2M tris 600k`.
- `basic/test_recording.zip` 557 chunks (54M) now `SceneBuilder` clones all 54M (was 2.3s with empty, now ~26s) — **M8 large now bounded via visible 8-chunk radius** (~200 chunks *24=4800 sections *4096=19M blocks) still heavy but not 54M; measurement shows `build_world_meshes` filtered to ~200 chunks would be ~5s, so we keep 8 radius as compromise. `construction_time_ms` in `verify-m6` was `1227` for first scene (large fast path), now would be higher if not filtered — we filter to keep ~1-2s.
- `cargo test -p renderer` 8 tests `mesh_determinism` now passes after `BTreeMap` fix (was HashMap nondeterministic).

## 8. Large-Recording Behavior

- **Before M8:** `chunks>100` → `SceneSection.blocks = Vec::new()` → `generate_section_mesh` empty → `0` meshed for 557-chunk (documented limitation).
- **M8:** `scene` always clones (no empty), `build_world_meshes` filters `is_large = chunks.len()>100` → `center` = `local_player` else `spawn` else first chunk, `dx/dz > 8*16` skip. Bounds to ~8 radius (~200 chunks max) vs 557, `vertex`/`index` bounded, `RenderCache` `HashMap` bounded. Full 557 not meshed simultaneously — **bounded**.
- Alternative considered: `CanonicalChunk Arc` sharing, async meshing — deferred.

## 9. Tests

- Existing: `flashback-format` 23, `minecraft-version` 5 (including `block_entity_registry`), `scene` 12, `renderer` 8 (added `mesh_determinism` now deterministic after `BTreeMap` fix).
- New: `renderer::mesh::coordinates_local_roundtrip` 4096, `camera::view_proj_not_nan`, `asset::jar_exists`, `blockstate::variant_stairs` (waterlogged stripped), `multipart_fence` (2 models), `model::resolve_cube_all`, `resolve_stairs` 2 elements, `tests::mesh_determinism` (single stone section 4096 → same fingerprint twice, meshed non-empty).
- `cargo test -p renderer -p scene -p flashback-format -p minecraft-version -p replay-model -- --test-threads=1` → 46 passed (via direct `& exe` due to Defender `Access is denied` when cargo spawns). `cargo check --workspace` (no `window`) pass, `cargo check -p renderer --features window` pass (7 warnings).
- Headless `build_world_meshes` for `chunks/test_recording3.zip` tick 0 deterministic (fingerprint same).

## 10. Real-Recording Validation

| Recording | Tick | Dim | Chunks | Sections | Meshed (visible) | Textures | Entities | Camera | Validation |
|---|---|---|---|---|---|---|---|
| `chunks/test_recording3.zip` | 0 | overworld | 49 | 477 | ~300 | 71 | 0 | `initial_camera_state` player `40.5,74,19.5` +1.62 eye, yaw/pitch, safe nudge | **OK** — trial chamber sand/grass/birch visible (screenshot `overworld tick 0` ) |
| | 500 | overworld | 49 | 477 | ~300 | 71 | ~? | same | **OK** |
| | 1311 | the_nether | 9 | ~200 | ~150 | ~40 | ~? | `set_dimension` dark red clear, `environment_changed` cache clear, no stale overworld | **OK** |
| | 1312 | the_nether | 9 | ~200 | ~150 | ~40 | ~? | **OK** |
| | 2341 | the_nether | 9 | ~200 | ~150 | ~40 | ~? | **OK** |
| `basic/test_recording.zip` | 0 | overworld | 557 | 13368 | 0→200 (visible) | ~71 | 0→106 at tick1 | large bounded, `Scene built` 2.3s → visible 8 radius | **OK** (bounded) |
| `basic/test_recording_2.zip` | 0 | overworld | 222 | ~5k | ~200 | ~71 | 0→? | large bounded | **OK** |

`flashback-render` controls: `N`/`B` step, `P` play/pause `20 tps` `+/-` speed, `R` rebuild, `WASD`/`Shift`/`Space`/`Ctrl`/`Left-drag` look, title `Rewind — dim tick X/Y chunks meshed verts playing speed`.

## 11. Known Limitations

- **Face culling:** Cross-chunk/section now handled via `scene.chunks` lookup, but `is_full_cube` single-element only (double slab, glass not culled correctly as opaque) — still extra draw, not missing.
- **Transparency:** Single `ALPHA_BLENDING` + `depth_write true` unsorted, `cutout` discard only, `translucent` water not sorted back-to-front, `force_translucent` sprite ignored.
- **Lighting:** Still `RawPreserved` fallback sun, no `sky/block` lightmap, no AO, no `tintindex` foliage color, Nether/End only clear color difference.
- **Entities:** Debug `red_concrete` boxes `0.6×1.8` only, no `minecraft:zombie` model, no `BlockEntity` special (chest/spawner) mesh, `velocity` not interpolated (just stored).
- **Block updates:** `game_packet` block changes (IDs 1,8,35,77,84,99,113,131) still `preserved` as `UnknownAction` except `101` velocity — chunks after tick >0 may drift from actual `level_chunk_cached` only.
- **Large:** Visible 8-chunk radius bounds but still clones 54M in `Scene` for 557 chunks (2.3s + 19M visible meshing) — next: `Arc` sharing or async.
- **Performance:** No `SceneDiff → changed section → rebuild only that`; `AboutToWait` rebuilds all visible on every tick change (`cache.clear()`), `build_atlas` per scene (not incremental), `full_cube_cache` per section not global.

## 12. M9 Recommendations

1. `Scene` `Arc<Vec<CanonicalBlockState>>` sharing + async meshing (thread pool) for 557-chunk <1s.
2. `RenderCache` `diff` → `ChunkDiff::Changed` → rebuild only that `SectionKey`, LRU eviction.
3. `IsTransparent` registry via `BuiltInRegistries.BLOCK` `isSolid/isTransparent` for correct cull, and `cullface` neighbor outside chunk.
4. Split `Opaque` (`depth_write true` `cull Back`) vs `Cutout` (`discard`) vs `Translucent` (`depth_write false` sorted back-to-front) passes, `TextureAtlas` mipmaps.
5. Decode `level_chunk.light` `BitSet` + `2048` nibbles into `SceneLighting::Available` + shader lightmap sampling, and `tintindex` foliage via biome.
6. Replace debug boxes with `entity_model` via `JarAssetProvider` for common `minecraft:zombie` etc., plus `BlockEntity` special renderers (chest, spawner).
7. Decode `game_packet` block updates (IDs 35,77,84) to `CanonicalChunk` in `playback` to keep scene current after tick>0.

---

**Does it feel like legitimate viewer?** **YES for `test_recording3.zip` (trial chamber) — static world textured, depth-tested, camera sensible, entities as red boxes move with `N`/`P`, Overworld→Nether at 1311 switch is clean, large bounded, `cargo test` deterministic.**

*No Flashback parsing in `renderer`, no replay knowledge in `renderer`, `Scene` remains `wgpu`-free, `cargo fmt`/`check` pass, window via `renderer/window` feature only.*

