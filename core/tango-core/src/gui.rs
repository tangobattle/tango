use egui::Context;

pub struct Gui {
    egui_glow: egui_glow::EguiGlow,
    state: std::sync::Arc<State>,
}

impl Gui {
    pub fn new(window: &winit::window::Window, gl: &glow::Context) -> Self {
        let egui_glow = egui_glow::EguiGlow::new(window, gl);
        Self {
            egui_glow,
            state: std::sync::Arc::new(State::new()),
        }
    }

    pub fn handle_event(&mut self, event: &glutin::event::WindowEvent) -> bool {
        self.egui_glow.on_event(event)
    }

    pub fn render(&mut self, window: &winit::window::Window, gl: &glow::Context) {
        self.egui_glow.run(window, |ctx| {
            self.state.layout(ctx);
        });
        self.egui_glow.paint(window, gl);
    }

    pub fn state(&self) -> std::sync::Arc<State> {
        self.state.clone()
    }
}

pub struct State {
    show_debug: std::sync::atomic::AtomicBool,
    debug_stats_getter: parking_lot::Mutex<Option<Box<dyn Fn() -> Option<DebugStats>>>>,
}

pub struct RoundDebugStats {
    pub local_player_index: u8,
    pub local_qlen: usize,
    pub remote_qlen: usize,
    pub local_delay: u32,
    pub remote_delay: u32,
    pub tps_adjustment: i32,
}

pub struct MatchDebugStats {
    pub round: Option<RoundDebugStats>,
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
        ctx.set_visuals(egui::Visuals::light());
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
                                if let Some(battle_debug_stats) = match_debug_stats.round {
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
