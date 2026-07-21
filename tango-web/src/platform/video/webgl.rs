//! The WebGL2 framebuffer presenter: one persistent 240x160 R16UI
//! texture holding mGBA's raw little-endian BGR555, decoded to RGB in
//! the fragment shader — a GLSL ES 3.00 port of the wgpu passthrough
//! pipeline (`platform/video/effects/{common,passthrough}.wgsl`), on
//! the non-sRGB-target path (the canvas default drawing buffer).

use wasm_bindgen::JsCast;
use web_sys::{WebGl2RenderingContext as Gl, WebGlProgram, WebGlShader, WebGlTexture};

pub const SCREEN_WIDTH: i32 = 240;
pub const SCREEN_HEIGHT: i32 = 160;

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

const FRAGMENT: &str = r#"#version 300 es
precision highp float;
precision highp usampler2D;
uniform usampler2D fb;
in vec2 v_uv;
out vec4 frag;
// Each 5-bit channel expands to 8-bit exactly as the CPU LUT did
// (c * 255 / 31, truncating); see common.wgsl.
vec3 decode(uint raw) {
    uint r = ((raw       ) & 0x1fu) * 255u / 31u;
    uint g = ((raw >>  5u) & 0x1fu) * 255u / 31u;
    uint b = ((raw >> 10u) & 0x1fu) * 255u / 31u;
    return vec3(float(r), float(g), float(b)) / 255.0;
}
void main() {
    ivec2 dims = textureSize(fb, 0);
    ivec2 p = clamp(ivec2(floor(v_uv * vec2(dims))), ivec2(0), dims - ivec2(1));
    frag = vec4(decode(texelFetch(fb, p, 0).r), 1.0);
}
"#;

pub struct WebGlPresenter {
    gl: Gl,
    _program: WebGlProgram,
    _texture: WebGlTexture,
    /// Staging copy of the frame as u16 texels: uploads need a
    /// Uint16Array view, and one lives here with guaranteed alignment.
    staging: Vec<u16>,
}

impl WebGlPresenter {
    pub fn new(canvas: &web_sys::HtmlCanvasElement) -> Result<WebGlPresenter, String> {
        let gl: Gl = canvas
            .get_context("webgl2")
            .map_err(|e| format!("webgl2 context: {e:?}"))?
            .ok_or("webgl2 unsupported")?
            .dyn_into()
            .map_err(|_| "webgl2 context type")?;

        let program = link_program(&gl, VERTEX, FRAGMENT)?;
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
