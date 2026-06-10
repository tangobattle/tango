//! Small iced widget helpers: icon-buttons (with tooltips), tab
//! buttons, button styles (`neutral`, `list_item`), and a handful
//! of HUD-chrome style fns (`hud_bar`, `hud_scanline_top`, `panel`,
//! `body_surface`) used by main.rs and the tabs to give the app
//! a more game-console / less generic-desktop look. Icon glyphs
//! come straight from the `lucide-icons` crate — call sites pass
//! `Icon::Foo` directly.

use crate::style::{PANE_GAP, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING};
use iced::widget::{button, container, text, tooltip};
use iced::{Alignment, Element, Length, Theme};
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
            // Lit-up plate. Gradient + glow shadow give the
            // selected row the same energy as the lobby Ready
            // button at rest, so the list "knows" what you've
            // picked rather than just hinting at it.
            let lighter = mix(primary, iced::Color::WHITE, 0.10);
            let darker = mix(primary, iced::Color::BLACK, 0.15);
            let (top, bottom, glow_alpha) = match status {
                button::Status::Hovered => (mix(lighter, iced::Color::WHITE, 0.10), primary, 0.55),
                button::Status::Pressed => (darker, mix(darker, iced::Color::BLACK, 0.10), 0.2),
                _ => (lighter, darker, 0.4),
            };
            return button::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom),
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: 0.0.into(),
                    width: 1.0,
                    color: mix(primary, iced::Color::WHITE, 0.35),
                },
                shadow: iced::Shadow {
                    color: iced::Color {
                        a: glow_alpha,
                        ..primary
                    },
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
    // Base plate: nudged toward text on dark (a hint of glow off
    // the navy bg) and toward white on light (a clean parchment).
    let plate = if p.is_dark {
        mix(bg, text, 0.12)
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
                radius: 8.0.into(),
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
    button::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, top)
                .add_stop(1.0, bottom),
        ))),
        text_color,
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: border_color,
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: shadow_alpha,
                ..iced::Color::BLACK
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
    tooltip(
        btn,
        container(text(label).size(TEXT_CAPTION))
            .padding(6)
            .style(tooltip_chrome),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

/// A pick_list option: a value paired with a pre-resolved display
/// label. The picker renders options via `Display`, which can't reach
/// the language or any other formatting context, so labels are built
/// when the option list is constructed. Equality is by value only, so
/// selection-matching survives label differences (e.g. a favorites
/// star prefix).
#[derive(Clone, Debug)]
pub struct Choice<T> {
    pub value: T,
    pub label: String,
}

impl<T> Choice<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
        }
    }
}

impl<T: PartialEq> PartialEq for Choice<T> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl<T> std::fmt::Display for Choice<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// A caption label stacked over a control — the standard "form row"
/// used by the settings and welcome screens.
pub fn labeled<'a, M: Clone + 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    sweeten::widget::column![text(label).size(TEXT_CAPTION).style(muted_text_style), ctrl.into(),]
        .spacing(4)
        .into()
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

/// Compact tab pill used by sub-navs (save_view's
/// Cover/Navi/Folder/Patch Cards/Auto Battle Data strip, etc).
/// Body-text size, modest padding — meant to sit inside a pane
/// without competing with the global top nav.
pub fn tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    tab_button_inner(icon, Some(label), msg, active, false)
}

/// Larger pill for the global top nav (Play / Replays / Patches).
/// TEXT_HEADING-sized icon + label so the chrome reads as the
/// primary navigation for the whole app.
pub fn nav_tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    tab_button_inner(icon, Some(label), msg, active, true)
}

/// Icon-only variant of [`nav_tab_button`] for the Settings cog.
pub fn nav_icon_tab_button<'a, M: Clone + 'a>(
    icon: Icon,
    tooltip_label: String,
    msg: M,
    active: bool,
) -> Element<'a, M> {
    let stacked = tab_button_inner(icon, None, msg, active, true);
    tooltip(
        stacked,
        container(text(tooltip_label).size(TEXT_CAPTION))
            .padding(6)
            .style(tooltip_chrome),
        tooltip::Position::Bottom,
    )
    .gap(4)
    .into()
}

