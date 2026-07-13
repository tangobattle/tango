// hq4x — faithful WGSL port of the hqx pixel-art upscaler: a 256-entry pattern
// dispatch over a YUV-thresholded 3x3 neighbourhood (per Maxim Stepin's hqx).
// Assembled after common.wgsl (vs_main, load) and hqx_common.wgsl (yuv_diff/
// diff, interp1..10); see video::framebuffer::Effect.

const SCALE_I: i32 = 4;
const SCALE_F: f32 = 4.0;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(fb_texture));
    let virt = vec2<i32>(floor(in.uv * vec2<f32>(dims) * SCALE_F));
    let src = virt / SCALE_I;
    let sub = virt - src * SCALE_I;
    let q = sub.y * SCALE_I + sub.x;

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

    var out: array<vec3<f32>, 16>;
    switch pattern {
        case 0u, 1u, 4u, 32u, 128u, 5u, 132u, 160u, 33u, 129u, 36u, 133u, 164u, 161u, 37u, 165u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 2u, 34u, 130u, 162u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 16u, 17u, 48u, 49u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 64u, 65u, 68u, 69u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 8u, 12u, 136u, 140u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 3u, 35u, 131u, 163u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 6u, 38u, 134u, 166u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 20u, 21u, 52u, 53u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 144u, 145u, 176u, 177u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 192u, 193u, 196u, 197u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 96u, 97u, 100u, 101u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 40u, 44u, 168u, 172u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 9u, 13u, 137u, 141u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 18u, 50u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[6] = w[5];
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 80u, 81u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 72u, 76u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp5(w[4], w[5]);
                out[9] = w[5];
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 10u, 138u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
                out[5] = w[5];
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 66u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 24u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 7u, 39u, 135u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 148u, 149u, 180u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 224u, 228u, 225u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 41u, 169u, 45u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 22u, 54u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 208u, 209u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 104u, 108u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 11u, 139u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 19u, 51u: {
            if (diff(w[2], w[6])) {
                out[0] = interp8(w[5], w[4]);
                out[1] = interp3(w[5], w[4]);
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[0] = interp1(w[5], w[2]);
                out[1] = interp1(w[2], w[5]);
                out[2] = interp8(w[2], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp2(w[6], w[5], w[2]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 146u, 178u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
                out[11] = interp3(w[5], w[8]);
                out[15] = interp8(w[5], w[8]);
            } else {
                out[2] = interp2(w[2], w[5], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp8(w[6], w[2]);
                out[11] = interp1(w[6], w[5]);
                out[15] = interp1(w[5], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
        }
        case 84u, 85u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            if (diff(w[6], w[8])) {
                out[3] = interp8(w[5], w[2]);
                out[7] = interp3(w[5], w[2]);
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[3] = interp1(w[5], w[6]);
                out[7] = interp1(w[6], w[5]);
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp8(w[6], w[8]);
                out[14] = interp2(w[8], w[5], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 112u, 113u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[12] = interp8(w[5], w[4]);
                out[13] = interp3(w[5], w[4]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp2(w[6], w[5], w[8]);
                out[12] = interp1(w[5], w[8]);
                out[13] = interp1(w[8], w[5]);
                out[14] = interp8(w[8], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 200u, 204u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
                out[14] = interp3(w[5], w[6]);
                out[15] = interp8(w[5], w[6]);
            } else {
                out[8] = interp2(w[4], w[5], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp8(w[8], w[4]);
                out[14] = interp1(w[8], w[5]);
                out[15] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
        }
        case 73u, 77u: {
            if (diff(w[8], w[4])) {
                out[0] = interp8(w[5], w[2]);
                out[4] = interp3(w[5], w[2]);
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[0] = interp1(w[5], w[4]);
                out[4] = interp1(w[4], w[5]);
                out[8] = interp8(w[4], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp2(w[8], w[5], w[4]);
            }
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 42u, 170u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
                out[8] = interp3(w[5], w[8]);
                out[12] = interp8(w[5], w[8]);
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp2(w[2], w[5], w[4]);
                out[4] = interp8(w[4], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
                out[8] = interp1(w[4], w[5]);
                out[12] = interp1(w[5], w[4]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 14u, 142u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[2] = interp3(w[5], w[6]);
                out[3] = interp8(w[5], w[6]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp8(w[2], w[4]);
                out[2] = interp1(w[2], w[5]);
                out[3] = interp1(w[5], w[2]);
                out[4] = interp2(w[4], w[5], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
            }
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 67u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 70u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 28u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 152u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 194u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 98u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 56u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 25u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 26u, 31u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[5] = w[5];
            out[6] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 82u, 214u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 88u, 248u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 74u, 107u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 27u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 86u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 216u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 106u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 30u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 210u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 120u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 75u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 29u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 198u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 184u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 99u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 57u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 71u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 156u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 226u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 60u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 195u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 102u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 153u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 58u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 83u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 92u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 202u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 78u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 154u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 114u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
        }
        case 89u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 90u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 55u, 23u: {
            if (diff(w[2], w[6])) {
                out[0] = interp8(w[5], w[4]);
                out[1] = interp3(w[5], w[4]);
                out[2] = w[5];
                out[3] = w[5];
                out[6] = w[5];
                out[7] = w[5];
            } else {
                out[0] = interp1(w[5], w[2]);
                out[1] = interp1(w[2], w[5]);
                out[2] = interp8(w[2], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp2(w[6], w[5], w[2]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 182u, 150u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[6] = w[5];
                out[7] = w[5];
                out[11] = interp3(w[5], w[8]);
                out[15] = interp8(w[5], w[8]);
            } else {
                out[2] = interp2(w[2], w[5], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp8(w[6], w[2]);
                out[11] = interp1(w[6], w[5]);
                out[15] = interp1(w[5], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
        }
        case 213u, 212u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            if (diff(w[6], w[8])) {
                out[3] = interp8(w[5], w[2]);
                out[7] = interp3(w[5], w[2]);
                out[10] = w[5];
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[3] = interp1(w[5], w[6]);
                out[7] = interp1(w[6], w[5]);
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp8(w[6], w[8]);
                out[14] = interp2(w[8], w[5], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 241u, 240u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = w[5];
                out[11] = w[5];
                out[12] = interp8(w[5], w[4]);
                out[13] = interp3(w[5], w[4]);
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp2(w[6], w[5], w[8]);
                out[12] = interp1(w[5], w[8]);
                out[13] = interp1(w[8], w[5]);
                out[14] = interp8(w[8], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 236u, 232u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[9] = w[5];
                out[12] = w[5];
                out[13] = w[5];
                out[14] = interp3(w[5], w[6]);
                out[15] = interp8(w[5], w[6]);
            } else {
                out[8] = interp2(w[4], w[5], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp8(w[8], w[4]);
                out[14] = interp1(w[8], w[5]);
                out[15] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
        }
        case 109u, 105u: {
            if (diff(w[8], w[4])) {
                out[0] = interp8(w[5], w[2]);
                out[4] = interp3(w[5], w[2]);
                out[8] = w[5];
                out[9] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[0] = interp1(w[5], w[4]);
                out[4] = interp1(w[4], w[5]);
                out[8] = interp8(w[4], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp2(w[8], w[5], w[4]);
            }
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 171u, 43u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
                out[5] = w[5];
                out[8] = interp3(w[5], w[8]);
                out[12] = interp8(w[5], w[8]);
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp2(w[2], w[5], w[4]);
                out[4] = interp8(w[4], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
                out[8] = interp1(w[4], w[5]);
                out[12] = interp1(w[5], w[4]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 143u, 15u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[2] = interp3(w[5], w[6]);
                out[3] = interp8(w[5], w[6]);
                out[4] = w[5];
                out[5] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp8(w[2], w[4]);
                out[2] = interp1(w[2], w[5]);
                out[3] = interp1(w[5], w[2]);
                out[4] = interp2(w[4], w[5], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
            }
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 124u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 203u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 62u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 211u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 118u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 217u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 110u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 155u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 188u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 185u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 61u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 157u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 103u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 227u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 230u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 199u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 220u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 158u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[6] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 234u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 242u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
        }
        case 59u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[5] = w[5];
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 121u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 87u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = w[5];
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 79u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 122u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 94u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[6] = w[5];
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 218u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 91u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[5] = w[5];
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 229u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 167u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 173u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 181u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 186u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 115u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
        }
        case 93u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 206u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 205u, 201u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = interp1(w[5], w[7]);
                out[9] = interp3(w[5], w[7]);
                out[12] = interp8(w[5], w[7]);
                out[13] = interp1(w[5], w[7]);
            } else {
                out[8] = interp1(w[5], w[4]);
                out[9] = w[5];
                out[12] = interp2(w[5], w[8], w[4]);
                out[13] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 174u, 46u: {
            if (diff(w[4], w[2])) {
                out[0] = interp8(w[5], w[1]);
                out[1] = interp1(w[5], w[1]);
                out[4] = interp1(w[5], w[1]);
                out[5] = interp3(w[5], w[1]);
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
                out[1] = interp1(w[5], w[2]);
                out[4] = interp1(w[5], w[4]);
                out[5] = w[5];
            }
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 179u, 147u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            if (diff(w[2], w[6])) {
                out[2] = interp1(w[5], w[3]);
                out[3] = interp8(w[5], w[3]);
                out[6] = interp3(w[5], w[3]);
                out[7] = interp1(w[5], w[3]);
            } else {
                out[2] = interp1(w[5], w[2]);
                out[3] = interp2(w[5], w[2], w[6]);
                out[6] = w[5];
                out[7] = interp1(w[5], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 117u, 116u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = interp3(w[5], w[9]);
                out[11] = interp1(w[5], w[9]);
                out[14] = interp1(w[5], w[9]);
                out[15] = interp8(w[5], w[9]);
            } else {
                out[10] = w[5];
                out[11] = interp1(w[5], w[6]);
                out[14] = interp1(w[5], w[8]);
                out[15] = interp2(w[5], w[8], w[6]);
            }
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
        }
        case 189u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 231u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 126u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 219u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 125u: {
            if (diff(w[8], w[4])) {
                out[0] = interp8(w[5], w[2]);
                out[4] = interp3(w[5], w[2]);
                out[8] = w[5];
                out[9] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[0] = interp1(w[5], w[4]);
                out[4] = interp1(w[4], w[5]);
                out[8] = interp8(w[4], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp2(w[8], w[5], w[4]);
            }
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 221u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            if (diff(w[6], w[8])) {
                out[3] = interp8(w[5], w[2]);
                out[7] = interp3(w[5], w[2]);
                out[10] = w[5];
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[3] = interp1(w[5], w[6]);
                out[7] = interp1(w[6], w[5]);
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp8(w[6], w[8]);
                out[14] = interp2(w[8], w[5], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 207u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[2] = interp3(w[5], w[6]);
                out[3] = interp8(w[5], w[6]);
                out[4] = w[5];
                out[5] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp8(w[2], w[4]);
                out[2] = interp1(w[2], w[5]);
                out[3] = interp1(w[5], w[2]);
                out[4] = interp2(w[4], w[5], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
            }
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 238u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[9] = w[5];
                out[12] = w[5];
                out[13] = w[5];
                out[14] = interp3(w[5], w[6]);
                out[15] = interp8(w[5], w[6]);
            } else {
                out[8] = interp2(w[4], w[5], w[8]);
                out[9] = interp7(w[5], w[4], w[8]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp8(w[8], w[4]);
                out[14] = interp1(w[8], w[5]);
                out[15] = interp1(w[5], w[8]);
            }
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
        }
        case 190u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[6] = w[5];
                out[7] = w[5];
                out[11] = interp3(w[5], w[8]);
                out[15] = interp8(w[5], w[8]);
            } else {
                out[2] = interp2(w[2], w[5], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp8(w[6], w[2]);
                out[11] = interp1(w[6], w[5]);
                out[15] = interp1(w[5], w[6]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
        }
        case 187u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
                out[5] = w[5];
                out[8] = interp3(w[5], w[8]);
                out[12] = interp8(w[5], w[8]);
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp2(w[2], w[5], w[4]);
                out[4] = interp8(w[4], w[2]);
                out[5] = interp7(w[5], w[4], w[2]);
                out[8] = interp1(w[4], w[5]);
                out[12] = interp1(w[5], w[4]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 243u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            if (diff(w[6], w[8])) {
                out[10] = w[5];
                out[11] = w[5];
                out[12] = interp8(w[5], w[4]);
                out[13] = interp3(w[5], w[4]);
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[10] = interp7(w[5], w[6], w[8]);
                out[11] = interp2(w[6], w[5], w[8]);
                out[12] = interp1(w[5], w[8]);
                out[13] = interp1(w[8], w[5]);
                out[14] = interp8(w[8], w[6]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 119u: {
            if (diff(w[2], w[6])) {
                out[0] = interp8(w[5], w[4]);
                out[1] = interp3(w[5], w[4]);
                out[2] = w[5];
                out[3] = w[5];
                out[6] = w[5];
                out[7] = w[5];
            } else {
                out[0] = interp1(w[5], w[2]);
                out[1] = interp1(w[2], w[5]);
                out[2] = interp8(w[2], w[6]);
                out[3] = interp5(w[2], w[6]);
                out[6] = interp7(w[5], w[6], w[2]);
                out[7] = interp2(w[6], w[5], w[2]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 237u, 233u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[6]);
            out[3] = interp2(w[5], w[2], w[6]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp7(w[5], w[6], w[2]);
            out[7] = interp6(w[5], w[6], w[2]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 175u, 47u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = w[5];
            out[5] = w[5];
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp7(w[5], w[6], w[8]);
            out[11] = interp6(w[5], w[6], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[6]);
            out[15] = interp2(w[5], w[8], w[6]);
        }
        case 183u, 151u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp6(w[5], w[4], w[8]);
            out[9] = interp7(w[5], w[4], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp2(w[5], w[8], w[4]);
            out[13] = interp6(w[5], w[8], w[4]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 245u, 244u: {
            out[0] = interp2(w[5], w[2], w[4]);
            out[1] = interp6(w[5], w[2], w[4]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp6(w[5], w[4], w[2]);
            out[5] = interp7(w[5], w[4], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = w[5];
            out[11] = w[5];
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 250u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
        }
        case 123u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 95u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[5] = w[5];
            out[6] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 222u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 252u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp6(w[5], w[2], w[1]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = w[5];
            out[11] = w[5];
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 249u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp6(w[5], w[2], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
        }
        case 235u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp6(w[5], w[6], w[3]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 111u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = w[5];
            out[5] = w[5];
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp6(w[5], w[6], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 63u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = w[5];
            out[5] = w[5];
            out[6] = w[5];
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp6(w[5], w[8], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 159u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[5] = w[5];
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[7]);
            out[13] = interp6(w[5], w[8], w[7]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 215u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp6(w[5], w[4], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 246u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp6(w[5], w[4], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = w[5];
            out[11] = w[5];
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 254u: {
            out[0] = interp8(w[5], w[1]);
            out[1] = interp1(w[5], w[1]);
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = interp1(w[5], w[1]);
            out[5] = interp3(w[5], w[1]);
            out[6] = w[5];
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = w[5];
            out[11] = w[5];
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 253u: {
            out[0] = interp8(w[5], w[2]);
            out[1] = interp8(w[5], w[2]);
            out[2] = interp8(w[5], w[2]);
            out[3] = interp8(w[5], w[2]);
            out[4] = interp3(w[5], w[2]);
            out[5] = interp3(w[5], w[2]);
            out[6] = interp3(w[5], w[2]);
            out[7] = interp3(w[5], w[2]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = w[5];
            out[11] = w[5];
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 251u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = interp1(w[5], w[3]);
            out[3] = interp8(w[5], w[3]);
            out[5] = w[5];
            out[6] = interp3(w[5], w[3]);
            out[7] = interp1(w[5], w[3]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
        }
        case 239u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            out[2] = interp3(w[5], w[6]);
            out[3] = interp8(w[5], w[6]);
            out[4] = w[5];
            out[5] = w[5];
            out[6] = interp3(w[5], w[6]);
            out[7] = interp8(w[5], w[6]);
            out[8] = w[5];
            out[9] = w[5];
            out[10] = interp3(w[5], w[6]);
            out[11] = interp8(w[5], w[6]);
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
            out[14] = interp3(w[5], w[6]);
            out[15] = interp8(w[5], w[6]);
        }
        case 127u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            if (diff(w[2], w[6])) {
                out[2] = w[5];
                out[3] = w[5];
                out[7] = w[5];
            } else {
                out[2] = interp5(w[2], w[5]);
                out[3] = interp5(w[2], w[6]);
                out[7] = interp5(w[6], w[5]);
            }
            out[4] = w[5];
            out[5] = w[5];
            out[6] = w[5];
            if (diff(w[8], w[4])) {
                out[8] = w[5];
                out[12] = w[5];
                out[13] = w[5];
            } else {
                out[8] = interp5(w[4], w[5]);
                out[12] = interp5(w[8], w[4]);
                out[13] = interp5(w[8], w[5]);
            }
            out[9] = w[5];
            out[10] = interp3(w[5], w[9]);
            out[11] = interp1(w[5], w[9]);
            out[14] = interp1(w[5], w[9]);
            out[15] = interp8(w[5], w[9]);
        }
        case 191u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[4] = w[5];
            out[5] = w[5];
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp3(w[5], w[8]);
            out[9] = interp3(w[5], w[8]);
            out[10] = interp3(w[5], w[8]);
            out[11] = interp3(w[5], w[8]);
            out[12] = interp8(w[5], w[8]);
            out[13] = interp8(w[5], w[8]);
            out[14] = interp8(w[5], w[8]);
            out[15] = interp8(w[5], w[8]);
        }
        case 223u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
                out[1] = w[5];
                out[4] = w[5];
            } else {
                out[0] = interp5(w[2], w[4]);
                out[1] = interp5(w[2], w[5]);
                out[4] = interp5(w[4], w[5]);
            }
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[5] = w[5];
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp1(w[5], w[7]);
            out[9] = interp3(w[5], w[7]);
            out[10] = w[5];
            if (diff(w[6], w[8])) {
                out[11] = w[5];
                out[14] = w[5];
                out[15] = w[5];
            } else {
                out[11] = interp5(w[6], w[5]);
                out[14] = interp5(w[8], w[5]);
                out[15] = interp5(w[8], w[6]);
            }
            out[12] = interp8(w[5], w[7]);
            out[13] = interp1(w[5], w[7]);
        }
        case 247u: {
            out[0] = interp8(w[5], w[4]);
            out[1] = interp3(w[5], w[4]);
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[4] = interp8(w[5], w[4]);
            out[5] = interp3(w[5], w[4]);
            out[6] = w[5];
            out[7] = w[5];
            out[8] = interp8(w[5], w[4]);
            out[9] = interp3(w[5], w[4]);
            out[10] = w[5];
            out[11] = w[5];
            out[12] = interp8(w[5], w[4]);
            out[13] = interp3(w[5], w[4]);
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        case 255u: {
            if (diff(w[4], w[2])) {
                out[0] = w[5];
            } else {
                out[0] = interp2(w[5], w[2], w[4]);
            }
            out[1] = w[5];
            out[2] = w[5];
            if (diff(w[2], w[6])) {
                out[3] = w[5];
            } else {
                out[3] = interp2(w[5], w[2], w[6]);
            }
            out[4] = w[5];
            out[5] = w[5];
            out[6] = w[5];
            out[7] = w[5];
            out[8] = w[5];
            out[9] = w[5];
            out[10] = w[5];
            out[11] = w[5];
            if (diff(w[8], w[4])) {
                out[12] = w[5];
            } else {
                out[12] = interp2(w[5], w[8], w[4]);
            }
            out[13] = w[5];
            out[14] = w[5];
            if (diff(w[6], w[8])) {
                out[15] = w[5];
            } else {
                out[15] = interp2(w[5], w[8], w[6]);
            }
        }
        default: { return vec4<f32>(w[5], 1.0); }
    }
    return vec4<f32>(out[q], 1.0);
}
