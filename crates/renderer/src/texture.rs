//! Texture loading and simple atlas.

use std::collections::HashMap;

pub struct TextureAtlas {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub size: u32,
    pub map: HashMap<String, [f32; 4]>, // key -> [u0,v0,u1,v1] in 0-1
    pub bind_group: wgpu::BindGroup,
    pub layout: wgpu::BindGroupLayout,
}

impl TextureAtlas {
    /// Build atlas from distinct texture keys (e.g., "minecraft:block/stone").
    /// Each texture is 16x16. Packs into `cols` columns.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        provider: &crate::asset::JarAssetProvider,
        keys: &[String],
    ) -> Result<Self, String> {
        let cols = 16u32;
        let tile = 16u32;
        let rows = ((keys.len() as u32 + cols - 1) / cols).max(1);
        let width = cols * tile;
        let height = rows * tile;
        let mut rgba = vec![255u8; (width * height * 4) as usize];
        // Fill with magenta for missing
        for chunk in rgba.chunks_mut(4) {
            chunk[0] = 255;
            chunk[1] = 0;
            chunk[2] = 255;
            chunk[3] = 255;
        }
        let mut map = HashMap::new();
        for (i, key) in keys.iter().enumerate() {
            let col = (i as u32) % cols;
            let row = (i as u32) / cols;
            let x0 = col * tile;
            let y0 = row * tile;
            // Load PNG
            let bytes = provider.load_texture_bytes(key).unwrap_or_else(|_| vec![]);
            let img = if bytes.is_empty() {
                None
            } else {
                image::load_from_memory(&bytes).ok()
            };
            if let Some(img) = img {
                let img = img.to_rgba8();
                // Resize to 16x16 if needed (nearest)
                for ty in 0..tile {
                    for tx in 0..tile {
                        let sx = (tx as f32 * img.width() as f32 / tile as f32) as u32;
                        let sy = (ty as f32 * img.height() as f32 / tile as f32) as u32;
                        let px = img.get_pixel(sx.min(img.width() - 1), sy.min(img.height() - 1));
                        let dst_idx = ((y0 + ty) * width + x0 + tx) as usize * 4;
                        rgba[dst_idx] = px[0];
                        rgba[dst_idx + 1] = px[1];
                        rgba[dst_idx + 2] = px[2];
                        rgba[dst_idx + 3] = px[3];
                    }
                }
            }
            let u0 = x0 as f32 / width as f32;
            let v0 = y0 as f32 / height as f32;
            let u1 = (x0 + tile) as f32 / width as f32;
            let v1 = (y0 + tile) as f32 / height as f32;
            map.insert(key.clone(), [u0, v0, u1, v1]);
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("atlas"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("atlas layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas bg"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        Ok(Self {
            texture,
            view,
            sampler,
            size: width,
            map,
            bind_group,
            layout,
        })
    }

    pub fn uv_for(&self, key: &str, uv: [f32; 2]) -> [f32; 2] {
        if let Some([u0, v0, u1, v1]) = self.map.get(key) {
            let u = u0 + (u1 - u0) * uv[0];
            let v = v0 + (v1 - v0) * uv[1];
            [u, v]
        } else {
            uv
        }
    }
}
