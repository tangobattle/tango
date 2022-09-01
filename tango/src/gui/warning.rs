pub fn show(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.colored_label(egui::Color32::from_rgb(0xf4, 0xba, 0x51), "⚠️")
        .on_hover_text(text)
}
