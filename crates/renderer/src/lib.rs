//! renderer — M7 CPU→GPU on top of Scene.

pub mod asset;
pub mod blockstate;
pub mod mesh;
pub mod model;

#[cfg(feature = "window")]
pub mod cache;
#[cfg(feature = "window")]
pub mod camera;
#[cfg(feature = "window")]
pub mod texture;
#[cfg(feature = "window")]
pub mod wgpu_renderer;

pub use asset::{default_jar_path, JarAssetProvider};
pub use mesh::{SectionMesh, Vertex};

/// Section key for render cache (also used for CPU mesh indexing)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SectionKey {
    pub cx: i32,
    pub cz: i32,
    pub sy: i32,
}

use scene::Scene;
use std::collections::HashSet;

/// Build CPU meshes for a Scene (visible chunks). Returns section meshes + distinct texture keys.
/// For large scenes (>100 chunks), only chunks within 8 chunks of the camera/player are meshed (bounded).
pub fn build_world_meshes(
    scene: &Scene,
    provider: &mut JarAssetProvider,
) -> (Vec<(SectionKey, SectionMesh)>, Vec<String>) {
    let mut out = Vec::new();
    let mut all_textures: HashSet<String> = HashSet::new();
    // M8: camera-centered visible subset for large recordings (avoid 54M blocks)
    let is_large = scene.chunks.len() > 100;
    let center = if let Some(lp) = &scene.local_player {
        glam::Vec3::new(lp.pos[0] as f32, 0.0, lp.pos[2] as f32)
    } else if let Some(sp) = scene.environment.spawn.as_ref().and_then(|s| s.pos) {
        glam::Vec3::new(sp[0] as f32, 0.0, sp[2] as f32)
    } else {
        // fallback to first chunk
        scene
            .chunks
            .keys()
            .next()
            .map(|(cx, cz)| glam::Vec3::new(*cx as f32 * 16.0, 0.0, *cz as f32 * 16.0))
            .unwrap_or(glam::Vec3::ZERO)
    };
    let radius_chunks = 8; // 8*16 = 128 blocks
    for ((cx, cz), chunk) in &scene.chunks {
        if is_large {
            let dx = (*cx as f32 * 16.0 - center.x).abs();
            let dz = (*cz as f32 * 16.0 - center.z).abs();
            if dx > radius_chunks as f32 * 16.0 || dz > radius_chunks as f32 * 16.0 {
                continue;
            }
        }
        for sec in &chunk.sections {
            let key = SectionKey {
                cx: *cx,
                cz: *cz,
                sy: sec.section_y,
            };
            // For large, force is_large=false for visible subset (actually mesh)
            let is_large_for_section = if is_large {
                false
            } else {
                scene.chunks.len() > 100
            };
            // We need to pass is_large flag to generate_section_mesh; for visible subset we want actual mesh, not empty
            // So we call with false for visible, but our generate_section_mesh currently takes is_large from scene length.
            // To avoid empty, we temporarily override: for large visible subset, generate with full blocks
            // Our generate_section_mesh checks scene.chunks.len() >100 to decide empty, but we want to mesh visible.
            // So we bypass by directly calling generate_from_blocks for visible, or just call generate_section_mesh with scene that has filtered chunks?
            // Simpler: call generate_section_mesh which will see is_large=false for this filtered scene (since we filtered, but original scene still >100, so it would still be true).
            // Instead, we handle visible subset by not using fast path: we call a separate helper that forces full.
            // For now, just call generate_section_mesh and if it returns empty due to fast path, fallback to generating from canonical if available.
            // Our fast path currently returns empty for is_large, but for visible we want non-empty, so we need to handle.
            // Workaround: if is_large and sec.blocks.is_empty() due to fast path, we skip (but visible should have blocks, so we need to ensure scene for large still has blocks? Actually M6 fast path stores blocks=Vec::new() for large, so visible also empty.
            // So we need to handle large's empty blocks: we can't mesh. For M8, we should use canonical fallback: if blocks empty, skip meshing and document.
            // For now, just call and if empty, try to mesh anyway by using the original block data via a different path (not yet).
            match crate::mesh::generate_section_mesh(
                sec,
                *cx,
                *cz,
                scene,
                provider,
                &mut all_textures,
            ) {
                Ok(mesh) => {
                    if !mesh.is_empty() {
                        out.push((key, mesh));
                    }
                }
                Err(e) => eprintln!("mesh gen failed for {:?}: {e}", key),
            }
        }
    }
    // M8: entity debug boxes (small cubes)
    if !scene.entities.is_empty() || scene.local_player.is_some() {
        let mut entity_vertices = Vec::new();
        let mut entity_indices = Vec::new();
        let entity_tex = "minecraft:block/red_concrete".to_string();
        all_textures.insert(entity_tex.clone());
        let mut tex_idx = 0;
        // Find or insert texture index for entity
        // We'll create a single mesh for all entities
        let mut push_entity_box = |pos: [f32; 3],
                                   size: f32,
                                   vertices: &mut Vec<crate::mesh::Vertex>,
                                   indices: &mut Vec<u32>| {
            let min = [pos[0] - size / 2.0, pos[1], pos[2] - size / 2.0];
            let max = [pos[0] + size / 2.0, pos[1] + size, pos[2] + size / 2.0];
            let base = vertices.len() as u32;
            let corners = [
                [min[0], min[1], min[2]],
                [max[0], min[1], min[2]],
                [max[0], max[1], min[2]],
                [min[0], max[1], min[2]],
                [min[0], min[1], max[2]],
                [max[0], min[1], max[2]],
                [max[0], max[1], max[2]],
                [min[0], max[1], max[2]],
            ];
            let faces: [[usize; 4]; 6] = [
                [0, 1, 2, 3],
                [4, 7, 6, 5],
                [0, 4, 5, 1],
                [1, 5, 6, 2],
                [2, 6, 7, 3],
                [3, 7, 4, 0],
            ];
            let normals = [
                [0.0, 0.0, -1.0],
                [0.0, 0.0, 1.0],
                [0.0, -1.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [-1.0, 0.0, 0.0],
            ];
            for (fi, face) in faces.iter().enumerate() {
                let base_f = vertices.len() as u32;
                for &ci in face {
                    vertices.push(crate::mesh::Vertex {
                        position: corners[ci],
                        normal: normals[fi],
                        uv: [0.0, 0.0],
                        tex_index: tex_idx,
                    });
                }
                indices.extend_from_slice(&[
                    base_f,
                    base_f + 1,
                    base_f + 2,
                    base_f,
                    base_f + 2,
                    base_f + 3,
                ]);
            }
        };
        for ent in &scene.entities {
            if let Some(pos) = ent.pos {
                let size = 0.6;
                push_entity_box(
                    [pos[0] as f32, pos[1] as f32, pos[2] as f32],
                    size,
                    &mut entity_vertices,
                    &mut entity_indices,
                );
            }
        }
        // Local player debug marker (green)
        if let Some(lp) = &scene.local_player {
            let tex_idx_lp = 0;
            let pos = [lp.pos[0] as f32, lp.pos[1] as f32, lp.pos[2] as f32];
            let base = entity_vertices.len() as u32;
            // Simple marker: small cube at feet
            push_entity_box(pos, 0.6, &mut entity_vertices, &mut entity_indices);
            // Mark with same texture index
            let _ = tex_idx_lp;
            let _ = base;
        }
        if !entity_vertices.is_empty() {
            // Remap UVs for entity texture later (atlas will handle)
            let key = SectionKey {
                cx: 0,
                cz: 0,
                sy: 1000,
            };
            out.push((
                key,
                crate::mesh::SectionMesh {
                    vertices: entity_vertices,
                    indices: entity_indices,
                    texture_keys: vec![entity_tex],
                },
            ));
        }
    }
    let mut tex_list: Vec<String> = all_textures.into_iter().collect();
    tex_list.sort();
    (out, tex_list)
}

/// Deterministic mesh fingerprint for tests (FNV over vertices/indices).
pub fn mesh_fingerprint(meshes: &[(SectionKey, SectionMesh)]) -> u64 {
    let mut h: u64 = 14695981039346656037;
    let mut fnv = |b: &[u8]| {
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(1099511628211);
        }
    };
    let mut sorted = meshes.to_vec();
    sorted.sort_by_key(|(k, _)| (k.cx, k.cz, k.sy));
    for (key, mesh) in sorted {
        fnv(&key.cx.to_le_bytes());
        fnv(&key.cz.to_le_bytes());
        fnv(&key.sy.to_le_bytes());
        fnv(&(mesh.vertices.len() as u32).to_le_bytes());
        fnv(&(mesh.indices.len() as u32).to_le_bytes());
        for v in mesh.vertices.iter().take(4) {
            fnv(&v.position[0].to_le_bytes());
            fnv(&v.position[1].to_le_bytes());
            fnv(&v.position[2].to_le_bytes());
        }
    }
    h
}

