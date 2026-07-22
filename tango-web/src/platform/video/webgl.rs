//! The WebGL2 framebuffer presenter: one persistent 240x160 R16UI
//! texture holding mGBA's raw little-endian BGR555, decoded to RGB in
//! the fragment shader — a GLSL ES 3.00 port of the wgpu effect
//! pipeline (`platform/video/effects/` on the desktop), on the
//! non-sRGB-target path (the canvas default drawing buffer).
//!
//! Effects are uv-relative fragment magnifiers over the same native
//! texture, so the canvas backing store can be any size; the ported
//! set is the pass-through, MMPX, and the LCD grid (the hqx family's
//! generated shaders await an offline WGSL→GLSL transpile).

use wasm_bindgen::JsCast;
use web_sys::{WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader, WebGlTexture};

pub const SCREEN_WIDTH: i32 = 240;
pub const SCREEN_HEIGHT: i32 = 160;

/// The video-filter registry, in pick-list order: `config.video_filter`
/// key → display name. Keys match the desktop's so configs mean the
/// same thing.
pub static FILTERS: &[(&str, &str)] = &[("", "—"), ("mmpx", "mmpx"), ("lcd", "LCD")];

const VERTEX: &str = r#"#version 300 es
precision highp float;
out vec2 v_uv;
void main() {
    uint index = uint(gl_VertexID);
    vec2 uv = vec2(float((index << 1u) & 2u), float(index & 2u));
    gl_Position = vec4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    v_uv = uv;
}
"#;

/// Shared fragment prelude, the desktop's `common.wgsl`: the BGR555
/// decode (each 5-bit channel expands as the CPU LUT did — `c * 255 /
/// 31`, truncating) and the clamped texel fetch (clamp-to-edge
/// reproduces the CPU upscalers' edge replication).
const COMMON: &str = r#"#version 300 es
precision highp float;
precision highp usampler2D;
uniform usampler2D fb;
in vec2 v_uv;
out vec4 frag;
vec3 decode(uint raw) {
    uint r = ((raw       ) & 0x1fu) * 255u / 31u;
    uint g = ((raw >>  5u) & 0x1fu) * 255u / 31u;
    uint b = ((raw >> 10u) & 0x1fu) * 255u / 31u;
    return vec3(float(r), float(g), float(b)) / 255.0;
}
vec3 load(ivec2 p) {
    ivec2 hi = textureSize(fb, 0) - ivec2(1);
    return decode(texelFetch(fb, clamp(p, ivec2(0), hi), 0).r);
}
"#;

const PASSTHROUGH: &str = r#"
void main() {
    ivec2 dims = textureSize(fb, 0);
    frag = vec4(load(ivec2(floor(v_uv * vec2(dims)))), 1.0);
}
"#;

/// GBA-style LCD pixel grid (the desktop's `lcd.wgsl`): nearest
/// magnification plus a screen-space ~1px dark line along every
/// native-pixel boundary via fragment derivatives.
const LCD: &str = r#"
const float GRID_DARKNESS = 0.6;
const float LINE_WIDTH = 1.0;
void main() {
    vec2 dims = vec2(textureSize(fb, 0));
    vec2 coord = v_uv * dims;
    vec3 rgb = load(ivec2(floor(coord)));
    vec2 dist = abs(fract(coord - 0.5) - 0.5) / fwidth(coord);
    float line = min(dist.x, dist.y);
    float inside = smoothstep(0.0, LINE_WIDTH, line);
    float shade = mix(GRID_DARKNESS, 1.0, inside);
    frag = vec4(rgb * shade, 1.0);
}
"#;

/// MMPX 2x magnifier — the desktop's `mmpx.wgsl` rule cascade, line
/// for line (GLSL vector `==`/`!=` are the all/any comparisons WGSL
/// spells out). See that file for the algorithm notes.
const MMPX: &str = r#"
float luma(vec3 c) { return dot(c, vec3(0.299, 0.587, 0.114)); }
bool any_eq3(vec3 b, vec3 a0, vec3 a1, vec3 a2) { return b == a0 || b == a1 || b == a2; }
bool all_eq2(vec3 b, vec3 a0, vec3 a1) { return b == a0 && b == a1; }
bool all_eq3(vec3 b, vec3 a0, vec3 a1, vec3 a2) { return b == a0 && b == a1 && b == a2; }
bool all_eq4(vec3 b, vec3 a0, vec3 a1, vec3 a2, vec3 a3) { return b == a0 && b == a1 && b == a2 && b == a3; }
bool none_eq2(vec3 b, vec3 a0, vec3 a1) { return b != a0 && b != a1; }
bool none_eq4(vec3 b, vec3 a0, vec3 a1, vec3 a2, vec3 a3) { return b != a0 && b != a1 && b != a2 && b != a3; }

