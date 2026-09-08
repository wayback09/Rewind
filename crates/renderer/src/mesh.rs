//! CPU mesh generation — Scene → SectionMesh.

use crate::asset::JarAssetProvider;
use crate::blockstate::resolve_blockstate;
use crate::model::resolve_model;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

/// M12: packed per-vertex light: low byte = sky 0..15, next byte = block
/// 0..15. `LIGHT_MISSING` (all bits set) selects the sun-fallback path in
/// the shader (entities, unknown chunks) instead of fabricated darkness.
pub const LIGHT_MISSING: u32 = 0xFFFF_FFFF;

pub fn pack_light(sky: u8, block: u8) -> u32 {
    debug_assert!(sky < 16 && block < 16);
    (sky as u32) | ((block as u32) << 8)
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tex_index: u32,
    /// M10: 1 when the source model face has a tintindex (foliage/grass overlay),
    /// 0 otherwise. Consumed by the shader's fallback tint color.
    pub tint: u32,
    /// M12: packed light from [`pack_light`], or [`LIGHT_MISSING`].
    pub light: u32,
}

#[cfg(feature = "window")]
impl Vertex {
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 24,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: 32,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 36,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Uint32,
                },
                wgpu::VertexAttribute {
                    offset: 40,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Uint32,
                },
            ],
        }
    }
}

#[derive(Debug, Clone)]
pub struct SectionMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub texture_keys: Vec<String>,
}

impl SectionMesh {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// M12: per-mesh-build lighting context. Column tops (highest non-air wy
/// per local column) back the documented above-terrain sky rule; arrays
/// themselves are read straight from the scene sections (never copied).
pub struct LightCache {
    has_skylight: bool,
    tops: HashMap<(i32, i32), [i32; 256]>,
}

impl LightCache {
    /// Empty cache (no chunks, no skylight): every sample is LIGHT_MISSING.
    /// Used by unit tests that mesh without a scene.
    pub fn empty() -> Self {
        Self {
            has_skylight: false,
            tops: HashMap::new(),
        }
    }

    pub fn for_scene(scene: &scene::Scene) -> Self {
        let has_skylight = scene.environment.dimension == "minecraft:overworld";
        let mut tops = HashMap::new();
        for ((cx, cz), chunk) in &scene.chunks {
            let mut col = [i32::MIN; 256];
            for sec in &chunk.sections {
                for (idx, st) in sec.blocks.iter().enumerate() {
                    if st.name != "minecraft:air" {
                        let (lx, ly, lz) = coordinates_local(idx);
                        let wy = sec.y_base + ly as i32;
                        let c = &mut col[lz * 16 + lx];
                        if wy > *c {
                            *c = wy;
                        }
                    }
                }
            }
            tops.insert((*cx, *cz), col);
        }
        Self { has_skylight, tops }
    }

    /// M12: packed light for the cell containing world-space `pos`, sampled
    /// half a block along `normal` (the air the face looks into). Corner
    /// positions give vanilla-style smooth vertex gradients at zero extra
    /// cost (one lookup per vertex). Positions snap to the 1/16 grid first
    /// so f32 rotation dust (~1e-7) cannot flip a floor() at integer planes.
    pub fn sample_vertex(
        &self,
        scene: Option<&scene::Scene>,
        pos: [f32; 3],
        normal: [f32; 3],
    ) -> u32 {
        let scene = match scene {
            Some(s) => s,
            None => return LIGHT_MISSING,
        };
        let snap = |v: f32| (v * 16.0).round() / 16.0;
        let cx = (snap(pos[0] + normal[0] * 0.5)).floor() as i32;
        let cy = (snap(pos[1] + normal[1] * 0.5)).floor() as i32;
        let cz = (snap(pos[2] + normal[2] * 0.5)).floor() as i32;
        self.sample_cell(Some(scene), cx, cy, cz)
    }