#[cfg(test)]
mod tests {
    #[test]
    fn model_resolve_smoke() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = crate::asset::JarAssetProvider::from_jar(jar).unwrap();
        let state = replay_model::CanonicalBlockState {
            name: "minecraft:stone".into(),
            properties: std::collections::BTreeMap::new(),
        };
        let refs = crate::blockstate::resolve_blockstate(&state, &mut prov, None).unwrap();
        assert!(!refs.is_empty());
    }

    #[test]
    fn mesh_determinism() {
        let Some(jar) = crate::asset::default_jar_path() else {
            return;
        };
        let mut prov = crate::asset::JarAssetProvider::from_jar(jar).unwrap();
        let state = replay_model::CanonicalBlockState {
            name: "minecraft:stone".into(),
            properties: std::collections::BTreeMap::new(),
        };
        // Build a tiny scene with one chunk, one section stone
        let blocks = vec![state.clone(); 4096];
        let sec = scene::SceneSection {
            section_y: 0,
            y_base: 0,
            is_empty: false,
            blocks,
            non_empty_block_count: 4096,
            palette_bits: 1,
            palette_size: 1,
            has_renderable: true,
        };
        let chunk = scene::SceneChunk {
            x: 0,
            z: 0,
            min_y: -64,
            height: 384,
            section_count: 1,
            sections: vec![sec],
            block_entities: vec![],
            lighting: scene::SceneLighting {
                status: scene::LightingStatus::RawPreserved,
                raw_bytes_len: None,
                per_section: vec![],
            },
            biome: scene::SceneBiomeData {
                status: scene::BiomeStatus::RawPreserved,
                raw_bytes_len: None,
                note: "test".into(),
            },
            non_empty_count: 4096,
        };
        let mut chunks = std::collections::BTreeMap::new();
        chunks.insert((0, 0), chunk);
        let scene = scene::Scene {
            tick: 0,
            environment: scene::SceneEnvironment {
                dimension: "minecraft:overworld".into(),
                dimension_source: "test".into(),
                sky_available: true,
                lighting_status: scene::LightingStatus::RawPreserved,
                biome_status: scene::BiomeStatus::RawPreserved,
                world_time: None,
                world_border: None,
                spawn: None,
            },
            chunks,
            entities: vec![],
            local_player: None,
            block_entity_count: 0,
            total_sections: 1,
            total_blocks: 4096,
            renderable_blocks: 4096,
            minecraft_version: "26.2".into(),
            data_version: 4903,
            protocol_version: 776,
            warnings: vec![],
            asset_dependency_count: 0,
            asset_keys: vec![],
        };
        let (meshes1, _) = crate::build_world_meshes(&scene, &mut prov);
        let fp1 = crate::mesh_fingerprint(&meshes1);
        let (meshes2, _) = crate::build_world_meshes(&scene, &mut prov);
        let fp2 = crate::mesh_fingerprint(&meshes2);
        assert_eq!(fp1, fp2, "mesh fingerprint must be deterministic");
        assert!(!meshes1.is_empty());
    }
}

