use crate::gui;

pub fn show(ui: &mut egui::Ui, font_families: &gui::FontFamilies, language: &mut unic_langid::LanguageIdentifier) {
    let languages = &[
        (
            unic_langid::langid!("en-US"),
            egui::RichText::new("English (United States)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("ja-JP"),
            egui::RichText::new("日本語").family(font_families.jpan.egui.clone()),
        ),
        (
            unic_langid::langid!("zh-CN"),
            egui::RichText::new("简体中文").family(font_families.hans.egui.clone()),
        ),
        (
            unic_langid::langid!("zh-TW"),
            egui::RichText::new("繁體中文").family(font_families.hant.egui.clone()),
        ),
        (
            unic_langid::langid!("es-419"),
            egui::RichText::new("Español (Latinoamérica)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("pt-BR"),
            egui::RichText::new("Português (Brasil)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("fr-FR"),
            egui::RichText::new("Français (France)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("de-DE"),
            egui::RichText::new("Deutsch (Deutschland)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("vi-VN"),
            egui::RichText::new("Tiếng Việt").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("ru-RU"),
            egui::RichText::new("Русский (Россия)").family(font_families.latn.egui.clone()),
        ),
        (
            unic_langid::langid!("nl-NL"),
            egui::RichText::new("Nederlands (Nederland)").family(font_families.latn.egui.clone()),
        ),
    ];

    egui::ComboBox::from_id_source("settings-window-general-language")
        .width(200.0)
        .selected_text(
            languages
                .iter()
                .find(|(lang, _)| language.matches(lang, false, false))
                .map(|(_, label)| label.clone())
                .unwrap_or_else(|| egui::RichText::new("")),
        )
        .show_ui(ui, |ui| {
            for (lang, label) in languages.iter() {
                ui.selectable_value(language, lang.clone(), label.clone());
            }
        });
}
