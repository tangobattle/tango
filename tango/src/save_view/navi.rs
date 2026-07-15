use super::*;
use sweeten::widget::{column, row};

pub(super) fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    container(navi_card_content::<M>(lang, loaded, false))
        .width(Fill)
        .align_x(Alignment::Center)
        .padding(crate::ui::style::PANE_PADDING)
        .style(crate::ui::widgets::pane)
        .into()
}

/// The navi card's inner content (no pane wrapper): emblem on the left, the
/// navi's name stacked over its stats on the right — base max HP and, where the
/// game exposes them (BN6), the live MegaBuster levels. The navi-less games
/// (BN1–4) drop the emblem/name and just show the base HP. With `editing_hint`
/// set, a small pencil sits by the name to signal the card is the change-navi
/// button.
fn navi_card_content<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    editing_hint: bool,
) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();
    // Every game has a player navi with a base max HP. Games with a link-navi
    // roster (BN5/BN6/EXE4.5) also report which navi is equipped (id + emblem +
    // name); the rest (BN1–4) have no navi to pick.
    let navi = loaded.save.view_navi();
    let navi_id = navi.as_ref().map(|nv| nv.navi());
    let base_max_hp = navi.as_ref().map(|nv| nv.max_hp(assets));
    let buster = navi.as_ref().and_then(|nv| nv.buster_stats(assets));

    // Only the games with a real navi roster (BN5/BN6/EXE4.5) get an emblem +
    // name. BN1–4 report a placeholder navi the ROM has no entry for, so they
    // just show the HP.
    let roster_navi = navi_id.filter(|&id| assets.navi(id).is_some());

    if let Some(navi_id) = roster_navi {
        let name = assets
            .navi(navi_id)
            .and_then(|n| n.name())
            .unwrap_or_else(|| format!("Navi #{navi_id}"));

        // Flat emblem (no plate/glow), sized to an integer multiple of its 15px
        // crop so the nearest-neighbor upscale lands on even pixels.
        let emblem: Element<'static, M> = loaded
            .navi_emblems
            .get(&navi_id)
            .cloned()
            .map(|h| {
                Image::new(h)
                    .width(Length::Fixed(45.0))
                    .height(Length::Fixed(45.0))
                    .filter_method(iced_image::FilterMethod::Nearest)
                    .content_fit(ContentFit::Contain)
                    .into()
            })
            .unwrap_or_else(|| {
                Space::new()
                    .width(Length::Fixed(45.0))
                    .height(Length::Fixed(45.0))
                    .into()
            });

        // Stats on their own row beneath the name: base max HP, then the
        // MegaBuster levels (attack / rapid / charge) as a tight group. Each
        // label bottom-aligns with its value.
        let mut stats = row![].spacing(16).align_y(Alignment::End);
        if let Some(hp) = base_max_hp {
            stats = stats.push(stat_inline::<M>(t!(lang, "navi-base-hp"), hp.to_string()));
        }
        if let Some(b) = buster {
            stats = stats.push(
                row![
                    stat_inline::<M>(t!(lang, "navi-buster-attack"), b.attack.to_string()),
                    stat_inline::<M>(t!(lang, "navi-buster-rapid"), b.speed.to_string()),
                    stat_inline::<M>(t!(lang, "navi-buster-charge"), b.charge.to_string()),
                ]
                .spacing(12)
                .align_y(Alignment::End),
            );
        }

        // Emblem on the left, name stacked over its stats on the right. While
        // editing, a pencil glyph trails the name as the change-navi cue.
        let name_el: Element<'static, M> = if editing_hint {
            row![
                text(name).size(style::TEXT_TITLE),
                lucide_icons::Icon::Pencil
                    .widget()
                    .size(TEXT_CAPTION)
                    .style(muted_text_style),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        } else {
            text(name).size(style::TEXT_TITLE).into()
        };
        let info = column![name_el, stats].spacing(4);
        row![emblem, info].spacing(10).align_y(Alignment::Center).into()
    } else {
        // Roster-less games (BN1–4): just the base max HP, inline.
        let mut card = row![].spacing(14).align_y(Alignment::Center);
        if let Some(hp) = base_max_hp {
            card = card.push(stat_inline::<M>(t!(lang, "navi-base-hp"), hp.to_string()));
        }
        card.into()
    }
}

/// The persistent navi identity strip shown above the tab body (it replaces the
/// old standalone Navi tab) and the home for the save's primary actions. The
/// card on the left, the `actions` cluster (Edit / Play, or Save / Cancel while
/// editing) on the right. When `edit` is `Some`, the card itself becomes the
/// change-navi button (a flat press target with a pencil hint), opening the
/// picker as the body — there's no separate Edit pencil to confuse it with.
pub(super) fn render_navi_strip<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    edit: Option<Action>,
    actions: Element<'a, Action>,
) -> Element<'a, Action> {
    // The pane pads its content off the edges with a uniform `6`; the card
    // carries the rest of the left inset (`[4, 6]`, so the content sits at 12px
    // horizontal / 10px vertical, with that room inside the change-navi button's
    // hover-highlight area). Both card modes pad identically so toggling edit
    // doesn't nudge it: flat press target in edit mode (with a pencil cue), plain
    // container otherwise.
    let card: Element<'a, Action> = match edit {
        Some(action) => button(navi_card_content::<Action>(lang, loaded, true))
            .padding([4.0, 6.0])
            .style(crate::ui::widgets::flat)
            .on_press(action)
            .into(),
        None => container(navi_card_content::<Action>(lang, loaded, false))
            .padding([4.0, 6.0])
            .into(),
    };
    // A little horizontal breathing room for the actions cluster (none
    // vertically — the row centers it).
    let actions = container(actions).padding([0.0, 4.0]);
    container(
        row![card, Space::new().width(Fill), actions]
            .align_y(Alignment::Center)
            .width(Fill),
    )
    .padding(6.0)
    .width(Fill)
    .style(crate::ui::widgets::pane)
    .into()
}

/// One stat as a tight inline pair: a muted label with its value flush beside
/// it (no stretched gap).
fn stat_inline<M: 'static>(label: String, value: String) -> Element<'static, M> {
    row![
        text(label).size(TEXT_CAPTION).style(muted_text_style),
        text(value).size(style::TEXT_HEADING),
    ]
    .spacing(5)
    .align_y(Alignment::End)
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
    out.push_str(&navi.max_hp(assets).to_string());
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

    // Pad the content, not the pane: that keeps the scrollbar flush with the
    // pane's right edge like every other editor pane (a `.padding()` on the
    // outer container would inset the whole scrollable, scrollbar included).
    container(
        scrollable(container(body).padding(crate::ui::style::PANE_PADDING).width(Fill))
            .style(crate::ui::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill),
    )
    .style(crate::ui::widgets::pane)
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
                background: Some(iced::Background::Color(crate::ui::widgets::mix(
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
