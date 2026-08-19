//! The widget helpers both sides draw with: icon-buttons (with
//! tooltips), the tab pill, the button and text styles (`neutral`,
//! `list_item`, `muted_text_style`), the `pane` plate everything sits
//! on, and the chunky form controls. Icon glyphs come straight from
//! the `lucide-icons` crate — call sites pass `Icon::Foo` directly.
//!
//! Chrome only one frontend wears (the app's HUD bars and cyberworld
//! backdrop, the editor's zebra rows) lives in that frontend's own
//! `widgets` module, which re-exports this one.

use crate::style::{TEXT_BODY, TEXT_CAPTION, TEXT_HEADING};
use iced::widget::{button, container, text, tooltip};
use iced::{Alignment, Element, Theme};
use lucide_icons::Icon;
use sweeten::widget::row;

/// Icon-only button for low-emphasis toolbar actions (rescan,
/// copy, open-folder, etc.). Uses [`neutral`] — a soft, theme-
/// aware style that doesn't compete with primary CTAs in the
/// same row. The plain-text label is exposed as a hover tooltip.
pub fn icon_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, padding: [f32; 2]) -> Element<'a, M> {
    icon_button_styled(icon, label, Some(msg), padding, neutral)
}

/// `icon_button` with the on_press wrapped in an Option so callers
/// can render a disabled (greyed-out, no on_press) variant without
/// duplicating the chrome.
pub fn icon_button_maybe<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: Option<M>,
    padding: [f32; 2],
) -> Element<'a, M> {
    icon_button_styled(icon, label, msg, padding, neutral)
}

/// Icon button for clipboard copies, with feedback: once the copy
/// actually lands, the update path calls
/// [`crate::copy_feedback::flash`] with this button's `key`, and
/// until the flash expires the glyph flips to a primary-tinted
/// clipboard-check and the tooltip to `copied_label` ("Copied!").
/// `icon` is the idle glyph — ClipboardCopy for plain copies,
/// something more specific (ImageDown) where the payload kind needs
/// distinguishing. `key` must be stable and unique per button — see
/// [`crate::copy_feedback`].
pub fn copy_icon_button<'a, M: Clone + 'a>(
    key: &str,
    icon: Icon,
    icon_size: f32,
    label: String,
    copied_label: String,
    msg: Option<M>,
    padding: [f32; 2],
) -> Element<'a, M> {
    let lit = crate::copy_feedback::is_lit(key);
    let (glyph, tip) = if lit {
        (Icon::ClipboardCheck, copied_label)
    } else {
        (icon, label)
    };
    let mut glyph_el = glyph.widget().size(icon_size);
    if lit {
        glyph_el = glyph_el.style(primary_text_style);
    }
    let mut btn = button(glyph_el).padding(padding).style(neutral);
    if let Some(m) = msg {
        btn = btn.on_press(m);
    }
    tooltip(btn, tooltip_bubble(tip), tooltip::Position::Top).gap(4).into()
}

/// The Legacy Collection's selection gold — BNLC paints the picked
/// list row / focused thumbnail in this yellow with dark ink text.
/// Used by [`list_item`] for selected rows so "what you've picked"
/// reads in a different register from the green chrome.
pub const SELECT_YELLOW: iced::Color =
    iced::Color::from_rgb(0xff as f32 / 255.0, 0xd2 as f32 / 255.0, 0x3d as f32 / 255.0);

