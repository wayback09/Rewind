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
    // M12: packed light (sky | block << 8), 0xFFFFFFFF = missing.
    @location(5) light: u32,
};

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tint: u32,
    @location(3) light: u32,
};

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    out.clip = camera.view_proj * vec4<f32>(in.position, 1.0);
    out.uv = in.uv;
    out.normal = in.normal;
    out.tint = in.tint;
    out.light = in.light;
    return out;
}

// M12 TEMPORARY FALLBACK: smooth brightness ramp approximating vanilla's
// LightTexture dimension tables (0 -> near black, 15 -> full). Replace with
// decoded dimension ramps when available.
fn light_ramp(l: u32) -> f32 {
    let x = (f32(l) + 1.0) / 16.0;
    return pow(x, 1.5);
}

// M12: vanilla per-face diffuse (top 1.0, bottom 0.5, z sides 0.8, x sides 0.6).
fn face_shade(n: vec3<f32>) -> f32 {
    if (n.y > 0.5) { return 1.0; }
    if (n.y < -0.5) { return 0.5; }
    if (abs(n.x) > 0.5) { return 0.6; }
    return 0.8;
}

@fragment
fn fs_main(in: VertexOut) -> @location(0) vec4<f32> {
    var tex = textureSample(atlas_tex, atlas_sampler, in.uv);
    // Discard transparent texels for cutout (alpha < 0.5)
    if (tex.a < 0.1) {
        discard;
    }
    // M10 TEMPORARY FALLBACK: tinted overlay faces (grass/leaves) are multiplied
    // by the fallback foliage color until biome tints are decoded.
    if (in.tint == 1u) {
        tex = vec4<f32>(tex.rgb * camera.tint_color.rgb, tex.a);
    }
    var brightness: f32;
    var warmth = vec3<f32>(1.0);
    if (in.light == 0xFFFFFFFFu) {
        // M12: no decoded light here (entities, unknown chunks) — keep the
        // legacy directional sun exactly as before.
        let sun = normalize(vec3<f32>(0.3, 1.0, 0.2));
        brightness = max(dot(in.normal, sun), 0.0) * 0.4 + 0.6;
    } else {
        let sky = in.light & 0xFFu;
        let blk = (in.light >> 8u) & 0xFFu;
        let sb = light_ramp(sky);
        let bb = light_ramp(blk);
        brightness = max(max(sb, bb) * face_shade(in.normal), 0.015);
        // Cheap daylight/torch split: shift toward warm where block light
        // dominates the sky contribution.
        let dom = clamp((bb - sb) * 2.0, 0.0, 1.0);
        warmth = mix(vec3<f32>(1.0), vec3<f32>(1.0, 0.82, 0.62), dom);
    }
    return vec4<f32>(tex.rgb * brightness * warmth, tex.a);
}
