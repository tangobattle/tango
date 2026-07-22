//! Native framebuffer presentation through dioxus-native's custom
//! paint hook. The runtime pump publishes each new BGR555 frame into a
//! shared [`FrameCell`]; a [`PaintSource`] registered with the Blitz
//! renderer (via `dioxus_native::use_wgpu` + `canvas { "src": id }`)
//! uploads it to a persistent 240×160 `R16Uint` texture and draws it
//! through one of the desktop client's WGSL effects into an
//! element-sized target texture that vello composites like any other
//! DOM content.
//!
//! The GPU path is the desktop client's `platform/video/framebuffer.rs`
//! condensed: same texture format, same bind-group layout, same
//! fullscreen-triangle draw, and literally the same WGSL sources
//! (`include_str!` straight out of the desktop tree — the web backend's
//! GLSL was naga-transpiled from these same files). Only the outer
//! plumbing differs: instead of an iced shader primitive, `render()`
//! fires on every window repaint (a `<canvas>` keeps Blitz in a
//! continuous redraw loop), re-uploading only when the revision moved.

use std::sync::{Arc, Mutex};

use dioxus_native::{CustomPaintCtx, CustomPaintSource, DeviceHandle, TextureHandle};

const COMMON: &str = include_str!("../../../../tango/src/platform/video/effects/common.wgsl");
const HQX_COMMON: &str = include_str!("../../../../tango/src/platform/video/effects/hqx/common.wgsl");

/// WGSL parts per `config.video_filter` key (the shared prelude first,
/// then any family prelude, then the fragment) — the same composition
/// the desktop's `Effect::source` performs.
fn effect_parts(filter: &str) -> &'static [&'static str] {
    const PASSTHROUGH: &[&str] = &[
        COMMON,
        include_str!("../../../../tango/src/platform/video/effects/passthrough.wgsl"),
    ];
    const HQ2X: &[&str] = &[
        COMMON,
        HQX_COMMON,
        include_str!("../../../../tango/src/platform/video/effects/hqx/hq2x.wgsl"),
    ];
    const HQ3X: &[&str] = &[
        COMMON,
        HQX_COMMON,
        include_str!("../../../../tango/src/platform/video/effects/hqx/hq3x.wgsl"),
    ];
    const HQ4X: &[&str] = &[
        COMMON,
        HQX_COMMON,
        include_str!("../../../../tango/src/platform/video/effects/hqx/hq4x.wgsl"),
    ];
    const MMPX: &[&str] = &[
        COMMON,
        include_str!("../../../../tango/src/platform/video/effects/mmpx/mmpx.wgsl"),
    ];
    const LCD: &[&str] = &[
        COMMON,
        include_str!("../../../../tango/src/platform/video/effects/lcd/lcd.wgsl"),
    ];
    match filter {
        "hq2x" => HQ2X,
        "hq3x" => HQ3X,
        "hq4x" => HQ4X,
        "mmpx" => MMPX,
        "lcd" => LCD,
        _ => PASSTHROUGH,
    }
}

/// The latest presented frame, shared between the runtime pump (writer)
/// and the paint source (reader on window repaint). The mutex is held
/// only for a 76 KiB memcpy on either side.
pub struct FrameCell {
    frame: Mutex<FrameData>,
}

struct FrameData {
    /// Raw BGR555 bytes, or empty if nothing presented yet.
    pixels: Vec<u8>,
    /// Bumped per real frame so `render` can skip redundant uploads.
    revision: u64,
}

impl FrameCell {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            frame: Mutex::new(FrameData {
                pixels: Vec::new(),
                revision: 0,
            }),
        })
    }
}

/// The runtime-facing half: mirrors `WebGlPresenter`'s `present`
/// contract (240×160 BGR555 in, latest frame wins).
pub struct Presenter {
    cell: Arc<FrameCell>,
    filter: Arc<Mutex<String>>,
}

impl Presenter {
    pub fn new(filter: &str) -> Self {
        Self {
            cell: FrameCell::new(),
            filter: Arc::new(Mutex::new(filter.to_owned())),
        }
    }

    /// Publish a frame. `bgr555` must be `SCREEN_BYTES` long.
    pub fn present(&mut self, bgr555: &[u8]) {
        let mut g = self.cell.frame.lock().unwrap();
        g.pixels.clear();
        g.pixels.extend_from_slice(bgr555);
        g.revision = g.revision.wrapping_add(1);
    }

    /// Switch the upscale effect; takes hold on the next repaint.
    pub fn set_filter(&self, filter: &str) {
        *self.filter.lock().unwrap() = filter.to_owned();
    }

    /// Build the paint source that feeds this presenter's frames to the
    /// Blitz renderer. Hand the result to `dioxus_native::use_wgpu` and
    /// put the returned id on a `canvas`'s `"src"` attribute.
    pub fn paint_source(&self) -> PaintSource {
        PaintSource {
            cell: self.cell.clone(),
            filter: self.filter.clone(),
            gpu: None,
            resident_revision: None,
            displayed_target: None,
            next_target: None,
        }
    }
}

/// Persistent per-device GPU state, built in `resume`.
struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    /// Compiled pipeline for the currently-selected filter, keyed by
    /// filter id. Rebuilt lazily when the filter changes.
    compiled: Option<(String, wgpu::RenderPipeline)>,
    /// The persistent 240×160 R16Uint source texture + its bind group.
    source: Option<(wgpu::Texture, wgpu::BindGroup)>,
}