/// List-item button style for selectable rows (patches list,
/// replays list). Zebra-striped at rest, lit-up primary plate
/// when selected (gradient + glow shadow + chunky border, the
/// same visual register as primary_button so the active row reads
/// as a console widget, not a flat highlight). Hover gets a
/// primary-tinted wash plus a left-edge accent stripe — a tiny
/// "chevron" cue the eye can pick out before the click.
/// Selectable list/palette row. Square corners + a zebra base so a
/// scrollable list reads as a flush table rather than a stack of
/// separated pills; selected rows get a lit gradient plate, hovered
/// rows a faint primary wash.
pub fn list_item(selected: bool, idx: usize) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let p = theme.extended_palette();
        let primary = theme.palette().primary;
        let bg = theme.palette().background;
        let text = theme.palette().text;
        if selected {
            // Lit-up plate in the Legacy Collection's selection
            // gold — BNLC highlights the picked row / focused
            // thumbnail in yellow against the chrome color, so the
            // selection reads in its own register instead of
            // blending into the accent-colored CTAs. Yellow→amber
            // gradient with navy ink text, like the music player's
            // active track bar. Stays gold under every accent, the
            // gold chrome included.
            let sel = SELECT_YELLOW;
            let amber = mix(sel, iced::Color::from_rgb(0.95, 0.55, 0.05), 0.40);
            let lighter = mix(sel, iced::Color::WHITE, 0.15);
            let (top, bottom, glow_alpha) = match status {
                button::Status::Hovered => (mix(lighter, iced::Color::WHITE, 0.12), mix(sel, amber, 0.5), 0.5),
                button::Status::Pressed => (amber, mix(amber, iced::Color::BLACK, 0.10), 0.2),
                _ => (lighter, amber, 0.35),
            };
            return button::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom),
                ))),
                text_color: ACCENT_INK,
                border: iced::Border {
                    radius: 0.0.into(),
                    width: 1.0,
                    color: mix(sel, iced::Color::WHITE, 0.45),
                },
                shadow: iced::Shadow {
                    color: iced::Color { a: glow_alpha, ..sel },
                    offset: iced::Vector::new(0.0, 3.0),
                    blur_radius: 12.0,
                },
                snap: false,
            };
        }
        // Zebra base — every other row gets a faint text-tinted
        // wash so the list reads as tabular rather than a
        // featureless wall of text.
        let stripe = if idx % 2 == 1 {
            Some(iced::Background::Color(if p.is_dark {
                iced::Color { a: 0.05, ..text }
            } else {
                iced::Color { a: 0.04, ..text }
            }))
        } else {
            None
        };
        let base = button::Style {
            background: stripe,
            text_color: text,
            border: iced::Border {
                radius: 0.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            shadow: iced::Shadow {
                color: iced::Color::TRANSPARENT,
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 0.0,
            },
            snap: false,
        };
        match status {
            button::Status::Active | button::Status::Pressed | button::Status::Disabled => base,
            button::Status::Hovered => button::Style {
                background: Some(iced::Background::Color(mix(bg, primary, 0.15))),
                border: iced::Border {
                    radius: 0.0.into(),
                    width: 1.0,
                    color: iced::Color { a: 0.6, ..primary },
                },
                // Centered accent bloom so the hovered row reads as
                // lit chrome, not just a tinted wash — the quiet
                // cousin of the selected plate's gold glow. Zero
                // offset: a dropped glow would smear onto the row
                // below.
                shadow: iced::Shadow {
                    color: iced::Color { a: 0.30, ..primary },
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 10.0,
                },
                ..base
            },
        }
    }
}

/// Theme-aware "neutral" button style for low-emphasis toolbar
/// actions. Two-stop vertical gradient (lighter top → darker
/// bottom) so it reads as a 3D plastic button rather than a
/// flat rectangle. Drop shadow at rest gives it a lifted feel;
/// the Pressed state collapses the shadow + nudges the fill
/// darker for a tactile "I clicked that" snap. Hover brightens
/// the plate and tints the border toward primary.
pub fn neutral(theme: &Theme, status: button::Status) -> button::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let primary = theme.palette().primary;
    // Base plate: nudged toward the accent-gray lift on dark (a
    // hint of glow off the navy bg — see [`plate_lift`]) and
    // toward white on light (a clean parchment).
    let plate = if p.is_dark {
        mix(bg, plate_lift(theme), 0.13)
    } else {
        mix(bg, iced::Color::WHITE, 0.5)
    };
    // Disabled gets the loud treatment: flat washed-out plate, no
    // shadow, near-invisible border, text dropped to ~35% alpha.
    // Keeps "you can't click this" obvious instead of pretending to
    // be a slightly-different normal button.
    if matches!(status, button::Status::Disabled) {
        let dim = mix(plate, bg, 0.55);
        return button::Style {
            background: Some(iced::Background::Color(dim)),
            text_color: iced::Color { a: 0.35, ..text },
            border: iced::Border {
                radius: tech_radius(10.0),
                width: 1.0,
                color: iced::Color {
                    a: 0.15,
                    ..p.background.strong.color
                },
            },
            shadow: iced::Shadow::default(),
            snap: false,
        };
    }
    let (top, bottom, border_color, shadow_y, shadow_alpha, text_color) = match status {
        button::Status::Hovered => (
            mix(plate, iced::Color::WHITE, if p.is_dark { 0.15 } else { 0.25 }),
            plate,
            iced::Color { a: 0.7, ..primary },
            4.0,
            if p.is_dark { 0.5 } else { 0.18 },
            text,
        ),
        button::Status::Pressed => (
            mix(plate, iced::Color::BLACK, 0.08),
            mix(plate, iced::Color::BLACK, 0.12),
            mix(plate, primary, 0.4),
            1.0,
            if p.is_dark { 0.25 } else { 0.08 },
            text,
        ),
        // Disabled is handled above by the early return.
        button::Status::Disabled => unreachable!(),
        button::Status::Active => (
            mix(plate, iced::Color::WHITE, if p.is_dark { 0.05 } else { 0.10 }),
            plate,
            p.background.strong.color,
            3.0,
            if p.is_dark { 0.4 } else { 0.12 },
            text,
        ),
    };
    // On dark, hover warms the drop shadow most of the way toward
    // the accent so the button blooms like the panel frames' glow
    // instead of dropping a darker blob. Light theme keeps plain
    // black — a colored glow on parchment reads as smudge (same
    // rule as the host's framed panels).
    let shadow_base = if p.is_dark && matches!(status, button::Status::Hovered) {
        mix(iced::Color::BLACK, primary, 0.65)
    } else {
        iced::Color::BLACK
    };
    button::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, top)
                .add_stop(1.0, bottom),
        ))),
        text_color,
        border: iced::Border {
            radius: tech_radius(10.0),
            width: 1.0,
            color: border_color,
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: shadow_alpha,
                ..shadow_base
            },
            offset: iced::Vector::new(0.0, shadow_y),
            blur_radius: 10.0,
        },
        snap: false,
    }
}

