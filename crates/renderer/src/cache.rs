//! Section mesh cache.

use crate::mesh::SectionMesh;
use crate::SectionKey;
use std::collections::HashMap;
use wgpu::util::DeviceExt;

pub struct RenderCache {
    pub meshes: HashMap<SectionKey, GpuSectionMesh>,
}

pub struct GpuSectionMesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub texture_keys: Vec<String>,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            meshes: HashMap::new(),
        }
    }
    pub fn clear(&mut self) {
        self.meshes.clear();
    }
    pub fn insert(&mut self, key: SectionKey, mesh: GpuSectionMesh) {
        self.meshes.insert(key, mesh);
    }
    pub fn remove(&mut self, key: &SectionKey) {
        self.meshes.remove(key);
    }
    pub fn get(&self, key: &SectionKey) -> Option<&GpuSectionMesh> {
        self.meshes.get(key)
    }
}

pub fn build_gpu_mesh(device: &wgpu::Device, mesh: &SectionMesh) -> Option<GpuSectionMesh> {
    if mesh.is_empty() {
        return None;
    }
    let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("section vb"),
        contents: bytemuck::cast_slice(&mesh.vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let ib = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("section ib"),
        contents: bytemuck::cast_slice(&mesh.indices),
        usage: wgpu::BufferUsages::INDEX,
    });
    Some(GpuSectionMesh {
        vertex_buffer: vb,
        index_buffer: ib,
        index_count: mesh.indices.len() as u32,
        texture_keys: mesh.texture_keys.clone(),
    })
}
