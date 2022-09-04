use fluent_templates::Loader;

use crate::{gui, i18n, rom, save};

pub struct State {}

impl State {
    pub fn new() -> Self {
        Self {}
    }
}

fn navicust_part_colors(color: &rom::NavicustPartColor) -> (egui::Color32, egui::Color32) {
    match color {
        rom::NavicustPartColor::Red => (
            egui::Color32::from_rgb(0xde, 0x10, 0x00),
            egui::Color32::from_rgb(0xbd, 0x00, 0x00),
        ),
        rom::NavicustPartColor::Pink => (
            egui::Color32::from_rgb(0xde, 0x8c, 0xc6),
            egui::Color32::from_rgb(0xbd, 0x6b, 0xa5),
        ),
        rom::NavicustPartColor::Yellow => (
            egui::Color32::from_rgb(0xde, 0xde, 0x00),
            egui::Color32::from_rgb(0xbd, 0xbd, 0x00),
        ),
        rom::NavicustPartColor::Green => (
            egui::Color32::from_rgb(0x18, 0xc6, 0x00),
            egui::Color32::from_rgb(0x00, 0xa5, 0x00),
        ),
        rom::NavicustPartColor::Blue => (
            egui::Color32::from_rgb(0x29, 0x84, 0xde),
            egui::Color32::from_rgb(0x08, 0x60, 0xb8),
        ),
        rom::NavicustPartColor::White => (
            egui::Color32::from_rgb(0xde, 0xde, 0xde),
            egui::Color32::from_rgb(0xbd, 0xbd, 0xbd),
        ),
        rom::NavicustPartColor::Orange => (
            egui::Color32::from_rgb(0xde, 0x7b, 0x00),
            egui::Color32::from_rgb(0xde, 0x7b, 0x00),
        ),
        rom::NavicustPartColor::Purple => (
            egui::Color32::from_rgb(0x94, 0x00, 0xce),
            egui::Color32::from_rgb(0x94, 0x00, 0xce),
        ),
        rom::NavicustPartColor::Gray => (
            egui::Color32::from_rgb(0x84, 0x84, 0x84),
            egui::Color32::from_rgb(0x84, 0x84, 0x84),
        ),
    }
}

fn show_part_name(
    ui: &mut egui::Ui,
    name: egui::RichText,
    is_enabled: bool,
    color: &rom::NavicustPartColor,
) {
    egui::Frame::none()
        .inner_margin(egui::style::Margin::symmetric(4.0, 0.0))
        .rounding(egui::Rounding::same(2.0))
        .fill(if is_enabled {
            navicust_part_colors(color).0
        } else {
            egui::Color32::from_rgb(0xbd, 0xbd, 0xbd)
        })
        .show(ui, |ui| {
            ui.label(name.color(egui::Color32::BLACK));
        });
}

pub fn show<'a>(
    ui: &mut egui::Ui,
    clipboard: &mut arboard::Clipboard,
    font_families: &gui::FontFamilies,
    lang: &unic_langid::LanguageIdentifier,
    game_lang: &unic_langid::LanguageIdentifier,
    navicust_view: &Box<dyn save::NavicustView<'a> + 'a>,
    assets: &Box<dyn rom::Assets + Send + Sync>,
    _state: &mut State,
) {
    const NCP_CHIP_WIDTH: f32 = 150.0;

    let items = (0..navicust_view.count())
        .flat_map(|i| {
            navicust_view.navicust_part(i).and_then(|ncp| {
                assets
                    .navicust_part(ncp.id, ncp.variant)
                    .and_then(|info| info.color.as_ref().map(|color| (info, color)))
            })
        })
        .collect::<Vec<_>>();

    let style = navicust_view.style().and_then(|id| assets.style(id));

    ui.horizontal(|ui| {
        if ui
            .button(format!(
                "📋 {}",
                i18n::LOCALES.lookup(lang, "copy-to-clipboard").unwrap(),
            ))
            .clicked()
        {
            let mut buf = vec![];
            if let Some(style) = style {
                buf.push(style.name.clone());
            }
            buf.extend(
                itertools::Itertools::zip_longest(
                    items
                        .iter()
                        .filter(|(info, _)| info.is_solid)
                        .map(|(info, _)| info.name.as_str()),
                    items
                        .iter()
                        .filter(|(info, _)| !info.is_solid)
                        .map(|(info, _)| info.name.as_str()),
                )
                .map(|v| match v {
                    itertools::EitherOrBoth::Both(l, r) => format!("{}\t{}", l, r),
                    itertools::EitherOrBoth::Left(l) => format!("{}\t", l),
                    itertools::EitherOrBoth::Right(r) => format!("\t{}", r),
                }),
            );
            let _ = clipboard.set_text(buf.join("\n"));
        }
    });

    if let Some(style) = style {
        ui.label(&style.name);
    }

    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.set_width(NCP_CHIP_WIDTH);
            for (info, color) in items.iter().filter(|(info, _)| info.is_solid) {
                show_part_name(
                    ui,
                    egui::RichText::new(&info.name).family(font_families.for_language(game_lang)),
                    true,
                    color,
                );
            }
        });
        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            ui.set_width(NCP_CHIP_WIDTH);
            for (info, color) in items.iter().filter(|(info, _)| !info.is_solid) {
                show_part_name(
                    ui,
                    egui::RichText::new(&info.name).family(font_families.for_language(game_lang)),
                    true,
                    color,
                );
            }
        });
    });
}
