struct Camera {
    view_proj: mat4x4<f32>,
    // M10 TEMPORARY FALLBACK: plains foliage color until biome tints are decoded.
    tint_color: vec4<f32>,
};
@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var atlas_tex: texture_2d<f32>;
@group(1) @binding(1) var atlas_sampler: sampler;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) tex_index: u32,
    @location(4) tint: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tint: u32,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.normal = in.normal;
    out.tint = in.tint;
    return out;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var tex = textureSample(atlas_tex, atlas_sampler, in.uv);
    // Simple directional light fallback (sun from +Y)
    let sun = normalize(vec3<f32>(0.3, 1.0, 0.2));
    let diff = max(dot(in.normal, sun), 0.0) * 0.4 + 0.6;
    // Discard transparent texels for cutout (alpha < 0.5)
    if (tex.a < 0.1) {
        discard;
    }
    // M10 TEMPORARY FALLBACK: tinted overlay faces (grass/leaves) are multiplied
    // by the fallback foliage color until biome tints are decoded.
    if (in.tint == 1u) {
        tex = vec4<f32>(tex.rgb * camera.tint_color.rgb, tex.a);
    }
    return vec4<f32>(tex.rgb * diff, tex.a);
}