/// Borderless / transparent button style for "indicator-shaped"
/// toggles like the favorite-star in the patches header. No
/// background, no border at rest. Caller is expected to color the
/// inner icon themselves to convey state (e.g. primary when on,
/// muted when off). Hover and pressed states just nudge the
/// background alpha so the user gets click feedback without the
/// button looking like a CTA.
pub fn flat(theme: &Theme, status: button::Status) -> button::Style {
    let text = theme.palette().text;
    let (bg, text_color) = match status {
        button::Status::Hovered => (iced::Background::Color(iced::Color { a: 0.08, ..text }), text),
        button::Status::Pressed => (iced::Background::Color(iced::Color { a: 0.15, ..text }), text),
        // Borderless flat buttons have no plate to dim, so the only
        // disabled cue is text alpha. Drop it hard.
        button::Status::Disabled => (
            iced::Background::Color(iced::Color::TRANSPARENT),
            iced::Color { a: 0.3, ..text },
        ),
        button::Status::Active => (iced::Background::Color(iced::Color::TRANSPARENT), text),
    };
    button::Style {
        background: Some(bg),
        text_color,
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// Icon-plus-label button. Icon and label use distinct fonts
/// (icon = lucide, label = app default), laid out as a row.
pub fn labeled_icon_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: M,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    labeled_icon_button_maybe(icon, label, Some(msg), padding, style)
}

/// `labeled_icon_button` with the on_press wrapped in an Option so
/// callers can render a disabled (greyed-out, no on_press) variant
/// without duplicating the chrome. Mirrors [`icon_button_maybe`].
pub fn labeled_icon_button_maybe<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: Option<M>,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    let mut btn = button(row![icon.widget(), text(label)].spacing(8).align_y(Alignment::Center))
        .padding(padding)
        .style(style);
    if let Some(m) = msg {
        btn = btn.on_press(m);
    }
    btn.into()
}

/// The tab pill both sides' tab strips are built from: an icon, an
/// optional label, and [`pill_tab_style`]. `large` picks the global
/// top-nav size (heading-sized icon + label, roomier padding) over the
/// compact one that sits inside a pane.
pub fn pill_tab<'a, M: Clone + 'a>(
    icon: Icon,
    label: Option<String>,
    msg: M,
    active: bool,
    large: bool,
) -> Element<'a, M> {
    let icon_size = if large { TEXT_HEADING } else { TEXT_BODY };
    let mut content = row![icon.widget().size(icon_size)]
        .spacing(8)
        .align_y(Alignment::Center);
    if let Some(label) = label {
        // No wrapping — when a tab strip gets squeezed (e.g.
        // narrow window) we want labels to clip / overflow
        // rather than break into a second line that doubles the
        // tab's height.
        let mut lbl = text(label).wrapping(iced::widget::text::Wrapping::None);
        if large {
            lbl = lbl.size(TEXT_HEADING);
        }
        content = content.push(lbl);
    }
    let padding = if large { [8.0, 18.0] } else { [6.0, 14.0] };
    button(content)
        .padding(padding)
        .style(pill_tab_style(active))
        .on_press(msg)
        .into()
}