    /// M12: packed light for one integer cell. Rules (all documented):
    /// - unknown chunk, or y inside the world but section absent from the
    ///   scene -> LIGHT_MISSING (shader sun fallback; never fabricated).
    /// - below chunk min_y -> dark void (0, 0).
    /// - section present: decoded nibble, else geometric rule — sky 15 iff
    ///   skylight dimension and the cell is above the column top, else 0;
    ///   block always 0 when its array is absent.
    /// - above the chunk height in a skylight dimension -> open sky (15, 0).
    pub fn sample_cell(&self, scene: Option<&scene::Scene>, wx: i32, wy: i32, wz: i32) -> u32 {
        let scene = match scene {
            Some(s) => s,
            None => return LIGHT_MISSING,
        };
        let ccx = wx.div_euclid(16);
        let ccz = wz.div_euclid(16);
        let chunk = match scene.chunks.get(&(ccx, ccz)) {
            Some(c) => c,
            None => return LIGHT_MISSING,
        };
        if wy < chunk.min_y {
            return pack_light(0, 0);
        }
        if wy >= chunk.min_y + chunk.height {
            return if self.has_skylight {
                pack_light(15, 0)
            } else {
                pack_light(0, 0)
            };
        }
        let sec = match chunk
            .sections
            .iter()
            .find(|s| s.section_y == wy.div_euclid(16))
        {
            Some(s) => s,
            None => return LIGHT_MISSING,
        };
        let lx = wx.rem_euclid(16) as usize;
        let ly = (wy - sec.y_base) as usize;
        let lz = wz.rem_euclid(16) as usize;
        let sky = match sec.sky_at_local(lx, ly, lz) {
            Some(v) => v,
            None => {
                let top = self
                    .tops
                    .get(&(ccx, ccz))
                    .map(|c| c[lz * 16 + lx])
                    .unwrap_or(i32::MIN);
                if self.has_skylight && wy > top {
                    15
                } else {
                    0
                }
            }
        };
        let block = sec.block_at_local_light(lx, ly, lz).unwrap_or(0);
        pack_light(sky, block)
    }
}

pub fn generate_section_mesh(
    section: &scene::SceneSection,
    chunk_x: i32,
    chunk_z: i32,
    scene: &scene::Scene,
    provider: &mut JarAssetProvider,
    texture_set: &mut HashSet<String>,
    light: &LightCache,
) -> Result<SectionMesh, String> {
    if section.blocks.is_empty() {
        return Ok(SectionMesh {
            vertices: Vec::new(),
            indices: Vec::new(),
            texture_keys: Vec::new(),
        });
    }
    generate_from_blocks(
        &section.blocks,
        section.y_base,
        chunk_x,
        chunk_z,
        scene,
        provider,
        texture_set,
        light,
    )
}

fn generate_from_blocks(
    blocks: &[replay_model::CanonicalBlockState],
    y_base: i32,
    chunk_x: i32,
    chunk_z: i32,
    scene: &scene::Scene,
    provider: &mut JarAssetProvider,
    texture_set: &mut HashSet<String>,
    light: &LightCache,
) -> Result<SectionMesh, String> {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    let mut texture_keys = Vec::new();
    let mut tex_index_map: HashMap<String, u32> = HashMap::new();
    let mut full_cube_cache: HashMap<String, bool> = HashMap::new();

    for idx in 0..blocks.len() {
        let state = &blocks[idx];
        if state.name == "minecraft:air" {
            continue;
        }
        let (lx, ly, lz) = coordinates_local(idx);
        let wx = chunk_x * 16 + lx as i32;
        let wy = y_base + ly as i32;
        let wz = chunk_z * 16 + lz as i32;

        let model_refs = match resolve_blockstate(state, provider, Some((wx, wy, wz))) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for mref in model_refs {
            let resolved = match resolve_model(&mref.key, provider) {
                Ok(r) => r,
                Err(_) => continue,
            };
            for elem in &resolved.elements {
                for (face_name, face) in &elem.faces {
                    let tex_key = face.texture.trim_start_matches('#');
                    let tex = resolved.textures.get(tex_key).cloned().unwrap_or_else(|| {
                        resolved
                            .textures
                            .values()
                            .next()
                            .cloned()
                            .unwrap_or_default()
                    });
                    if tex.is_empty() {
                        continue;
                    }
                    if let Some(cull) = &face.cullface {
                        // M10: cull against the ROTATED facing direction; the
                        // unrotated cullface names the wrong neighbor for
                        // models with x/y rotation (logs, weighted variants).
                        let rotated = rotated_cullface(cull, &mref);
                        let cull_dir = rotated.unwrap_or(cull.as_str());
                        if should_cull(
                            face_name,
                            cull_dir,
                            wx,
                            wy,
                            wz,
                            scene,
                            &mut full_cube_cache,
                            provider,
                        ) {
                            continue;
                        }
                    }
                    let tex_idx = *tex_index_map.entry(tex.clone()).or_insert_with(|| {
                        let idx = texture_keys.len() as u32;
                        texture_keys.push(tex.clone());
                        texture_set.insert(tex);
                        idx
                    });
                    push_face_quad(
                        &mut vertices,
                        &mut indices,
                        elem,
                        face,
                        face_name,
                        &mref,
                        wx as f32,
                        wy as f32,
                        wz as f32,
                        tex_idx,
                        Some(scene),
                        light,
                    );
                }
            }
        }
    }
    Ok(SectionMesh {
        vertices,
        indices,
        texture_keys,
    })
}

/// M10: rotate an axis-aligned direction vector through a blockstate model
/// rotation, X first then Y — the same order and convention as the corner
/// positions in `push_face_quad` (R_x(+90): +Y→+Z, R_y(+90): +Z→+X, validated
/// against the oak_log axis=x/z blockstate which must move ring textures
/// from ±Y to ±Z/±X). Exact integer math: rotations are multiples of 90°.
///
/// Without this, faces of rotated models (grass/dirt/stone/sand y-variants,
/// logs on axis x/z) keep their unrotated normal and cullface: shading uses
/// the wrong face direction and `should_cull` tests the wrong neighbor,
/// deleting visible faces (holes) or keeping hidden ones.
fn rotate_dir_vec(dir: [i32; 3], mref: &crate::blockstate::BlockModelRef) -> [i32; 3] {
    let steps = |deg: i32| ((deg % 360 + 360) % 360 / 90) as u32;
    let [mut x, mut y, mut z] = dir;
    for _ in 0..steps(mref.x) {
        // R_x(+90): (x, y, z) -> (x, -z, y)
        (y, z) = (-z, y);
    }
    for _ in 0..steps(mref.y) {
        // R_y(+90): (x, y, z) -> (z, y, -x)
        (x, z) = (z, -x);
    }
    [x, y, z]
}

fn dir_name_to_vec(name: &str) -> Option<[i32; 3]> {
    match name {
        "down" => Some([0, -1, 0]),
        "up" => Some([0, 1, 0]),
        "north" => Some([0, 0, -1]),
        "south" => Some([0, 0, 1]),
        "west" => Some([-1, 0, 0]),
        "east" => Some([1, 0, 0]),
        _ => None,
    }
}

fn dir_vec_to_name(v: [i32; 3]) -> Option<&'static str> {
    match v {
        [0, -1, 0] => Some("down"),
        [0, 1, 0] => Some("up"),
        [0, 0, -1] => Some("north"),
        [0, 0, 1] => Some("south"),
        [-1, 0, 0] => Some("west"),
        [1, 0, 0] => Some("east"),
        _ => None,
    }
}

