// Compact hqx evaluator. The per-scale programs are uploaded from packed Rust
// data as an R32Uint lookup texture. Keeping that data out of the shader's
// control-flow graph avoids asking the platform shader compiler to optimize
// thousands of assignments.

// Rec.601 YUV distance with thresholds 48/7/6 on the 0..255 scale the CPU
// implementation uses, expressed here in normalised 0..1 units. The constant
// +128 U/V offsets cancel in the difference, so they are omitted.
fn yuv_diff(a: vec3<f32>, b: vec3<f32>) -> bool {
    let d = a - b;
    let y = dot(d, vec3<f32>(0.299, 0.587, 0.114));
    let u = dot(d, vec3<f32>(-0.169, -0.331, 0.5));
    let v = dot(d, vec3<f32>(0.5, -0.419, -0.081));
    return abs(y) > 48.0 / 255.0 || abs(u) > 7.0 / 255.0 || abs(v) > 6.0 / 255.0;
}

fn diff(a: vec3<f32>, b: vec3<f32>) -> bool {
    return yuv_diff(a, b);
}

// Interpolation rules, performed per channel. The framebuffer is opaque, so
// alpha is supplied only once at the fragment output.
fn interp1(c1: vec3<f32>, c2: vec3<f32>) -> vec3<f32> { return (c1 * 3.0 + c2) / 4.0; }
fn interp2(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 2.0 + c2 + c3) / 4.0; }
fn interp3(c1: vec3<f32>, c2: vec3<f32>) -> vec3<f32> { return (c1 * 7.0 + c2) / 8.0; }
fn interp4(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 2.0 + (c2 + c3) * 7.0) / 16.0; }
fn interp5(c1: vec3<f32>, c2: vec3<f32>) -> vec3<f32> { return (c1 + c2) / 2.0; }
fn interp6(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 5.0 + c2 * 2.0 + c3) / 8.0; }
fn interp7(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 6.0 + c2 + c3) / 8.0; }
fn interp8(c1: vec3<f32>, c2: vec3<f32>) -> vec3<f32> { return (c1 * 5.0 + c2 * 3.0) / 8.0; }
fn interp9(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 2.0 + (c2 + c3) * 3.0) / 8.0; }
fn interp10(c1: vec3<f32>, c2: vec3<f32>, c3: vec3<f32>) -> vec3<f32> { return (c1 * 14.0 + c2 + c3) / 16.0; }

@group(0) @binding(1) var hqx_table: texture_2d<u32>;

// One small shader serves all three scales. wgpu specializes this override
// while creating each effect's pipeline.
override SCALE: i32 = 2;

fn apply_recipe(recipe: u32, w: ptr<function, array<vec3<f32>, 10>>) -> vec3<f32> {
    let operation = recipe & 0xfu;
    let ai = (recipe >> 4u) & 0xfu;
    let bi = (recipe >> 8u) & 0xfu;
    let ci = (recipe >> 12u) & 0xfu;

    switch operation {
        case 0u: { return (*w)[ai]; }
        case 1u: { return interp1((*w)[ai], (*w)[bi]); }
        case 2u: { return interp2((*w)[ai], (*w)[bi], (*w)[ci]); }
        case 3u: { return interp3((*w)[ai], (*w)[bi]); }
        case 4u: { return interp4((*w)[ai], (*w)[bi], (*w)[ci]); }
        case 5u: { return interp5((*w)[ai], (*w)[bi]); }
        case 6u: { return interp6((*w)[ai], (*w)[bi], (*w)[ci]); }
        case 7u: { return interp7((*w)[ai], (*w)[bi], (*w)[ci]); }
        case 8u: { return interp8((*w)[ai], (*w)[bi]); }
        case 9u: { return interp9((*w)[ai], (*w)[bi], (*w)[ci]); }
        case 10u: { return interp10((*w)[ai], (*w)[bi], (*w)[ci]); }
        default: { return (*w)[5]; }
    }
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(fb_texture));
    let virt = vec2<i32>(floor(in.uv * vec2<f32>(dims) * f32(SCALE)));
    let src = virt / SCALE;
    let sub = virt - src * SCALE;
    let q = sub.y * SCALE + sub.x;

    var w: array<vec3<f32>, 10>;
    w[1] = load(src + vec2<i32>(-1, -1));
    w[2] = load(src + vec2<i32>(0, -1));
    w[3] = load(src + vec2<i32>(1, -1));
    w[4] = load(src + vec2<i32>(-1, 0));
    w[5] = load(src + vec2<i32>(0, 0));
    w[6] = load(src + vec2<i32>(1, 0));
    w[7] = load(src + vec2<i32>(-1, 1));
    w[8] = load(src + vec2<i32>(0, 1));
    w[9] = load(src + vec2<i32>(1, 1));

    var pattern = 0u;
    if (yuv_diff(w[5], w[1])) { pattern |= 1u; }
    if (yuv_diff(w[5], w[2])) { pattern |= 2u; }
    if (yuv_diff(w[5], w[3])) { pattern |= 4u; }
    if (yuv_diff(w[5], w[4])) { pattern |= 8u; }
    if (yuv_diff(w[5], w[6])) { pattern |= 16u; }
    if (yuv_diff(w[5], w[7])) { pattern |= 32u; }
    if (yuv_diff(w[5], w[8])) { pattern |= 64u; }
    if (yuv_diff(w[5], w[9])) { pattern |= 128u; }

    // The first SCALE² rows hold one rule per output subpixel. Each rule
    // names the expression to use when its optional edge test is false/true;
    // the following row is the expression dictionary.
    let rule = textureLoad(hqx_table, vec2<i32>(i32(pattern), q), 0).x;
    let condition = (rule >> 16u) & 0x7u;
    var is_different = false;
    switch condition {
        case 1u: { is_different = diff(w[4], w[2]); }
        case 2u: { is_different = diff(w[2], w[6]); }
        case 3u: { is_different = diff(w[8], w[4]); }
        case 4u: { is_different = diff(w[6], w[8]); }
        default: {}
    }

    let shift = select(0u, 8u, is_different);
    let expression = (rule >> shift) & 0xffu;
    let recipe = textureLoad(hqx_table, vec2<i32>(i32(expression), SCALE * SCALE), 0).x;
    return vec4<f32>(apply_recipe(recipe, &w), 1.0);
}