fn tab_button_inner<'a, M: Clone + 'a>(
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
            (Some(grad), iced::Color::WHITE, g, b)
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
            border: iced::Border {
                radius: 999.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: glow_alpha,
                    ..primary
                },
                offset: iced::Vector::new(0.0, 3.0),
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
    let bg = theme.palette().background;
    let text = theme.palette().text;
    // A 5% mix toward the foreground is enough contrast against the
    // page bg to read as a region without competing with content.
    let plate = mix(bg, text, 0.05);
    iced::widget::container::Style {
        background: Some(iced::Background::Color(plate)),
        text_color: Some(p.background.weak.text),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
        },
        ..Default::default()
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

pub fn tooltip_chrome(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(p.background.strong.color)),
        text_color: Some(p.background.strong.text),
        border: iced::Border {
            radius: 4.0.into(),
            ..Default::default()
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

fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r * (1.0 - t) + b.r * t,
        g: a.g * (1.0 - t) + b.g * t,
        b: a.b * (1.0 - t) + b.b * t,
        a: 1.0,
    }
}

/// Top nav strip background. Vertical gradient (lighter top, darker
/// bottom) so it reads as a console plate catching overhead light
/// rather than a flat sheet of pixels. Drops a soft shadow onto
/// the body surface below so the seam between HUD and content
/// feels lifted, not stamped. The accent scanline is rendered as
/// a separate row underneath; this style intentionally has no
/// bottom border so the two layers don't fight.
pub fn hud_bar(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let (top, bottom) = if p.is_dark {
        // Pull toward black at the bottom; the top stays close to
        // the bg color so the gradient is felt, not seen.
        (
            iced::Color {
                r: bg.r * 0.7,
                g: bg.g * 0.7,
                b: bg.b * 0.8,
                a: 1.0,
            },
            iced::Color {
                r: bg.r * 0.4,
                g: bg.g * 0.4,
                b: bg.b * 0.5,
                a: 1.0,
            },
        )
    } else {
        // Light theme: subtle parchment gradient — top slightly
        // tinted toward text, bottom slightly more so.
        (mix(bg, text, 0.05), mix(bg, text, 0.12))
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(0.0)
                .add_stop(0.0, top)
                .add_stop(1.0, bottom),
        ))),
        text_color: Some(text),
        shadow: iced::Shadow {
            color: iced::Color {
                a: if p.is_dark { 0.45 } else { 0.18 },
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..Default::default()
    }
}

/// Body surface (everything below the HUD bar). Uses the bare
/// palette background so we don't double-paint, but specifies it
/// explicitly so future tweaks have a single hook to land in.
pub fn body_surface(theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: Some(iced::Background::Color(theme.palette().background)),
        text_color: Some(theme.palette().text),
        ..Default::default()
    }
}

/// The top accent strip, rendered under the HUD bar. 3-px tall,
/// normally a left→right primary→cooler gradient so the rule has
/// motion — not a single flat color stripe across the window.
pub fn hud_scanline_top<'a, M: 'a>() -> Element<'a, M> {
    hud_scanline(crate::theme::is_gay_time().then(|| flag_background(&crate::theme::rainbow_flag_stops())))
}

/// The bottom-edge accent strip.
pub fn hud_scanline_bottom<'a, M: 'a>() -> Element<'a, M> {
    hud_scanline(crate::theme::is_gay_time().then(|| flag_background(&crate::theme::trans_flag_stops())))
}

/// A flat left→right linear gradient through `stops`, packaged as a
/// `Background` ready to drop into a scanline override.
fn flag_background(stops: &[(f32, iced::Color)]) -> iced::Background {
    iced::Background::Gradient(iced::Gradient::Linear(stops.iter().fold(
        iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2),
        |grad, &(offset, color)| grad.add_stop(offset, color),
    )))
}

/// Shared scanline body. `override_bg` replaces the fill when `Some`
/// (e.g. a pride-flag gradient in June); when `None` it falls back to
/// the usual primary→cooler accent rule derived from the live theme.
fn hud_scanline<'a, M: 'a>(override_bg: Option<iced::Background>) -> Element<'a, M> {
    container(
        iced::widget::Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(3.0)),
    )
    .width(Length::Fill)
    .height(Length::Fixed(3.0))
    .style(move |theme: &Theme| {
        let background = override_bg.unwrap_or_else(|| {
            let primary = theme.palette().primary;
            // Cool the right edge by pulling primary toward a
            // cyan-ish tone — keeps the rule from looking like a
            // dumb solid green bar, gives it a console-trim vibe.
            let right = iced::Color {
                r: primary.r * 0.4,
                g: primary.g * 0.85 + 0.15,
                b: primary.b * 0.4 + 0.55,
                a: 1.0,
            };
            iced::Background::Gradient(iced::Gradient::Linear(
                iced::gradient::Linear::new(std::f32::consts::FRAC_PI_2)
                    .add_stop(0.0, primary)
                    .add_stop(1.0, right),
            ))
        });
        iced::widget::container::Style {
            background: Some(background),
            ..Default::default()
        }
    })
    .into()
}