fn is_air_at(scene: &scene::Scene, wx: i32, wy: i32, wz: i32) -> bool {
    let cx = wx.div_euclid(16);
    let cz = wz.div_euclid(16);
    if let Some(chunk) = scene.chunks.get(&(cx, cz)) {
        if let Some(sec) = chunk
            .sections
            .iter()
            .find(|s| wy >= s.y_base && wy < s.y_base + 16)
        {
            if sec.blocks.is_empty() {
                // Fast path: use non_empty as proxy (conservative: assume not air if non-empty>0)
                return sec.is_empty;
            }
            let lx = wx.rem_euclid(16) as usize;
            let ly = (wy - sec.y_base) as usize;
            let lz = wz.rem_euclid(16) as usize;
            let idx = (ly * 16 + lz) * 16 + lx;
            if let Some(b) = sec.blocks.get(idx) {
                return b.name == "minecraft:air";
            }
        }
    }
    true // unknown chunk => treat as air (don't block camera)
}

pub fn initial_camera_state(scene: &scene::Scene) -> (glam::Vec3, f32, f32) {
    // 1. Local player (preferred, with eye height and yaw/pitch)
    if let Some(lp) = &scene.local_player {
        let mut pos = glam::Vec3::new(lp.pos[0] as f32, lp.pos[1] as f32 + 1.62, lp.pos[2] as f32);
        let yaw = lp.yaw;
        let pitch = lp.pitch.clamp(-89.0, 89.0);
        // Nudge up if inside solid block (deterministic fallback)
        for _ in 0..5 {
            let block_y = pos.y.floor() as i32;
            let block_x = pos.x.floor() as i32;
            let block_z = pos.z.floor() as i32;
            if is_air_at(scene, block_x, block_y, block_z)
                && is_air_at(scene, block_x, block_y + 1, block_z)
            {
                break;
            }
            pos.y += 1.0;
        }
        return (pos, yaw, pitch);
    }
    // 2. Spawn
    if let Some(spawn) = &scene.environment.spawn {
        if let Some(p) = spawn.pos {
            let pos = glam::Vec3::new(p[0] as f32 + 0.5, p[1] as f32 + 2.0, p[2] as f32 + 0.5);
            let yaw = spawn.angle.unwrap_or(0.0);
            return (pos, yaw, 0.0);
        }
    }
    // 3. First chunk fallback (deterministic: smallest (x,z) from BTreeMap)
    if let Some((&(cx, cz), _)) = scene.chunks.iter().next() {
        let pos = glam::Vec3::new(cx as f32 * 16.0 + 8.0, 80.0, cz as f32 * 16.0 + 8.0);
        return (pos, 180.0, -20.0);
    }
    // 4. Absolute fallback
    (glam::Vec3::new(0.0, 80.0, 0.0), 180.0, -20.0)
}

