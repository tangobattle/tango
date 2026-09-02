// Shared infrastructure for every framebuffer effect: the fullscreen-triangle
// vertex shader, the framebuffer texture binding, and the clamped texel fetch.
// Renderer implementations prepend this module to their fragment shader; see
// `video::framebuffer::EffectRenderer`.
//
// The framebuffer arrives already expanded from mGBA's native BGR555 to RGBA8
// on the CPU (`tango_replay_renderer::bgr555_to_rgba8`, the same table every
// other framebuffer consumer uses), so no color decode happens here. The
// texture's format carries the render target's gamma (`Rgba8UnormSrgb` under
// an sRGB target, `Rgba8Unorm` under a linear one — see `Pipeline::upload`),
// so `textureLoad` already returns the working-space RGB the effects operate
// in: linear under an sRGB target (which re-encodes on write), the encoded
// value unchanged under a linear (web-colors) target.
//
// Effects sample the *native* (240x160) framebuffer and magnify it in the
// fragment shader, so the uploaded texture is the same for every effect and
// only the pipeline (this prelude + a fragment) changes. WGSL allows
// module-scope declarations in any order, so a fragment may use anything here.

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// Fullscreen triangle synthesised from the vertex index (no vertex buffer).
// UV origin is top-left so texture row 0 renders at the top.
@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var out: VsOut;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

// One RGBA8 pixel per texel, CPU-expanded; alpha is always opaque.
@group(0) @binding(0) var fb_texture: texture_2d<f32>;

// Clamped texel fetch. Clamp-to-edge reproduces the CPU upscalers'
// edge replication exactly (a clamped read equals copying the edge neighbour).
fn load(p: vec2<i32>) -> vec3<f32> {
    let hi = vec2<i32>(textureDimensions(fb_texture)) - vec2<i32>(1, 1);
    return textureLoad(fb_texture, clamp(p, vec2<i32>(0, 0), hi), 0).rgb;
}
