// adapted from egui_kittest: https://github.com/emilk/egui/tree/master/crates/egui_kittest

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender};

// for clean text, while avoiding issues with egui using the incorrect zoom factor on the first frame
const BAKED_PIXELS_PER_POINT: f32 = 2.0;
// for resolving the clip rect of any side with 0 length
const DEFAULT_SCREEN_LEN: u32 = 10000;

enum ToThread {
    Paint {
        width: u32,
        height: u32,
        pixels_per_point: f32,
        textures_delta: egui::TexturesDelta,
        primitives: Vec<egui::epaint::ClippedPrimitive>,
    },
    CaptureImage(Box<dyn Send + Sync + FnOnce(image::RgbaImage)>),
    FreeTexture,
}

pub struct OffscreenUi {
    ctx: egui::Context,
    width: u32,
    height: u32,
    sender: Sender<ToThread>,
}

impl OffscreenUi {
    pub fn new() -> Self {
        let (main_sender, thread_receiver) = std::sync::mpsc::channel();

        thread::OffscreenUiThread::spawn(thread_receiver);

        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);

        Self {
            ctx,
            width: 0,
            height: 0,
            sender: main_sender,
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    /// Any field set to 0 will automatically resolve length
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = (width as f32 * BAKED_PIXELS_PER_POINT) as _;
        self.height = (height as f32 * BAKED_PIXELS_PER_POINT) as _;
    }

    pub fn run(&mut self, mut run_ui: impl FnMut(&mut egui::Ui)) {
        let mut width = self.width;
        let mut height = self.height;

        let raw_input = egui::RawInput {
            screen_rect: Some(egui::Rect {
                min: egui::Pos2::ZERO,
                max: egui::Pos2::new(
                    if width == 0 { DEFAULT_SCREEN_LEN } else { self.width } as f32,
                    if height == 0 { DEFAULT_SCREEN_LEN } else { self.height } as f32,
                ) / BAKED_PIXELS_PER_POINT,
            }),
            viewports: HashMap::from_iter([(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    native_pixels_per_point: Some(BAKED_PIXELS_PER_POINT),
                    ..Default::default()
                },
            )]),
            ..Default::default()
        };

        // mutably borrowing to avoid moving everything into the closure
        let run_ui = &mut run_ui;

        let egui::FullOutput {
            platform_output: _,
            textures_delta,
            shapes,
            pixels_per_point,
            viewport_output: _,
        } = self.ctx.run(raw_input, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                let response = ui.scope(|ui| run_ui(ui)).response;

                if width == 0 {
                    width = (response.rect.width() * BAKED_PIXELS_PER_POINT) as _;
                }

                if height == 0 {
                    height = (response.rect.height() * BAKED_PIXELS_PER_POINT) as _;
                }
            });
        });

        let primitives = self.ctx.tessellate(shapes, pixels_per_point);
        let _ = self.sender.send(ToThread::Paint {
            width,
            height,
            pixels_per_point,
            textures_delta,
            primitives,
        });
    }

    pub fn capture_image(&self, f: impl Send + Sync + FnOnce(image::RgbaImage) + 'static) {
        let _ = self.sender.send(ToThread::CaptureImage(Box::new(f)));
    }

    pub fn copy_to_clipboard(&self) {
        self.capture_image(|image| {
            let Ok(mut clipboard) = arboard::Clipboard::new() else {
                log::error!("failed to instantiate clipboard");
                return;
            };

            let _ = clipboard.set_image(arboard::ImageData {
                width: image.width() as usize,
                height: image.height() as usize,
                bytes: std::borrow::Cow::Borrowed(&image),
            });
        });
    }

    pub fn sweep(&self) {
        let _ = self.sender.send(ToThread::FreeTexture);
    }
}

// rendering is moved to another thread, as some graphics backends complain about multiple contexts on the same thread
#[cfg(not(feature = "wgpu"))]
mod thread {
    use super::{Receiver, ToThread};

    pub struct OffscreenUiThread {}

    impl OffscreenUiThread {
        pub fn spawn(_thread_receiver: Receiver<ToThread>, _width: u32, _height: u32) {}
    }
}

#[cfg(feature = "wgpu")]
mod thread {
    use super::{Receiver, ToThread};

    pub struct OffscreenUiThread {
        width: u32,
        height: u32,
        render_state: egui_wgpu::RenderState,
        texture: Option<wgpu::Texture>,
    }

    impl OffscreenUiThread {
        pub fn spawn(thread_receiver: Receiver<ToThread>) {
            std::thread::spawn(move || {
                let mut data = match OffscreenUiThread::new() {
                    Ok(data) => data,
                    Err(err) => {
                        log::error!("failed to create offscreen ui thread: {err:?}");
                        return;
                    }
                };

                while let Ok(message) = thread_receiver.recv() {
                    match message {
                        ToThread::Paint {
                            width,
                            height,
                            textures_delta,
                            primitives,
                            pixels_per_point,
                        } => {
                            data.width = width;
                            data.height = height;
                            data.paint(width, height, pixels_per_point, textures_delta, primitives)
                        }
                        ToThread::CaptureImage(callback) => {
                            callback(data.capture_image());
                        }
                        ToThread::FreeTexture => {
                            data.texture = None;
                        }
                    }
                }
            });
        }