/// M10: cullface direction after the blockstate rotation. Returns `None`
/// when the rotated direction is not axis-aligned (never for 90° multiples)
/// — callers must fall back to the unrotated cullface then.
fn rotated_cullface(
    cullface: &str,
    mref: &crate::blockstate::BlockModelRef,
) -> Option<&'static str> {
    let v = dir_name_to_vec(cullface)?;
    dir_vec_to_name(rotate_dir_vec(v, mref))
}

fn is_transparent_block(name: &str) -> bool {
    matches!(
        name,
        "minecraft:water"
            | "minecraft:lava"
            | "minecraft:glass"
            | "minecraft:glass_pane"
            | "minecraft:ice"
            | "minecraft:barrier"
            | "minecraft:air"
    ) || name.ends_with("_glass")
        || name.ends_with("_leaves")
        || name.contains("vine")
        || name.contains("flower")
        || name.contains("tall_grass")
        || name.contains("snow")
        || name == "minecraft:grass"
        || name == "minecraft:fern"
        || name.contains("sapling")
        || name.contains("door")
        || name.contains("trapdoor")
        || name.contains("fence")
        || name.contains("pane")
        || name.contains("wall")
}

fn is_state_full_cube(
    state: &replay_model::CanonicalBlockState,
    provider: &mut JarAssetProvider,
) -> bool {
    if is_transparent_block(&state.name) {
        return false;
    }
    let refs = match crate::blockstate::resolve_blockstate(state, provider, None) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if refs.len() != 1 {
        return false;
    }
    let resolved = match resolve_model(&refs[0].key, provider) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if resolved.elements.len() != 1 {
        return false;
    }
    let e = &resolved.elements[0];
    e.from == [0.0, 0.0, 0.0] && e.to == [16.0, 16.0, 16.0]
}