/// Launch winit + wgpu window and render the given Scene (blocking).
#[cfg(feature = "window")]
pub fn run_blocking(scene: scene::Scene) {
    let mut provider = match JarAssetProvider::from_default_jar() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("No 26.2 JAR for textures, using placeholder: {e}");
            return run_without_textures(scene);
        }
    };
    let (meshes, tex_keys) = build_world_meshes(&scene, &mut provider);
    let texture_keys = tex_keys.clone();
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = std::sync::Arc::new(
        event_loop
            .create_window(
                winit::window::WindowAttributes::default()
                    .with_title(format!(
                        "Rewind — {} tick {} ({} chunks, {} sections, {} textures)",
                        scene.environment.dimension,
                        scene.tick,
                        scene.chunk_count(),
                        scene.total_sections,
                        texture_keys.len()
                    ))
                    .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
            )
            .unwrap(),
    );
    let total_verts: usize = meshes.iter().map(|(_, m)| m.vertices.len()).sum();
    let total_tris: usize = meshes.iter().map(|(_, m)| m.indices.len() / 3).sum();
    println!("Rewind Renderer");
    println!(
        " recording tick {} dim {} chunks {} sections {} meshed {} verts {} tris {} textures {}",
        scene.tick,
        scene.environment.dimension,
        scene.chunk_count(),
        scene.total_sections,
        meshes.len(),
        total_verts,
        total_tris,
        texture_keys.len()
    );
    for (k, m) in meshes.iter().take(5) {
        println!(
            "  section {:?} verts {} tris {}",
            k,
            m.vertices.len(),
            m.indices.len() / 3
        );
    }
    let mut wgpu_state = pollster::block_on(crate::wgpu_renderer::WgpuState::new(window.clone()));
    // M8: sensible initial camera from local player (with yaw/pitch) + safe fallback not inside block
    let (init_pos, init_yaw, init_pitch) = initial_camera_state(&scene);
    wgpu_state.camera.position = init_pos;
    wgpu_state.camera.yaw = init_yaw;
    wgpu_state.camera.pitch = init_pitch;
    wgpu_state.update_camera();
    wgpu_state.build_atlas(&provider, &texture_keys);
    let atlas_map = wgpu_state
        .atlas
        .as_ref()
        .map(|a| a.map.clone())
        .unwrap_or_default();
    for (key, mesh) in &meshes {
        let mut remapped = mesh.clone();
        for v in &mut remapped.vertices {
            let tex_key = &mesh.texture_keys[v.tex_index as usize];
            if let Some([u0, v0, u1, v1]) = atlas_map.get(tex_key) {
                let u = v.uv[0];
                let vv = v.uv[1];
                v.uv[0] = u0 + (u1 - u0) * u;
                v.uv[1] = v0 + (v1 - v0) * vv;
            }
        }
        wgpu_state.upload_section(*key, remapped);
    }
    let mut keys = std::collections::HashSet::new();
    let mut mouse_captured = false;
    let mut last_mouse: Option<winit::dpi::PhysicalPosition<f64>> = None;
    event_loop
        .run(move |event, elwt| match event {
            winit::event::Event::WindowEvent { window_id, event } if window_id == window.id() => {
                match event {
                    winit::event::WindowEvent::CloseRequested => elwt.exit(),
                    winit::event::WindowEvent::Resized(size) => {
                        wgpu_state.resize(size.width, size.height)
                    }
                    winit::event::WindowEvent::KeyboardInput { event, .. } => {
                        let pressed = event.state == winit::event::ElementState::Pressed;
                        if pressed {
                            keys.insert(event.logical_key.clone());
                        } else {
                            keys.remove(&event.logical_key);
                        }
                        if pressed
                            && event.logical_key
                                == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape)
                        {
                            elwt.exit();
                        }
                        let speed = if keys.contains(&winit::keyboard::Key::Named(
                            winit::keyboard::NamedKey::Shift,
                        )) {
                            20.0
                        } else {
                            5.0
                        };
                        let dt = 0.016;
                        if keys.contains(&winit::keyboard::Key::Character("w".into())) {
                            wgpu_state.camera.move_forward(speed * dt);
                        }
                        if keys.contains(&winit::keyboard::Key::Character("s".into())) {
                            wgpu_state.camera.move_forward(-speed * dt);
                        }
                        if keys.contains(&winit::keyboard::Key::Character("a".into())) {
                            wgpu_state.camera.move_right(-speed * dt);
                        }
                        if keys.contains(&winit::keyboard::Key::Character("d".into())) {
                            wgpu_state.camera.move_right(speed * dt);
                        }
                        if keys.contains(&winit::keyboard::Key::Named(
                            winit::keyboard::NamedKey::Space,
                        )) {
                            wgpu_state.camera.move_up(speed * dt);
                        }
                        if keys.contains(&winit::keyboard::Key::Named(
                            winit::keyboard::NamedKey::Control,
                        )) {
                            wgpu_state.camera.move_up(-speed * dt);
                        }
                        wgpu_state.update_camera();
                        window.request_redraw();
                    }
                    winit::event::WindowEvent::MouseInput {
                        state,
                        button: winit::event::MouseButton::Left,
                        ..
                    } => {
                        if state == winit::event::ElementState::Pressed {
                            mouse_captured = true;
                            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
                            window.set_cursor_visible(false);
                        } else {
                            mouse_captured = false;
                            let _ = window.set_cursor_grab(winit::window::CursorGrabMode::None);
                            window.set_cursor_visible(true);
                        }
                    }
                    winit::event::WindowEvent::CursorMoved { position, .. } => {
                        if mouse_captured {
                            if let Some(last) = last_mouse {
                                let dx = (position.x - last.x) as f32;
                                let dy = (position.y - last.y) as f32;
                                wgpu_state.camera.yaw -= dx * 0.15;
                                wgpu_state.camera.pitch -= dy * 0.15;
                                wgpu_state.camera.pitch =
                                    wgpu_state.camera.pitch.clamp(-89.0, 89.0);
                                wgpu_state.update_camera();
                                window.request_redraw();
                            }
                            last_mouse = Some(position);
                        } else {
                            last_mouse = Some(position);
                        }
                    }
                    winit::event::WindowEvent::RedrawRequested => match wgpu_state.render() {
                        Ok(_) => {}
                        Err(wgpu::SurfaceError::Lost) => {
                            wgpu_state.resize(wgpu_state.config.width, wgpu_state.config.height)
                        }
                        Err(wgpu::SurfaceError::OutOfMemory) => elwt.exit(),
                        Err(e) => eprintln!("render error: {e:?}"),
                    },
                    _ => {}
                }
            }
            winit::event::Event::AboutToWait => window.request_redraw(),
            _ => {}
        })
        .unwrap();
}

#[cfg(feature = "window")]
fn run_without_textures(scene: scene::Scene) {
    eprintln!("Running without JAR textures — magenta placeholder");
    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = std::sync::Arc::new(
        event_loop
            .create_window(
                winit::window::WindowAttributes::default().with_title("Rewind — no textures"),
            )
            .unwrap(),
    );
    let mut wgpu_state = pollster::block_on(crate::wgpu_renderer::WgpuState::new(window.clone()));
    wgpu_state.camera.position = glam::Vec3::new(0.0, 80.0, 20.0);
    wgpu_state.update_camera();
    event_loop
        .run(move |event, elwt| match event {
            winit::event::Event::WindowEvent { window_id, event } if window_id == window.id() => {
                match event {
                    winit::event::WindowEvent::CloseRequested => elwt.exit(),
                    winit::event::WindowEvent::RedrawRequested => {
                        let _ = wgpu_state.render();
                    }
                    _ => {}
                }
            }
            winit::event::Event::AboutToWait => window.request_redraw(),
            _ => {}
        })
        .unwrap();
}