/// Float a 7 px glowing status pip over a pill tab's top-right corner without
/// changing the pill's layout bounds. `color` lets callers distinguish kinds
/// of attention while sharing the same placement and glow treatment.
pub fn pill_tab_badge<'a, M: 'a>(
    pill: Element<'a, M>,
    color: impl Fn(&Theme) -> iced::Color + 'a,
) -> Element<'a, M> {
    let pip = container(iced::widget::Space::new().width(7).height(7)).style(move |theme: &Theme| {
        let color = color(theme);
        container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: 3.5.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color { a: 0.7, ..color },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 6.0,
            },
            ..Default::default()
        }
    });
    iced::widget::Stack::new()
        .push(pill)
        .push(
            container(pip)
                .width(iced::Length::Fill)
                .height(iced::Length::Fill)
                .align_x(iced::alignment::Horizontal::Right)
                .align_y(iced::alignment::Vertical::Top)
                .padding(4),
        )
        .into()
}

/// Shared "pill tab" button style — used by the global top nav,
/// save_view's sub-tab strip, and the settings sidebar so every
/// tab affordance in the app reads as the same widget family.
///
/// Active tabs render as a solid primary-gradient pill with
/// white text and a glow shadow underneath; inactive tabs are
/// transparent at rest and brighten on hover with a faint
/// primary wash. The caller controls the layout (icon + label,
/// label-only, full-width vertical, etc.) — this fn only owns
/// the visual style.
pub fn pill_tab_style(active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme: &Theme, status: button::Status| {
        let p = theme.extended_palette();
        let primary = theme.palette().primary;
        let (bg, text_color, glow_alpha, blur) = if active {
            let lighter = mix(primary, iced::Color::WHITE, 0.22);
            let darker = mix(primary, iced::Color::BLACK, 0.18);
            let grad = iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(0.0)
                    .add_stop(0.0, lighter)
                    .add_stop(1.0, darker),
            ));
            let (g, b) = if matches!(status, button::Status::Hovered) {
                (0.85, 22.0)
            } else {
                (0.65, 18.0)
            };
            // Contrast-aware text — white on tango green, navy ink
            // if the accent ever goes light again (see
            // [`on_accent`]).
            (Some(grad), on_accent(primary), g, b)
        } else {
            let hover = matches!(status, button::Status::Hovered);
            let bg = if hover {
                Some(iced::Background::Color(iced::Color { a: 0.18, ..primary }))
            } else {
                None
            };
            let text_color = if hover {
                mix(theme.palette().text, primary, 0.45)
            } else {
                // Slightly muted so the active tab pops harder
                // against its siblings.
                mix(theme.palette().text, p.background.base.color, 0.18)
            };
            let glow = if hover { 0.25 } else { 0.0 };
            (bg, text_color, glow, 10.0)
        };
        button::Style {
            background: bg,
            text_color,
            // Tech-frame corners instead of a full pill — the
            // active tab reads as one of BNLC's clipped chips.
            border: iced::Border {
                radius: tech_radius(12.0),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            // Centered glow — zero offset. A downward-offset glow
            // visually drags the chip off the strip's centerline
            // and the whole tab row reads as mis-centered.
            shadow: iced::Shadow {
                color: iced::Color {
                    a: glow_alpha,
                    ..primary
                },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: blur,
            },
            snap: false,
        }
    }
}

/// Minimal "pane" demarcation — a barely-perceptible tinted plate
/// with a small radius and no border or shadow. Used where we used
/// to drop `horizontal_rule` / `vertical_rule` between regions; the
/// page background shows through the gaps between panes and that's
/// what separates them, no explicit lines needed. Pair with
/// `.padding(PANE_PADDING)` at the call site for consistent
/// breathing room across the app.
pub fn pane(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let plate = plate_color(theme);
    iced::widget::container::Style {
        background: Some(iced::Background::Color(plate)),
        text_color: Some(p.background.weak.text),
        // Faint accent hairline — the quiet cousin of a framed
        // panel, just enough edge that panes read as PET
        // screen regions against the cyberworld backdrop.
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color {
                a: if p.is_dark { 0.20 } else { 0.30 },
                ..theme.palette().primary
            },
        },
        ..Default::default()
    }
}