fn should_cull(
    _face_name: &str,
    cullface: &str,
    wx: i32,
    wy: i32,
    wz: i32,
    scene: &scene::Scene,
    cache: &mut HashMap<String, bool>,
    provider: &mut JarAssetProvider,
) -> bool {
    let (nx, ny, nz) = match cullface {
        "down" => (wx, wy - 1, wz),
        "up" => (wx, wy + 1, wz),
        "north" => (wx, wy, wz - 1),
        "south" => (wx, wy, wz + 1),
        "west" => (wx - 1, wy, wz),
        "east" => (wx + 1, wy, wz),
        _ => return false,
    };
    let n_cx = nx.div_euclid(16);
    let n_cz = nz.div_euclid(16);
    let n_sy = ny.div_euclid(16);
    // Look up neighbor chunk
    let chunk = match scene.chunks.get(&(n_cx, n_cz)) {
        Some(c) => c,
        None => return false,
    };
    let sec = match chunk.sections.iter().find(|s| s.section_y == n_sy) {
        Some(s) => s,
        None => return false,
    };
    if sec.blocks.is_empty() {
        // Fast path: use non_empty as proxy (if is_empty true => air)
        if sec.is_empty {
            return false;
        }
        // For fast path we can't know per-block, so don't cull (conservative)
        return false;
    }
    let lx = nx.rem_euclid(16) as usize;
    let ly = (ny - sec.y_base) as usize;
    let lz = nz.rem_euclid(16) as usize;
    if ly >= 16 {
        return false;
    }
    let nidx = (ly * 16 + lz) * 16 + lx;
    if nidx >= sec.blocks.len() {
        return false;
    }
    let nstate = &sec.blocks[nidx];
    if nstate.name == "minecraft:air" {
        return false;
    }
    let is_full = {
        let key = format!("{}|{:?}", nstate.name, nstate.properties);
        if let Some(v) = cache.get(&key) {
            *v
        } else {
            let v = is_state_full_cube(nstate, provider);
            cache.insert(key, v);
            v
        }
    };
    is_full
}

