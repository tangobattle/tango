use std::str::FromStr;
use std::sync::Arc;

pub struct FontFamily {
    pub egui: egui::FontFamily,
    pub raw: &'static [u8],
}

impl FontFamily {
    fn new(name: &str, raw: &'static [u8]) -> Self {
        Self {
            egui: egui::FontFamily::Name(name.into()),
            raw,
        }
    }
}

pub struct FontFamilies {
    pub latn: FontFamily,
    pub jpan: FontFamily,
    pub hans: FontFamily,
    pub hant: FontFamily,
}

impl FontFamilies {
    pub fn new() -> Self {
        Self {
            latn: FontFamily::new("Latn", include_bytes!("../fonts/NotoSans-Regular.ttf")),
            jpan: FontFamily::new("Jpan", include_bytes!("../fonts/NotoSansJP-Regular.otf")),
            hans: FontFamily::new("Hans", include_bytes!("../fonts/NotoSansSC-Regular.otf")),
            hant: FontFamily::new("Hant", include_bytes!("../fonts/NotoSansTC-Regular.otf")),
        }
    }

    pub fn for_language(&self, lang: &unic_langid::LanguageIdentifier) -> egui::FontFamily {
        let mut lang = lang.clone();
        lang.maximize();
        match lang.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => self.jpan.egui.clone(),
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => self.hans.egui.clone(),
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => self.hant.egui.clone(),
            _ => self.latn.egui.clone(),
        }
    }

    pub fn resolve_definitions(&self, mut language: unic_langid::LanguageIdentifier) -> egui::FontDefinitions {
        language.maximize();

        let primary_font = match language.script {
            Some(s) if s == unic_langid::subtags::Script::from_str("Jpan").unwrap() => "NotoSansJP-Regular",
            Some(s) if s == unic_langid::subtags::Script::from_str("Hans").unwrap() => "NotoSansSC-Regular",
            Some(s) if s == unic_langid::subtags::Script::from_str("Hant").unwrap() => "NotoSansTC-Regular",
            _ => "NotoSans-Regular",
        };

        let proportional = vec![
            primary_font.to_string(),
            "NotoSans-Regular".to_string(),
            "NotoSansJP-Regular".to_string(),
            "NotoSansSC-Regular".to_string(),
            "NotoSansTC-Regular".to_string(),
            "NotoEmoji-Regular".to_string(),
        ];

        let mut monospace = vec!["NotoSansMono-Regular".to_string()];
        monospace.extend(proportional.clone());

        egui::FontDefinitions {
            font_data: std::collections::BTreeMap::from([
                (
                    "NotoSans-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(self.latn.raw)),
                ),
                (
                    "NotoSansJP-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(self.jpan.raw)),
                ),
                (
                    "NotoSansSC-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(self.hans.raw)),
                ),
                (
                    "NotoSansTC-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(self.hant.raw)),
                ),
                (
                    "NotoSansMono-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(include_bytes!(
                        "../fonts/NotoSansMono-Regular.ttf"
                    ))),
                ),
                (
                    "NotoEmoji-Regular".to_string(),
                    Arc::new(egui::FontData::from_static(include_bytes!(
                        "../fonts/NotoEmoji-Regular.ttf"
                    ))),
                ),
            ]),
            families: std::collections::BTreeMap::from([
                (egui::FontFamily::Proportional, proportional),
                (egui::FontFamily::Monospace, monospace),
                (self.jpan.egui.clone(), vec!["NotoSansJP-Regular".to_string()]),
                (self.hans.egui.clone(), vec!["NotoSansSC-Regular".to_string()]),
                (self.hant.egui.clone(), vec!["NotoSansTC-Regular".to_string()]),
                (self.latn.egui.clone(), vec!["NotoSans-Regular".to_string()]),
            ]),
        }
    }
}