void main() {
    ivec2 dims = textureSize(fb, 0);
    ivec2 virt = ivec2(floor(v_uv * vec2(dims) * 2.0));
    ivec2 src = virt / 2;
    ivec2 sub = virt - src * 2;
    int q = sub.y * 2 + sub.x;

    vec3 a = load(src + ivec2(-1, -1));
    vec3 b = load(src + ivec2(0, -1));
    vec3 c = load(src + ivec2(1, -1));
    vec3 d = load(src + ivec2(-1, 0));
    vec3 e = load(src + ivec2(0, 0));
    vec3 f = load(src + ivec2(1, 0));
    vec3 g = load(src + ivec2(-1, 1));
    vec3 h = load(src + ivec2(0, 1));
    vec3 i = load(src + ivec2(1, 1));
    vec3 p = load(src + ivec2(0, -2));
    vec3 qq = load(src + ivec2(-2, 0));
    vec3 r = load(src + ivec2(2, 0));
    vec3 s = load(src + ivec2(0, 2));

    float b_luma = luma(b);
    float d_luma = luma(d);
    float e_luma = luma(e);
    float f_luma = luma(f);
    float h_luma = luma(h);

    vec3 j = e;
    vec3 k = e;
    vec3 l = e;
    vec3 m = e;

    // 1:1 slope rules
    if ((d == b && d != h && d != f)
        && (e_luma >= d_luma || e == a)
        && any_eq3(e, a, c, g)
        && ((e_luma < d_luma) || a != d || e != p || e != qq)) {
        j = d;
    }
    if ((b == f && b != d && b != h)
        && (e_luma >= b_luma || e == c)
        && any_eq3(e, a, c, i)
        && ((e_luma < b_luma) || c != b || e != p || e != r)) {
        k = b;
    }
    if ((h == d && h != f && h != b)
        && (e_luma >= h_luma || e == g)
        && any_eq3(e, a, g, i)
        && ((e_luma < h_luma) || g != h || e != s || e != qq)) {
        l = h;
    }
    if ((f == h && f != b && f != d)
        && (e_luma >= f_luma || e == i)
        && any_eq3(e, c, g, i)
        && ((e_luma < f_luma) || i != h || e != r || e != s)) {
        m = f;
    }

    // Intersection rules
    if ((e != f && all_eq4(e, c, i, d, qq) && all_eq2(f, b, h))
        && f != load(src + ivec2(3, 0))) {
        k = f;
        m = f;
    }
    if ((e != d && all_eq4(e, a, g, f, r) && all_eq2(d, b, h))
        && d != load(src + ivec2(-3, 0))) {
        j = d;
        l = d;
    }
    if ((e != h && all_eq4(e, g, i, b, p) && all_eq2(h, d, f))
        && h != load(src + ivec2(0, 3))) {
        l = h;
        m = h;
    }
    if ((e != b && all_eq4(e, a, c, h, s) && all_eq2(b, d, f))
        && b != load(src + ivec2(0, -3))) {
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
    if (h != b) {
        if (h != a && h != e && h != c) {
            if (all_eq3(h, g, f, r) && none_eq2(h, d, load(src + ivec2(2, -1)))) {
                l = m;
            }
            if (all_eq3(h, i, d, qq) && none_eq2(h, f, load(src + ivec2(-2, -1)))) {
                m = l;
            }
        }
        if (b != i && b != g && b != e) {
            if (all_eq3(b, a, f, r) && none_eq2(b, d, load(src + ivec2(2, 1)))) {
                j = k;
            }
            if (all_eq3(b, c, d, qq) && none_eq2(b, f, load(src + ivec2(-2, 1)))) {
                k = j;
            }
        }
    }
    if (f != d) {
        if (d != i && d != e && d != c) {
            if (all_eq3(d, a, h, s) && none_eq2(d, b, load(src + ivec2(1, 2)))) {
                j = l;
            }
            if (all_eq3(d, g, b, p) && none_eq2(d, h, load(src + ivec2(1, -2)))) {
                l = j;
            }
        }
        if (f != e && f != a && f != g) {
            if (all_eq3(f, c, h, s) && none_eq2(f, b, load(src + ivec2(-1, 2)))) {
                k = m;
            }
            if (all_eq3(f, i, b, p) && none_eq2(f, h, load(src + ivec2(-1, -2)))) {
                m = k;
            }
        }
    }

    vec3 result = j;
    if (q == 1) {
        result = k;
    } else if (q == 2) {
        result = l;
    } else if (q == 3) {
        result = m;
    }
    frag = vec4(result, 1.0);
}
"#;

/// Resolve a `config.video_filter` key to its fragment body. Unknown /
/// empty keys fall back to the pass-through, like the desktop registry.
fn fragment_for(filter: &str) -> &'static str {
    match filter {
        "mmpx" => MMPX,
        "lcd" => LCD,
        _ => PASSTHROUGH,
    }
}