fn push_face_quad(
    vertices: &mut Vec<Vertex>,
    indices: &mut Vec<u32>,
    elem: &crate::model::ModelElement,
    face: &crate::model::ModelFace,
    face_name: &str,
    mref: &crate::blockstate::BlockModelRef,
    wx: f32,
    wy: f32,
    wz: f32,
    tex_idx: u32,
    scene: Option<&scene::Scene>,
    light: &LightCache,
) {
    let from = elem.from;
    let to = elem.to;
    let base = vertices.len() as u32;
    let uv = face.uv.unwrap_or([0.0, 0.0, 16.0, 16.0]);
    let mut uv0 = [uv[0] / 16.0, uv[1] / 16.0];
    let mut uv1 = [uv[2] / 16.0, uv[3] / 16.0];
    if let Some(rot) = face.rotation {
        match rot % 360 {
            90 => {
                let tmp = uv0;
                uv0 = [uv0[0], uv1[1]];
                uv1 = [uv1[0], tmp[1]];
            }
            180 => {
                let tmp = uv0;
                uv0 = uv1;
                uv1 = tmp;
            }
            270 => {
                let tmp = uv0;
                uv0 = [uv1[0], uv0[1]];
                uv1 = [tmp[0], uv1[1]];
            }
            _ => {}
        }
    }
    // M10: the normal follows the blockstate rotation like the corners do;
    // otherwise shading (and the overlay nudge below) uses a stale direction.
    let base_normal = match face_name {
        "down" => [0, -1, 0],
        "up" => [0, 1, 0],
        "north" => [0, 0, -1],
        "south" => [0, 0, 1],
        "west" => [-1, 0, 0],
        "east" => [1, 0, 0],
        _ => [0, 1, 0],
    };
    let rn = rotate_dir_vec(base_normal, mref);
    let normal = [rn[0] as f32, rn[1] as f32, rn[2] as f32];
    let corners: [[f32; 3]; 4] = match face_name {
        "down" => [
            [from[0], from[1], from[2]],
            [to[0], from[1], from[2]],
            [to[0], from[1], to[2]],
            [from[0], from[1], to[2]],
        ],
        "up" => [
            [from[0], to[1], to[2]],
            [to[0], to[1], to[2]],
            [to[0], to[1], from[2]],
            [from[0], to[1], from[2]],
        ],
        "north" => [
            [to[0], from[1], from[2]],
            [from[0], from[1], from[2]],
            [from[0], to[1], from[2]],
            [to[0], to[1], from[2]],
        ],
        "south" => [
            [from[0], from[1], to[2]],
            [to[0], from[1], to[2]],
            [to[0], to[1], to[2]],
            [from[0], to[1], to[2]],
        ],
        "west" => [
            [from[0], from[1], from[2]],
            [from[0], from[1], to[2]],
            [from[0], to[1], to[2]],
            [from[0], to[1], from[2]],
        ],
        "east" => [
            [to[0], from[1], to[2]],
            [to[0], from[1], from[2]],
            [to[0], to[1], from[2]],
            [to[0], to[1], to[2]],
        ],
        _ => [
            [from[0], from[1], from[2]],
            [to[0], from[1], from[2]],
            [to[0], to[1], from[2]],
            [from[0], to[1], from[2]],
        ],
    };
    let mut rotated = [[0.0f32; 3]; 4];
    for (i, p) in corners.iter().enumerate() {
        let mut pos = Vec3::new(p[0], p[1], p[2]);
        if mref.x != 0 {
            let rad = (mref.x as f32).to_radians();
            let cos = rad.cos();
            let sin = rad.sin();
            let y = pos.y - 8.0;
            let z = pos.z - 8.0;
            pos.y = y * cos - z * sin + 8.0;
            pos.z = y * sin + z * cos + 8.0;
        }
        if mref.y != 0 {
            let rad = (mref.y as f32).to_radians();
            let cos = rad.cos();
            let sin = rad.sin();
            let x = pos.x - 8.0;
            let z = pos.z - 8.0;
            pos.x = x * cos + z * sin + 8.0;
            pos.z = -x * sin + z * cos + 8.0;
        }
        rotated[i] = [pos.x, pos.y, pos.z];
    }
    // M10: UV slot order must match vanilla FaceBakery. Our corners are vanilla's
    // [v1, v2, v3, v0] cyclic rotation, and vanilla assigns
    // v0:(minU,minV) v1:(minU,maxV) v2:(maxU,maxV) v3:(maxU,minV), so our slots
    // get [(minU,maxV),(maxU,maxV),(maxU,minV),(minU,minV)]. The previous code
    // used [(minU,minV),(maxU,minV),(maxU,maxV),(minU,maxV)], i.e. V inverted:
    // texture-top rendered at block-bottom (grass green strip at base, etc.).
    let uvs = [
        [uv0[0], uv1[1]],
        [uv1[0], uv1[1]],
        [uv1[0], uv0[1]],
        [uv0[0], uv0[1]],
    ];
    // M10: tinted overlay faces (grass/leaf overlays) share the base face plane.
    // Nudge them slightly along the normal so they pass the Less depth test
    // against the base quad (TEMPORARY FALLBACK until translucent sorting).
    let tinted = face.tintindex.is_some();
    let nudge = if tinted { 0.002 } else { 0.0 };
    let tint = if tinted { 1 } else { 0 };
    for i in 0..4 {
        let corner = [
            wx + rotated[i][0] / 16.0,
            wy + rotated[i][1] / 16.0,
            wz + rotated[i][2] / 16.0,
        ];
        // M12: smooth vertex light sampled from the cell the corner looks
        // into (nudge excluded: it must not shift the sampled cell).
        let lv = light.sample_vertex(scene, corner, normal);
        vertices.push(Vertex {
            position: [
                corner[0] + normal[0] * nudge,
                corner[1] + normal[1] * nudge,
                corner[2] + normal[2] * nudge,
            ],
            normal,
            uv: uvs[i],
            tex_index: tex_idx,
            tint,
            light: lv,
        });
    }
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

pub(crate) fn coordinates_local(idx: usize) -> (usize, usize, usize) {
    let lx = idx % 16;
    let lz = (idx / 16) % 16;
    let ly = idx / 256;
    (lx, ly, lz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[cfg(feature = "window")]
    #[test]
    fn vertex_desc_ok() {
        let _ = Vertex::desc();
    }
    #[test]
    fn coordinates_local_roundtrip() {
        for idx in 0..4096 {
            let (lx, ly, lz) = coordinates_local(idx);
            let nidx = (ly * 16 + lz) * 16 + lx;
            assert_eq!(idx, nidx);
        }
    }

    fn test_face(name: &str, tinted: bool) -> crate::model::ModelFace {
        crate::model::ModelFace {
            texture: "#all".into(),
            uv: Some([0.0, 0.0, 16.0, 16.0]),
            rotation: None,
            cullface: Some(name.into()),
            tintindex: if tinted { Some(0) } else { None },
        }
    }

    fn test_elem() -> crate::model::ModelElement {
        crate::model::ModelElement {
            from: [0.0, 0.0, 0.0],
            to: [16.0, 16.0, 16.0],
            rotation: None,
            shade: None,
            faces: BTreeMap::new(),
        }
    }

    fn test_mref() -> crate::blockstate::BlockModelRef {
        crate::blockstate::BlockModelRef {
            key: "minecraft:block/stone".into(),
            x: 0,
            y: 0,
            uvlock: false,
            weight: 1,
        }
    }

    fn quad_for(face_name: &str, tinted: bool) -> Vec<Vertex> {
        quad_for_rot(face_name, tinted, 0, 0)
    }

    fn quad_for_rot(face_name: &str, tinted: bool, rx: i32, ry: i32) -> Vec<Vertex> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let elem = test_elem();
        let face = test_face(face_name, tinted);
        let mut mref = test_mref();
        mref.x = rx;
        mref.y = ry;
        let light = LightCache::empty();
        push_face_quad(
            &mut vertices,
            &mut indices,
            &elem,
            &face,
            face_name,
            &mref,
            0.0,
            0.0,
            0.0,
            0,
            None,
            &light,
        );
        assert_eq!(vertices.len(), 4);
        // M12: no scene -> every sample is LIGHT_MISSING (sun fallback).
        assert!(vertices.iter().all(|v| v.light == LIGHT_MISSING));
        vertices
    }

    /// M10: vanilla FaceInfo assigns v0:(minU,minV) to the TOP corner of side
    /// faces (e.g. NORTH v0/v3 have MAX_Y). Texture-top (v=0) must land on
    /// block-top (y=1.0 in world units for a full cube at origin).
    #[test]
    fn side_face_texture_top_at_block_top() {
        for face in ["north", "south", "west", "east"] {
            let verts = quad_for(face, false);
            for v in &verts {
                let is_top = (v.position[1] - 1.0).abs() < 1e-6;
                let is_bottom = v.position[1].abs() < 1e-6;
                assert!(is_top || is_bottom, "{face} y={}", v.position[1]);
                if is_top {
                    assert!(
                        v.uv[1].abs() < 1e-6,
                        "{face} top vertex must sample texture top, got v={}",
                        v.uv[1]
                    );
                } else {
                    assert!(
                        (v.uv[1] - 1.0).abs() < 1e-6,
                        "{face} bottom vertex must sample texture bottom, got v={}",
                        v.uv[1]
                    );
                }
                assert_eq!(v.tint, 0);
            }
        }
    }

    /// M10: faces with tintindex (grass/leaf overlays) must carry the tint flag
    /// so the shader can apply the fallback foliage color; untinted faces stay 0.
    #[test]
    fn tint_flag_follows_tintindex() {
        let verts = quad_for("north", true);
        assert!(verts.iter().all(|v| v.tint == 1));
        let verts = quad_for("up", false);
        assert!(verts.iter().all(|v| v.tint == 0));
    }

    /// M10: Y rotation preserves the vertical slot assignment (rotation about
    /// the vertical axis cannot move texture-top to block-bottom), so the
    /// V orientation invariant holds for every weighted-rotation variant.
    #[test]
    fn rotation_y_preserves_side_v_orientation() {
        for face in ["north", "south", "west", "east"] {
            for ry in [0, 90, 180, 270] {
                let verts = quad_for_rot(face, false, 0, ry);
                for v in &verts {
                    let is_top = (v.position[1] - 1.0).abs() < 1e-6;
                    let is_bottom = v.position[1].abs() < 1e-6;
                    assert!(is_top || is_bottom, "{face} y={ry} y={}", v.position[1]);
                    if is_top {
                        assert!(
                            v.uv[1].abs() < 1e-6,
                            "{face} y={ry} top must sample texture top, got v={}",
                            v.uv[1]
                        );
                    } else {
                        assert!(
                            (v.uv[1] - 1.0).abs() < 1e-6,
                            "{face} y={ry} bottom must sample texture bottom, got v={}",
                            v.uv[1]
                        );
                    }
                }
            }
        }
    }

    /// M10: x=90 moves the up face (log ring texture) onto the south side,
    /// carrying its normal with it. Regression geometry for axis=z logs.
    #[test]
    fn rotation_x90_moves_up_face_south() {
        let verts = quad_for_rot("up", false, 90, 0);
        assert!(verts.iter().all(|v| (v.position[2] - 1.0).abs() < 1e-4));
        assert!(verts.iter().all(|v| v.normal == [0.0, 0.0, 1.0]));
    }

    /// M10: direction rotation must match the corner rotation (same order:
    /// X first, then Y). The oak_log axis=x case (x=90 then y=90) must move
    /// ring textures from ±Y to ±X — the constraint that fixes the convention.
    #[test]
    fn rotated_cullface_matches_geometry() {
        let mref = |x, y| crate::blockstate::BlockModelRef {
            key: "minecraft:block/oak_log".into(),
            x,
            y,
            uvlock: false,
            weight: 1,
        };
        // Identity.
        for d in ["down", "up", "north", "south", "west", "east"] {
            assert_eq!(rotated_cullface(d, &mref(0, 0)), Some(d));
        }
        // Y rotations cycle the compass.
        assert_eq!(rotated_cullface("north", &mref(0, 90)), Some("west"));
        assert_eq!(rotated_cullface("west", &mref(0, 90)), Some("south"));
        assert_eq!(rotated_cullface("south", &mref(0, 90)), Some("east"));
        assert_eq!(rotated_cullface("east", &mref(0, 90)), Some("north"));
        assert_eq!(rotated_cullface("up", &mref(0, 90)), Some("up"));
        assert_eq!(rotated_cullface("north", &mref(0, 180)), Some("south"));
        // X rotation tips top/bottom onto the sides.
        assert_eq!(rotated_cullface("up", &mref(90, 0)), Some("south"));
        assert_eq!(rotated_cullface("down", &mref(90, 0)), Some("north"));
        assert_eq!(rotated_cullface("north", &mref(90, 0)), Some("up"));
        assert_eq!(rotated_cullface("south", &mref(90, 0)), Some("down"));
        // axis=x log composition: rings ±Y -> ±Z -> ±X.
        assert_eq!(rotated_cullface("up", &mref(90, 90)), Some("east"));
        assert_eq!(rotated_cullface("down", &mref(90, 90)), Some("west"));
        assert_eq!(rotate_dir_vec([0, 1, 0], &mref(90, 90)), [1, 0, 0]);
        assert_eq!(rotate_dir_vec([0, -1, 0], &mref(90, 90)), [-1, 0, 0]);
    }
}
