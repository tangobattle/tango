#[cfg(feature = "glutin")]
pub mod glutin;
pub mod offscreen;
#[cfg(feature = "wgpu")]
pub mod wgpu;

pub trait Backend {
    fn recreate_window(
        &mut self,
        event_loop: &winit::event_loop::ActiveEventLoop,
        window_attributes: winit::window::WindowAttributes,
    );
    fn window(&self) -> &winit::window::Window;
    fn paint(&mut self);
    fn egui_ctx(&self) -> &egui::Context;
    fn run(&mut self, run_ui: &mut dyn FnMut(&egui::Context)) -> std::time::Duration;
    fn on_window_event(&mut self, event: &winit::event::WindowEvent) -> egui_winit::EventResponse;
    fn exiting(&mut self) {}
    fn should_take_on_exit(&mut self) -> bool;
}
