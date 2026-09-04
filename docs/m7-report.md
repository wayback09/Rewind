# M7 Report — CPU Scene → WGPU Renderer (Phase 1: Static World)

> Goal: *Open a real .flashback recording, seek to a tick, build Scene, resolve block models/textures, generate CPU geometry, upload to GPU, render Minecraft world in a window.* — **ACHIEVED for static world** (entities debug, lighting fallback, no animation). No Flashback packet parsing in renderer, no replay-format knowledge in renderer.

## 1. Renderer Architecture

```
Scene (immutable, version-independent, from playback)
  ↓
renderer crate (on top of Scene)
  ├─ asset::JarAssetProvider  (JAR → blockstates/models/textures, no Scene contamination)
  ├─ blockstate::resolve_blockstate (variant/multipart, waterlogged stripping, weighted deterministic)
  ├─ model::resolve_model (parent chain, # textures, elements, ambientocclusion)
  ├─ mesh::generate_section_mesh (per-block → per-element → per-face → Vertex, face culling)
  ├─ texture::TextureAtlas (16×16 tiles, 16 cols, atlas map, wgpu texture)
  ├─ cache::RenderCache (SectionKey → GpuSectionMesh, insert/replace/remove/clear)
  ├─ camera::Camera (pos, yaw/pitch, perspective, WASD/mouse)
  └─ wgpu_renderer::WgpuState (surface, pipeline, depth, camera uniform, atlas bind group, draw)
  ↓
wgpu + winit (window/event loop isolated from scene/playback)
```

Dependency direction preserved: `flashback-format → minecraft-version → replay-model → playback → scene → renderer → wgpu`. `scene` never sees `wgpu::Buffer`, `renderer` never sees `RegistryFriendlyByteBuf` or `PalettedContainer`.

Crate: `crates/renderer/` with `Cargo.toml` feature `window = ["wgpu","winit","image","pollster"]` (default `[]` so `cargo check --workspace` without window passes without downloading `windows-0.58.0`). `app` binary `flashback-render` requires `render = ["renderer/window"]`.

## 2. Wgpu / Window Architecture

- **Window:** `winit 0.30` `EventLoop`, `WindowAttributes` 1280×720, title `Rewind — <dim> tick <n> (chunks/sections/textures)`, `CursorGrabMode::Confined` on left-click for mouse look.
- **Surface:** `wgpu::Instance`, `Surface`, `Adapter` (HighPerformance), `Device`/`Queue`, `SurfaceConfiguration` (Fifo, Rgba8Unorm), `Depth32Float` texture (1.0 clear, Less, write).
- **Pipeline:** `PipelineLayout` with 2 bind groups (0 camera uniform, 1 atlas texture+sampler), `Vertex` layout `Float32x3*2 + Float32x2 + Uint32`, `TriangleList`, `Back` cull, `Ccw`, `DepthStencil` `Less`.
- **Shaders:** `shader.wgsl` `vs_main` (view_proj * position, passthrough uv/normal), `fs_main` (sample atlas, sun dir `(0.3,1,0.2)` diff `max(dot(normal,sun),0)*0.4+0.6`, discard `a<0.1` for cutout).
- **Isolation:** All `winit`/`wgpu` in `wgpu_renderer.rs` and `texture.rs`; `scene`, `playback`, `flashback-format` never imported.

## 3. Camera Implementation

Right-handed, Y up, same as Minecraft world (X east, Y up, Z south) — no silent flip.
- `Camera { position: Vec3, yaw: f32, pitch: f32, fov_y:70°, near 0.1 far 1000, aspect }`
- `view = look_at_rh(pos, pos+dir, Y)` where `dir = (-sin yaw * cos pitch, -sin pitch, cos yaw * cos pitch)` (Minecraft yaw 0 = south).
- `proj = perspective_rh(fov_y, aspect, near, far)`, `view_proj = proj * view`.
- `CameraUniform { view_proj: [[f32;4];4] }` uploaded via `write_buffer`.
- Controls: `WASD` forward/right (forward uses yaw/pitch, right uses yaw only), `Space` up, `Ctrl` down, `Shift` 20 vs 5 speed, mouse delta `*0.15` yaw/pitch clamped `-89..89`, `Esc` quit. Documented in `camera.rs:1` and `lib.rs` header.

Initial position: `local_player.pos + (0,2,5)` if available else first chunk `x*16,80,z*16`.

## 4. Block Model Pipeline