/// The [`pane`] plate fill. Exposed so exit washes
/// ([`crate::anim::exit_fade`]) can dissolve departing controls
/// into the same color they sit on. On dark, the lift runs through
/// [`plate_lift`] (neutral with a whisper of accent); on light, a
/// 5% nudge toward text. Either way it's just enough contrast
/// against the page bg to read as a region without competing with
/// content.
pub fn plate_color(theme: &Theme) -> iced::Color {
    let p = theme.extended_palette();
    if p.is_dark {
        mix(theme.palette().background, plate_lift(theme), 0.06)
    } else {
        mix(theme.palette().background, theme.palette().text, 0.05)
    }
}

/// Theme-aware muted text color: mix the palette's text into the
/// background until the contrast drops to "secondary". Works on
/// both light + dark themes — alpha-fading the text on a dark bg
/// turns it into a washed-out near-bg blob; mixing yields a true
/// mid-tone gray instead.
pub fn muted_color(theme: &iced::Theme) -> iced::Color {
    let p = theme.palette();
    // Heavy mix breaks contrast on Dark (text tops out at 0.9
    // and bg is ~0.18, so 0.45 lands at ~2.8:1 contrast —
    // basically invisible). 0.25 stays around 4:1 on both
    // themes — visibly secondary but still legible.
    let t = 0.25;
    iced::Color {
        r: p.text.r * (1.0 - t) + p.background.r * t,
        g: p.text.g * (1.0 - t) + p.background.g * t,
        b: p.text.b * (1.0 - t) + p.background.b * t,
        a: 1.0,
    }
}

pub fn muted_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(muted_color(theme)),
    }
}

/// "OK / success" text color tuned for readability on both Light
/// and Dark themes. The default `extended_palette().success.base`
/// is a dark teal that disappears on a dark background, so we
/// reach for the `strong` variant which iced derives by deviating
/// from base toward higher contrast.
pub fn success_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().success.strong.color),
    }
}

/// Same idea as [`success_text_style`] for danger — the `strong`
/// variant of palette.danger reads brightly on dark backgrounds
/// where the base color washes out.
pub fn danger_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.extended_palette().danger.strong.color),
    }
}

/// Caption text inside a [`list_item`] row: muted at rest, but on
/// the selected row `color: None` so the caption inherits the lit
/// plate's ink instead of vanishing into the gold.
pub fn list_caption_style(selected: bool) -> impl Fn(&Theme) -> iced::widget::text::Style {
    move |theme: &Theme| {
        if selected {
            iced::widget::text::Style { color: None }
        } else {
            muted_text_style(theme)
        }
    }
}

pub fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r * (1.0 - t) + b.r * t,
        g: a.g * (1.0 - t) + b.g * t,
        b: a.b * (1.0 - t) + b.b * t,
        a: 1.0,
    }
}

/// The tone dark-theme control plates (buttons, inputs, pickers,
/// checkbox boxes, slider rails) are lifted toward: the neutral
/// text white warmed with a whisper (~18%) of the accent, so
/// plates read as neutral gray with a hint of the chrome's green
/// rather than as colored surfaces. Both stronger recipes failed
/// on sight: lifting toward a tinted text color cast every control
/// blue, and lifting toward a heavy accent mix turned the whole UI
/// green. Light theme keeps its white/parchment lifts and doesn't
/// use this.
pub fn plate_lift(theme: &Theme) -> iced::Color {
    mix(theme.palette().primary, theme.palette().text, 0.82)
}

/// Dark "ink" for text sitting on a bright accent plate — the
/// selection gold today, any light accent tomorrow. BNLC letters
/// its bright chrome in a dark ink, not white (white genuinely
/// fails contrast on these light fills); ours leans green-black to
/// match the rest of the dark family instead of BNLC's navy.
pub const ACCENT_INK: iced::Color =
    iced::Color::from_rgb(0x0a as f32 / 255.0, 0x20 as f32 / 255.0, 0x12 as f32 / 255.0);

