use crate::gui;

pub fn show(ui: &mut egui::Ui, font_families: &gui::FontFamilies, language: &mut unic_langid::LanguageIdentifier) {
    let en_us_label = egui::RichText::new("English").family(font_families.latn.egui.clone());
    let ja_jp_label = egui::RichText::new("日本語").family(font_families.jpan.egui.clone());
    let zh_cn_label = egui::RichText::new("简体中文").family(font_families.hans.egui.clone());
    let zh_tw_label = egui::RichText::new("繁體中文").family(font_families.hant.egui.clone());
    let es_es_label = egui::RichText::new("Español").family(font_families.latn.egui.clone());
    let pt_br_label = egui::RichText::new("Português (Brasil)").family(font_families.latn.egui.clone());
    let fr_fr_label = egui::RichText::new("Français").family(font_families.latn.egui.clone());
    let de_de_label = egui::RichText::new("Deutsch").family(font_families.latn.egui.clone());
    let vi_vn_label = egui::RichText::new("Tiếng Việt").family(font_families.latn.egui.clone());
    let ru_ru_label = egui::RichText::new("Русский").family(font_families.latn.egui.clone());

    egui::ComboBox::from_id_source("settings-window-general-language")
        .width(200.0)
        .selected_text(match &language {
            lang if lang.matches(&unic_langid::langid!("en-US"), false, true) => en_us_label.clone(),
            lang if lang.matches(&unic_langid::langid!("ja-JP"), false, true) => ja_jp_label.clone(),
            lang if lang.matches(&unic_langid::langid!("zh-CN"), false, true) => zh_cn_label.clone(),
            lang if lang.matches(&unic_langid::langid!("zh-TW"), false, true) => zh_tw_label.clone(),
            lang if lang.matches(&unic_langid::langid!("es-ES"), false, true) => es_es_label.clone(),
            lang if lang.matches(&unic_langid::langid!("pt-BR"), false, true) => pt_br_label.clone(),
            lang if lang.matches(&unic_langid::langid!("fr-FR"), false, true) => fr_fr_label.clone(),
            lang if lang.matches(&unic_langid::langid!("de-DE"), false, true) => de_de_label.clone(),
            lang if lang.matches(&unic_langid::langid!("vi-VN"), false, true) => vi_vn_label.clone(),
            lang if lang.matches(&unic_langid::langid!("ru-RU"), false, true) => ru_ru_label.clone(),
            _ => egui::RichText::new(""),
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(language, unic_langid::langid!("en-US"), en_us_label.clone());
            ui.selectable_value(language, unic_langid::langid!("ja-JP"), ja_jp_label.clone());
            ui.selectable_value(language, unic_langid::langid!("zh-CN"), zh_cn_label.clone());
            ui.selectable_value(language, unic_langid::langid!("zh-TW"), zh_tw_label.clone());
            ui.selectable_value(language, unic_langid::langid!("es-ES"), es_es_label.clone());
            ui.selectable_value(language, unic_langid::langid!("pt-BR"), pt_br_label.clone());
            ui.selectable_value(language, unic_langid::langid!("fr-FR"), fr_fr_label.clone());
            ui.selectable_value(language, unic_langid::langid!("de-DE"), de_de_label.clone());
            ui.selectable_value(language, unic_langid::langid!("vi-VN"), vi_vn_label.clone());
            ui.selectable_value(language, unic_langid::langid!("ru-RU"), ru_ru_label.clone());
        });
}