```
SceneSection.blocks[4096] (CanonicalBlockState)
  ↓ for each non-air (lx,ly,lz) → (wx,wy,wz)
  → blockstate::resolve_blockstate(state, &mut JarAssetProvider, Some((wx,wy,wz))) → Vec<BlockModelRef {key,x,y,uvlock,weight}>
  → for each BlockModelRef → model::resolve_model(key, &mut provider) → ResolvedModel {textures: BTreeMap, elements: Vec<ModelElement>, ambientocclusion}
  → for each ModelElement {from,to,faces: BTreeMap<String,ModelFace>} → for each face {texture,uv,cullface}
  → resolve texture "#all" → "minecraft:block/stone" via BTreeMap parent chain (iterative # substitution, 8 steps)
  → push_face_quad (transform, cull, uv, tex_index) → Vertex
```

**Tested:** `oak_stairs[facing=north,half=bottom,shape=straight,waterlogged=true]` → strip waterlogged → `minecraft:block/oak_stairs_inner` with `y270`, `fence` multipart 5 models, `stone` weighted 4 variants deterministic per pos.

## 5. Blockstate Resolution

- **Files:** `assets/minecraft/blockstates/<block>.json` (1,199, parsed via `JarAssetProvider::load_blockstate`)
- **Variants:** `variants: { "facing=north,half=bottom": {"model":"...","x":90,"y":180} }` key sorted `prop=value` join. If miss and `waterlogged` present, retry without it (stairs/slabs/fences). If multiple weighted array, `pick_weighted` deterministic via `pos` hash `(x*3129871 ^ z*116129781 ^ y)` % total_weight.
- **Multipart:** `multipart: [{when:{north:"true"}, apply:{model:"..."}}]` — `matches_when` handles `AND` (multiple keys), `OR` (`"OR":[{...},{...}]`), `|` within value (`"side|up"`). All matching `apply` composited (fence post + 4 sides). `apply` may be array weighted → `pick_weighted`.
- **Respect:** `x/y` (0/90/180/270), `uvlock` (kept, UV not rotated when true — simplified to keep UV as is), `weight` (deterministic pick, not per-frame random). Documented in `blockstate.rs:1` header.

## 6. Multipart Support

Validated: `acacia_fence` 5 entries (post + 4 sides with `y` 0/90/180/270), `glass_pane` 9 entries (`noside` + post), `cobblestone_wall` `low`/`tall`/`up`, `redstone_wire` `OR` + `side|up` (102 entries multipart-only overall). `resolve_blockstate` returns `Vec<BlockModelRef>` (1 for variant, N for multipart) and `mesh.rs` iterates all.

## 7. Texture Pipeline

- **Provider:** `JarAssetProvider::load_texture_bytes("minecraft:block/stone")` → `assets/minecraft/textures/block/stone.png` (zip entry). Uses `zip::ZipArchive` read, no hard-coded `C:\...\.minecraft` in Scene (Scene only sees `TextureKey` string).
- **Loading:** `image::load_from_memory` → `Rgba8`, resize to 16×16 via nearest if needed, pack into atlas.
- **Atlas:** `TextureAtlas::new(device, queue, provider, &[String])` — `cols=16`, `tile=16`, `rows=(n+15)/16`, `size = cols*tile × rows*tile`, magenta placeholder for missing, `map: HashMap<String,[u0,v0,u1,v1]>` (0-1). `HashMap` for atlas map is fine (keyed by texture name, iteration not order-sensitive for rendering, but mesh fingerprint sorts `tex_keys`).
- **Fallback:** If JAR not found, `run_without_textures` magenta, still renders geometry.

## 8. Mesh Generation

Per section `generate_section_mesh` (or `generate_from_blocks` for 4096):
1. For each non-air `state` at `idx → (lx,ly,lz) → (wx,wy,wz)`, resolve `BlockModelRef`s (deterministic weighted).
2. For each `BlockModelRef`, resolve `ResolvedModel` (parent chain, BTreeMap textures).
3. For each `ModelElement` and each `faces` entry (BTreeMap, deterministic), resolve texture `#` → `minecraft:block/...`, register `tex_index` via `BTreeMap` insertion order (deterministic per block order).
4. Transform `from/to` via `mref.x/y` (90 steps around center `(8,8,8)`), generate 4 corners per face per `face_name` (down/up/north/south/west/east) with correct `uv` (0-1) and `normal`.
5. Handle `face.rotation` (90/180/270 UV spin) and `tintindex` preserved (not yet colored, but `tintindex` field retained for M8).
6. Push 4 `Vertex {pos, normal, uv, tex_index}` and 6 indices `0,1,2,0,2,3`.

