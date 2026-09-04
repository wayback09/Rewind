//! CPU mesh generation — Scene → SectionMesh.

use crate::asset::JarAssetProvider;
use crate::blockstate::resolve_blockstate;
use crate::model::resolve_model;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub tex_index: u32,
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

pub fn generate_section_mesh(
    section: &scene::SceneSection,
    chunk_x: i32,
    chunk_z: i32,
    provider: &mut JarAssetProvider,
    texture_set: &mut HashSet<String>,
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
        provider,
        texture_set,
    )
}

fn generate_from_blocks(
    blocks: &[replay_model::CanonicalBlockState],
    y_base: i32,
    chunk_x: i32,
    chunk_z: i32,
    provider: &mut JarAssetProvider,
    texture_set: &mut HashSet<String>,
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
                        if should_cull(
                            face_name,
                            cull,
                            wx,
                            wy,
                            wz,
                            blocks,
                            chunk_x,
                            chunk_z,
                            y_base,
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

fn is_state_full_cube(
    state: &replay_model::CanonicalBlockState,
    provider: &mut JarAssetProvider,
) -> bool {
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
    blocks: &[replay_model::CanonicalBlockState],
    chunk_x: i32,
    chunk_z: i32,
    y_base: i32,
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
    if n_cx != chunk_x || n_cz != chunk_z {
        return false;
    }
    let n_sy = ny.div_euclid(16);
    if n_sy != y_base.div_euclid(16) {
        return false;
    }
    let nlx = nx.rem_euclid(16) as usize;
    let nly = (ny - y_base) as usize;
    let nlz = nz.rem_euclid(16) as usize;
    if nly >= 16 {
        return false;
    }
    let nidx = (nly * 16 + nlz) * 16 + nlx;
    if nidx >= blocks.len() {
        return false;
    }
    let nstate = &blocks[nidx];
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
    let normal = match face_name {
        "down" => [0.0, -1.0, 0.0],
        "up" => [0.0, 1.0, 0.0],
        "north" => [0.0, 0.0, -1.0],
        "south" => [0.0, 0.0, 1.0],
        "west" => [-1.0, 0.0, 0.0],
        "east" => [1.0, 0.0, 0.0],
        _ => [0.0, 1.0, 0.0],
    };
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
    let uvs = [
        [uv0[0], uv0[1]],
        [uv1[0], uv0[1]],
        [uv1[0], uv1[1]],
        [uv0[0], uv1[1]],
    ];
    for i in 0..4 {
        vertices.push(Vertex {
            position: [
                wx + rotated[i][0] / 16.0,
                wy + rotated[i][1] / 16.0,
                wz + rotated[i][2] / 16.0,
            ],
            normal,
            uv: uvs[i],
            tex_index: tex_idx,
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
}
