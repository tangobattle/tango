//! GPU presentation of the live emulator framebuffer via a custom iced
//! `wgpu` shader primitive, plus a small pluggable **effect** framework that
//! does upscaling (hqx/mmpx) on the GPU.
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
//! is exactly the hq4x flicker documented in [`crate::platform::video`].
//!
//! A *stable* handle id can't fix this: iced only (re)uploads when its
//! cache doesn't already contain the id (`load_image` → `!cache.contains`),
//! so reusing an id would freeze the picture on the first frame's pixels.
//! The only way to update a texture in place is to own it ourselves.
//!
//! ## What this does
//!
//! We keep ONE persistent GPU texture sized to the **native** 240×160
//! framebuffer and `queue.write_texture` the new pixels into it once per
//! frame — no atlas, no per-frame allocate/free, no worker detour. A
//! `revision` counter lets `prepare` skip the upload entirely when the
//! same frame is presented twice (e.g. a UI redraw with no new emu frame).
//!
//! Upscaling happens on the GPU: each [`Effect`] is a fragment shader that
//! samples the native texture and magnifies it while drawing (see
//! `shaders/*.wgsl`). So the uploaded texture is identical for every effect
//! and only the selected render pipeline changes. The widget is sized to
//! `native·scale` by the caller (`session::view`), the same rectangle the
//! old CPU upscalers produced, so the on-screen result matches.
//!
//! iced sets the render-pass **viewport** to the widget's bounds before
//! calling [`Primitive::draw`] (see `iced_wgpu`'s `lib.rs`: `set_viewport`
//! to `instance.bounds`), so a fullscreen triangle drawn in NDC lands
//! exactly on the widget with no transform uniform.
//!
//! Note: this is a `wgpu`-only widget. On a pure software (`tiny_skia`)
//! fallback it draws nothing — but Tango already forces a wgpu adapter
//! (DX12/Vulkan/Metal, or ANGLE/GLES via the `main.rs` fallback probe), so
//! in practice there is always a GPU backend behind this.

use std::{collections::HashMap, sync::Arc};

use iced::advanced::mouse;
use iced::widget::shader::{self, Viewport};
use iced::Rectangle;

/// The native GBA framebuffer is 240×160; the uploaded texture is always
/// native and the selected [`Effect`] magnifies it in the fragment shader.
/// The pixels arrive already CPU-expanded to RGBA8 (sessions publish
/// RGBA8 — see [`tango_session::Session::frame`]), so the shaders never
/// touch the console-native format — the texture hands them ready-made RGB.
const BYTES_PER_PIXEL: u32 = 4;

/// Opaque rendering implementation behind an [`Effect`].
pub(crate) trait EffectRenderer: Sync {
    fn compile(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
        effect_id: &'static str,
    ) -> Box<dyn CompiledEffect>;
}

/// Fully-owned GPU state for one compiled effect. The shared framebuffer
/// pipeline only supplies the current source texture view.
pub(crate) trait CompiledEffect: std::fmt::Debug + Send + Sync {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        framebuffer: &wgpu::TextureView,
        texture_generation: u64,
    );

    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>);
}

/// Renderer for effects implemented entirely in WGSL.
#[derive(Debug)]
pub(crate) struct WgslRenderer {
    parts: &'static [&'static str],
}

impl WgslRenderer {
    pub(crate) const fn new(parts: &'static [&'static str]) -> Self {
        Self { parts }
    }
}

impl EffectRenderer for WgslRenderer {
    fn compile(
        &self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
        effect_id: &'static str,
    ) -> Box<dyn CompiledEffect> {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("framebuffer effect bind group layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline = compile_render_pipeline(device, target, effect_id, &bind_group_layout, self.parts, &[]);
        Box::new(WgslCompiledEffect {
            pipeline,
            bind_group_layout,
            bind_group: None,
            texture_generation: None,
        })
    }
}

#[derive(Debug)]
struct WgslCompiledEffect {
    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,
    bind_group: Option<wgpu::BindGroup>,
    texture_generation: Option<u64>,
}

impl CompiledEffect for WgslCompiledEffect {
    fn prepare(
        &mut self,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        framebuffer: &wgpu::TextureView,
        texture_generation: u64,
    ) {
        if self.texture_generation == Some(texture_generation) {
            return;
        }
        self.bind_group = Some(device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("framebuffer effect bind group"),
            layout: &self.bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(framebuffer),
            }],
        }));
        self.texture_generation = Some(texture_generation);
    }

    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>) {
        let Some(bind_group) = &self.bind_group else {
            return;
        };
        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, bind_group, &[]);
        render_pass.draw(0..3, 0..1);
    }
}