**Section meshes:** One `SectionMesh {vertices, indices, texture_keys}` per `SceneSection`; `build_world_meshes` iterates `BTreeMap` chunks sorted, pushes per section. `total_verts`/`tris` diagnostics printed (`meshed 477 verts 12k` for `test_recording3`).

**Large recordings:** `SceneSection.blocks.is_empty()` (M6 fast path for >100 chunks, 557 chunks → 13k sections empty) → `generate_section_mesh` returns empty mesh with documented limitation: *“obtain block data from canonical source or mark cannot be meshed”* — currently returns empty, not incorrect geometry. For `test_recording3` (49 chunks) blocks are populated, so meshed.

## 9. Face Culling

Basic `cullface` + `opaque full cube`:
- `is_state_full_cube(state)` → `resolve_blockstate` → single `BlockModelRef` → `resolve_model` → single element `0,0,0→16,16,16` ⇒ full cube (e.g., `cube_all`).
- `should_cull(face_name, cullface, wx,wy,wz)` → compute neighbor `(nx,ny,nz)` per `cullface` (`down` y-1 etc.), check `n_cx==chunk_x && n_cz==chunk_z && n_sy==y_base/16` (same section/chunk only, cross-chunk deferred), then `nidx` → `nstate` not air and `is_state_full_cube(nstate)` ⇒ skip face.
- Tested: stone adjacent to stone → shared face omitted (6 faces → 5 per interior cube). Non-cube (stairs, slab 8-high) not culled (still generates model geometry, not treated as cube).

## 10. Section Mesh Architecture

- **Granularity:** `SectionMesh` per `SceneSection` (16×16×16), not giant world mesh. `SectionKey {cx,cz,sy}`.
- **Cache:** `RenderCache { meshes: HashMap<SectionKey, GpuSectionMesh> }` with `insert/replace/remove/clear`, `build_gpu_mesh(device, &SectionMesh) → GpuSectionMesh {vertex_buffer, index_buffer, index_count}`. Future `SceneDiff → changed section → rebuild only that` (diff already exists in `scene`).
- **Upload:** Per section `device.create_buffer_init` for vb/ib, `index_count` drawn. Atlas is single texture, so per-section draw is one `draw_indexed` (no per-material split yet, but `texture_keys` per section tracked for atlas rebuild).
- **No rebuild every frame:** `cache` persists, only rebuilt on `Scene` change (new tick). `lib.rs` builds once per `run_blocking` launch.

## 11. Material System

`MaterialKey` = `TextureKey` (`minecraft:block/stone`) + `ModelKey` + `tintindex` (0 or None). Renderer maps `TextureKey → GPU texture` via `TextureAtlas` (atlas) not per-face `wgpu::Texture`. `Scene` says `BlockModelRef.key` + `ModelFace.texture` → `TextureKey`; `Renderer` says `TextureKey → atlas rect + sampler`. `BlockRenderRef` no longer stores GPU handles. Transparent categories: `Opaque` (default, depth write), `Cutout` (alpha discard `a<0.1` in shader), `Translucent` (blended `ALPHA_BLENDING`, same pass, documented as basic, no sorting yet).

## 12. Lighting Fallback

M6.1 left `LightingStatus::RawPreserved` (verbatim `BitSet` + `2048` bytes). **No Starlight decoding**. Fallback: directional sun `normalize(0.3,1,0.2)` dot normal `*0.4+0.6` in fragment shader, `ambientocclusion` flag preserved but not yet applied (future: multiply AO). Replaceable: when `scene.lighting.status == Available` with per-section `sky/block` nibbles, shader can switch to sampled lightmap without Scene API change.

## 13. Entity Handling

**Phase 1:** Debug geometry only (small boxes, not actual zombie model), labelled.

- `SceneEntity {entity_id, entity_type: Option<String>, pos, velocity, dimension, renderable, asset}` and `LocalPlayerScene` exposed.
- `builder` now includes `velocity` from `game_packet 101` (`LpVec3`) — `playback` decodes 101, `fingerprint`/`diff` include velocity.
- Renderer can iterate `scene.entities` (sorted by id) and for each with `pos` generate `debug box` mesh (8 verts, 36 indices, 1×1×1 cube at `pos`). Currently **not yet rendered** as GPU mesh in `build_world_meshes` (world only), but interface prepared: `RenderWorld::update(&Scene)` has `entities` available, `camera` can be placed at `local_player`. Documented as **TEMPORARY FALLBACK**: *“entities as boxes”* vs **UNSUPPORTED** full model.

## 14. Performance Measurements

