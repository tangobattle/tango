// Nearest pass-through (the "—" / no-filter default). Samples the framebuffer
// texture through `fb_sampler` (nearest, clamp-to-edge) and forces alpha to
// 1.0 — the GBA framebuffer is always opaque. RGB is unaffected by the stored
// alpha because the texture holds straight (non-premultiplied) values.

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(fb_texture, fb_sampler, in.uv).rgb, 1.0);
}
