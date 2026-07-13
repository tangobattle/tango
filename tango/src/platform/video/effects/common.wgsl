// Shared infrastructure for every framebuffer effect: the fullscreen-triangle
// vertex shader, the framebuffer texture binding, the BGR555 decode, and the
// clamped texel fetch. Effect modules are assembled as an `SRGB_TARGET` const
// (injected by `Effect::source` from the render target's gamma) + `common.wgsl`
// (+ `hqx_common.wgsl` for the hqx family) + one fragment shader; see
// `video::framebuffer::Effect`.
//
// The framebuffer texture holds mGBA's native BGR555 — one little-endian u16
// per pixel, the GBA's 15-bit color — uploaded raw (2 bytes/pixel) and decoded
// to RGB here in the shader. This halves the per-frame upload versus expanding
// to RGBA8 on the CPU first and moves the expansion onto the GPU; the CPU
// `bgr555_to_rgba8` now only feeds the offline replay export.
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

// One little-endian BGR555 u16 per pixel; `textureLoad` returns it in `.r`.
@group(0) @binding(0) var fb_texture: texture_2d<u32>;

// Standard sRGB EOTF (8-bit-encoded value -> linear) — the inverse of what a
// `*Srgb` render target applies on write.
fn srgb_to_linear(c: f32) -> f32 {
    if (c <= 0.04045) {
        return c / 12.92;
    }
    return pow((c + 0.055) / 1.055, 2.4);
}

// Decode one raw BGR555 texel to the RGB working space the effects operate in.
//
// Each 5-bit channel expands to 8-bit exactly as the CPU LUT did
// (`c * 255 / 31`, truncating); the result is an 8-bit *sRGB-encoded* color
// (the GBA's native palette). When the render target is sRGB it applies
// linear->sRGB on write, so we hand it linear here — matching what sampling the
// old `Rgba8UnormSrgb` framebuffer texture used to return. When the target is
// linear (web-colors) we hand back the encoded value unchanged. Both reproduce
// the old image path's sampled values: the linear case exactly (integer unorm
// round-trip), the sRGB case to fp precision, which the final 8-bit
// requantization snaps back.
fn decode(raw: u32) -> vec3<f32> {
    let r = (raw & 0x1fu) * 255u / 31u;
    let g = ((raw >> 5u) & 0x1fu) * 255u / 31u;
    let b = ((raw >> 10u) & 0x1fu) * 255u / 31u;
    var c = vec3<f32>(f32(r), f32(g), f32(b)) / 255.0;
    if (SRGB_TARGET) {
        c = vec3<f32>(srgb_to_linear(c.r), srgb_to_linear(c.g), srgb_to_linear(c.b));
    }
    return c;
}

// Clamped integer texel fetch. Clamp-to-edge reproduces the CPU upscalers'
// edge replication exactly (a clamped read equals copying the edge neighbour).
fn load(p: vec2<i32>) -> vec3<f32> {
    let hi = vec2<i32>(textureDimensions(fb_texture)) - vec2<i32>(1, 1);
    let raw = textureLoad(fb_texture, clamp(p, vec2<i32>(0, 0), hi), 0).r;
    return decode(raw);
}