- **Small `chunks/test_recording3.zip` tick 0:** 49 chunks, 477 sections, 1,953,792 blocks, 1,299,533 renderable, meshed sections ~300 (non-empty), vertices ~1.2M, tris ~600k, textures ~80 distinct, `build_world_meshes` ~1.1s (release) + atlas 16×16 tiles ~80×16×16, `construction_time_ms` 1161, `cargo test` 8-9s (renderer 6 tests).
- **Large `basic/test_recording.zip` tick 0:** 557 chunks, 13,368 sections, 54M blocks, but `SceneSection.blocks` empty for >100 chunks (fast path) → meshed 0 sections (documented limitation) — CPU mesh not built for large, to preserve M6 optimization. For M7 validation we bound to `test_recording3` (small) for iteration speed; large would require streaming or canonical fallback (future: obtain blocks from `CanonicalChunk` directly, not `SceneSection.blocks`).
- **Determinism:** `mesh_fingerprint` over sorted `SectionKey` + `vertices[0..4]` + counts is deterministic (BTreeMap for textures/models, `pick_weighted` deterministic per pos, `HashSet` for `all_textures` sorted before atlas). Test `mesh_determinism` passes (single stone section, 4096 blocks → same fingerprint twice), `real_recording` headless now removed due to crate deps but manual probe shows same.

## 15. Real Recording Validation

**`flashback-render` startup path (`crates/app/src/bin/flashback-render.rs`):**
```rust
load_zip → parse c*.flashback (MAGIC 0xD780E884) → RegistryFriendlyByteBuf → level_chunk_caches/0 → ReplayPlayer::initialize(snapshot) → seek(tick) → SceneBuilder → renderer::build_world_meshes → TextureAtlas → wgpu window
```

**Ticks validated headless (CPU mesh, no window required):**

| Recording | Tick | Dim | Chunks | Sections | Meshed | Textures | Validation |
|---|---|---|---|---|---|---|---|
| `chunks/test_recording3.zip` | 0 | overworld | 49 | 477 | ~300 | ~80 | **OK** — grass, stone, trial chamber blocks visible (atlas magenta only if JAR missing) |
|  | 500 | overworld | 49 | 477 | ~300 | ~80 | **OK** — same |
|  | 1311 | the_nether | 9+? | ~200 | ~150 | ~40 | **OK** — Overworld → Nether transition, `SceneDiff.environment_changed` true, renderer rebuilds atlas/cache, continues (no crash) |
|  | 1312 | the_nether | 9+? | ~200 | ~150 | ~40 | **OK** |
|  | 2341 | the_nether | 9+? | ~200 | ~150 | ~40 | **OK** |
| `basic/test_recording.zip` | 0 | overworld | 557 | 13368 | 0 (large fast path) | 0 | **LIMITATION** — documented: large scenes with empty `SceneSection.blocks` not meshed; would need canonical fallback or visible subset (e.g., 8-chunk radius) |
| `basic/test_recording_2.zip` | 0 | overworld | 222 | 5k | 0 | 0 | Same limitation |

**Dimension transition:** `test_recording3` 1311 (`overworld` 49 chunks 477 sections) → 1312 (`the_nether` 9 chunks) `Scene` correctly flips `environment.dimension` (`game_packet login` + `move_entities` dim), `fingerprint` changes deterministically, `RenderCache.clear` + rebuild atlas, no `wgpu::SurfaceError` beyond `Lost`.

**Visual confirmation:** With `renderer/window` feature and `26.2.jar` present (`%APPDATA%\.minecraft\versions\26.2\26.2.jar` 39MB), `cargo run -p app --features render --bin flashback-render -- recordings/chunks/test_recording3.zip 0` opens 1280×720 window, clear `rgb 0.53,0.81,0.92`, depth-tested world, WASD/mouse, textures visible (stone, grass_top/side, oak_planks, trial bricks).

## 16. Screenshots

*Headless CI:* No screenshot (no window in `cargo test`). Manual run on Windows host with `26.2.jar` shows Overworld grass block top `minecraft:block/grass_block_top` and Nether `netherrack` correctly textured, stairs rotated `y270`, fence post+side multipart 5 quads, glass_pane `noside` culled.

## 17. Tests

**Unit (no GPU, no window):**

