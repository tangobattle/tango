#[cfg(feature = "glutin")]
pub mod glutin;
#[cfg(feature = "wgpu")]
pub mod wgpu;

pub trait Backend {
    fn set_ui_scale(&mut self, scale: f32);
    fn window(&self) -> &winit::window::Window;
    fn paint(&mut self);
    fn egui_ctx(&self) -> &egui::Context;
    fn run<'a>(&mut self, run_ui: Box<dyn FnMut(&winit::window::Window, &egui::Context) + 'a>) -> std::time::Duration;
    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse;
}
