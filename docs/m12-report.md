# M12 report — Minecraft lighting reconstruction

## 1. Previous lighting behavior

The renderer shaded every face with a single directional-sun fallback
(`shader.wgsl`: `diff = max(dot(n,sun),0)*0.4+0.6`, sun from +Y).
Underground scenes rendered with the same brightness logic as open sky;
no block/sky distinction existed anywhere in the pipeline.

Data-wise, lighting was preserved but unusable: `decode_canonical_chunk`
stored the whole `lightData` tail as one raw blob
(`minecraft-version/src/chunk.rs`), per-section nibble fields stayed
`None`, `Scene` kept only a byte-length plus presence booleans, and the
renderer received nothing.

## 2. Root causes / limitations found

1. **Undecoded light tail (CONFIRMED).** The blob layout was never
   validated or split; `LightingData.per_section` was always empty and
   `CanonicalSection.{sky,block}_light` always `None`.
2. **Scene dropped the bytes (CONFIRMED).** `SceneSection` had no light
   fields; `SceneSectionLight` flags were derived from the always-empty
   canonical split (hence always false).
3. **No vertex/shader path (CONFIRMED).** `Vertex` had no light channel;
   the shader knew only texture × tint × sun.
4. **Misaligned-blob hazard (CONFIRMED).** The block-entity loop breaks
   early on unparseable NBT, so `light_raw` for 2/58 recording chunks
   starts mid-NBT (`0A 0A 00 09 "SpawnDat..."`). Any decoder must
   validate strictly and degrade gracefully instead of trusting the offset.

## 3. Recording evidence

`recordings/chunks/test_recording3.zip` (c0 overworld + c1 nether):

- Vanilla lightData layout parses with **exact byte consumption on 56/58
  chunks**; array counts equal mask popcounts in every case.
- Nether chunks carry **zero sky arrays, 5–8 block arrays** — the same
  parser yields dimension-correct results with no special-casing.
- Surface columns in chunk (-1,-2): sky **15** above solid, **14** at the
  water surface, **0** twelve blocks down.
- Torch cells (`minecraft:torch`, 4 found in c0) read block light
  **exactly 14**.
- The 2 rejecting chunks both show truncated-BE evidence
  (`be_count>0`, `0x0A` head) — strict rejection, not garbage decode.

Headless mesh evidence (temporary probe, since removed):

| tick | dim | chunks | meshes | verts | sky=15 | dark (0,0) | missing |
|---|---|---|---|---|---|---|---|
| 0 | overworld | 49 | 439 | 1,600,816 | 248,209 | 683,495 | 220,063 (99.99% world-edge fringe) |
| 2341 | nether | 201 | 1369 | 2,897,276 | 0 | 1,286,757 | 244,512 (98.9% edge) |

- Overworld sky histogram is bimodal (767k at 0, 248k at 15) with a
  smooth 7→14 transition ramp — the Minecraft day/surface/cave split.
- Block histogram decays 1→14 with spikes at 14/15 (torches/lava);
  nether block histogram spikes at 15 (471k verts: lava lakes).
- Decoded arrays in-scene: 147 sky + 225 block sections (744 KB) at
  tick 0; 1270 block sections (2.6 MB) at tick 2341.

No screenshots: pixel validation needs a display (viewer launch by the
operator). Mesh-level distributions above are the headless substitute.

## 4. Lighting representation discovered

Vanilla 26.2 `lightData` tail (CONFIRMED, §3):

```text
BitSet skyYMask, BitSet blockYMask, BitSet emptySkyYMask, BitSet emptyBlockYMask,
VarInt skyCount, skyCount x (VarInt(2048) + 2048 bytes),
VarInt blockCount, blockCount x (VarInt(2048) + 2048 bytes)
```

- BitSet = VarInt(n_longs) + n_longs × BE i64, **LSB-first** bit numbering.
- Light bit `i` ↔ `section_y = min_y/16 - 1 + i` (base −5 for minY −64;
  26 light sections). Array `k` ↔ k-th set bit (rank order).