/// Shared chunky-button kernel — gradient fill in the given accent
/// color, accent-tinted glow shadow, hover/press/disabled state
/// math. The shape (radius, border width, white text) is identical
/// across CTAs so `primary_button` (primary green) and
/// `danger_button` (red) read as the same widget family in
/// different moods.
pub fn tinted_button(theme: &Theme, status: button::Status, accent: iced::Color) -> button::Style {
    // Disabled drops the accent entirely — no green/red glow, no
    // gradient, no shadow. Flat de-saturated plate + dim text reads
    // as "this is OFF" loud and clear instead of "this is just a
    // dimmer version of the active button".
    if matches!(status, button::Status::Disabled) {
        let p = theme.extended_palette();
        let bg = theme.palette().background;
        let text = theme.palette().text;
        let dim = if p.is_dark {
            mix(bg, plate_lift(theme), 0.11)
        } else {
            mix(bg, text, 0.08)
        };
        return button::Style {
            background: Some(iced::Background::Color(dim)),
            text_color: iced::Color { a: 0.35, ..text },
            border: iced::Border {
                radius: tech_radius(10.0),
                width: 1.0,
                color: iced::Color {
                    a: 0.15,
                    ..p.background.strong.color
                },
            },
            shadow: iced::Shadow::default(),
            snap: false,
        };
    }
    let lighter = mix(accent, iced::Color::WHITE, 0.20);
    let darker = mix(accent, iced::Color::BLACK, 0.20);
    let (top, bottom, glow_alpha, offset_y) = match status {
        button::Status::Hovered => (mix(lighter, iced::Color::WHITE, 0.10), accent, 0.65, 5.0),
        button::Status::Pressed => (darker, mix(darker, iced::Color::BLACK, 0.10), 0.25, 1.0),
        button::Status::Disabled => unreachable!(),
        button::Status::Active => (lighter, darker, 0.45, 4.0),
    };
    button::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, top)
                .add_stop(1.0, bottom),
        ))),
        // White on the dark accents (green, red), ink if a light
        // one ever lands here — see [`on_accent`].
        text_color: on_accent(accent),
        border: iced::Border {
            radius: tech_radius(10.0),
            width: 1.0,
            color: mix(accent, iced::Color::WHITE, 0.35),
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: glow_alpha,
                ..accent
            },
            offset: iced::Vector::new(0.0, offset_y),
            blur_radius: 14.0,
        },
        snap: false,
    }
}

/// Standard primary call-to-action — Play, Fight, Watch, Update
/// Now, Ready confirms, etc. Gradient fill in palette primary
/// with a green-tinted glow shadow.
pub fn primary_button(theme: &Theme, status: button::Status) -> button::Style {
    tinted_button(theme, status, theme.palette().primary)
}

/// Destructive call-to-action: Delete save, leave session, clear
/// data. Same chrome as [`primary_button`] but tinted in the
/// danger palette so the button's mood reads as "this will hurt"
/// before the user even reads the label.
pub fn danger_button(theme: &Theme, status: button::Status) -> button::Style {
    tinted_button(theme, status, theme.palette().danger)
}

/// Chunky text input matching the button bevel. Gradient plate
/// (lighter top → darker bottom) so it reads as the same
/// "physical widget" family as the buttons sitting next to it.
/// Focus = thicker primary border; hover = tinted border.
pub fn chunky_text_input(
    theme: &Theme,
    status: sweeten::widget::text_input::Status,
) -> sweeten::widget::text_input::Style {
    use sweeten::widget::text_input::Status;
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let plate_top = if p.is_dark {
        mix(bg, plate_lift(theme), 0.09)
    } else {
        iced::Color::WHITE
    };
    let plate_bottom = if p.is_dark {
        mix(bg, plate_lift(theme), 0.15)
    } else {
        mix(bg, iced::Color::WHITE, 0.55)
    };
    let (border_color, width) = match status {
        Status::Active => (p.background.strong.color, 1.0),
        Status::Hovered => (iced::Color { a: 0.6, ..primary }, 1.0),
        Status::Focused { .. } => (primary, 2.0),
        Status::Disabled => (p.background.strong.color, 1.0),
    };
    let background = if matches!(status, Status::Disabled) {
        iced::Background::Color(p.background.weak.color)
    } else {
        iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, plate_top)
                .add_stop(1.0, plate_bottom),
        ))
    };
    sweeten::widget::text_input::Style {
        background,
        border: iced::Border {
            radius: tech_radius(10.0),
            width,
            color: border_color,
        },
        icon: text,
        placeholder: muted_color(theme),
        value: if matches!(status, Status::Disabled) {
            muted_color(theme)
        } else {
            text
        },
        selection: iced::Color { a: 0.35, ..primary },
    }
}