/// A selectable GPU upscaler, defined as a named constant in
/// [`crate::platform::video::effects`] (e.g. `effects::hqx::HQ2X`). `id` is the
/// `config.video_filter` key; `name` is the picker label; `scale` is the
/// integer magnification the fragment shader emulates (used by
/// `session::view` to size the widget to the same rectangle the old CPU
/// upscalers produced). Rendering details live behind an opaque renderer.
#[derive(Clone, Copy)]
pub struct Effect {
    /// Stable identifier stored in `config.video_filter` ("" = pass-through,
    /// "hq2x", …); also keys the compiled-pipeline cache.
    pub id: &'static str,
    /// Picker label shown in settings.
    pub name: &'static str,
    pub scale: u32,
    renderer: &'static dyn EffectRenderer,
}

impl std::fmt::Debug for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Effect")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("scale", &self.scale)
            .finish_non_exhaustive()
    }
}

impl Effect {
    pub(crate) const fn new(
        id: &'static str,
        name: &'static str,
        scale: u32,
        renderer: &'static dyn EffectRenderer,
    ) -> Self {
        Self {
            id,
            name,
            scale,
            renderer,
        }
    }

    fn compile(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: wgpu::TextureFormat,
    ) -> Box<dyn CompiledEffect> {
        self.renderer.compile(device, queue, target, self.id)
    }
}