- Cell `idx = (ly*16+lz)*16+lx` — **same convention as blocks** — even
  idx = **LOW nibble first**.
- A set bit without an array, or an unset bit, carries no data (see §6).

Canonical form: existing `CanonicalSection.{sky,block}_light`
(`Option<Vec<u8>>`, 2048 bytes) plus filled `LightingData.per_section`
and status `"decoded"`. Renderer form: `Vertex.light: u32`
(sky | block<<8, `0xFFFFFFFF` = missing); arrays are sliced, never copied
per pass.

## 5. Byte/nibble ordering evidence

See §3–§4. Decisive checks: exact-consumption parse on 56 chunks;
popcount agreement; uniform `0xFF` top-sky arrays; water-surface 14;
torch 14; nether zero-sky. A wrong nibble order or section mapping would
produce checkerboard/garbage values instead of clean 15/14/0 columns.

## 6. Coordinate mapping

One convention everywhere: `idx=(y*16+z)*16+x`, `section_y=-4+idx`,
`y_base=section_y*16`, `div_euclid`/`rem_euclid` for negative Y
(M9.9 baseline, untouched). Light bit → section_y via base −5 (+1 for
the below-minY light section, which has no block data and is dropped).
Sampling crosses chunk/section borders with the same helpers as culling.
Negative-Y paths are covered by test (`light_chunk_boundary_and_negative_y`).

## 7. Fallback behavior (all documented, none silent)

| Situation | Result |
|---|---|
| Array present (incl. all-zero) | Decoded nibble (trusted, even when 0) |
| Array absent, skylight dim, cell above column top | Sky 15 (open sky by construction) |
| Array absent, otherwise | Sky 0; block always 0 |
| Nether/End (no skylight) | Sky 0 everywhere, incl. above terrain |
| Below chunk minY | Dark void (0,0) |
| Unknown chunk / section absent in range | `LIGHT_MISSING` → legacy sun in shader |
| Blob fails strict parse (2 known chunks) | Sections stay `None` → geometric rule above |
| Entities (debug boxes) | `LIGHT_MISSING` → unchanged sun look |

No global ambient value is fabricated; every non-decoded value derives
from geometry (column tops) or dimension. Torch/object motion during a
recording does **not** update light (packets carry static snapshots;
vanilla light-update packets are preserved-unknown) — documented limit.

## 8. Shader changes

`texture × tint × light` replaces `texture × tint × sun`, tint untouched:

- New `light` vertex attribute (`location 5`) → fragment (`location 3`).
- `light_ramp`: TEMPORARY FALLBACK smooth curve `((L+1)/16)^1.5`
  approximating vanilla LightTexture tables (0→near-black, 15→full).
- `face_shade`: vanilla per-face diffuse (up 1.0, down 0.5, z 0.8, x 0.6).
- `brightness = max(sky, block) × shade` (floor 0.015) plus a subtle warm
  shift where block light dominates (daylight/torch split).
- `0xFFFFFFFF` keeps the **exact legacy sun formula** (entities, fringe).
- UVs, culling, rotation, tint, atlas mapping untouched (M10 suite green).

Bind-group visibility was already `VERTEX_FRAGMENT` (M10 follow-up); the
new attribute needed no layout change beyond `Vertex::desc` (struct-sized).

## 9. Performance impact

- Decode: one strict pass per chunk packet (µs–ms; only at snapshot/decode).
- Scene: +2 `Option<Vec<u8>>` per section with data — measured 744 KB
  (49-chunk scene) / 2.6 MB (201-chunk scene). Snapshot clones carry them
  like block data (<2% overhead by construction).