/// Chunky checkbox: 4 px rounded box, primary-tinted border when
/// hovered or checked, gradient fill when checked. iced 0.14's
/// checkbox::Style has no shadow, but the thick accent border
/// plus the saturated primary fill give it enough presence to
/// match the rest of the chrome.
pub fn chunky_checkbox(theme: &Theme, status: iced::widget::checkbox::Status) -> iced::widget::checkbox::Style {
    use iced::widget::checkbox::Status;
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let (is_checked, is_hover, is_disabled) = match status {
        Status::Active { is_checked } => (is_checked, false, false),
        Status::Hovered { is_checked } => (is_checked, true, false),
        Status::Disabled { is_checked } => (is_checked, false, true),
    };
    // Unchecked plate matches the neutral button base — same
    // mix() so checkboxes feel like family with the toolbar
    // buttons sitting next to them.
    let unchecked_plate = if p.is_dark {
        mix(bg, plate_lift(theme), 0.13)
    } else {
        mix(bg, iced::Color::WHITE, 0.5)
    };
    let background = if is_checked {
        // Sharp primary fill so the check itself doesn't need to
        // do much work — the whole box lights up.
        iced::Background::Color(if is_disabled {
            mix(primary, iced::Color::BLACK, 0.4)
        } else if is_hover {
            mix(primary, iced::Color::WHITE, 0.12)
        } else {
            primary
        })
    } else {
        iced::Background::Color(if is_hover {
            mix(unchecked_plate, primary, 0.15)
        } else {
            unchecked_plate
        })
    };
    let border_color = if is_disabled {
        p.background.strong.color
    } else if is_checked {
        mix(primary, iced::Color::WHITE, 0.35)
    } else if is_hover {
        iced::Color { a: 0.85, ..primary }
    } else {
        p.background.strong.color
    };
    iced::widget::checkbox::Style {
        background,
        icon_color: iced::Color::WHITE,
        border: iced::Border {
            radius: 5.0.into(),
            width: 2.0,
            color: border_color,
        },
        text_color: Some(if is_disabled { muted_color(theme) } else { text }),
    }
}

/// Slim rounded scrollbar replacing iced's boxy default: no rail
/// plate at rest, just a pill scroller that rides muted until the
/// cursor reaches it, then lights up primary while hovered or
/// dragged — the same "quiet until touched" register as the rest
/// of the chrome.
pub fn chunky_scrollable(theme: &Theme, status: iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    use iced::widget::scrollable::{Rail, Scroller, Status, Style};
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let (v_lit, h_lit) = match status {
        Status::Active { .. } => (false, false),
        Status::Hovered {
            is_vertical_scrollbar_hovered,
            is_horizontal_scrollbar_hovered,
            ..
        } => (is_vertical_scrollbar_hovered, is_horizontal_scrollbar_hovered),
        Status::Dragged {
            is_vertical_scrollbar_dragged,
            is_horizontal_scrollbar_dragged,
            ..
        } => (is_vertical_scrollbar_dragged, is_horizontal_scrollbar_dragged),
    };
    let rail = |lit: bool| Rail {
        // Faint plate only under a lit scroller — at rest the rail
        // disappears into the pane and only the thumb shows.
        background: lit.then_some(iced::Background::Color(iced::Color { a: 0.06, ..text })),
        border: iced::Border {
            radius: 999.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
        scroller: Scroller {
            background: iced::Background::Color(if lit {
                primary
            } else if p.is_dark {
                mix(bg, plate_lift(theme), 0.33)
            } else {
                mix(bg, text, 0.35)
            }),
            border: iced::Border {
                radius: 999.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
        },
    };
    Style {
        container: iced::widget::container::Style::default(),
        vertical_rail: rail(v_lit),
        horizontal_rail: rail(h_lit),
        gap: None,
        // Keep iced's stock auto-scroll puck but tint its arrow
        // icons primary so even that overlay matches the chrome.
        auto_scroll: iced::widget::scrollable::AutoScroll {
            background: iced::Background::Color(iced::Color { a: 0.92, ..bg }),
            border: iced::Border {
                radius: 999.0.into(),
                width: 1.0,
                color: iced::Color { a: 0.6, ..primary },
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: 0.5,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 4.0,
            },
            icon: primary,
        },
    }
}

/// Lower-level helper for callers that need to pick the button
/// style explicitly — `button::primary` for the one emphasized
/// action in a row, `button::danger` for destructive ones, etc.
pub fn icon_button_styled<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: Option<M>,
    padding: [f32; 2],
    style: impl Fn(&Theme, button::Status) -> button::Style + 'a,
) -> Element<'a, M> {
    let mut btn = button(icon.widget()).padding(padding).style(style);
    if let Some(m) = msg {
        btn = btn.on_press(m);
    }
    tooltip(btn, tooltip_bubble(label), tooltip::Position::Bottom)
        .gap(4)
        .into()
}

