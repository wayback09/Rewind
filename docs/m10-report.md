# M10 report — renderer texture/UV forensics and fix

## 1. What was investigated

Incorrect/missing block textures in the M7/M8 live viewer on
`recordings/chunks/test_recording3.zip` (reference view: tick 2341).
Regression set: grass block (green strip at base), dirt, stone, sand,
oak/spruce/pale-oak leaves, oak log sides/tops, snow layers.
Constraint: renderer-only fix; chunk decoding, palette, Scene coordinates,
and camera math frozen at the M9.9 baseline (`f3f0baf`).

Five layers were audited independently, in pipeline order:

1. BLOCKSTATE→MODEL (`blockstate.rs`)
2. MODEL→TEXTURE (`model.rs`)
3. TEXTURE KEY→ATLAS (`texture.rs`, `lib.rs`)
4. PNG→ATLAS (`texture.rs`)
5. MODEL UV→MESH UV (`mesh.rs`)

Plus two cross-cutting concerns found during the audit: blockstate
rotation handling and `tintindex` plumbing.

Evidence priority followed AGENTS.md: 26.2 JAR JSON (source of truth for
vanilla behavior) > focused mesh tests > constraint derivation. No
Flashback or Minecraft code was copied; only JSON field values were
observed as behavior evidence.

## 2. Layer findings

### Layer 1 — BLOCKSTATE→MODEL: CORRECT (CONFIRMED)

`resolve_blockstate` handles `variants` (weighted pick via deterministic
pos-hash), `multipart` (fences, probed earlier), `x/y/uvlock/weight`
parsing (`blockstate.rs:9-14`, `:214-217`). Observed resolutions
(26.2 JAR + resolution test):

- `grass_block snowy=false` → `grass_block` model, 4 y-rotation variants
- `grass_block snowy=true` → `grass_block_snow` (`cube_bottom_top`:
  side=`grass_block_snow`, top=`grass_block_top`, bottom=`dirt`)
- `oak_log axis=y` → `oak_log` (`cube_column`); `axis=z` → horizontal +
  `x=90`; `axis=x` → horizontal + `x=90,y=90` (probed JSON, §3)
- `snow layers=1..8` → `snow_height2..14`, `snow_block`
- leaves → `leaves` parent (single full-cube element)

No defect here. `uvlock` is parsed but ignored — acceptable: **zero**
`uvlock` occurrences in all nine M10 blockstates (probed from JAR).

### Layer 2 — MODEL→TEXTURE: CORRECT (CONFIRMED)

Parent-chain + `#` reference resolution verified per model. Decisive
probes (26.2 JAR):

- `grass_block.json`: **two** elements — base (bottom=`#dirt`,
  top=`#top`+`tintindex:0`, 4 sides=`#side`) and overlay (4 side
  faces=`#overlay`+`tintindex:0`). → 10 quads/block, 5 tinted.
- `leaves` parent: all six faces carry `tintindex: 0`. Leaves were
  therefore affected by the missing-tint defect (§5), not by key
  resolution.
- `cube_column_horizontal`: `up` face has `rotation: 180`; all other
  M10 model faces have **no** UV rotation (regex probe over 10 models).
- `stone_mirrored` → `cube_mirrored_all`, same `stone` texture
  (U-mirror only; V unaffected).

### Layer 3 — TEXTURE KEY→ATLAS: CORRECT (CONFIRMED)

Distinct keys flow end-to-end (matrix test asserts per-block texture
sets, §6): grass yields 4–5 keys (`grass_block_top/side`,
`grass_block_side_overlay`, `dirt`); logs yield exactly
(`oak_log`, `oak_log_top`); leaves/snow/dirt/stone/sand single keys.
`TextureAtlas::remap_uv` (new, pure, unit-tested) is orientation
preserving; `flashback-render.rs` now calls it instead of inline math
(identical formula, single source of truth).

### Layer 4 — PNG→ATLAS: NO DEFECT FOUND (STRONGLY INFERRED)

Not re-audited tile by tile (M7/M8 prior). The isolated mesh-level
unit test reproduces the exact reported symptom (green strip at base)
with tile-local UVs, pinning the defect to Layer 5, not the atlas.

### Layer 5 — MODEL UV→MESH UV: DEFECT, FIXED (CONFIRMED)