/// Compile WGSL for an effect-owned bind group layout.
pub(crate) fn compile_render_pipeline(
    device: &wgpu::Device,
    target: wgpu::TextureFormat,
    effect_id: &'static str,
    bind_group_layout: &wgpu::BindGroupLayout,
    parts: &'static [&'static str],
    constants: &'static [(&'static str, f64)],
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(&format!("framebuffer shader: {effect_id}")),
        source: wgpu::ShaderSource::Wgsl(parts.join("\n").into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(&format!("framebuffer pipeline layout: {effect_id}")),
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(&format!("framebuffer pipeline: {effect_id}")),
        layout: Some(&pipeline_layout),
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
                format: target,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions {
                constants,
                ..Default::default()
            },
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

/// A framebuffer ready to present. Cheap to clone — the pixels live behind
/// an `Arc`, so [`crate::session::view`] can rebuild this every redraw
/// without copying. `revision` is monotonic per real frame so the pipeline
/// can tell "same frame again" (skip upload) from "new frame" (upload).
/// `effect` selects which render pipeline draws it.
#[derive(Debug, Clone)]
pub struct Frame {
    pub pixels: Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub revision: u64,
    pub effect: &'static Effect,
}

impl Frame {
    /// A 1×1 opaque-black frame for "no frame yet" (between sessions and
    /// before the first vblank). One opaque-black RGBA8 texel, sampled over
    /// the whole widget, reads as a solid black pane. The fixed sentinel
    /// revision keeps it from re-uploading on every redraw; the pass-through
    /// effect draws it plainly.
    pub fn black() -> Self {
        Self {
            pixels: Arc::new(vec![0, 0, 0, 0xff]),
            width: 1,
            height: 1,
            revision: u64::MAX,
            effect: &crate::platform::video::effects::PASSTHROUGH,
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

/// A second, independent framebuffer surface — the replay PiP (the
/// opponent's screen).
///
/// `iced_wgpu` keys persistent pipeline state by primitive *type*: all
/// primitives of one type share a single [`Pipeline`], and ours holds a
/// single resident texture. Two [`Program`] widgets in one window would
/// therefore fight over that texture — each `prepare` uploads its own
/// frame and both draws sample whichever landed last. The PiP instead
/// draws through these delegation newtypes: identical logic, distinct
/// `TypeId`, so iced gives it its own [`Pipeline`] (and texture).
#[derive(Debug)]
pub struct PipProgram(Program);

impl PipProgram {
    pub fn new(frame: Frame) -> Self {
        Self(Program::new(frame))
    }
}

impl<Message> shader::Program<Message> for PipProgram {
    type State = ();
    type Primitive = PipPrimitive;

    fn draw(&self, state: &(), cursor: mouse::Cursor, bounds: Rectangle) -> PipPrimitive {
        PipPrimitive(shader::Program::<Message>::draw(&self.0, state, cursor, bounds))
    }
}

/// See [`PipProgram`].
#[derive(Debug)]
pub struct PipPrimitive(Primitive);

impl shader::Primitive for PipPrimitive {
    type Pipeline = PipPipeline;

    fn prepare(
        &self,
        pipeline: &mut PipPipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        viewport: &Viewport,
    ) {
        self.0.prepare(&mut pipeline.0, device, queue, bounds, viewport);
    }

    fn draw(&self, pipeline: &PipPipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        shader::Primitive::draw(&self.0, &pipeline.0, render_pass)
    }
}

/// See [`PipProgram`].
#[derive(Debug)]
pub struct PipPipeline(Pipeline);

impl shader::Pipeline for PipPipeline {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self(<Pipeline as shader::Pipeline>::new(device, queue, format))
    }
}

/// The per-frame primitive. Carries the frame into `prepare`/`draw`; the
/// persistent GPU resources live in [`Pipeline`] (one per primitive type,
/// shared across all instances of that type — the main screen and the
/// replay PiP are distinct types for exactly this reason).
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
        pipeline.ensure(device, queue, self.frame.effect);
        pipeline.prepare_effect(device, queue, self.frame.effect);
    }

    fn draw(&self, pipeline: &Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        pipeline.draw(render_pass, self.frame.effect);
        // We drew into the existing pass; tell iced not to call `render`.
        true
    }
}

/// Persistent wgpu state: one opaque compiled renderer per used [`Effect`],
/// plus a lazily (re)created texture that tracks the current framebuffer size.
#[derive(Debug)]
pub struct Pipeline {
    /// Compiled renderer, keyed by [`Effect::id`]. Populated lazily on first
    /// use so startup only pays for the pass-through effect.
    compiled: HashMap<&'static str, Box<dyn CompiledEffect>>,
    /// Render-pass target format, needed for the lazy pipeline builds.
    target_format: wgpu::TextureFormat,
    texture_generation: u64,
    texture: Option<FrameTexture>,
}

/// The current framebuffer texture and view, sized to the frame.
#[derive(Debug)]
struct FrameTexture {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    generation: u64,
    width: u32,
    height: u32,
    /// Revision of the pixels currently resident, or `None` if just
    /// (re)created and not yet written.
    revision: Option<u64>,
}

impl shader::Pipeline for Pipeline {
    fn new(_device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        Self {
            // Built lazily in `ensure` as effects are selected, then retained
            // so switching filters never recompiles an already-used shader.
            compiled: HashMap::new(),
            target_format: format,
            texture_generation: 0,
            texture: None,
        }
    }
}

impl Pipeline {
    /// Ensure a texture of the right size exists and holds `frame`'s pixels.
    /// The framebuffer texture is always native (240×160) now — only a
    /// resolution change (never, in practice) would resize it — and uploads
    /// only when the resident revision differs from the frame's.
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
                // The uploaded pixels are 8-bit sRGB-encoded (the GBA's
                // native palette, CPU-expanded from BGR555). Matching the
                // texture's gamma to the render target's replaces the old
                // in-shader decode: an sRGB view makes `textureLoad` return
                // linear, which the sRGB target re-encodes on write; a
                // linear (web-colors) target reads the encoded value
                // unchanged. Same values either way, now decoded in
                // fixed-function hardware.
                format: if self.target_format.is_srgb() {
                    wgpu::TextureFormat::Rgba8UnormSrgb
                } else {
                    wgpu::TextureFormat::Rgba8Unorm
                },
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            self.texture_generation = self.texture_generation.wrapping_add(1);
            self.texture = Some(FrameTexture {
                texture,
                view,
                generation: self.texture_generation,
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
        // 256-byte row-alignment requirement, so a 240-wide (960 B/row at 4
        // bytes/pixel) GBA frame uploads directly.
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

    /// Compile `effect`'s pipeline if it hasn't been built yet. Called from
    /// `prepare`, before `draw`; already-used effects remain cached.
    fn ensure(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, effect: &'static Effect) {
        if self.compiled.contains_key(effect.id) {
            return;
        }
        let pipeline = effect.compile(device, queue, self.target_format);
        self.compiled.insert(effect.id, pipeline);
    }

    fn prepare_effect(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, effect: &'static Effect) {
        let Some(texture) = &self.texture else {
            return;
        };
        let Some(compiled) = self.compiled.get_mut(effect.id) else {
            return;
        };
        compiled.prepare(device, queue, &texture.view, texture.generation);
    }

    /// Draw the framebuffer as a fullscreen triangle into iced's render
    /// pass, using the pipeline for `effect`. The pass viewport is already
    /// set to the widget bounds, so NDC maps onto the widget.
    fn draw(&self, render_pass: &mut wgpu::RenderPass<'_>, effect: &'static Effect) {
        // Built by `ensure` in `prepare`, which iced runs before `draw`.
        let Some(pipeline) = self.compiled.get(effect.id) else {
            return;
        };
        pipeline.draw(render_pass);
    }
}