pub struct WebGlPresenter {
    gl: Gl,
    _program: WebGlProgram,
    _texture: WebGlTexture,
    /// Staging copy of the frame as u16 texels: uploads need a
    /// Uint16Array view, and one lives here with guaranteed alignment.
    staging: Vec<u16>,
}

impl WebGlPresenter {
    /// Build the pipeline for `filter` (a `config.video_filter` key;
    /// unknown keys fall back to the pass-through).
    pub fn new(canvas: &web_sys::HtmlCanvasElement, filter: &str) -> Result<WebGlPresenter, String> {
        let gl: Gl = canvas
            .get_context("webgl2")
            .map_err(|e| format!("webgl2 context: {e:?}"))?
            .ok_or("webgl2 unsupported")?
            .dyn_into()
            .map_err(|_| "webgl2 context type")?;

        let fragment = format!("{COMMON}{}", fragment_for(filter));
        let program = link_program(&gl, VERTEX, &fragment)?;
        gl.use_program(Some(&program));

        let texture = gl.create_texture().ok_or("create_texture")?;
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
        // Integer textures require NEAREST; the shader's texelFetch
        // never samples anyway.
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::NEAREST as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::NEAREST as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
        gl.tex_storage_2d(Gl::TEXTURE_2D, 1, Gl::R16UI, SCREEN_WIDTH, SCREEN_HEIGHT);

        let fb_uniform = gl.get_uniform_location(&program, "fb");
        gl.uniform1i(fb_uniform.as_ref(), 0);

        Ok(WebGlPresenter {
            gl,
            _program: program,
            _texture: texture,
            staging: vec![0u16; (SCREEN_WIDTH * SCREEN_HEIGHT) as usize],
        })
    }

    /// Upload a raw little-endian BGR555 frame (2 bytes/pixel) and draw
    /// it as a fullscreen triangle.
    pub fn present(&mut self, bgr555: &[u8]) {
        for (texel, pair) in self.staging.iter_mut().zip(bgr555.chunks_exact(2)) {
            *texel = u16::from_le_bytes([pair[0], pair[1]]);
        }
        let gl = &self.gl;
        // SAFETY: the view is created and consumed with no allocation
        // in between (wasm memory growth would invalidate it).
        let view = unsafe { js_sys::Uint16Array::view(&self.staging) };
        gl.tex_sub_image_2d_with_i32_and_i32_and_u32_and_type_and_opt_array_buffer_view(
            Gl::TEXTURE_2D,
            0,
            0,
            0,
            SCREEN_WIDTH,
            SCREEN_HEIGHT,
            Gl::RED_INTEGER,
            Gl::UNSIGNED_SHORT,
            Some(&view),
        )
        .expect("texSubImage2D");

        let canvas: web_sys::HtmlCanvasElement = gl.canvas().unwrap().dyn_into().unwrap();
        gl.viewport(0, 0, canvas.width() as i32, canvas.height() as i32);
        gl.draw_arrays(Gl::TRIANGLES, 0, 3);
    }
}

fn compile_shader(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, String> {
    let shader = gl.create_shader(kind).ok_or("create_shader")?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(shader)
    } else {
        Err(gl
            .get_shader_info_log(&shader)
            .unwrap_or_else(|| "unknown shader error".into()))
    }
}

fn link_program(gl: &Gl, vertex: &str, fragment: &str) -> Result<WebGlProgram, String> {
    let vs = compile_shader(gl, Gl::VERTEX_SHADER, vertex)?;
    let fs = compile_shader(gl, Gl::FRAGMENT_SHADER, fragment)?;
    let program = gl.create_program().ok_or("create_program")?;
    gl.attach_shader(&program, &vs);
    gl.attach_shader(&program, &fs);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(gl
            .get_program_info_log(&program)
            .unwrap_or_else(|| "unknown link error".into()))
    }
}