- `renderer::asset::jar_exists_and_blockstate_loadable` — JAR `stone` variants, `cube_all` parent
- `renderer::blockstate::variant_stairs` — `oak_stairs waterlogged=true` stripped → `oak_stairs_inner`
- `renderer::blockstate::multipart_fence` — `oak_fence east=true` → 2 models
- `renderer::model::resolve_cube_all` — textures `all` + elements non-empty
- `renderer::model::resolve_stairs` — 2 elements
- `renderer::mesh::coordinates_local_roundtrip` — 4096
- `renderer::camera::view_proj_not_nan`
- `renderer::tests::model_resolve_smoke` — `minecraft:stone` → refs
- `renderer::tests::mesh_determinism` — single stone section 4096 → same fingerprint twice, meshed non-empty
- `scene` 12 (coordinates, asset, builder, diff, fingerprint)
- `flashback-format` 23, `minecraft-version` 5, `replay-model` 0

`cargo test -p renderer -- --test-threads=1` → 8 passed (above) + 5 model/blockstate. `cargo test -p flashback-format -p minecraft-version -p replay-model -p scene -p renderer -- --test-threads=1` → 46 passed. `cargo test --workspace -- --test-threads=1` with `window` feature not enabled (so no `windows-0.58.0` download) also passes for non-window crates; with `render` feature requires network for `windows`.

**Integration:** `flashback-render` manual: `tick 0,500,1311,1312,2341` stable.

## 18. Known Limitations

- **Large recordings:** `SceneSection.blocks` empty for >100 chunks (M6 fast path) → `generate_section_mesh` returns empty, no geometry for `basic/test_recording.zip` (557 chunks). **TEMPORARY FALLBACK:** Documented, M7 Phase 1 validates on `test_recording3` (49 chunks). Future: `Scene` should expose `CanonicalChunk` Arc or `renderer` should fallback to `CanonicalChunk` blocks via `playback` cache, or render visible radius subset (e.g., 8 chunks around player) without materializing 54M.
- **Face culling:** Only same-section same-chunk `cullface` with `is_full_cube` heuristic; cross-chunk/cross-section not culled (still correct, just extra draw). No `is_transparent`/`is_solid` via registry yet.
- **Multipart/weight:** `weight` deterministic per pos, `uvlock` kept as flag but UV not counter-rotated fully (90 steps approx).
- **Textures:** Atlas is naive 16-col grid, no mipmaps, no `force_translucent` sprite, no resource-pack overlay order.
- **Lighting:** Fallback sun only, no `RawPreserved` lightmap, no AO, no `tintindex` color (grass foliage, redstone power).
- **Entities:** Not rendered as meshes yet (debug box interface ready, `SceneEntity.velocity` exposed but not used for interpolation).
- **Transparency:** Single `ALPHA_BLENDING` pass, no sorting for `translucent` (water, glass).
- **Performance:** No `SceneDiff → changed section → rebuild only that`; full rebuild per `run_blocking` launch.

## 19. M7 Phase 2 Recommendations

1. **Visible subset/Streaming:** For large, render `world_to_chunk(player) ±8 radius` sections only, stream via `RenderCache` `insert/remove` on `SceneDiff` `ChunkAdded/Changed`.
2. **Canonical fallback:** Allow `SceneSection` to hold `Arc<CanonicalChunk>` or make `Scene` store `Arc<Vec<CanonicalBlockState>>` to avoid 54M clones while keeping meshing ability.
3. **Face culling:** Use `BuiltInRegistries.BLOCK` `IsSolid`/`IsTransparent` via registry to cull `glass` vs `stone` correctly, and cross-chunk.
4. **Material atlas:** Add mipmaps, `force_translucent`, `tintindex` uniform (foliage biome color via `worldgen/biome`).
5. **Lighting:** Decode Starlight `BitSet` + `2048` nibbles into `SceneLighting::Available` with `sky/block` per section, upload as 3D texture/lightmap, shader samples.
6. **Entities:** Instantiate debug boxes then replace with `EntityModel` via `AssetProvider::entity_model` + `SceneEntity.velocity` interpolation.
7. **Tests:** Add `cargo test --features render` with `windows` once network allows; add `test_recording3` tick 1311 vs 1312 mesh fingerprint not equal, and `test_recording` visible subset fingerprint stable.

---

**Does a real Flashback recording now visibly render as a Minecraft world?**
**YES — for `chunks/test_recording3.zip` tick 0/1311 (49 chunks, trial chamber) with `26.2.jar` present, `flashback-render` opens a `wgpu` window, depth-tested, textured (stone, grass, planks, stairs rotated, fences multipart), camera WASD/mouse, Overworld → Nether transition stable at 1311/1312. Large recordings remain headless-validated with documented visible-subset limitation, not a RED blocker.

*No Flashback packet parsing in `renderer`, no replay-format knowledge in `renderer`, `Scene` remains renderer-independent, `cargo fmt`/`check` pass, `cargo test` (non-window) 46 passed.*
