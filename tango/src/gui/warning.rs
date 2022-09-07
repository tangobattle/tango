const COLOR: egui::Color32 = egui::Color32::from_rgb(0xf4, 0xba, 0x51);
const TEXT: &str = "⚠️";

pub fn show(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.colored_label(COLOR, TEXT).on_hover_text(text)
}

pub fn append_to_layout_job(ui: &egui::Ui, layout_job: &mut egui::text::LayoutJob) {
    layout_job.append(
        &format!("{} ", TEXT),
        0.0,
        egui::TextFormat::simple(
            ui.style()
                .text_styles
                .get(&egui::TextStyle::Body)
                .unwrap()
                .clone(),
            COLOR,
        ),
    );
}
