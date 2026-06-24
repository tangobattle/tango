use super::*;
use sweeten::widget::{column, row};

pub(super) fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();
    // Every game has a player navi with a base max HP. Games with a link-navi
    // roster (BN5/BN6/EXE4.5) also report which navi is equipped (id + emblem +
    // name); the rest (BN1–4) have no navi to pick, so the card is just the HP.
    let navi = loaded.save.view_navi();
    let navi_id = navi.as_ref().map(|nv| nv.navi());
    let base_max_hp = navi.as_ref().map(|nv| nv.base_max_hp(assets));
    render_navi_card::<M>(lang, loaded, navi_id, base_max_hp)
}

/// The navi card: the equipped navi's emblem + name (for games with a
/// link-navi roster) over its base max HP. The navi-less games (BN1–4) drop
/// the emblem/name and the card is just the HP stat.
fn render_navi_card<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    navi_id: Option<usize>,
    base_max_hp: Option<u16>,
) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();

    // Only the games with a real navi roster (BN5/BN6/EXE4.5) get an emblem +
    // name plate. BN1–4 report a placeholder navi the ROM has no entry for, so
    // their card is just the HP.
    let roster_navi = navi_id.filter(|&id| assets.navi(id).is_some());

    // Plate/glow tint: the equipped emblem's own signature color, with a
    // neutral slate fallback (also used by the navi-less games).
    let accent = roster_navi
        .and_then(|id| loaded.navi_accents.get(&id).copied())
        .unwrap_or(iced::Color::from_rgb8(0x6b, 0x7a, 0x99));

    let mut card = column![].spacing(16).align_x(Alignment::Center);

    if let Some(navi_id) = roster_navi {
        let name = assets
            .navi(navi_id)
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("Navi #{navi_id}"));

        // Emblem at an integer multiple of its 15px crop so the
        // nearest-neighbor upscale lands on even pixels.
        let emblem: Element<'static, M> = loaded
            .navi_emblems
            .get(&navi_id)
            .cloned()
            .map(|h| {
                Image::new(h)
                    .width(Length::Fixed(90.0))
                    .height(Length::Fixed(90.0))
                    .filter_method(iced_image::FilterMethod::Nearest)
                    .content_fit(ContentFit::Contain)
                    .into()
            })
            .unwrap_or_else(|| {
                Space::new()
                    .width(Length::Fixed(90.0))
                    .height(Length::Fixed(90.0))
                    .into()
            });

        // Circular plate behind the emblem: accent-tinted fill, a
        // ring a shade brighter, and an accent glow lifting it off
        // the pane.
        let plate: Element<'static, M> = container(emblem)
            .width(Length::Fixed(140.0))
            .height(Length::Fixed(140.0))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .style(move |theme: &iced::Theme| {
                let bg = theme.palette().background;
                container::Style {
                    background: Some(iced::Background::Color(crate::widgets::mix(bg, accent, 0.22))),
                    border: iced::Border {
                        radius: 70.0.into(),
                        width: 2.0,
                        color: iced::Color { a: 0.8, ..accent },
                    },
                    shadow: iced::Shadow {
                        color: iced::Color { a: 0.45, ..accent },
                        offset: iced::Vector::new(0.0, 0.0),
                        blur_radius: 26.0,
                    },
                    ..Default::default()
                }
            })
            .into();

        card = card.push(plate).push(text(name).size(TEXT_DISPLAY));
    }

    // Every game's navi has a base max HP — the card's centerpiece for the
    // roster-less games, a stat beneath the name for the rest.
    if let Some(hp) = base_max_hp {
        card = card.push(hp_stat::<M>(lang, hp));
    }

    // The pane itself picks up a whisper of the accent, fading
    // back to the standard plate color toward the bottom.
    container(card)
        .width(Fill)
        .align_x(Alignment::Center)
        .padding([28.0, crate::style::PANE_PADDING])
        .style(move |theme: &iced::Theme| {
            let mut s = crate::widgets::pane(theme);
            if let Some(iced::Background::Color(plate_color)) = s.background {
                // Stop 0 sits at the bottom for a 0-radian linear
                // gradient, so the accent goes on stop 1 — the
                // tint halos the plate at the top of the card.
                s.background = Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, plate_color)
                        .add_stop(1.0, crate::widgets::mix(plate_color, accent, 0.10)),
                )));
            }
            s
        })
        .into()
}

/// The navi's base max HP as a centered stat block: a muted "Base Max HP"
/// caption over the value. The card's centerpiece for the roster-less games
/// (BN1–4), a secondary stat beneath the name for the rest.
fn hp_stat<M: 'static>(lang: &LanguageIdentifier, hp: u16) -> Element<'static, M> {
    column![
        text(t!(lang, "navi-base-hp"))
            .size(TEXT_CAPTION)
            .style(muted_text_style),
        text(format!("{hp}")).size(TEXT_DISPLAY),
    ]
    .spacing(2)
    .align_x(Alignment::Center)
    .into()
}

