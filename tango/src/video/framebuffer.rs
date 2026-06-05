//! GPU presentation of the live emulator framebuffer via a custom iced
//! `wgpu` shader primitive.
//!
//! ## Why this exists
//!
//! The previous path rebuilt an `image::Handle::from_rgba` every emulator
//! vblank and handed it to iced's `image` widget. `from_rgba` mints a
//! fresh `Id::unique()` each call, and iced's wgpu image cache keys on
//! that id — so every single frame it **allocated** a new region in the
//! shared texture atlas, **uploaded** into it, and (on the next `trim`)
//! **freed** the previous frame's region. Frames at or above
//! `MAX_SYNC_SIZE` (2 MiB — i.e. hq4x) additionally detoured through the
//! async upload worker thread, whose upload racing the vsync-off present
//! is exactly the hq4x flicker documented in [`crate::video`].
//!
//! A *stable* handle id can't fix this: iced only (re)uploads when its
//! cache doesn't already contain the id (`load_image` → `!cache.contains`),
//! so reusing an id would freeze the picture on the first frame's pixels.
//! The only way to update a texture in place is to own it ourselves.
//!
//! ## What this does
//!
//! We keep ONE persistent GPU texture sized to the post-filter
//! framebuffer and `queue.write_texture` the new pixels into it once per
//! frame — no atlas, no per-frame allocate/free, no worker detour. A
//! `revision` counter lets `prepare` skip the upload entirely when the
//! same frame is presented twice (e.g. a UI redraw with no new emu frame).
//!
//! iced sets the render-pass **viewport** to the widget's bounds before
//! calling [`Primitive::draw`] (see `iced_wgpu`'s `lib.rs`: `set_viewport`
//! to `instance.bounds`), so a fullscreen triangle drawn in NDC lands
//! exactly on the widget with no transform uniform. The widget is sized to
//! the framebuffer rect by the caller (`session::view`), so the triangle
//! always fills it; nearest sampling keeps pixels crisp, matching the old
//! `FilterMethod::Nearest` image.
//!
//! Note: this is a `wgpu`-only widget. On a pure software (`tiny_skia`)
//! fallback it draws nothing — but Tango already forces a wgpu adapter
//! (DX12/Vulkan/Metal, or ANGLE/GLES via the `main.rs` fallback probe), so
//! in practice there is always a GPU backend behind this.

use std::sync::Arc;

use iced::advanced::mouse;
use iced::widget::shader::{self, Viewport};
use iced::Rectangle;

/// The native GBA framebuffer is 240×160; the only thing that grows it is a
/// CPU upscale filter, whose output size we carry in [`Frame`].
const BYTES_PER_PIXEL: u32 = 4;

/// A framebuffer ready to present. Cheap to clone — the pixels live behind
/// an `Arc`, so [`crate::session::view`] can rebuild this every redraw
/// without copying. `revision` is monotonic per real frame so the pipeline
/// can tell "same frame again" (skip upload) from "new frame" (upload).
#[derive(Debug, Clone)]
pub struct Frame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
}

impl Frame {
    /// A 1×1 opaque-black frame for "no frame yet" (between sessions and
    /// before the first vblank). Sampled nearest over the whole widget it
    /// reads as a solid black pane — matching the old all-`0xff`-alpha
    /// placeholder handle. The fixed sentinel revision keeps it from
    /// re-uploading on every redraw.
    pub fn black() -> Self {
        Self {
            pixels: Arc::new(vec![0, 0, 0, 0xff]),
            width: 1,
            height: 1,
            revision: u64::MAX,
        }
    }
}

/// The iced [`shader::Program`] stored in the widget tree. Holds the frame
/// to present this redraw and hands it to a [`Primitive`] in `draw`.
#[derive(Debug)]
pub struct Program {
    frame: Frame,
}

impl Program {
    pub fn new(frame: Frame) -> Self {
        Self { frame }
    }
}

impl<Message> shader::Program<Message> for Program {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &(), _cursor: mouse::Cursor, _bounds: Rectangle) -> Primitive {
        Primitive {
            frame: self.frame.clone(),
        }
    }
}

/// The per-frame primitive. Carries the frame into `prepare`/`draw`; the
/// persistent GPU resources live in [`Pipeline`] (one per primitive type,
/// shared across all instances — we only ever show one framebuffer).
#[derive(Debug)]
pub struct Primitive {
    frame: Frame,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        pipeline.upload(device, queue, &self.frame);
    }

    fn draw(&self, pipeline: &Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.draw(render_pass);
        // We drew into the existing pass; tell iced not to call `render`.
        true
    }
}