/// Accent-tinted text — for "lit" indicators that belong to the
/// primary glow language (like the lobby's ready nicknames)
/// rather than the success/danger semantic colors.
pub fn primary_text_style(theme: &iced::Theme) -> iced::widget::text::Style {
    iced::widget::text::Style {
        color: Some(theme.palette().primary),
    }
}

/// The standard tooltip bubble: a caption-sized `label` on the
/// [`tooltip_chrome`] plate. Pass as the overlay to `iced::widget::tooltip`.
pub fn tooltip_bubble<'a, M: 'a>(label: impl Into<String>) -> iced::widget::Container<'a, M> {
    container(text(label.into()).size(TEXT_CAPTION))
        .padding(6)
        .style(tooltip_chrome)
}

pub fn tooltip_chrome(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(p.background.strong.color)),
        text_color: Some(p.background.strong.text),
        // Hairline accent edge so even tooltips read as tiny PET
        // chips rather than gray OS bubbles.
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.45,
                ..theme.palette().primary
            },
        },
        ..Default::default()
    }
}

// ---------- HUD chrome ----------
//
// Style helpers below are passed to `container.style(...)` so the
// app's top-level shell (nav bar, body surface, separator rules)
// and the inline empty-state cards all share a single look.
//
// The dark palette is tuned to look like a Battle Network "PET"
// screen: navy base, neon-green accents, cyan-tinted text. The
// light palette is its warm-cream cousin so users who prefer
// daylight still get tango-shaped chrome rather than a generic
// gray rectangle.

/// The signature "tech frame" corner treatment, after the Legacy
/// Collection's PET panels: one diagonal pair of corners gets a
/// big cut, the other stays nearly sharp, so plates lean like the
/// collection's clipped cyber-frames instead of sitting as evenly
/// rounded web cards. The sharp corners land top-right /
/// bottom-left so the implied diagonal runs "/" — the same
/// rightward lean as the collection's italic headers.
pub fn tech_radius(r: f32) -> iced::border::Radius {
    iced::border::Radius {
        top_left: r,
        top_right: (r * 0.25).min(3.0),
        bottom_right: r,
        bottom_left: (r * 0.25).min(3.0),
    }
}

/// Readable text color for a plate filled with `accent`: navy ink
/// on light accents (the selection gold), white on dark ones
/// (tango green, danger red). Keeps `tinted_button` / the active
/// tab pill legible no matter which accent the palette hands them.
pub fn on_accent(accent: iced::Color) -> iced::Color {
    let luma = 0.299 * accent.r + 0.587 * accent.g + 0.114 * accent.b;
    if luma > 0.6 {
        ACCENT_INK
    } else {
        iced::Color::WHITE
    }
}

/// Chunky pick_list matching the button bevel. Same gradient
/// plate + thicker border. Open state lights up the border in
/// primary so the dropdown reads as "live".
///
/// Typed against `sweeten::widget::pick_list`, not iced's stock
/// one — we use sweeten so the game picker can `.disabled()`
/// individual rows. The `Style`/`Status` types are structurally
/// identical to iced's but are a distinct type.
pub fn chunky_pick_list(
    theme: &Theme,
    status: sweeten::widget::pick_list::Status,
) -> sweeten::widget::pick_list::Style {
    use sweeten::widget::pick_list::Status;
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let text = theme.palette().text;
    // pick_list::Background is `Background` (Color or Gradient).
    // Drop in the same gradient as the text input so the two
    // widgets read as siblings.
    let plate_top = if p.is_dark {
        mix(theme.palette().background, plate_lift(theme), 0.11)
    } else {
        iced::Color::WHITE
    };
    let plate_bottom = if p.is_dark {
        mix(theme.palette().background, plate_lift(theme), 0.18)
    } else {
        mix(theme.palette().background, iced::Color::WHITE, 0.55)
    };
    let (border_color, width) = match status {
        Status::Active => (p.background.strong.color, 1.0),
        Status::Hovered => (iced::Color { a: 0.6, ..primary }, 1.0),
        Status::Opened { .. } => (primary, 2.0),
    };
    sweeten::widget::pick_list::Style {
        text_color: text,
        placeholder_color: muted_color(theme),
        handle_color: primary,
        background: iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, plate_top)
                .add_stop(1.0, plate_bottom),
        )),
        border: iced::Border {
            radius: tech_radius(10.0),
            width,
            color: border_color,
        },
    }
}