- Mesh build: one linear column-top pass per chunk + one hashmap section
  lookup per vertex. Measured full-scene mesh times (228s tick 0 / 553s
  tick 2341) are dominated by the pre-existing per-block model resolution
  (unchanged path); no isolated A/B was run because the added work is
  linear and small next to it. No per-frame recomputation (mesh-time only).
- Vertex: +4 bytes (40 → 44).

## 10. Tests added

- `minecraft-version/src/light.rs`: nibble order (incl. last-byte edge),
  known values 0/1/5/10/15 for sky+block, strict rejection (trailing
  bytes, count/popcount mismatch, truncation, NBT garbage, empty).
- `minecraft-version/tests/light_recording.rs`: exact-layout parse on all
  recording chunks; nether zero-sky; surface-column 15/14/0; torch ≥ 12.
- `renderer/src/lib.rs`: array sampling per vertex; geometric rule with
  roof-shadow/pillar matrix; chunk-boundary + negative-Y + below-minY
  darkness; nether sky-0 rule; byte-identical determinism incl. light.
- `renderer/src/mesh.rs`: no-scene unit quads carry `LIGHT_MISSING`.

## 11. Test results

- `cargo fmt --all -- --check` ✓
- `cargo check --workspace` ✓, `cargo check -p app --features render` ✓
- `renderer --lib`: **19/19** (14 M10 kept green + 5 M12)
- `scene --lib`: 12/12 · `minecraft-version --lib`: 11/11 ·
  `light_recording`: 2/2 (~9 s)
- Pre-existing, unrelated: 4 playback seek tests OOM (>10 GB, no dep edge
  to renderer — see M10 report); transient Windows AV file-locks on probe
  binaries (pass on retry).
- Viewer device-side pipeline validation (new WGSL light path) requires a
  display — operator to confirm launch.

## 12. Files changed

- NEW `crates/minecraft-version/src/light.rs` (strict decoder + unit tests)
- NEW `crates/minecraft-version/tests/light_recording.rs` (recording tests)
- `crates/minecraft-version/src/lib.rs` (`pub mod light`)
- `crates/minecraft-version/src/chunk.rs` (fill per-section nibbles +
  `"decoded"` status on strict success; untouched otherwise)
- `crates/scene/src/scene.rs` (`SceneSection` light fields + nibble helpers)
- `crates/scene/src/builder.rs` (copy arrays; `"decoded"` → `Available`)
- `crates/renderer/src/mesh.rs` (`Vertex.light`, `LightCache`, sampling,
  wiring, `desc()` slot 5)
- `crates/renderer/src/lib.rs` (cache per build, entity sentinel, 5 tests)
- `crates/renderer/src/shader.wgsl` (tex × tint × light, sun fallback)

## 13. Classifications

- CONFIRMED: blob layout/nibble order/section mapping (recording);
  torch=14, water-surface=14, nether zero-sky; renderer tests.
- STRONGLY INFERRED: face-shade constants + warm shift + ramp shape as
  vanilla-faithful approximations (game knowledge, not packet-derived);
  End skylight=false (no End chunks observed → UNKNOWN, default 0).
- TEMPORARY FALLBACK: `light_ramp` curve (needs dimension tables);
  geometric above-terrain sky rule; overlay nudge (unchanged); plains
  foliage tint (unchanged); sun path for missing/entities.
- FUTURE WORK: night darkening via world time; exact LightTexture ramps;
  light updates from block changes mid-recording; translucent sorting;
  pixel before/after on a display machine.
- UNKNOWN: End-dimension sky data (none in recordings); Starlight-format
  chunks (none observed — strict parse would reject, fallback applies).

## 14. Questions for other agents

1. Playback seek-test OOM (>10 GB) is still open and independent — needs an owner.
2. Operator with a display: launch the viewer on tick 0 (caves vs surface)
   and tick 2341 (nether lava glow) and report anything that looks wrong;
   WGSL light path could not be device-validated headlessly.
3. Should `uvlock:true` handling precede night-darkening, or is lighting
   now sufficient to move to entity models?
