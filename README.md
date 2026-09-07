# Rewind

Rewind is a standalone desktop viewer and editor for Moulberry's Flashback
`.flashback` recordings. It reads recordings directly, reconstructs the
Minecraft world state into a version-independent canonical replay
representation, and renders the reconstructed world — all without Minecraft
running.

> Status: Experimental — live 3D replay viewing works; editing is still under development.

## Features

- Flashback recording/container parsing
- Minecraft 26.2 decoding (blocks, chunks, entities, player state)
- Block-state registry resolution
- Canonical replay state (version-independent)
- Tick-by-tick playback
- Snapshot-based seeking (forward/backward random access)
- Renderer-independent Scene representation
- GPU block rendering (wgpu)
- Real Minecraft block models and textures
- Live 3D replay viewer
- No Minecraft client required at runtime

## Current status

| Milestone | Status |
|---|---|
| M0 — Flashback format | Complete |
| M1 — Minecraft decoding | Complete |
| M2 — Canonical chunks | Complete |
| M3 — Replay state | Complete |
| M4 — Playback | Complete |
| M5 — Seeking | Complete |
| M6 — Scene representation | Complete |
| M7 — GPU renderer | Complete |
| M8 — Live viewer | Complete |
| M9 — Ground-truth audit | Complete |
| M9.9 — Palette decoding | Complete |
| M10 — Texture/UV correctness | Complete |
| M11 — README cleanup | Complete |

## What works today

Rewind opens real Flashback recordings, reconstructs Minecraft world state
tick by tick, converts it into the canonical Scene representation, and
renders the world in a live 3D window — no Minecraft client needed.

Minecraft 26.2 is the currently validated target.

```powershell
cargo run -p app --features render --bin flashback-render -- recordings/chunks/test_recording3.zip 2341
```

## Getting started

Requirements:

- Rust 1.88+ with Cargo
- A display for the live viewer (the CLI probes run headless)
- Minecraft 26.2 client JAR at `%APPDATA%\.minecraft\versions\26.2\26.2.jar`
  (source of the block registry and model/texture assets; a prebuilt
  registry ships in-repo, and Java is only needed to regenerate it)

Build, check, and test:

```powershell
cargo fmt --all
cargo check --workspace
cargo test --workspace -- --test-threads=1
```

Headless probes (no display needed):

```powershell
cargo run --bin flashback-probe -- recordings/basic/test_recording.zip
cargo run --release --bin flashback-scene-probe -- recordings/chunks/test_recording3.zip
```

## What's incomplete

- Lighting (directional fallback only) and per-biome colors (flat fallback tint)
- Transparency sorting, real entity models, non-cube block shapes
- Full block-entity semantics
- Editing tools, timeline/keyframes, desktop GUI polish

## Research reports

Detailed reverse-engineering reports for each milestone live in `docs/`:

- `m0-report.md` — Flashback container and recording format
- `m1-report.md` — Minecraft registry and palette decoding
- `m6-report.md` / `m6.1-report.md` — Scene representation and archaeology
- `m7-report.md` — GPU renderer
- `m8-report.md` — fidelity and playback in the viewer
- `m9-report.md` — ground-truth audit
- `m10-report.md` — texture/UV forensics and fix

## License

MIT OR Apache-2.0
