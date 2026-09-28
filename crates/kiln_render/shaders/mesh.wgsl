// Forward shader for M0: base color (factor * texture * vertex color), Lambert diffuse from one
// directional light plus ambient. Outputs linear color; the sRGB render target encodes it.
//
// Conventions: world space is right-handed Y-up; clip space is Y-up with depth in [0, 1]
// (reverse-Z). The Vulkan backend flips Y with a negative viewport height.

struct Frame {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
    // xyz: direction the light travels (world space). w: unused.
    light_dir: vec4<f32>,
    // rgb: color * intensity. a: unused.
    light_color: vec4<f32>,
    ambient: vec4<f32>,
}

struct Object {
    model: mat4x4<f32>,
    // Inverse-transpose of the model matrix's upper 3x3, padded to 4x4.
    normal: mat4x4<f32>,
}

struct Material {
    base_color: vec4<f32>,
    // x: 1.0 for unlit.
    flags: vec4<f32>,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var<uniform> object: Object;
@group(1) @binding(0) var base_texture: texture_2d<f32>;
@group(1) @binding(1) var base_sampler: sampler;
@group(1) @binding(2) var<uniform> material: Material;

struct VertexIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct VertexOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world_normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}

@vertex
fn vs_main(in: VertexIn) -> VertexOut {
    var out: VertexOut;
    let world = object.model * vec4<f32>(in.position, 1.0);
    out.clip = frame.view_proj * world;
    out.world_normal = (object.normal * vec4<f32>(in.normal, 0.0)).xyz;
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOut, @builtin(front_facing) front_facing: bool) -> @location(0) vec4<f32> {
    let texel = textureSample(base_texture, base_sampler, in.uv);
    let albedo = material.base_color * in.color * texel;
    if material.flags.x > 0.5 {
        return albedo;
    }
    var n = normalize(in.world_normal);
    if !front_facing {
        n = -n; // double-sided materials light their back faces correctly
    }
    let to_light = normalize(-frame.light_dir.xyz);
    let diffuse = max(dot(n, to_light), 0.0) * frame.light_color.rgb;
    return vec4<f32>(albedo.rgb * (frame.ambient.rgb + diffuse), albedo.a);
}