/// HUD frame for inline cards (empty-state hints, lobby side
/// panels, settings groups). Adds a soft drop shadow + thicker
/// border so panels read as physical widgets sitting on the
/// console surface rather than CSS rectangles.
pub fn panel(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    // Slightly lifted plate. On dark, mix bg toward text 10% for
    // a navy plate that reads above the navy body. On light, go
    // toward white so the card looks like paper on parchment.
    let plate = if p.is_dark {
        mix(bg, text, 0.10)
    } else {
        mix(bg, iced::Color::WHITE, 0.4)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(plate)),
        text_color: Some(text),
        border: iced::Border {
            radius: 12.0.into(),
            width: 2.0,
            color: if p.is_dark {
                mix(bg, text, 0.20)
            } else {
                mix(bg, text, 0.18)
            },
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: if p.is_dark { 0.55 } else { 0.18 },
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 6.0),
            blur_radius: 18.0,
        },
        snap: false,
    }
}

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
        let dim = mix(bg, text, if p.is_dark { 0.10 } else { 0.08 });
        return button::Style {
            background: Some(iced::Background::Color(dim)),
            text_color: iced::Color { a: 0.35, ..text },
            border: iced::Border {
                radius: 8.0.into(),
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
        text_color: iced::Color::WHITE,
        border: iced::Border {
            radius: 8.0.into(),
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

/// P1 (red) accent — matches the matchup-pane diagonal split tints
/// in `play_pane`. Used by the in-session "show my setup" toggle so
/// the PvP toolbar reads as the same versus-coded family.
pub fn pvp_red_button(theme: &Theme, status: button::Status) -> button::Style {
    tinted_button(theme, status, iced::Color::from_rgb(0.85, 0.22, 0.28))
}

/// P2 (blue) accent — pairs with [`pvp_red_button`]. Used by the
/// in-session "show opponent setup" toggle.
pub fn pvp_blue_button(theme: &Theme, status: button::Status) -> button::Style {
    tinted_button(theme, status, iced::Color::from_rgb(0.18, 0.40, 0.85))
}

/// Zebra row style for data tables. Odd rows get a faint text-
/// tinted wash (alpha 0.05 dark / 0.04 light); even rows are
/// transparent and show the pane plate. Flat — no rounded corners
/// — since rows sit flush against the pane edges and rounded
/// per-row corners look like accidental indents.
pub fn zebra_row(idx: usize) -> impl Fn(&Theme) -> iced::widget::container::Style {
    move |theme: &Theme| {
        let p = theme.extended_palette();
        let text = theme.palette().text;
        let stripe = if idx % 2 == 1 {
            Some(iced::Background::Color(iced::Color {
                a: if p.is_dark { 0.05 } else { 0.04 },
                ..text
            }))
        } else {
            None
        };
        iced::widget::container::Style {
            background: stripe,
            text_color: Some(text),
            ..Default::default()
        }
    }
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
        mix(bg, text, 0.08)
    } else {
        iced::Color::WHITE
    };
    let plate_bottom = if p.is_dark {
        mix(bg, text, 0.14)
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
            radius: 8.0.into(),
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
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let _ = bg;
    // pick_list::Background is `Background` (Color or Gradient).
    // Drop in the same gradient as the text input so the two
    // widgets read as siblings.
    let plate_top = if p.is_dark {
        mix(theme.palette().background, text, 0.10)
    } else {
        iced::Color::WHITE
    };
    let plate_bottom = if p.is_dark {
        mix(theme.palette().background, text, 0.16)
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
            radius: 8.0.into(),
            width,
            color: border_color,
        },
    }
}

/// Container style that mimics a disabled `chunky_pick_list`. iced
/// 0.14's `pick_list::Status` has no Disabled variant, so we render
/// a styled `container` instead of the picker when the control isn't
/// usable. Same recipe as `tinted_button`'s Disabled branch (flat
/// desaturated plate + dim text + dim border) so disabled dropdowns
/// and disabled buttons read as the same family.
pub fn disabled_pick_list_style(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let dim = mix(bg, text, if p.is_dark { 0.10 } else { 0.08 });
    iced::widget::container::Style {
        text_color: Some(iced::Color { a: 0.35, ..text }),
        background: Some(iced::Background::Color(dim)),
        border: iced::Border {
            radius: 8.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.15,
                ..p.background.strong.color
            },
        },
        shadow: iced::Shadow::default(),
        snap: false,
    }
}

/// Drop-in stand-in for a `chunky_pick_list` when the choice isn't
/// available. Pads + radii match the live picker so the layout
/// doesn't shift when toggling between enabled/disabled states.
pub fn disabled_pick_list<'a, M: 'a>(label: impl Into<String>) -> iced::widget::Container<'a, M> {
    iced::widget::container(iced::widget::text(label.into()))
        .padding(crate::style::STANDARD_PADDING)
        .style(disabled_pick_list_style)
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
        mix(bg, text, 0.12)
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

/// Full-height "VS" splitter: paints a near-vertical band in the
/// body background color through the middle of its bounds so that,
/// when layered behind a padded row of content via
/// `Stack::push_under`, the pane reads as sliced diagonally in
/// half — the body surface peeking through the cut. "VS" sits
/// centered on the band.
///
/// Width and height are both [`Length::Fill`]; the splitter sizes
/// itself to whatever the layered content needs, so the cut
/// reaches the pane's top and bottom edges automatically. See
/// `tabs/play.rs` and `tabs/replays.rs` for the layout pattern.
pub fn vs_splitter<'a, M: 'a>() -> Element<'a, M> {
    use iced::widget::canvas::{Canvas, Frame, LineCap, Path, Stroke, Style, Text as CanvasText};
    use iced::{Pixels, Point, Rectangle, Renderer};

    /// Thickness of the cut, perpendicular to the band axis. Half
    /// the inter-pane gap so the slice reads as slimmer than the
    /// gaps separating sibling panes — a hairline, not a chasm.
    const BAND_W: f32 = PANE_GAP / 2.0;
    /// Horizontal offset of each band endpoint from the canvas
    /// center. Small relative to typical pane heights so the cut
    /// leans gently rather than racing across the pane — the
    /// "shallow gradient" close-to-vertical look.
    const TILT: f32 = 14.0;
    /// Distance the band extends past the canvas top/bottom edges
    /// before the butt cap kicks in. Has to be > a couple of pixels
    /// or anti-aliasing leaves a soft tapered edge that reads as
    /// the slash trailing off short of the pane border.
    const OVERSHOOT: f32 = 16.0;
    /// "V" / "S" glyph size.
    const GLYPH: f32 = 18.0;
    /// Radius of the body-bg-colored circle that the "VS" sits
    /// inside. Sized so the glyph pair has a comfortable margin
    /// to the rim; the circle merges seamlessly with the band
    /// (same color), reading as a node bulging out of the cut.
    const BADGE_R: f32 = 18.0;
    /// Half the horizontal spread between the V and S glyph
    /// centers. Less than the glyph half-width so the two
    /// letters smush into each other — the pair reads as one
    /// stamped "VS" mark, not two separate characters.
    const GLYPH_DX: f32 = 3.0;
    /// Half the vertical stagger between the V and S glyph
    /// centers. V sits above center, S sits below, giving the
    /// pair a fighting-game-style diagonal stack.
    const GLYPH_DY: f32 = 3.0;

    struct VsDiagonal;

    impl<M> iced::widget::canvas::Program<M> for VsDiagonal {
        type State = ();

        fn draw(
            &self,
            _state: &(),
            renderer: &Renderer,
            theme: &Theme,
            bounds: Rectangle,
            _cursor: iced::mouse::Cursor,
        ) -> Vec<iced::widget::canvas::Geometry> {
            let mut frame = Frame::new(renderer, bounds.size());
            let cx = bounds.width / 2.0;
            let w = bounds.width;
            let h = bounds.height;

            // Player-color tints — left half red (P1), right half
            // blue (P2), split by the diagonal cut. Outer corners
            // are rounded to [`PANE_RADIUS`] so the painted halves
            // match the pane plate's rounded chrome; inner edge is
            // the straight diagonal. Alpha is moderate so the
            // pane plate underneath still reads as the dominant
            // surface and the side cards' text stays legible.
            const PANE_RADIUS: f32 = 4.0;
            let red = iced::Color {
                a: 0.35,
                ..iced::Color::from_rgb(0.85, 0.22, 0.28)
            };
            let blue = iced::Color {
                a: 0.35,
                ..iced::Color::from_rgb(0.18, 0.40, 0.85)
            };
            let left = Path::new(|p| {
                // Start on the top edge, just right of the
                // top-left arc; trace the top edge to the
                // diagonal, down the diagonal, along the bottom
                // edge to the bottom-left arc, then round the two
                // outer corners on the way back up.
                p.move_to(Point::new(PANE_RADIUS, 0.0));
                p.line_to(Point::new(cx + TILT, 0.0));
                p.line_to(Point::new(cx - TILT, h));
                p.line_to(Point::new(PANE_RADIUS, h));
                p.arc_to(Point::new(0.0, h), Point::new(0.0, 0.0), PANE_RADIUS);
                p.arc_to(Point::new(0.0, 0.0), Point::new(w, 0.0), PANE_RADIUS);
                p.close();
            });
            frame.fill(&left, red);
            let right = Path::new(|p| {
                p.move_to(Point::new(w - PANE_RADIUS, 0.0));
                p.line_to(Point::new(cx + TILT, 0.0));
                p.line_to(Point::new(cx - TILT, h));
                p.line_to(Point::new(w - PANE_RADIUS, h));
                p.arc_to(Point::new(w, h), Point::new(w, 0.0), PANE_RADIUS);
                p.arc_to(Point::new(w, 0.0), Point::new(0.0, 0.0), PANE_RADIUS);
                p.close();
            });
            frame.fill(&right, blue);

            // Body-bg-colored band so the pane plate reads as
            // cut, with the page surface showing through. The
            // band has to share the polygons' slope, otherwise
            // the visible diagonals diverge — at the canvas edges
            // the band's centerline would land short of the
            // polygon corner. So extrapolate the polygon line
            // (cx±TILT at y=0/h) out to y=±OVERSHOOT, picking up
            // an extra horizontal swing of `slash_extra` at each
            // end. Butt caps land outside the canvas; visibly the
            // cut meets (and continues past) the pane edges.
            let body_bg = theme.palette().background;
            let slash_extra = TILT * 2.0 * OVERSHOOT / h;
            let line = Path::line(
                Point::new(cx + TILT + slash_extra, -OVERSHOOT),
                Point::new(cx - TILT - slash_extra, h + OVERSHOOT),
            );
            frame.stroke(
                &line,
                Stroke {
                    style: Style::Solid(body_bg),
                    width: BAND_W,
                    line_cap: LineCap::Butt,
                    ..Default::default()
                },
            );

            // Body-bg-colored circle the "VS" sits in. Same color
            // as the band so the two visually fuse into one shape:
            // a slim cut through the pane with a wider node where
            // the badge sits.
            let badge = Path::circle(Point::new(cx, h / 2.0), BADGE_R);
            frame.fill(&badge, body_bg);

            // V upper-left of center, S lower-right of center —
            // the cut runs diagonally between them. Heavy italic
            // in the theme's primary accent so the pair reads as
            // a fighting-game splash on top of the slash.
            let cy = h / 2.0;
            let color = muted_color(theme);
            let fun_font = iced::Font {
                family: iced::font::Family::Name("Noto Sans"),
                weight: iced::font::Weight::Black,
                style: iced::font::Style::Italic,
                ..iced::Font::default()
            };
            let mut glyph = |content: &str, x: f32, y: f32| {
                frame.fill_text(CanvasText {
                    content: content.into(),
                    position: Point::new(x, y),
                    color,
                    size: Pixels(GLYPH),
                    font: fun_font,
                    align_x: iced::advanced::text::Alignment::Center,
                    align_y: iced::alignment::Vertical::Center,
                    ..Default::default()
                });
            };
            glyph("V", cx - GLYPH_DX, cy - GLYPH_DY);
            glyph("S", cx + GLYPH_DX, cy + GLYPH_DY);

            vec![frame.into_geometry()]
        }
    }

    Canvas::new(VsDiagonal).width(Length::Fill).height(Length::Fill).into()
}