/// The Navi tab as text: the equipped navi's name (for games with a link-navi
/// roster) and its base max HP. Every game has a navi with HP, so this always
/// returns something.
pub(crate) fn navi_as_text(lang: &LanguageIdentifier, loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let navi = loaded.save.view_navi()?;
    let mut out = String::new();
    // Only a real roster navi (BN5/BN6/EXE4.5) has a name; BN1–4 report a
    // placeholder the ROM has no entry for, so they lead straight with the HP.
    if assets.navi(navi.navi()).is_some() {
        let name = assets
            .navi(navi.navi())
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("#{}", navi.navi()));
        out.push_str(&name);
        out.push('\n');
    }
    out.push_str(&t!(lang, "navi-base-hp"));
    out.push('\t');
    out.push_str(&navi.base_max_hp(assets).to_string());
    Some(out)
}

/// The Navi editor: a grid of the game's navis, each on its own
/// accent-tinted emblem plate (the equipped one lit up with a glow ring).
/// Clicking a plate emits [`Action::SetNavi`], which the embedder stages
/// into the loaded save.
pub(super) fn render_navi_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded) -> Element<'a, Action> {
    let assets = loaded.assets.as_ref();
    let current = loaded.save.view_navi().map(|nv| nv.navi());

    // The dataview lays the roster out in rows; render one plate per navi in
    // exactly that order.
    let mut grid = column![].spacing(14).align_x(Alignment::Center);
    for &order_row in assets.navi_order() {
        let mut r = row![].spacing(14).align_y(Alignment::Start);
        for &id in order_row {
            let name = assets
                .navi(id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{id}"));
            r = r.push(navi_cell(loaded, id, name, current == Some(id)));
        }
        grid = grid.push(r);
    }

    let body = column![
        text(t!(lang, "navi-edit-select"))
            .size(TEXT_BODY)
            .style(muted_text_style),
        grid,
    ]
    .spacing(16)
    .align_x(Alignment::Center)
    .width(Fill);

    container(
        scrollable(body)
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill),
    )
    .padding(crate::style::PANE_PADDING)
    .style(crate::widgets::pane)
    .width(Fill)
    .height(Fill)
    .into()
}

/// One selectable navi: its emblem on a circular accent-tinted plate (lit
/// with a glow ring when it's the equipped navi), the name beneath, all
/// wrapped in a borderless button that emits [`Action::SetNavi`].
fn navi_cell(loaded: &Loaded, id: usize, name: String, selected: bool) -> Element<'static, Action> {
    let accent = loaded
        .navi_accents
        .get(&id)
        .copied()
        .unwrap_or(iced::Color::from_rgb8(0x6b, 0x7a, 0x99));

    let emblem: Element<'static, Action> = loaded
        .navi_emblems
        .get(&id)
        .cloned()
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(48.0))
                .height(Length::Fixed(48.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                // The equipped navi stays full-color; the rest dim back so
                // it's the only vivid emblem in the grid.
                .opacity(if selected { 1.0 } else { 0.45 })
                .into()
        })
        .unwrap_or_else(|| {
            Space::new()
                .width(Length::Fixed(48.0))
                .height(Length::Fixed(48.0))
                .into()
        });

    let plate = container(emblem)
        .width(Length::Fixed(72.0))
        .height(Length::Fixed(72.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |theme: &iced::Theme| {
            let bg = theme.palette().background;
            container::Style {
                background: Some(iced::Background::Color(crate::widgets::mix(
                    bg,
                    accent,
                    if selected { 0.40 } else { 0.14 },
                ))),
                border: iced::Border {
                    radius: 36.0.into(),
                    width: if selected { 2.0 } else { 1.0 },
                    color: iced::Color {
                        a: if selected { 0.9 } else { 0.3 },
                        ..accent
                    },
                },
                shadow: if selected {
                    iced::Shadow {
                        color: iced::Color { a: 0.5, ..accent },
                        offset: iced::Vector::new(0.0, 0.0),
                        blur_radius: 16.0,
                    }
                } else {
                    iced::Shadow::default()
                },
                ..Default::default()
            }
        });

    let mut label = text(name).size(TEXT_CAPTION);
    if !selected {
        label = label.style(muted_text_style);
    }

    let cell = column![plate, label]
        .spacing(6)
        .align_x(Alignment::Center)
        .width(Length::Fixed(88.0));

    button(cell)
        .padding(4)
        .on_press(Action::SetNavi(id))
        .style(|theme: &iced::Theme, _status| button::Style {
            background: None,
            text_color: theme.palette().text,
            ..Default::default()
        })
        .into()
}
