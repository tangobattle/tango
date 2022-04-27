use egui::{ClippedMesh, Context, TexturesDelta};
use egui_wgpu_backend::{BackendError, RenderPass, ScreenDescriptor};
use pixels::{wgpu, PixelsContext};
use winit::window::Window;

pub struct Gui {
    ctx: Context,
    winit_state: egui_winit::State,
    screen_descriptor: ScreenDescriptor,
    rpass: RenderPass,
    paint_jobs: Vec<ClippedMesh>,
    textures: TexturesDelta,
    state: std::sync::Arc<State>,
}

impl Gui {
    pub fn new(width: u32, height: u32, scale_factor: f32, pixels: &pixels::Pixels) -> Self {
        let max_texture_size = pixels.device().limits().max_texture_dimension_2d as usize;

        let ctx = Context::default();
        ctx.set_visuals(egui::Visuals::light());

        let winit_state = egui_winit::State::from_pixels_per_point(max_texture_size, scale_factor);
        let screen_descriptor = ScreenDescriptor {
            physical_width: width,
            physical_height: height,
            scale_factor,
        };
        let rpass = RenderPass::new(pixels.device(), pixels.render_texture_format(), 1);
        let textures = TexturesDelta::default();

        Self {
            ctx,
            winit_state,
            screen_descriptor,
            rpass,
            paint_jobs: Vec::new(),
            textures,
            state: std::sync::Arc::new(State::new()),
        }
    }

    pub fn handle_event(&mut self, event: &winit::event::WindowEvent) -> bool {
        self.winit_state.on_event(&self.ctx, event)
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.screen_descriptor.physical_width = width;
            self.screen_descriptor.physical_height = height;
        }
    }

    pub fn prepare(&mut self, window: &Window) {
        let raw_input = self.winit_state.take_egui_input(window);
        let output = self.ctx.run(raw_input, |ctx| {
            self.state.layout(ctx);
        });

        self.textures.append(output.textures_delta);
        self.winit_state
            .handle_platform_output(window, &self.ctx, output.platform_output);
        self.paint_jobs = self.ctx.tessellate(output.shapes);
    }

    pub fn render(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        render_target: &wgpu::TextureView,
        context: &PixelsContext,
    ) -> Result<(), BackendError> {
        self.rpass
            .add_textures(&context.device, &context.queue, &self.textures)?;
        self.rpass.update_buffers(
            &context.device,
            &context.queue,
            &self.paint_jobs,
            &self.screen_descriptor,
        );

        self.rpass.execute(
            encoder,
            render_target,
            &self.paint_jobs,
            &self.screen_descriptor,
            None,
        )?;

        let textures = std::mem::take(&mut self.textures);
        self.rpass.remove_textures(textures)
    }

    pub fn state(&self) -> std::sync::Arc<State> {
        self.state.clone()
    }
}

pub struct State {
    show_debug: std::sync::atomic::AtomicBool,
    debug_stats_getter: parking_lot::Mutex<Option<Box<dyn Fn() -> Option<DebugStats>>>>,
}

pub struct BattleDebugStats {
    pub local_player_index: u8,
    pub local_qlen: usize,
    pub remote_qlen: usize,
    pub local_delay: u32,
    pub remote_delay: u32,
    pub tps_adjustment: i32,
}

pub struct MatchDebugStats {
    pub battle: Option<BattleDebugStats>,
}

pub struct DebugStats {
    pub fps: f32,
    pub emu_tps: f32,
    pub match_: Option<MatchDebugStats>,
}

impl State {
    pub fn new() -> Self {
        Self {
            show_debug: false.into(),
            debug_stats_getter: parking_lot::Mutex::new(None),
        }
    }

    pub fn set_debug_stats_getter(&self, getter: Option<Box<dyn Fn() -> Option<DebugStats>>>) {
        *self.debug_stats_getter.lock() = getter;
    }

    pub fn toggle_debug(&self) {
        self.show_debug
            .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
    }

    fn layout(&self, ctx: &Context) {
        let mut show_debug = self.show_debug.load(std::sync::atomic::Ordering::Relaxed);
        egui::Window::new("Debug")
            .id(egui::Id::new("debug-window"))
            .open(&mut show_debug)
            .auto_sized()
            .collapsible(false)
            .show(ctx, |ui| {
                if let Some(debug_stats_getter) = &*self.debug_stats_getter.lock() {
                    if let Some(debug_stats) = debug_stats_getter() {
                        egui::Grid::new("debug-grid").num_columns(2).show(ui, |ui| {
                            ui.label("FPS");
                            ui.label(format!("{:.0}", debug_stats.fps));
                            ui.end_row();

                            ui.label("TPS");
                            ui.label(format!("{:.0}", debug_stats.emu_tps));
                            ui.end_row();

                            if let Some(match_debug_stats) = debug_stats.match_ {
                                if let Some(battle_debug_stats) = match_debug_stats.battle {
                                    ui.label("Player index");
                                    ui.label(format!(
                                        "{:.0}",
                                        battle_debug_stats.local_player_index
                                    ));
                                    ui.end_row();

                                    ui.label("TPS adjustment");
                                    ui.label(format!("{:}", battle_debug_stats.tps_adjustment));
                                    ui.end_row();

                                    ui.label("Queue length");
                                    ui.label(format!(
                                        "{} (-{}) vs {} (-{})",
                                        battle_debug_stats.local_qlen,
                                        battle_debug_stats.local_delay,
                                        battle_debug_stats.remote_qlen,
                                        battle_debug_stats.remote_delay,
                                    ));
                                    ui.end_row();
                                }
                            }
                        });
                    }
                }
            });
        self.show_debug
            .store(show_debug, std::sync::atomic::Ordering::Relaxed);
    }
}