/// Persistent wgpu state: the render pipeline + sampler created once, and a
/// lazily (re)created texture that tracks the current framebuffer size.
#[derive(Debug)]
pub struct Pipeline {
    render_pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Texture storage format, chosen to mirror iced's atlas so our
    /// sample→write round-trip is byte-identical to the old image path in
    /// both gamma modes (see [`Pipeline::new`]).
    texture_format: wgpu::TextureFormat,
    texture: Option<FrameTexture>,
}

/// The current framebuffer texture + its bind group, sized to the frame.
#[derive(Debug)]
struct FrameTexture {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    /// Revision of the pixels currently resident, or `None` if just
    /// (re)created and not yet written.
    revision: Option<u64>,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // iced's image atlas stores pixels as `Rgba8UnormSrgb` when gamma
        // correction is on and `Rgba8Unorm` otherwise (web-colors), and it
        // renders into a matching target. The custom-primitive target
        // `format` iced hands us is sRGB in the first case and linear in
        // the second, so we can recover the same choice from its srgb-ness.
        // Sampling an sRGB texture (→linear) and writing to an sRGB target
        // (linear→sRGB) round-trips to identity, exactly like the old
        // pass-through image shader; likewise the linear/linear case. This
        // keeps colors pixel-identical to before in either mode.
        let texture_format = if format.is_srgb() {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("framebuffer bind group layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // Nearest everything — crisp integer-scaled pixels, matching the
        // old `FilterMethod::Nearest`.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("framebuffer sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("framebuffer shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("framebuffer pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("framebuffer pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            // iced draws custom primitives into the (non-multisampled)
            // surface pass — sample_count 1, matching its quad pipeline.
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            render_pipeline,
            bind_group_layout,
            sampler,
            texture_format,
            texture: None,
        }
    }
}

impl Pipeline {
    /// Ensure a texture of the right size exists and holds `frame`'s pixels.
    /// (Re)creates the texture when the framebuffer size changes (e.g. the
    /// user switches upscale filter), and uploads only when the resident
    /// revision differs from the frame's.
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame) {
        let needs_new = match &self.texture {
            Some(t) => t.width != frame.width || t.height != frame.height,
            None => true,
        };

        if needs_new {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("framebuffer texture"),
                size: wgpu::Extent3d {
                    width: frame.width,
                    height: frame.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.texture_format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("framebuffer bind group"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.texture = Some(FrameTexture {
                texture,
                bind_group,
                width: frame.width,
                height: frame.height,
                revision: None,
            });
        }

        let tex = self.texture.as_mut().expect("texture just ensured");
        if tex.revision == Some(frame.revision) {
            return; // same frame already resident — nothing to upload
        }

        // `write_texture` (unlike `copy_buffer_to_texture`) imposes no
        // 256-byte row-alignment requirement, so a 240-wide (960 B/row)
        // GBA frame uploads directly.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(frame.width * BYTES_PER_PIXEL),
                rows_per_image: Some(frame.height),
            },
            wgpu::Extent3d {
                width: frame.width,
                height: frame.height,
                depth_or_array_layers: 1,
            },
        );
        tex.revision = Some(frame.revision);
    }

    /// Draw the framebuffer as a fullscreen triangle into iced's render
    /// pass. The pass viewport is already set to the widget bounds, so NDC
    /// maps onto the widget.
    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        let Some(tex) = self.texture.as_ref() else {
            return;
        };
        render_pass.set_pipeline(&self.render_pipeline);
        render_pass.set_bind_group(0, &tex.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// Fullscreen-triangle pass-through. The vertex shader synthesizes the
/// triangle from `vertex_index` (no vertex buffer); UV origin is top-left
/// so texture row 0 renders at the top, matching the old image widget.
const SHADER: &str = r#"
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var out: VsOut;
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    out.position = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@group(0) @binding(0) var fb_texture: texture_2d<f32>;
@group(0) @binding(1) var fb_sampler: sampler;

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    // The GBA framebuffer is always opaque, so force alpha to 1.0 here. This
    // decouples display opacity from any CPU-side alpha handling, letting the
    // redundant per-frame alpha-stamp pass in `build_frame_pixels` go away.
    // RGB is unaffected by the stored alpha: textures hold straight
    // (non-premultiplied) values, so sampling returns RGB regardless of the
    // alpha byte.
    return vec4<f32>(textureSample(fb_texture, fb_sampler, in.uv).rgb, 1.0);
}
"#;