**Root cause #1 — inverted V slot order.** Our quad corners are
vanilla `FaceBakery`'s `[v1, v2, v3, v0]` cyclic rotation, and vanilla
assigns `v0:(minU,minV) v1:(minU,maxV) v2:(maxU,maxV) v3:(maxU,minV)`.
The old code assigned `[(minU,minV),(maxU,minV),(maxU,maxV),(minU,maxV)]`
— V inverted: texture-top rendered at block-bottom on every side face
(grass green strip at base, upside-down dirt/stone/sand/leaf/snow sides).
Fix (`mesh.rs`): slots `[(minU,maxV),(maxU,maxV),(maxU,minV),(minU,minV)]`.
Pre-fix replay (old order restored temporarily): exactly the 3
V-orientation tests fail; all 11 others pass. Post-fix: 14/14 green.

### Rotation handling: DEFECT, FIXED (CONFIRMED by code + math)

Corner positions were already correct (center-based X-then-Y, §3), but
normals and cullfaces were **never rotated**. Consequences for rotated
models (grass/dirt/stone/sand y-variants, logs on axis x/z): shading
used a stale direction, the grass-overlay nudge pushed along the wrong
axis, and `should_cull` tested the wrong neighbor — deleting visible
faces (holes) wherever the checked neighbor was solid and the true one
air. Fix (`mesh.rs`): `rotate_dir_vec` (exact integer steps) applied to
normals in `push_face_quad` and to cullface via `rotated_cullface` at
the `should_cull` call site, same X-then-Y order as positions.

### Tint handling: MISSING FEATURE, ADDED (CONFIRMED)

