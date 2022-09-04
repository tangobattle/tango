use fluent_templates::Loader;
use itertools::Itertools;

use crate::{gui, i18n, rom, save};

pub struct State {
    chip_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
    element_icon_texture_cache: std::collections::HashMap<usize, egui::TextureHandle>,
}

impl State {
    pub fn new() -> Self {
        Self {
            chip_icon_texture_cache: std::collections::HashMap::new(),
            element_icon_texture_cache: std::collections::HashMap::new(),
        }
    }
}

struct MaterializedDarkAI {
    secondary_standard_chips: Vec<usize>,
    standard_chips: Vec<usize>,
    mega_chips: Vec<usize>,
    giga_chip: Option<usize>,
    combos: Vec<usize>,
    pa: Option<usize>,
}

impl MaterializedDarkAI {
    fn new(
        dark_ai_view: &Box<dyn save::DarkAIView + '_>,
        assets: &Box<dyn rom::Assets + Send + Sync>,
    ) -> Self {
        let mut use_counts = vec![];
        loop {
            if let Some(count) = dark_ai_view.chip_use_count(use_counts.len()) {
                use_counts.push(count);
            } else {
                break;
            }
        }

        let mut secondary_use_counts = vec![];
        loop {
            if let Some(count) = dark_ai_view.secondary_chip_use_count(secondary_use_counts.len()) {
                secondary_use_counts.push(count);
            } else {
                break;
            }
        }

        MaterializedDarkAI {
            secondary_standard_chips: secondary_use_counts
                .iter()
                .enumerate()
                .filter(|(id, _)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class == rom::ChipClass::Standard)
                        .unwrap_or(false)
                })
                .sorted_by_key(|(_, count)| std::cmp::Reverse(**count))
                .take(3)
                .map(|(id, _)| id)
                .collect(),
            standard_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, _)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class == rom::ChipClass::Standard)
                        .unwrap_or(false)
                })
                .sorted_by_key(|(_, count)| std::cmp::Reverse(**count))
                .take(16)
                .map(|(id, _)| id)
                .collect(),
            mega_chips: use_counts
                .iter()
                .enumerate()
                .filter(|(id, _)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class == rom::ChipClass::Mega)
                        .unwrap_or(false)
                })
                .sorted_by_key(|(_, count)| std::cmp::Reverse(**count))
                .take(5)
                .map(|(id, _)| id)
                .collect(),
            giga_chip: use_counts
                .iter()
                .enumerate()
                .filter(|(id, _)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class == rom::ChipClass::Giga)
                        .unwrap_or(false)
                })
                .max_by_key(|(_, count)| **count)
                .map(|(id, _)| id),
            combos: vec![],
            pa: use_counts
                .iter()
                .enumerate()
                .filter(|(id, _)| {
                    assets
                        .chip(*id)
                        .map(|c| c.class == rom::ChipClass::ProgramAdvance)
                        .unwrap_or(false)
                })
                .max_by_key(|(_, count)| **count)
                .map(|(id, _)| id),
        }
    }
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    dark_ai_view: &Box<dyn save::DarkAIView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    state: &mut State,
) {
}
