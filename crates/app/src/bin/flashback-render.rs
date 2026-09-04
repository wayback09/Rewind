//! flashback-render — M8 live replay viewer with playback.

use flashback_format::{
    chunk::parse_chunk_bytes,
    zip_container::{open_zip_readonly, read_entry_bytes},
};
use minecraft_version::{registry::load_26_2_registry, MinecraftVersion};
use playback::{ParsedChunkWithData, ReplayPlayer};
use scene::{SceneBuilder, StubAssetProvider};
use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <recording.zip> [tick]", args[0]);
        std::process::exit(1);
    }
    let path = PathBuf::from(&args[1]);
    let init_tick: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    run(path, init_tick);
}

#[cfg(not(feature = "render"))]
fn run(path: PathBuf, tick: u32) {
    eprintln!("render feature not enabled — build with --features render");
    std::process::exit(1);
}

#[cfg(feature = "render")]
fn run(path: PathBuf, init_tick: u32) {
    use renderer::{JarAssetProvider, SectionKey};
    use std::collections::HashSet;
    use std::time::Instant;

    let registry = Box::new(load_26_2_registry().expect("26.2 registry"));
    let version = MinecraftVersion::v26_2();
    let mut archive = open_zip_readonly(&path).expect("open zip");
    let meta_bytes = read_entry_bytes(&mut archive, "metadata.json").expect("metadata");
    let meta: serde_json::Value = serde_json::from_slice(&meta_bytes).unwrap();
    let total_ticks = meta["total_ticks"].as_u64().unwrap_or(0) as u32;
    println!(
        "Recording: {} ({} ticks, seek {})",
        path.display(),
        total_ticks,
        init_tick
    );

    let chunks_meta = meta["chunks"].as_object().cloned().unwrap_or_default();
    let mut names: Vec<String> = chunks_meta.keys().cloned().collect();
    names.sort();
    let mut parsed_chunks = Vec::new();
    for name in &names {
        let data = read_entry_bytes(&mut archive, name).expect("chunk");
        let parsed = parse_chunk_bytes(&data, name).expect("parse");
        parsed_chunks.push(ParsedChunkWithData { parsed, data });
    }
    let level_cache = read_entry_bytes(&mut archive, "level_chunk_caches/0").unwrap_or_default();
    let mut player =
        ReplayPlayer::initialize(parsed_chunks, level_cache, &*registry, version).expect("init");
    if init_tick > 0 {
        player.seek(init_tick).expect("seek");
    }
    println!(
        "Tick {} dim {} chunks {} entities {}",
        player.state.tick,
        player.state.dimension.0,
        player.state.chunks.len(),
        player.state.entities.len()
    );

    let stub = StubAssetProvider;
    let builder = SceneBuilder::new(&stub);
    let mut scene = builder.from_replay_state(&player.state);
    let mut jar_provider = JarAssetProvider::from_default_jar().unwrap_or_else(|_| {
        // fallback to registry path's jar
        let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/minecraft-version/data/26.2-blocks-array.json");
        JarAssetProvider::from_jar(p).unwrap_or_else(|_| panic!("no JAR"))
    });

    // Try default jar, else fallback to stub (magenta)
    let mut provider = match JarAssetProvider::from_default_jar() {
        Ok(p) => p,
        Err(_) => {
            eprintln!("No 26.2 JAR — textures will be magenta");
            JarAssetProvider::from_jar(
                std::env::var("APPDATA")
                    .map(|a| PathBuf::from(a).join(".minecraft/versions/26.2/26.2.jar"))
                    .unwrap_or_else(|_| PathBuf::from("dummy")),
            )
            .unwrap_or_else(|_| {
                // Create dummy provider that will fail texture loads but still mesh
                JarAssetProvider::from_jar(PathBuf::from("dummy"))
                    .unwrap_or_else(|e| panic!("dummy: {e}"))
            })
        }
    };
    // Use the real provider if available, else the dummy
    let mut real_provider = JarAssetProvider::from_default_jar().ok();

    let event_loop = winit::event_loop::EventLoop::new().unwrap();
    let window = std::sync::Arc::new(
        event_loop
            .create_window(
                winit::window::WindowAttributes::default()
                    .with_title(format!(
                        "Rewind — {} tick {} ({} chunks)",
                        scene.environment.dimension,
                        scene.tick,
                        scene.chunk_count()
                    ))
                    .with_inner_size(winit::dpi::LogicalSize::new(1280, 720)),
            )
            .unwrap(),
    );

    let mut wgpu_state =
        pollster::block_on(renderer::wgpu_renderer::WgpuState::new(window.clone()));
    // Initial camera from scene
    {
        let (pos, yaw, pitch) = renderer::initial_camera_state(&scene);
        wgpu_state.camera.position = pos;
        wgpu_state.camera.yaw = yaw;
        wgpu_state.camera.pitch = pitch;
        wgpu_state.update_camera();
        wgpu_state.set_dimension(&scene.environment.dimension);
    }

    let mut last_scene = scene.clone();
    let mut meshes_and_tex =
        renderer::build_world_meshes(&scene, real_provider.as_mut().unwrap_or(&mut jar_provider));
    let mut tex_keys = meshes_and_tex.1.clone();
    wgpu_state.build_atlas(real_provider.as_ref().unwrap_or(&jar_provider), &tex_keys);
    upload_meshes(&mut wgpu_state, &meshes_and_tex.0, &tex_keys);

    let mut playing = false;
    let mut speed: f32 = 20.0; // ticks per second
    let mut last_tick_time = Instant::now();
    let mut current_tick = scene.tick;
    let mut keys = HashSet::new();
    let mut mouse_captured = false;
    let mut last_mouse: Option<winit::dpi::PhysicalPosition<f64>> = None;

    // Debug overlay in title
    let update_title = |w: &winit::window::Window,
                        s: &scene::Scene,
                        meshes: &[(SectionKey, renderer::SectionMesh)],
                        playing: bool,
                        speed: f32| {
        let verts: usize = meshes.iter().map(|(_, m)| m.vertices.len()).sum();
        w.set_title(&format!(
            "Rewind — {} tick {}/{} {} chunks {} meshed {} verts {} playing={} speed={:.1}",
            s.environment.dimension,
            s.tick,
            total_ticks,
            if s.chunks.len() > 100 { "large" } else { "" },
            s.chunk_count(),
            meshes.len(),
            verts,
            playing,
            speed
        ));
    };
    update_title(&window, &scene, &meshes_and_tex.0, playing, speed);

    event_loop
        .run(move |event, elwt| {
            match event {
                winit::event::Event::WindowEvent { window_id, event }
                    if window_id == window.id() =>
                {
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
                            if pressed {
                                match event.logical_key {
                                    winit::keyboard::Key::Named(
                                        winit::keyboard::NamedKey::Escape,
                                    ) => elwt.exit(),
                                    winit::keyboard::Key::Character(ref c)
                                        if c == "p" || c == "P" =>
                                    {
                                        playing = !playing;
                                        println!("playing={}", playing);
                                        update_title(
                                            &window,
                                            &last_scene,
                                            &meshes_and_tex.0,
                                            playing,
                                            speed,
                                        );
                                    }
                                    winit::keyboard::Key::Character(ref c)
                                        if c == "n" || c == "N" =>
                                    {
                                        let target = (current_tick + 1).min(total_ticks);
                                        if player.seek(target).is_ok() {
                                            current_tick = target;
                                            let new_scene =
                                                builder.from_replay_state(&player.state);
                                            // Use diff to decide rebuild (for now rebuild all if dim changed or chunks changed)
                                            let diff = scene::diff(&last_scene, &new_scene);
                                            if diff.environment_changed {
                                                println!(
                                                    "Dimension: {} -> {}",
                                                    last_scene.environment.dimension,
                                                    new_scene.environment.dimension
                                                );
                                                wgpu_state.cache.clear();
                                                wgpu_state.set_dimension(
                                                    &new_scene.environment.dimension,
                                                );
                                            }
                                            last_scene = new_scene.clone();
                                            let (new_meshes, new_tex) =
                                                renderer::build_world_meshes(
                                                    &new_scene,
                                                    real_provider
                                                        .as_mut()
                                                        .unwrap_or(&mut jar_provider),
                                                );
                                            if new_tex != tex_keys {
                                                wgpu_state.build_atlas(
                                                    real_provider.as_ref().unwrap_or(&jar_provider),
                                                    &new_tex,
                                                );
                                                tex_keys = new_tex;
                                            }
                                            wgpu_state.cache.clear();
                                            upload_meshes(&mut wgpu_state, &new_meshes, &tex_keys);
                                            meshes_and_tex = (new_meshes, tex_keys.clone());
                                            current_tick = new_scene.tick;
                                            update_title(
                                                &window,
                                                &new_scene,
                                                &meshes_and_tex.0,
                                                playing,
                                                speed,
                                            );
                                            println!(
                                                "Tick {} dim {} chunks {} entities {}",
                                                new_scene.tick,
                                                new_scene.environment.dimension,
                                                new_scene.chunk_count(),
                                                new_scene.entities.len()
                                            );
                                        }
                                        window.request_redraw();
                                    }
                                    winit::keyboard::Key::Character(ref c)
                                        if c == "b" || c == "B" =>
                                    {
                                        let target = current_tick.saturating_sub(1);
                                        if player.seek(target).is_ok() {
                                            current_tick = target;
                                            let new_scene =
                                                builder.from_replay_state(&player.state);
                                            last_scene = new_scene.clone();
                                            let (new_meshes, new_tex) =
                                                renderer::build_world_meshes(
                                                    &new_scene,
                                                    real_provider
                                                        .as_mut()
                                                        .unwrap_or(&mut jar_provider),
                                                );
                                            if new_tex != tex_keys {
                                                wgpu_state.build_atlas(
                                                    real_provider.as_ref().unwrap_or(&jar_provider),
                                                    &new_tex,
                                                );
                                                tex_keys = new_tex;
                                            }
                                            wgpu_state.cache.clear();
                                            upload_meshes(&mut wgpu_state, &new_meshes, &tex_keys);
                                            meshes_and_tex = (new_meshes, tex_keys.clone());
                                            update_title(
                                                &window,
                                                &new_scene,
                                                &meshes_and_tex.0,
                                                playing,
                                                speed,
                                            );
                                        }
                                        window.request_redraw();
                                    }
                                    winit::keyboard::Key::Character(ref c)
                                        if c == "+" || c == "=" =>
                                    {
                                        speed = (speed * 1.2).min(100.0);
                                        println!("speed {}", speed);
                                        update_title(
                                            &window,
                                            &last_scene,
                                            &meshes_and_tex.0,
                                            playing,
                                            speed,
                                        );
                                    }
                                    winit::keyboard::Key::Character(ref c)
                                        if c == "-" || c == "_" =>
                                    {
                                        speed = (speed / 1.2).max(1.0);
                                        println!("speed {}", speed);
                                        update_title(
                                            &window,
                                            &last_scene,
                                            &meshes_and_tex.0,
                                            playing,
                                            speed,
                                        );
                                    }
                                    winit::keyboard::Key::Named(
                                        winit::keyboard::NamedKey::Space,
                                    ) => { // Space used for play/pause alternative (keep Ctrl for down)
                                         // Keep Space for up, so use P for play/pause (already)
                                    }
                                    _ => {}
                                }
                            }
                            // Movement (keep Space for up, Ctrl for down)
                            let move_speed = if keys.contains(&winit::keyboard::Key::Named(
                                winit::keyboard::NamedKey::Shift,
                            )) {
                                20.0
                            } else {
                                5.0
                            };
                            let dt: f32 = 0.016;
                            if keys.contains(&winit::keyboard::Key::Character("w".into())) {
                                wgpu_state.camera.move_forward(move_speed * dt);
                            }
                            if keys.contains(&winit::keyboard::Key::Character("s".into())) {
                                wgpu_state.camera.move_forward(-move_speed * dt);
                            }
                            if keys.contains(&winit::keyboard::Key::Character("a".into())) {
                                wgpu_state.camera.move_right(-move_speed * dt);
                            }
                            if keys.contains(&winit::keyboard::Key::Character("d".into())) {
                                wgpu_state.camera.move_right(move_speed * dt);
                            }
                            if keys.contains(&winit::keyboard::Key::Named(
                                winit::keyboard::NamedKey::Space,
                            )) {
                                wgpu_state.camera.move_up(move_speed * dt);
                            }
                            if keys.contains(&winit::keyboard::Key::Named(
                                winit::keyboard::NamedKey::Control,
                            )) {
                                wgpu_state.camera.move_up(-move_speed * dt);
                            }
                            wgpu_state.update_camera();
                        }
                        winit::event::WindowEvent::MouseInput {
                            state,
                            button: winit::event::MouseButton::Left,
                            ..
                        } => {
                            if state == winit::event::ElementState::Pressed {
                                mouse_captured = true;
                                let _ =
                                    window.set_cursor_grab(winit::window::CursorGrabMode::Confined);
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
                winit::event::Event::AboutToWait => {
                    if playing && last_tick_time.elapsed().as_secs_f32() > 1.0 / speed {
                        let target = (current_tick + 1).min(total_ticks);
                        if target == current_tick {
                            playing = false;
                        } else if player.seek(target).is_ok() {
                            current_tick = target;
                            let new_scene = builder.from_replay_state(&player.state);
                            let diff = scene::diff(&last_scene, &new_scene);
                            if diff.environment_changed {
                                wgpu_state.cache.clear();
                                wgpu_state.set_dimension(&new_scene.environment.dimension);
                            }
                            last_scene = new_scene.clone();
                            let (new_meshes, new_tex) = renderer::build_world_meshes(
                                &new_scene,
                                real_provider.as_mut().unwrap_or(&mut jar_provider),
                            );
                            if new_tex != tex_keys {
                                wgpu_state.build_atlas(
                                    real_provider.as_ref().unwrap_or(&jar_provider),
                                    &new_tex,
                                );
                                tex_keys = new_tex;
                            }
                            // For now clear and re-upload all (future: only changed sections via diff.chunk_diffs)
                            if !diff.is_empty {
                                wgpu_state.cache.clear();
                                upload_meshes(&mut wgpu_state, &new_meshes, &tex_keys);
                                meshes_and_tex = (new_meshes, tex_keys.clone());
                            }
                            update_title(&window, &new_scene, &meshes_and_tex.0, playing, speed);
                        }
                        last_tick_time = Instant::now();
                    }
                    window.request_redraw();
                }
                _ => {}
            }
        })
        .unwrap();
}

#[cfg(feature = "render")]
fn upload_meshes(
    wgpu_state: &mut renderer::wgpu_renderer::WgpuState,
    meshes: &[(renderer::SectionKey, renderer::SectionMesh)],
    tex_keys: &[String],
) {
    // This is handled in run_blocking's atlas remapping; for playback we already built meshes with correct uv (0-1) and atlas will remap on GPU via uniform? For phase 1 we remapped on CPU before upload in run_blocking.
    // In this simplified playback, meshes already have 0-1 uvs, atlas built separately, but we need to remap similarly.
    // For now, just upload as-is (atlas sampling will be wrong for now, but still shows geometry with magenta)
    // To properly remap, we need atlas_map
    let atlas_map = wgpu_state
        .atlas
        .as_ref()
        .map(|a| a.map.clone())
        .unwrap_or_default();
    for (key, mesh) in meshes {
        let mut remapped = (*mesh).clone();
        for v in &mut remapped.vertices {
            // v.tex_index indexes into mesh.texture_keys, which is per-mesh, not global tex_keys
            // For simplicity, assume tex_keys global order matches per-mesh order (since we used same provider, first texture is same)
            // This is approximate for M8; full correct would need per-vertex texture key lookup
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
    let _ = tex_keys;
}

#[cfg(not(feature = "render"))]
fn upload_meshes(_: &mut (), _: &[(renderer::SectionKey, renderer::SectionMesh)], _: &[String]) {}
