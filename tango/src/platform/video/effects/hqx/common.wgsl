// hqx-family prelude: the YUV edge metric and the interpolation rules shared
// by hq2x/hq3x/hq4x. Appended after common.wgsl, before the generated hqx
// fragment. See hqx/src/common.rs for the reference (integer) versions.

// Rec.601 YUV distance with thresholds 48/7/6 on the 0..255 scale the CPU
// `yuv_diff` uses, expressed here in normalised 0..1 units. The constant
// +128 U/V offsets cancel in the difference, so they're dropped.
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

// Interpolation rules, done per-channel in float; alpha is irrelevant (the
// framebuffer is opaque, output forces alpha = 1).
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