**Root cause #3.** `tintindex` was parsed but dropped: grass tops,
grass overlays, and all leaf faces rendered untinted. Fix:
`Vertex.tint` flag (1 iff source face has `tintindex`), passed through
the WGSL shader (`location 4` → `location 2`) and multiplied by
`CameraUniform.tint_color` = `FALLBACK_FOLIAGE_TINT` (#91BD59, plains).
Classified TEMPORARY FALLBACK until biome tint data is decoded.

## 3. Vanilla composition rule (STRONGLY INFERRED, constraint-validated)

Order: face-UV rotation first, then blockstate rotation, UVs traveling
with vertices (`uvlock=false`, the default; no M10 blockstate sets it).
Convention `R_x(+90):+Y→+Z`, `R_y(+90):+Z→+X`, X applied before Y, is
fixed by the `oak_log` blockstate: `axis=z` (`x=90`) must move ring
textures ±Y→±Z, and `axis=x` (`x=90,y=90`) ±Y→±Z→±X — verified in the
`rotated_cullface_matches_geometry` test. Rotation *direction* (CW vs
CCW) is unobservable on the M10 set (all affected textures are symmetric
under the ambiguity); exact parity for asymmetric models (stairs are
already fallback cubes) is FUTURE WORK. A tipped bark frame on axis=x
log sides (U-along-Y, traced quad-by-quad to the rotated EAST face) is
vanilla-faithful behavior, not a bug — the matrix test asserts rigid
2×2 UV rectangles there instead of top/bottom orientation.

## 4. Changes

- `crates/renderer/src/mesh.rs` — V slot fix; `rotate_dir_vec`,
  `dir_name_to_vec`/`dir_vec_to_name`, `rotated_cullface`; rotated
  normal (fixes shading + overlay nudge); rotated cull direction;
  4 new tests.
- `crates/renderer/src/lib.rs` — Layer 1–3 resolution assertions kept;
  Layer-5 rewritten as 12-block matrix (grass dry/snowy, dirt, stone,
  sand, log x/y/z, 3 leaves, snow layer 1) meshed in air at pairwise
  non-adjacent cells: strict Vt<Vb per side quad (9 types), UV-rect
  rigidity + rings-on-axis-faces for logs, per-block texture sets,
  tinted-quad counts (grass 5, leaves all, dirt/stone/sand 0).
- `crates/renderer/src/texture.rs` — `TextureAtlas::remap_uv` + 2 tests.
- `crates/renderer/src/camera.rs` + `shader.wgsl` — `tint_color`
  uniform, `tint` vertex attribute, fallback multiply.
- `crates/app/src/bin/flashback-render.rs` — atlas remap via
  `TextureAtlas::remap_uv` (no behavior change).

## 5. Validation

- `cargo fmt --all -- --check` ✓; `cargo check --workspace` ✓;
  `cargo check -p app --features render` ✓ (uniform/vertex buffers are
  struct-sized via bytemuck; no hardcoded 64-byte layout).
- `cargo test -p renderer --lib`: **14/14 pass** (4 pre-existing +
  asset/blockstate/model tests + 7 new M10 tests + determinism/smoke).
- Pre-fix replay: old V order → `side_face_texture_top_at_block_top`,
  `rotation_y_preserves_side_v_orientation`, matrix test fail; rest pass.
- `cargo test --workspace --lib`: all suites pass **except 4 playback
  seek/determinism tests, which balloon past ~10 GB RAM and are OOM-killed
  in this environment**. Causally independent of M10: `playback` has no
  dependency edge to `renderer` (Cargo.toml verified), and the M10 diff
  touches only `renderer` + the render bin. Classified pre-existing /
  environmental; needs a separate investigation (and lighter test
  fixtures). Two probe binaries also hit a transient Windows AV file-lock
  ("Access is denied") that passes on retry — environmental noise.
- Headless limitation (honest substitution): `flashback-render` is a
  live winit viewer — no display here, so no pixel before/after at tick
  2341. Substituted with mesh-level proof (unit + 12-block matrix over
  the recording's block types). Pixel comparison on a display machine
  and WGSL runtime validation at first viewer launch are FOLLOW-UPs.

## 6. Recording evidence

`test_recording3.zip` (10 MB, `recordings/chunks/`) was not pixel-rendered
(headless, §5). Its block palette is covered by the matrix test at mesh
level. M9 ground-truth ticks (0/1311/1312/final) remain the scene-level
reference; the UV defect is tick-independent (every side face, every tick).

## 7. Data structures

- `mesh::Vertex` += `tint: u32` (40 bytes; `desc()` extended at
  `location 4`).
- `camera::CameraUniform` += `tint_color: [f32; 4]` (80 bytes; matches
  WGSL `Camera` struct layout).
- `TextureAtlas::remap_uv(uv, rect)` — tile-local → atlas UV.
- `BlockModelRef{x, y, uvlock, …}` — `uvlock` parsed, intentionally
  unused (none in M10 set).

## 8. Dependencies / version dependencies

No new crates. Evidence: 26.2 client JAR blockstates/models (behavioral
reference only). Data/protocol versions uninvolved in M10.

## 9. Implications for a standalone implementation

Portable rules, no Flashback/Minecraft code reproduced: (a) FaceBakery
slot assignment `v0:(minU,minV)…` with `[v1,v2,v3,v0]` corner order;
(b) bake order face-rotation → blockstate rotation, X-then-Y, UVs travel
with vertices under `uvlock=false`; (c) normals/cullfaces must follow
the same rotation or culling deletes visible faces; (d) `tintindex`
faces need a biome/fallback multiply, and grass needs its second
(overlay) element rendered slightly proud of the base (`0.002` nudge is
our TEMPORARY FALLBACK for missing translucent sorting).

## 10. Classifications

- TEMPORARY FALLBACK: `FALLBACK_FOLIAGE_TINT` (needs biome tint decode);
  overlay depth nudge (needs translucent sorting); `uvlock:true` ignored
  (no M10 user; revisit with rails/doors/walls).
- UNSUPPORTED: none added.
- FUTURE WORK: stair shapes (pre-existing); WGSL runtime check at viewer
  launch; pixel before/after on a display machine; vanilla rotation-direction
  parity for asymmetric models; playback seek-test OOM.

## 11. Unknowns and questions for other agents

1. Playback seek tests OOM (>10 GB) — is that expected fixture weight or
   a snapshot-cloning leak? (Independent of renderer; needs an owner.)
2. Exact vanilla Euler direction for asymmetric-model parity — can anyone
   with a display compare axis=x log bark-stripe direction against 26.2?
3. Should the fallback foliage tint vary by dimension/biome sample, or
   stay plains-constant until the tint pipeline lands?
4. Methodology note: an early matrix-test failure (`y≈1 ⇒ block-top`)
   was a test bug — the vertex was the *bottom* of the block above.
   Block-relative (per-quad) assertions replaced absolute-Y filters.
