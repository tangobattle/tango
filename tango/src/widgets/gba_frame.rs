//! Custom wgpu shader widget for the GBA framebuffer. Bypasses iced's
//! image atlas — `image::Handle::from_rgba` stamps a fresh `Id::unique()`
//! every Tick, so iced re-uploads the full RGBA texture through the
//! atlas each frame (up to ~1.4 MB at HQ3x). This widget owns a
//! persistent `wgpu::Texture` and updates it in-place via
//! `Queue::write_texture` in `prepare()`. Sampling is nearest-neighbor
//! to match the legacy GBA look.
//!
//! Layout still happens at the iced level — give the widget a Fixed
//! size (integer-scale branch) or Fill (Contain branch); the fragment
//! shader just samples the whole texture across the widget's bounds.

use std::sync::Arc;

use iced::widget::shader::{self, Viewport};
use iced::{mouse, Length, Rectangle};

/// Latest GBA framebuffer + dimensions. Cheap to clone (the pixel
/// buffer is shared via `Arc`).
#[derive(Clone, Debug)]
pub struct FrameData {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

/// Iced [`Program`] for the GBA framebuffer shader widget. Carries the
/// current frame; gets re-constructed by the parent's `view` each
/// render. The persistent GPU state lives in [`Pipeline`].
pub struct Program {
    data: FrameData,
}

impl Program {
    pub fn new(data: FrameData) -> Self {
        Self { data }
    }
}

impl<Message> shader::Program<Message> for Program {
    type State = ();
    type Primitive = Primitive;

    fn draw(&self, _state: &Self::State, _cursor: mouse::Cursor, _bounds: Rectangle) -> Self::Primitive {
        Primitive {
            data: self.data.clone(),
        }
    }
}

/// Per-frame snapshot handed to iced_wgpu. Borrows the pixel buffer via
/// Arc so cloning across the renderer boundary is cheap.
#[derive(Debug)]
pub struct Primitive {
    data: FrameData,
}

impl shader::Primitive for Primitive {
    type Pipeline = Pipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        pipeline.upload(device, queue, &self.data);
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.render(render_pass);
        true
    }
}

/// Persistent per-renderer GPU state: a single sampled texture
/// (re-sized when the upscale filter's output dimensions change),
/// a nearest-neighbor sampler, and a fullscreen-triangle pipeline.
/// Iced creates one of these the first time it sees a [`Primitive`]
/// of our type and reuses it forever after.
pub struct Pipeline {
    bundle: Option<TextureBundle>,
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

struct TextureBundle {
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
}

impl shader::Pipeline for Pipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gba_frame.bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
            ],
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("gba_frame.sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gba_frame.wgsl"),
            source: wgpu::ShaderSource::Wgsl(WGSL.into()),
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gba_frame.pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gba_frame.pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        Self {
            bundle: None,
            pipeline,
            bind_group_layout,
            sampler,
        }
    }
}

impl Pipeline {
    fn upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, data: &FrameData) {
        let needs_alloc = match &self.bundle {
            Some(tb) => tb.width != data.width || tb.height != data.height,
            None => true,
        };
        if needs_alloc {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("gba_frame.texture"),
                size: wgpu::Extent3d {
                    width: data.width,
                    height: data.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gba_frame.bind_group"),
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
            self.bundle = Some(TextureBundle {
                width: data.width,
                height: data.height,
                texture,
                bind_group,
            });
        }

        let Some(tb) = &self.bundle else { return };
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tb.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data.pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * tb.width),
                rows_per_image: Some(tb.height),
            },
            wgpu::Extent3d {
                width: tb.width,
                height: tb.height,
                depth_or_array_layers: 1,
            },
        );
    }

    fn render(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        let Some(tb) = &self.bundle else { return };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &tb.bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

const WGSL: &str = r#"
struct VSOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VSOut {
    // Fullscreen triangle: 3 vertices, covers [-1,1]x[-1,1] NDC with
    // UVs [0,1]x[0,1]. idx 0 -> (0,0), 1 -> (2,0), 2 -> (0,2).
    let u = f32((idx << 1u) & 2u);
    let v = f32(idx & 2u);
    var out: VSOut;
    out.uv = vec2<f32>(u, v);
    out.pos = vec4<f32>(u * 2.0 - 1.0, 1.0 - v * 2.0, 0.0, 1.0);
    return out;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;

@fragment
fn fs_main(in: VSOut) -> @location(0) vec4<f32> {
    // hqx outputs RGB with alpha masked to 0; force opaque so we
    // don't show through to the iced background underneath.
    let c = textureSample(tex, samp, in.uv);
    return vec4<f32>(c.rgb, 1.0);
}
"#;

/// Construct the iced Element. Iced still owns layout; pass `Fill` for
/// the Contain branch (let aspect be handled by the parent wrapper) or
/// a `Fixed(...)` for the integer-scale branch.
pub fn view<'a, Message: 'a>(
    data: FrameData,
    width: Length,
    height: Length,
) -> iced::widget::shader::Shader<Message, Program> {
    iced::widget::shader::Shader::new(Program::new(data))
        .width(width)
        .height(height)
}
