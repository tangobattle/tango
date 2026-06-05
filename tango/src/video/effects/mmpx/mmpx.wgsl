// MMPX 2x magnifier — faithful WGSL port of the MMPX pixel-art algorithm
// (Morgan McGuire / Mara Gagiu). Assembled after common.wgsl (vs_main, load);
// luma + the equality helpers are defined below. Each fragment resolves the
// source pixel + 2x2 sub-quadrant it represents, runs the full rule cascade,
// and returns the matching one of j/k/l/m.
//
// Neighbourhood (e is centre); p/q/r/s are the distance-2 cross samples:
//   a b c
//   d e f
//   g h i
// Equality is exact source-pixel equality (== on bytes -> == after the
// texture's fixed sRGB decode), matching the integer algorithm.

fn luma(c: vec3<f32>) -> f32 { return dot(c, vec3<f32>(0.299, 0.587, 0.114)); }
fn eq(a: vec3<f32>, b: vec3<f32>) -> bool { return all(a == b); }
fn ne(a: vec3<f32>, b: vec3<f32>) -> bool { return any(a != b); }
fn any_eq3(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>, a2: vec3<f32>) -> bool {
    return eq(b, a0) || eq(b, a1) || eq(b, a2);
}
fn all_eq2(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>) -> bool {
    return eq(b, a0) && eq(b, a1);
}
fn all_eq3(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>, a2: vec3<f32>) -> bool {
    return eq(b, a0) && eq(b, a1) && eq(b, a2);
}
fn all_eq4(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>, a2: vec3<f32>, a3: vec3<f32>) -> bool {
    return eq(b, a0) && eq(b, a1) && eq(b, a2) && eq(b, a3);
}
fn none_eq2(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>) -> bool {
    return ne(b, a0) && ne(b, a1);
}
fn none_eq4(b: vec3<f32>, a0: vec3<f32>, a1: vec3<f32>, a2: vec3<f32>, a3: vec3<f32>) -> bool {
    return ne(b, a0) && ne(b, a1) && ne(b, a2) && ne(b, a3);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(fb_texture));
    let virt = vec2<i32>(floor(in.uv * vec2<f32>(dims) * 2.0));
    let src = virt / 2;
    let sub = virt - src * 2;
    let q = sub.y * 2 + sub.x;

    let a = load(src + vec2<i32>(-1, -1));
    let b = load(src + vec2<i32>(0, -1));
    let c = load(src + vec2<i32>(1, -1));
    let d = load(src + vec2<i32>(-1, 0));
    let e = load(src + vec2<i32>(0, 0));
    let f = load(src + vec2<i32>(1, 0));
    let g = load(src + vec2<i32>(-1, 1));
    let h = load(src + vec2<i32>(0, 1));
    let i = load(src + vec2<i32>(1, 1));
    let p = load(src + vec2<i32>(0, -2));
    let qq = load(src + vec2<i32>(-2, 0));
    let r = load(src + vec2<i32>(2, 0));
    let s = load(src + vec2<i32>(0, 2));

    let b_luma = luma(b);
    let d_luma = luma(d);
    let e_luma = luma(e);
    let f_luma = luma(f);
    let h_luma = luma(h);

    var j = e;
    var k = e;
    var l = e;
    var m = e;

    // 1:1 slope rules
    if ((eq(d, b) && ne(d, h) && ne(d, f))
        && (e_luma >= d_luma || eq(e, a))
        && any_eq3(e, a, c, g)
        && ((e_luma < d_luma) || ne(a, d) || ne(e, p) || ne(e, qq))) {
        j = d;
    }
    if ((eq(b, f) && ne(b, d) && ne(b, h))
        && (e_luma >= b_luma || eq(e, c))
        && any_eq3(e, a, c, i)
        && ((e_luma < b_luma) || ne(c, b) || ne(e, p) || ne(e, r))) {
        k = b;
    }
    if ((eq(h, d) && ne(h, f) && ne(h, b))
        && (e_luma >= h_luma || eq(e, g))
        && any_eq3(e, a, g, i)
        && ((e_luma < h_luma) || ne(g, h) || ne(e, s) || ne(e, qq))) {
        l = h;
    }
    if ((eq(f, h) && ne(f, b) && ne(f, d))
        && (e_luma >= f_luma || eq(e, i))
        && any_eq3(e, c, g, i)
        && ((e_luma < f_luma) || ne(i, h) || ne(e, r) || ne(e, s))) {
        m = f;
    }

    // Intersection rules
    if ((ne(e, f) && all_eq4(e, c, i, d, qq) && all_eq2(f, b, h))
        && ne(f, load(src + vec2<i32>(3, 0)))) {
        k = f;
        m = f;
    }
    if ((ne(e, d) && all_eq4(e, a, g, f, r) && all_eq2(d, b, h))
        && ne(d, load(src + vec2<i32>(-3, 0)))) {
        j = d;
        l = d;
    }
    if ((ne(e, h) && all_eq4(e, g, i, b, p) && all_eq2(h, d, f))
        && ne(h, load(src + vec2<i32>(0, 3)))) {
        l = h;
        m = h;
    }
    if ((ne(e, b) && all_eq4(e, a, c, h, s) && all_eq2(b, d, f))
        && ne(b, load(src + vec2<i32>(0, -3)))) {
        j = b;
        k = b;
    }

    if (b_luma < e_luma && all_eq4(e, g, h, i, s) && none_eq4(e, a, d, c, f)) {
        j = b;
        k = b;
    }
    if (h_luma < e_luma && all_eq4(e, a, b, c, p) && none_eq4(e, d, g, i, f)) {
        l = h;
        m = h;
    }
    if (f_luma < e_luma && all_eq4(e, a, d, g, qq) && none_eq4(e, b, c, i, h)) {
        k = f;
        m = f;
    }
    if (d_luma < e_luma && all_eq4(e, c, f, i, r) && none_eq4(e, b, a, g, h)) {
        j = d;
        l = d;
    }

    // 2:1 slope rules
    if (ne(h, b)) {
        if (ne(h, a) && ne(h, e) && ne(h, c)) {
            if (all_eq3(h, g, f, r) && none_eq2(h, d, load(src + vec2<i32>(2, -1)))) {
                l = m;
            }
            if (all_eq3(h, i, d, qq) && none_eq2(h, f, load(src + vec2<i32>(-2, -1)))) {
                m = l;
            }
        }
        if (ne(b, i) && ne(b, g) && ne(b, e)) {
            if (all_eq3(b, a, f, r) && none_eq2(b, d, load(src + vec2<i32>(2, 1)))) {
                j = k;
            }
            if (all_eq3(b, c, d, qq) && none_eq2(b, f, load(src + vec2<i32>(-2, 1)))) {
                k = j;
            }
        }
    }
    if (ne(f, d)) {
        if (ne(d, i) && ne(d, e) && ne(d, c)) {
            if (all_eq3(d, a, h, s) && none_eq2(d, b, load(src + vec2<i32>(1, 2)))) {
                j = l;
            }
            if (all_eq3(d, g, b, p) && none_eq2(d, h, load(src + vec2<i32>(1, -2)))) {
                l = j;
            }
        }
        if (ne(f, e) && ne(f, a) && ne(f, g)) {
            if (all_eq3(f, c, h, s) && none_eq2(f, b, load(src + vec2<i32>(-1, 2)))) {
                k = m;
            }
            if (all_eq3(f, i, b, p) && none_eq2(f, h, load(src + vec2<i32>(-1, -2)))) {
                m = k;
            }
        }
    }

    var result = j;
    if (q == 1) {
        result = k;
    } else if (q == 2) {
        result = l;
    } else if (q == 3) {
        result = m;
    }
    return vec4<f32>(result, 1.0);
}
