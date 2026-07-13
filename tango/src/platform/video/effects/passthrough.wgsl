// Nearest pass-through (the "—" / no-filter default). Reads the nearest native
// texel — `floor(uv * dims)`, the same rule the other effects use, which
// reproduces the old nearest-clamp sampler — and forces alpha to 1.0, since the
// GBA framebuffer is always opaque.

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(fb_texture));
    return vec4<f32>(load(vec2<i32>(floor(in.uv * dims))), 1.0);
}