struct Target {
    texture: wgpu::Texture,
    handle: TextureHandle,
}

pub struct PaintSource {
    cell: Arc<FrameCell>,
    filter: Arc<Mutex<String>>,
    gpu: Option<Gpu>,
    /// Revision of the pixels resident in the source texture.
    resident_revision: Option<u64>,
    /// Double-buffered element-sized render targets (the texture being
    /// composited this frame must not be the one we draw the next frame
    /// into).
    displayed_target: Option<Target>,
    next_target: Option<Target>,
}

impl PaintSource {
    fn render_inner(&mut self, mut ctx: CustomPaintCtx<'_>, width: u32, height: u32) -> Option<TextureHandle> {
        if width == 0 || height == 0 {
            return None;
        }
        let gpu = self.gpu.as_mut()?;

        // Upload the latest frame if it moved (or the texture is new).
        {
            let frame = self.cell.frame.lock().unwrap();
            if frame.pixels.len() == crate::platform::video::SCREEN_BYTES {
                if gpu.source.is_none() {
                    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("framebuffer texture"),
                        size: wgpu::Extent3d {
                            width: crate::platform::video::SCREEN_WIDTH as u32,
                            height: crate::platform::video::SCREEN_HEIGHT as u32,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::R16Uint,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                        view_formats: &[],
                    });
                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let bind_group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("framebuffer bind group"),
                        layout: &gpu.bind_group_layout,
                        entries: &[wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        }],
                    });
                    gpu.source = Some((texture, bind_group));
                    self.resident_revision = None;
                }
                if self.resident_revision != Some(frame.revision) {
                    let (texture, _) = gpu.source.as_ref().unwrap();
                    // `write_texture` imposes no 256-byte row alignment,
                    // so the 480 B/row GBA frame uploads directly.
                    gpu.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        &frame.pixels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(crate::platform::video::SCREEN_WIDTH as u32 * 2),
                            rows_per_image: Some(crate::platform::video::SCREEN_HEIGHT as u32),
                        },
                        wgpu::Extent3d {
                            width: crate::platform::video::SCREEN_WIDTH as u32,
                            height: crate::platform::video::SCREEN_HEIGHT as u32,
                            depth_or_array_layers: 1,
                        },
                    );
                    self.resident_revision = Some(frame.revision);
                }
            }
        }

        // (Re)compile the effect pipeline if the filter changed.
        let filter = self.filter.lock().unwrap().clone();
        if gpu.compiled.as_ref().map(|(id, _)| id.as_str()) != Some(filter.as_str()) {
            // The target is Rgba8Unorm (non-sRGB), same convention as
            // the web backend's canvas: decode() passes the encoded
            // value through unchanged.
            let mut parts = vec!["const SRGB_TARGET: bool = false;"];
            parts.extend_from_slice(effect_parts(&filter));
            let module = gpu.device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("framebuffer shader: {filter}")),
                source: wgpu::ShaderSource::Wgsl(parts.join("\n").into()),
            });
            let pipeline = gpu.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(&format!("framebuffer pipeline: {filter}")),
                layout: Some(&gpu.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
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
            gpu.compiled = Some((filter, pipeline));
        }

        // Element-sized render target, double-buffered; drop + recreate
        // on resize.
        if self
            .next_target
            .as_ref()
            .is_some_and(|t| t.texture.width() != width || t.texture.height() != height)
        {
            let target = self.next_target.take().unwrap();
            ctx.unregister_texture(target.handle);
        }
        if self.next_target.is_none() {
            let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("framebuffer target"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let handle = ctx.register_texture(texture.clone());
            self.next_target = Some(Target { texture, handle });
        }

        // Draw: clear to opaque black, then (if a frame is resident)
        // the fullscreen-triangle effect pass.
        let target = self.next_target.as_ref().unwrap();
        let view = target.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("framebuffer encoder") });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("framebuffer pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if self.resident_revision.is_some() {
                if let (Some((_, bind_group)), Some((_, pipeline))) = (gpu.source.as_ref(), gpu.compiled.as_ref()) {
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, bind_group, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
        gpu.queue.submit(Some(encoder.finish()));

        std::mem::swap(&mut self.next_target, &mut self.displayed_target);
        self.displayed_target.as_ref().map(|t| t.handle.clone())
    }
}

impl CustomPaintSource for PaintSource {
    fn resume(&mut self, device_handle: &DeviceHandle) {
        let device = device_handle.device.clone();
        let queue = device_handle.queue.clone();
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("framebuffer bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    // Integer texture: not filterable, fetched via `textureLoad`.
                    sample_type: wgpu::TextureSampleType::Uint,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("framebuffer pipeline layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        self.gpu = Some(Gpu {
            device,
            queue,
            bind_group_layout,
            pipeline_layout,
            compiled: None,
            source: None,
        });
        self.resident_revision = None;
    }

    fn suspend(&mut self) {
        // Targets were registered against the suspended device's
        // renderer; drop everything and rebuild on resume.
        self.gpu = None;
        self.resident_revision = None;
        self.displayed_target = None;
        self.next_target = None;
    }

    fn render(&mut self, ctx: CustomPaintCtx<'_>, width: u32, height: u32, _scale: f64) -> Option<TextureHandle> {
        self.render_inner(ctx, width, height)
    }
}