        fn new() -> anyhow::Result<Self> {
            let config = crate::graphics::wgpu::Backend::wgpu_configuration();
            let instance = pollster::block_on(config.wgpu_setup.new_instance());
            let render_state =
                pollster::block_on(egui_wgpu::RenderState::create(&config, &instance, None, None, 1, false))?;

            Ok(Self {
                width: 0,
                height: 0,
                render_state,
                texture: None,
            })
        }

        fn ensure_texture(&mut self, width: u32, height: u32) {
            if self.width == 0 || self.height == 0 {
                self.texture = None;
                return;
            }

            let current = self.texture.as_ref();
            if current.is_some_and(|t| t.width() == width && t.height() == height) {
                return;
            }

            self.texture = Some(self.render_state.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Offscreen Texture"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.render_state.target_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            }));
        }

        fn paint(
            &mut self,
            width: u32,
            height: u32,
            pixels_per_point: f32,
            textures_delta: egui::TexturesDelta,
            tessellated: Vec<egui::epaint::ClippedPrimitive>,
        ) {
            self.ensure_texture(width, height);

            let mut renderer = self.render_state.renderer.write();

            // update textures
            for (id, image) in &textures_delta.set {
                renderer.update_texture(&self.render_state.device, &self.render_state.queue, *id, image);
            }

            // painting
            if let Some(texture) = &self.texture {
                let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

                let mut encoder = self
                    .render_state
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("Offscreen Command Encoder"),
                    });

                let screen_descriptor = egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [width, height],
                    pixels_per_point,
                };

                let user_buffers = renderer.update_buffers(
                    &self.render_state.device,
                    &self.render_state.queue,
                    &mut encoder,
                    &tessellated,
                    &screen_descriptor,
                );

                let mut pass = encoder
                    .begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("Offscreen Render Pass"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &texture_view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        ..Default::default()
                    })
                    .forget_lifetime();

                renderer.render(&mut pass, &tessellated, &screen_descriptor);
                std::mem::drop(pass);

                let command_buffers = user_buffers.into_iter().chain(std::iter::once(encoder.finish()));
                self.render_state.queue.submit(command_buffers);

                self.render_state.device.poll(wgpu::Maintain::Wait);
            }

            // free textures
            for id in &textures_delta.free {
                renderer.free_texture(id);
            }
        }

        fn capture_image(&self) -> image::RgbaImage {
            let Some(texture) = &self.texture else {
                return image::RgbaImage::default();
            };

            let device = &self.render_state.device;
            let queue = &self.render_state.queue;

            let buffer_dimensions = BufferDimensions::new(texture.width() as usize, texture.height() as usize);

            let output_buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Texture to bytes output buffer"),
                size: (buffer_dimensions.padded_bytes_per_row * buffer_dimensions.height) as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Texture to bytes encoder"),
            });

            // Copy the data from the texture to the buffer
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &output_buffer,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(buffer_dimensions.padded_bytes_per_row as u32),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: texture.width(),
                    height: texture.height(),
                    depth_or_array_layers: 1,
                },
            );

            let submission_index = queue.submit([encoder.finish()]);

            // Note that we're not calling `.await` here.
            let buffer_slice = output_buffer.slice(..);
            // Sets the buffer up for mapping, sending over the result of the mapping back to us when it is finished.
            let (sender, receiver) = std::sync::mpsc::channel();
            buffer_slice.map_async(wgpu::MapMode::Read, move |v| drop(sender.send(v)));

            // Poll the device in a blocking manner so that our future resolves.
            device.poll(wgpu::Maintain::WaitForSubmissionIndex(submission_index));

            receiver.recv().unwrap().unwrap();
            let buffer_slice = output_buffer.slice(..);
            let data = buffer_slice.get_mapped_range();
            let data = data
                .chunks_exact(buffer_dimensions.padded_bytes_per_row)
                .flat_map(|row| row.iter().take(buffer_dimensions.unpadded_bytes_per_row))
                .copied()
                .collect::<Vec<_>>();

            image::RgbaImage::from_raw(texture.width(), texture.height(), data).expect("Failed to create image")
        }
    }

    struct BufferDimensions {
        height: usize,
        unpadded_bytes_per_row: usize,
        padded_bytes_per_row: usize,
    }

    impl BufferDimensions {
        fn new(width: usize, height: usize) -> Self {
            let bytes_per_pixel = size_of::<u32>();
            let unpadded_bytes_per_row = width * bytes_per_pixel;
            let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
            let padded_bytes_per_row_padding = (align - unpadded_bytes_per_row % align) % align;
            let padded_bytes_per_row = unpadded_bytes_per_row + padded_bytes_per_row_padding;
            Self {
                height,
                unpadded_bytes_per_row,
                padded_bytes_per_row,
            }
        }
    }
}
