use glow::HasContext;
mod shader_version;
mod vao;

pub struct Framebuffer {
    gl: std::rc::Rc<glow::Context>,
    program: glow::Program,
    vao: vao::VertexArrayObject,
    vbo: glow::Buffer,
    texture: glow::Texture,
}

impl Framebuffer {
    pub fn new(gl: std::rc::Rc<glow::Context>) -> Result<Self, String> {
        unsafe {
            let shader_version = shader_version::ShaderVersion::get(&gl);

            let vertex_shader = gl.create_shader(glow::VERTEX_SHADER)?;
            gl.shader_source(
                vertex_shader,
                &format!(
                    "{}{}{}",
                    shader_version.version(),
                    shader_version.is_new_shader_interface(),
                    include_str!("./shaders/fb.vert")
                ),
            );
            gl.compile_shader(vertex_shader);
            if !gl.get_shader_compile_status(vertex_shader) {
                return Err(format!(
                    "failed to compile vertex shader: {}",
                    gl.get_shader_info_log(vertex_shader)
                ));
            }

            let fragment_shader = gl.create_shader(glow::FRAGMENT_SHADER)?;
            gl.shader_source(
                fragment_shader,
                &format!(
                    "{}{}{}",
                    shader_version.version(),
                    shader_version.is_new_shader_interface(),
                    include_str!("./shaders/fb.frag")
                ),
            );
            gl.compile_shader(fragment_shader);
            if !gl.get_shader_compile_status(fragment_shader) {
                return Err(format!(
                    "failed to compile fragment shader: {}",
                    gl.get_shader_info_log(fragment_shader)
                ));
            }

            let program = gl.create_program().unwrap();
            gl.attach_shader(program, vertex_shader);
            gl.attach_shader(program, fragment_shader);
            gl.link_program(program);
            if !gl.get_program_link_status(program) {
                return Err(format!(
                    "failed to link program: {}",
                    gl.get_program_info_log(program)
                ));
            }

            gl.use_program(Some(program));
            let sampler_location =
                if let Some(location) = gl.get_uniform_location(program, "u_sampler") {
                    location
                } else {
                    return Err("could not find u_sampler uniform".to_string());
                };
            gl.uniform_1_i32(Some(&sampler_location), 0);
            gl.use_program(None);

            let texture = gl.create_texture()?;
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::NEAREST as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.bind_texture(glow::TEXTURE_2D, None);

            let vbo = gl.create_buffer()?;
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            let vertices: [[f32; 2]; 12] = [
                // top left
                [-1.0, 1.0],
                [0.0, 0.0],
                // bottom left
                [-1.0, -1.0],
                [0.0, 1.0],
                // bottom right
                [1.0, -1.0],
                [1.0, 1.0],
                // top left
                [-1.0, 1.0],
                [0.0, 0.0],
                // bottom right
                [1.0, -1.0],
                [1.0, 1.0],
                // top right
                [1.0, 1.0],
                [1.0, 0.0],
            ];
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, vertices.align_to().1, glow::STATIC_DRAW);

            let pos_attrib = if let Some(location) = gl.get_attrib_location(program, "a_pos") {
                location
            } else {
                return Err("could not find a_pos attribute".to_string());
            };

            let uv_attrib = if let Some(location) = gl.get_attrib_location(program, "a_uv") {
                location
            } else {
                return Err("could not find a_uv attribute".to_string());
            };

            let vao = vao::VertexArrayObject::new(
                gl.clone(),
                vbo,
                vec![
                    vao::BufferInfo {
                        location: pos_attrib,
                        vector_size: 2,
                        data_type: glow::FLOAT,
                        normalized: false,
                        stride: 16,
                        offset: 0,
                    },
                    vao::BufferInfo {
                        location: uv_attrib,
                        vector_size: 2,
                        data_type: glow::FLOAT,
                        normalized: false,
                        stride: 16,
                        offset: 8,
                    },
                ],
            );

            gl.bind_buffer(glow::ARRAY_BUFFER, None);

            Ok(Self {
                gl,
                program,
                vao,
                vbo,
                texture,
            })
        }
    }

    pub fn draw(&mut self, viewport_size: (u32, u32), buffer_size: (u32, u32), pixels: &[u8]) {
        unsafe {
            let mut scaling_factor = std::cmp::min_by(
                viewport_size.0 as f32 / buffer_size.0 as f32,
                viewport_size.1 as f32 / buffer_size.1 as f32,
                |a, b| a.partial_cmp(b).unwrap(),
            );
            if scaling_factor >= 1.0 {
                scaling_factor = scaling_factor.floor();
            }

            let width = (buffer_size.0 as f32 * scaling_factor) as u32;
            let height = (buffer_size.1 as f32 * scaling_factor) as u32;

            self.gl.viewport(
                ((viewport_size.0 - width) / 2) as i32,
                ((viewport_size.1 - height) / 2) as i32,
                width as i32,
                height as i32,
            );
            self.gl.use_program(Some(self.program));
            self.vao.bind();
            self.gl.active_texture(glow::TEXTURE0);
            self.gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            self.gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                buffer_size.0 as i32,
                buffer_size.1 as i32,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                Some(pixels),
            );
            self.gl.draw_arrays(glow::TRIANGLES, 0, 6);
            self.gl.bind_texture(glow::TEXTURE_2D, None);
            self.vao.unbind();
            self.gl.use_program(None);
        }
    }
}

impl Drop for Framebuffer {
    fn drop(&mut self) {
        unsafe {
            self.gl.delete_program(self.program);
            self.gl.delete_texture(self.texture);
            self.gl.delete_buffer(self.vbo);
        }
    }
}
