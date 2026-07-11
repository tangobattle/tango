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
use sweeten::widget::{column, row};

mod menu_button;
pub use menu_button::{MenuButton, MenuItem};

/// Icon-only button for low-emphasis toolbar actions (rescan,
/// copy, open-folder, etc.). Uses [`neutral`] — a soft, theme-
/// aware style that doesn't compete with primary CTAs in the
/// same row. The plain-text label is exposed as a hover tooltip.
pub fn icon_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, padding: [f32; 2]) -> Element<'a, M> {
    icon_button_styled(icon, label, Some(msg), padding, neutral)
}

/// A ⋮ "more actions" button: [`icon_button`] chrome on the trigger,
/// the standard dropdown overlay for the actions. `label` is the
/// hover tooltip; each item's message fires on selection (the
/// dropdown closes itself, on selection or click-away). Disabled
/// (greyed, won't open) when `enabled` is false — for rows whose
/// actions all need a selection to act on.
pub fn menu_button<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    items: Vec<MenuItem<M>>,
    enabled: bool,
    padding: [f32; 2],
) -> Element<'a, M> {
    let btn = menu_button::MenuButton::new(
        icon.widget(),
        items,
        enabled,
        padding,
        crate::style::STANDARD_PADDING,
        neutral,
    );
    // Tooltip above, not below — below is where the dropdown lands,
    // and the bubble lingers while the cursor rests on the trigger.
    tooltip(btn, tooltip_bubble(label), tooltip::Position::Top)
        .gap(4)
        .into()
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
            let sel = crate::theme::SELECT_YELLOW;
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
    // rule as [`panel`]).
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

/// The fullscreen top bar's app-close X — window chrome, not a
/// toolbar action. Borderless and muted at rest so it doesn't
/// compete with the nav pills, flipping to a solid danger plate
/// with a white glyph on hover: the universal titlebar-close
/// idiom, so "this closes the whole app" lands before the tooltip
/// does.
pub fn window_close(theme: &Theme, status: button::Status) -> button::Style {
    let danger = theme.palette().danger;
    let (bg, text_color) = match status {
        button::Status::Hovered => (danger, iced::Color::WHITE),
        button::Status::Pressed => (mix(danger, iced::Color::BLACK, 0.15), iced::Color::WHITE),
        button::Status::Active | button::Status::Disabled => (iced::Color::TRANSPARENT, muted_color(theme)),
    };
    button::Style {
        background: Some(iced::Background::Color(bg)),
        text_color,
        border: iced::Border {
            color: iced::Color::TRANSPARENT,
            width: 0.0,
            radius: tech_radius(8.0),
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
    tooltip(btn, tooltip_bubble(label), tooltip::Position::Bottom)
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

/// A caption label stacked over a control — the "form row" shape
/// used by the welcome screen (settings rows use [`option_row`]).
pub fn labeled<'a, M: Clone + 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    sweeten::widget::column![text(label).size(TEXT_CAPTION).style(muted_text_style), ctrl.into(),]
        .spacing(4)
        .into()
}

/// Fixed height of every [`option_row`] — one slot size whatever
/// the control (a text input, a picker, a bare checkbox), so a
/// settings pane reads as an even options list, not a form whose
/// rows breathe with their contents.
const OPTION_ROW_HEIGHT: f32 = 40.0;

/// A full-width "options screen" row: label on the left, control
/// hugging the right edge, every row exactly
/// [`OPTION_ROW_HEIGHT`] tall — the console-menu shape, not a
/// desktop form's caption-over-control. The label is body-sized
/// ink (not a muted caption): on an options screen the setting's
/// name IS the row, not an annotation on it.
pub fn option_row<'a, M: 'a>(label: String, ctrl: impl Into<Element<'a, M>>) -> Element<'a, M> {
    row![
        text(label).size(TEXT_BODY),
        iced::widget::space::horizontal(),
        ctrl.into(),
    ]
    .spacing(12)
    .padding([0, 10])
    .align_y(Alignment::Center)
    .width(Length::Fill)
    .height(Length::Fixed(OPTION_ROW_HEIGHT))
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

/// Larger pill for the global top nav (Play / Replays).
/// TEXT_HEADING-sized icon + label so the chrome reads as the
/// primary navigation for the whole app.
pub fn nav_tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    tab_button_inner(icon, Some(label), msg, active, true)
}

/// [`nav_tab_button`] with an attention dot: a small primary-glow pip
/// floated over the pill's top-right corner, for "something is live
/// on this tab while you're looking at another" (e.g. an open lobby).
/// The pip is an overlay, not row content — it takes no layout space,
/// so the pill is exactly [`nav_tab_button`]-sized whether the dot is
/// lit, unlit, or never there, and the tab strip never shifts.
pub fn nav_tab_button_badged<'a, M: Clone + 'a>(
    icon: Icon,
    label: String,
    msg: M,
    active: bool,
    badge: bool,
) -> Element<'a, M> {
    let pill = tab_button_inner(icon, Some(label), msg, active, true);
    if !badge {
        return pill;
    }
    // 7 px glowing pip in the pill's top-right corner. The Fill
    // container resolves to the base layer's (the pill's) bounds —
    // see iced's Stack sizing — so this floats inside the pill's
    // own chrome.
    let pip = container(iced::widget::Space::new().width(7).height(7)).style(|theme: &Theme| {
        let primary = theme.palette().primary;
        container::Style {
            background: Some(iced::Background::Color(primary)),
            border: iced::Border {
                radius: 3.5.into(),
                ..Default::default()
            },
            shadow: iced::Shadow {
                color: iced::Color { a: 0.7, ..primary },
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

/// Icon-only variant of [`nav_tab_button`] for the right-aligned
/// utility tabs (Patches, Settings).
pub fn nav_icon_tab_button<'a, M: Clone + 'a>(
    icon: Icon,
    tooltip_label: String,
    msg: M,
    active: bool,
) -> Element<'a, M> {
    let stacked = tab_button_inner(icon, None, msg, active, true);
    tooltip(stacked, tooltip_bubble(tooltip_label), tooltip::Position::Bottom)
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
        // Faint accent hairline — the quiet cousin of [`panel`]'s
        // full frame, just enough edge that panes read as PET
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

/// The standard tab body: a full-width `top` strip above a left/right split,
/// with every gap and the outer inset set to [`PANE_GAP`]. Shared by the
/// Patches and Replays tabs.
pub fn top_split_pane<'a, M: 'a>(
    top: impl Into<Element<'a, M>>,
    left: impl Into<Element<'a, M>>,
    right: impl Into<Element<'a, M>>,
) -> Element<'a, M> {
    let top: Element<'a, M> = top.into();
    let left: Element<'a, M> = left.into();
    let right: Element<'a, M> = right.into();
    column![top, row![left, right].spacing(PANE_GAP).height(Length::Fill)]
        .spacing(PANE_GAP)
        .padding(PANE_GAP)
        .height(Length::Fill)
        .into()
}

/// A detail pane's empty state: `message` centered on the [`pane`] plate.
/// Shown by the Patches / Replays tabs when nothing is selected.
pub fn pane_prompt<'a, M: 'a>(message: String) -> Element<'a, M> {
    container(text(message).size(TEXT_BODY))
        .center(Length::Fill)
        .style(pane)
        .into()
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

pub fn mix(a: iced::Color, b: iced::Color, t: f32) -> iced::Color {
    iced::Color {
        r: a.r * (1.0 - t) + b.r * t,
        g: a.g * (1.0 - t) + b.g * t,
        b: a.b * (1.0 - t) + b.b * t,
        a: 1.0,
    }
}

/// Rotate a color's hue by `deg` degrees (HSV space; saturation and
/// value hold). This is how accent-relative companion tones are
/// derived — e.g. the scanline's far stop sits a quarter-turn from
/// the accent so the pair reads as one energy family no matter
/// which chrome color the user picked.
pub fn rotate_hue(c: iced::Color, deg: f32) -> iced::Color {
    let max = c.r.max(c.g).max(c.b);
    let min = c.r.min(c.g).min(c.b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == c.r {
        60.0 * ((c.g - c.b) / d).rem_euclid(6.0)
    } else if max == c.g {
        60.0 * ((c.b - c.r) / d + 2.0)
    } else {
        60.0 * ((c.r - c.g) / d + 4.0)
    };
    let h = (h + deg).rem_euclid(360.0);
    let (s, v) = (if max == 0.0 { 0.0 } else { d / max }, max);
    let chroma = v * s;
    let x = chroma * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - chroma;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    iced::Color {
        r: r + m,
        g: g + m,
        b: b + m,
        a: c.a,
    }
}

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

/// The tone dark-theme control plates (buttons, inputs, pickers,
/// checkbox boxes, slider rails) are lifted toward: the neutral
/// text white warmed with a whisper (~18%) of the accent, so
/// plates read as neutral gray with a hint of the chrome's green
/// rather than as colored surfaces. Both stronger recipes failed
/// on sight: lifting toward a tinted text color cast every control
/// blue, and lifting toward a heavy accent mix turned the whole UI
/// green. Light theme keeps its white/parchment lifts and doesn't
/// use this.
fn plate_lift(theme: &Theme) -> iced::Color {
    mix(theme.palette().primary, theme.palette().text, 0.82)
}

/// Dark "ink" for text sitting on a bright accent plate — the
/// selection gold today, any light accent tomorrow. BNLC letters
/// its bright chrome in a dark ink, not white (white genuinely
/// fails contrast on these light fills); ours leans green-black to
/// match the rest of the dark family instead of BNLC's navy.
pub const ACCENT_INK: iced::Color =
    iced::Color::from_rgb(0x0a as f32 / 255.0, 0x20 as f32 / 255.0, 0x12 as f32 / 255.0);

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
        // the bg color so the gradient is felt, not seen. Uniform
        // channel decay — the old blue-retaining multipliers were
        // a navy-era trick that re-tints a neutral base cool.
        (
            iced::Color {
                r: bg.r * 0.7,
                g: bg.g * 0.7,
                b: bg.b * 0.7,
                a: 1.0,
            },
            iced::Color {
                r: bg.r * 0.4,
                g: bg.g * 0.4,
                b: bg.b * 0.4,
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

/// Body surface (everything below the HUD bar). Paints no
/// background of its own — the content layer rides on
/// [`cyber_backdrop`], stacked underneath by `App::view`, and an
/// opaque fill here would blot the cyberworld out.
pub fn body_surface(theme: &Theme) -> iced::widget::container::Style {
    iced::widget::container::Style {
        background: None,
        text_color: Some(theme.palette().text),
        ..Default::default()
    }
}

/// The cyberworld backdrop — the Legacy Collection's PET menu
/// background, drawn instead of shipped as a bitmap: a vertical
/// wash that's lit at the top and falls toward black, two big
/// soft ring clusters (the de-focused "net" circles behind BNLC's
/// menus), a dashed orbit ring, and a loose scatter of hexagons.
/// Static — no animation — and cached; the geometry only
/// re-tessellates when the canvas resizes or the theme flips.
pub fn cyber_backdrop<'a, M: 'a>() -> Element<'a, M> {
    use iced::widget::canvas::{self, gradient, Canvas, LineDash, Path, Stroke, Style};
    use iced::{Point, Rectangle, Renderer};

    struct Backdrop;

    #[derive(Default)]
    struct State {
        cache: canvas::Cache,
        /// Palette fingerprint the cached geometry was drawn with.
        /// `Cache` only invalidates on size changes, so theme flips
        /// have to clear it by hand or the old colors stick. Covers
        /// both the background AND the primary — an accent change
        /// keeps the background identical, and the whole point of
        /// the backdrop is the accent-colored glow.
        key: std::cell::Cell<u64>,
    }

    impl<M> canvas::Program<M> for Backdrop {
        type State = State;

        fn draw(
            &self,
            state: &State,
            renderer: &Renderer,
            theme: &Theme,
            bounds: Rectangle,
            _cursor: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            let bg = theme.palette().background;
            let primary = theme.palette().primary;
            let dark = theme.extended_palette().is_dark;
            let fp = |c: iced::Color| {
                (((c.r * 255.0) as u64) << 16) | (((c.g * 255.0) as u64) << 8) | ((c.b * 255.0) as u64)
            };
            let key = fp(bg) | (fp(primary) << 24) | ((dark as u64) << 63);
            if state.key.replace(key) != key {
                state.cache.clear();
            }
            let geom = state.cache.draw(renderer, bounds.size(), |frame| {
                let w = frame.width();
                let h = frame.height();
                // Master intensity — the whole backdrop runs at a
                // fraction of this on light so it stays a texture,
                // not a watermark fighting dark text. Dialed down a
                // notch from 0.45 when the lattice + traces landed:
                // more geometry at the same alpha reads busier.
                let lvl = if dark { 1.0 } else { 0.40 };
                let glow = move |a: f32| iced::Color { a: a * lvl, ..primary };

                // Base wash: a faint screen-glow at the top falling
                // to a darker floor, so the page reads as a lit PET
                // screen rather than a flat sheet.
                frame.fill_rectangle(
                    Point::ORIGIN,
                    frame.size(),
                    gradient::Linear::new(Point::ORIGIN, Point::new(0.0, h))
                        .add_stop(0.0, mix(bg, primary, if dark { 0.06 } else { 0.03 }))
                        .add_stop(0.55, bg)
                        .add_stop(1.0, mix(bg, iced::Color::BLACK, if dark { 0.28 } else { 0.06 })),
                );

                // One "net ring" cluster: a fat blurry-reading band
                // (low alpha, huge stroke), a mid ring, a crisp thin
                // rim, and a dashed orbit — the de-focused circle
                // stacks behind every BNLC menu.
                let cluster = |frame: &mut canvas::Frame, c: Point, s: f32, boost: f32| {
                    let g = |a: f32| glow(a * boost);
                    frame.fill(&Path::circle(c, s * 0.20), g(0.05));
                    frame.stroke(
                        &Path::circle(c, s * 0.46),
                        Stroke {
                            style: Style::Solid(g(0.05)),
                            width: s * 0.16,
                            ..Stroke::default()
                        },
                    );
                    frame.stroke(
                        &Path::circle(c, s * 0.62),
                        Stroke {
                            style: Style::Solid(g(0.08)),
                            width: s * 0.05,
                            ..Stroke::default()
                        },
                    );
                    frame.stroke(
                        &Path::circle(c, s * 0.72),
                        Stroke {
                            style: Style::Solid(g(0.16)),
                            width: 1.5,
                            ..Stroke::default()
                        },
                    );
                    frame.stroke(
                        &Path::circle(c, s * 0.54),
                        Stroke {
                            style: Style::Solid(g(0.13)),
                            width: 2.0,
                            line_dash: LineDash {
                                segments: &[18.0, 12.0],
                                offset: 0,
                            },
                            ..Stroke::default()
                        },
                    );
                };
                cluster(frame, Point::new(w * 0.16, h * 0.40), h * 0.85, 1.0);
                cluster(frame, Point::new(w * 0.88, h * 0.74), h * 0.55, 0.8);
                cluster(frame, Point::new(w * 0.60, h * 0.08), h * 0.30, 0.6);

                // Hexagon drift — the collection's other signature
                // motif, scattered loosely toward the corners the
                // rings leave empty.
                let hex = |c: Point, r: f32| {
                    Path::new(|b| {
                        for i in 0..6 {
                            let ang = std::f32::consts::FRAC_PI_3 * i as f32;
                            let pt = Point::new(c.x + r * ang.cos(), c.y + r * ang.sin());
                            if i == 0 {
                                b.move_to(pt);
                            } else {
                                b.line_to(pt);
                            }
                        }
                        b.close();
                    })
                };
                let outline = |frame: &mut canvas::Frame, c: Point, r: f32, a: f32| {
                    frame.stroke(
                        &hex(c, r),
                        Stroke {
                            style: Style::Solid(glow(a)),
                            width: 1.5,
                            ..Stroke::default()
                        },
                    );
                };
                outline(frame, Point::new(w * 0.90, h * 0.18), 18.0, 0.12);
                frame.fill(&hex(Point::new(w * 0.94, h * 0.27), 11.0), glow(0.08));
                outline(frame, Point::new(w * 0.855, h * 0.295), 9.0, 0.08);
                outline(frame, Point::new(w * 0.105, h * 0.80), 15.0, 0.10);
                frame.fill(&hex(Point::new(w * 0.155, h * 0.875), 9.0), glow(0.06));

                // Honeycomb lattice sunk into the bottom edge — a
                // patch of the cyberworld's floor grid showing
                // through between the ring clusters. Alpha falls
                // off away from the center column and a few cells
                // are skipped (deterministically — the cached
                // geometry must redraw identically) so it reads as
                // a ragged lit floor, not wallpaper tiling.
                let lat_r = 16.0_f32;
                let lat = Point::new(w * 0.52, h * 1.02);
                for col in -4i32..=4 {
                    for row in -1i32..=1 {
                        if (col * 7 + row * 5).rem_euclid(5) == 0 {
                            continue;
                        }
                        let c = Point::new(
                            lat.x + 1.5 * lat_r * col as f32,
                            lat.y + 3f32.sqrt() * lat_r * (row as f32 + if col.rem_euclid(2) == 1 { 0.5 } else { 0.0 }),
                        );
                        let fall = 1.0 - (col.abs() as f32 / 4.0) * 0.75;
                        frame.stroke(
                            &hex(c, lat_r),
                            Stroke {
                                style: Style::Solid(glow((0.10 * fall).max(0.02))),
                                width: 1.0,
                                ..Stroke::default()
                            },
                        );
                    }
                }
                // One lit cell in the patch — the grid's "live node",
                // same trick as the hex chain's lead hex.
                frame.fill(
                    &hex(
                        Point::new(lat.x + 1.5 * lat_r, lat.y - 3f32.sqrt() * lat_r * 0.5),
                        lat_r,
                    ),
                    glow(0.05),
                );

                // Circuit traces — the 45°-jog runs the HUD's hex
                // chain ends in, etched big and faint across the
                // flanks the rings leave empty, each terminating in
                // a haloed node dot.
                let trace = |frame: &mut canvas::Frame, pts: &[Point], a: f32| {
                    let path = Path::new(|b| {
                        b.move_to(pts[0]);
                        for pt in &pts[1..] {
                            b.line_to(*pt);
                        }
                    });
                    frame.stroke(
                        &path,
                        Stroke {
                            style: Style::Solid(glow(a)),
                            width: 1.5,
                            ..Stroke::default()
                        },
                    );
                    let end = pts[pts.len() - 1];
                    frame.fill(&Path::circle(end, 2.5), glow(a * 1.8));
                    frame.stroke(
                        &Path::circle(end, 5.5),
                        Stroke {
                            style: Style::Solid(glow(a)),
                            width: 1.0,
                            ..Stroke::default()
                        },
                    );
                };
                // Left flank, running in from the window edge; the
                // jogs keep equal dx/dy so the diagonals hold 45°.
                trace(
                    frame,
                    &[
                        Point::new(0.0, h * 0.66),
                        Point::new(w * 0.05, h * 0.66),
                        Point::new(w * 0.05 + h * 0.06, h * 0.60),
                        Point::new(w * 0.22, h * 0.60),
                    ],
                    0.10,
                );
                // Down from the top edge between the HUD and the
                // small ring cluster.
                trace(
                    frame,
                    &[
                        Point::new(w * 0.70, 0.0),
                        Point::new(w * 0.70, h * 0.10),
                        Point::new(w * 0.70 - h * 0.05, h * 0.15),
                        Point::new(w * 0.70 - h * 0.05, h * 0.24),
                    ],
                    0.08,
                );
            });
            vec![geom]
        }
    }

    Canvas::new(Backdrop).width(Length::Fill).height(Length::Fill).into()
}

/// The Legacy Collection's header hexagon motif, upgraded from a
/// flat row of pips to a honeycomb burst: a zigzag cluster whose
/// lead hex burns hot (halo + bright core + rim) and whose tail
/// decays through dimmer fills into bare outlines, with a circuit
/// trace carrying the energy off to the right and terminating in
/// a node dot. Decorative only; `height` pins the canvas so it
/// slots into the nav row without affecting the strip's height.
pub fn hex_chain<'a, M: 'a>(height: f32) -> Element<'a, M> {
    use iced::widget::canvas::{self, Canvas, Path, Stroke, Style};
    use iced::{Point, Rectangle, Renderer};

    /// Hexes in the honeycomb cluster (zigzag, alternating above /
    /// below the centerline).
    const COUNT: usize = 7;
    /// Length of the circuit trace running out of the last hex,
    /// including the terminal node.
    const TRACE: f32 = 30.0;

    struct HexChain {
        height: f32,
    }

    impl<M> canvas::Program<M> for HexChain {
        type State = ();

        fn draw(
            &self,
            _state: &(),
            renderer: &Renderer,
            theme: &Theme,
            bounds: Rectangle,
            _cursor: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            let primary = theme.palette().primary;
            let cy = bounds.height / 2.0;
            // Hex circumradius sized so the zigzag (hex height
            // √3·r plus the ±0.433r row stagger) fills the canvas.
            let r = self.height / 2.6;

            // Flat-top hexagon (points left/right), like BNLC's.
            let hex = |c: Point, r: f32| {
                Path::new(|b| {
                    for k in 0..6 {
                        let ang = std::f32::consts::FRAC_PI_3 * k as f32;
                        let pt = Point::new(c.x + r * ang.cos(), c.y + r * ang.sin());
                        if k == 0 {
                            b.move_to(pt);
                        } else {
                            b.line_to(pt);
                        }
                    }
                    b.close();
                })
            };

            let center = |i: usize| {
                Point::new(
                    r + 1.0 + 1.5 * r * i as f32,
                    // True honeycomb stagger: adjacent columns sit
                    // ±(√3/4)·r off the centerline.
                    cy + if i.is_multiple_of(2) { 0.433 * r } else { -0.433 * r },
                )
            };

            for i in 0..COUNT {
                let c = center(i);
                match i {
                    // Lead hex: soft halo underneath, hot core fill,
                    // bright rim on top — the "live node".
                    0 => {
                        frame.fill(&hex(c, r * 1.55), iced::Color { a: 0.12, ..primary });
                        frame.fill(&hex(c, r), mix(primary, iced::Color::WHITE, 0.25));
                        frame.stroke(
                            &hex(c, r),
                            Stroke {
                                style: Style::Solid(mix(primary, iced::Color::WHITE, 0.6)),
                                width: 1.2,
                                ..Stroke::default()
                            },
                        );
                    }
                    // Decaying solid tail.
                    1 => frame.fill(&hex(c, r), iced::Color { a: 0.85, ..primary }),
                    2 => frame.fill(&hex(c, r), iced::Color { a: 0.40, ..primary }),
                    // Outline fade-out, floored so the tail never
                    // quite vanishes (or goes negative).
                    _ => frame.stroke(
                        &hex(c, r),
                        Stroke {
                            style: Style::Solid(iced::Color {
                                a: (0.50 - 0.12 * (i - 3) as f32).max(0.10),
                                ..primary
                            }),
                            width: 1.5,
                            ..Stroke::default()
                        },
                    ),
                }
            }

            // Circuit trace out of the last hex: a short run at the
            // hex's row, a 45° jog back to the centerline, then on
            // to a terminal node dot.
            let last = center(COUNT - 1);
            let jog = (last.y - cy).abs();
            let x0 = last.x + r + 1.0;
            let trace = Path::new(|b| {
                b.move_to(Point::new(x0, last.y));
                b.line_to(Point::new(x0 + 5.0, last.y));
                b.line_to(Point::new(x0 + 5.0 + jog, cy));
                b.line_to(Point::new(x0 + TRACE - 5.0, cy));
            });
            frame.stroke(
                &trace,
                Stroke {
                    style: Style::Solid(iced::Color { a: 0.45, ..primary }),
                    width: 1.5,
                    ..Stroke::default()
                },
            );
            frame.fill(
                &Path::circle(Point::new(x0 + TRACE - 2.0, cy), 2.0),
                iced::Color { a: 0.8, ..primary },
            );

            vec![frame.into_geometry()]
        }
    }

    let r = height / 2.6;
    let w = (r + 1.0 + 1.5 * r * (COUNT - 1) as f32) + r + 1.0 + TRACE + 2.0;
    Canvas::new(HexChain { height })
        .width(Length::Fixed(w))
        .height(Length::Fixed(height))
        .into()
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
            // Shift the right edge a quarter-turn around the hue
            // wheel (green→teal, blue→violet, red→orange…) so the
            // rule has motion without leaving the accent's family —
            // the old green-tuned channel math landed off-brand
            // colors under other accents.
            let shifted = rotate_hue(primary, 45.0);
            // Re-punch the rotated stop: push it away from gray so
            // the far end burns as hot as the old hand-tuned teal
            // did, instead of a mid-tone accent fading politely.
            let gray = (shifted.r + shifted.g + shifted.b) / 3.0;
            let right = iced::Color {
                r: (gray + (shifted.r - gray) * 1.4).clamp(0.0, 1.0),
                g: (gray + (shifted.g - gray) * 1.4).clamp(0.0, 1.0),
                b: (gray + (shifted.b - gray) * 1.4).clamp(0.0, 1.0),
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
/// panels, settings groups). The full Legacy Collection treatment:
/// accent-cast plate, glowing accent frame, tech-radius corners —
/// the PET menu's framed panels, not CSS rectangles.
pub fn panel(theme: &Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let text = theme.palette().text;
    let primary = theme.palette().primary;
    // Slightly lifted plate. On dark, lift through [`plate_lift`]
    // so the card reads above the body without taking on the
    // accent's hue — the green lives in the frame, not the fill.
    // On light, go toward white so the card looks like paper on
    // parchment.
    let plate = if p.is_dark {
        mix(bg, plate_lift(theme), 0.12)
    } else {
        mix(bg, iced::Color::WHITE, 0.4)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(plate)),
        text_color: Some(text),
        border: iced::Border {
            radius: tech_radius(14.0),
            width: 1.5,
            color: iced::Color {
                a: if p.is_dark { 0.65 } else { 0.45 },
                ..primary
            },
        },
        // On dark the shadow is the frame's accent glow (centered,
        // no offset — light radiating off the border, not a drop
        // shadow). Light theme keeps a soft black drop; a colored
        // glow on a pale page reads as smudge.
        shadow: if p.is_dark {
            iced::Shadow {
                color: iced::Color { a: 0.28, ..primary },
                offset: iced::Vector::new(0.0, 0.0),
                blur_radius: 16.0,
            }
        } else {
            iced::Shadow {
                color: iced::Color {
                    a: 0.18,
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            }
        },
        snap: false,
    }
}

/// The scaffolding every modal overlay shares: `panel` (already
/// pop-animated by the caller if it animates) wrapped in a
/// click-swallowing mouse_area and centered, stacked over a dim
/// backdrop wash at `backdrop_alpha` (callers scale their resting
/// alpha by the open-transition's progress so the dim fades with
/// the panel). `dismiss`, when armed, closes the modal on a
/// backdrop click — pass `None` while the modal is animating out
/// so a click mid-fade can't re-fire the close (and for modals
/// that must not be click-dismissed at all).
pub fn modal_layer<'a, M: Clone + 'a>(
    panel: Element<'a, M>,
    backdrop_alpha: f32,
    swallow: M,
    dismiss: Option<M>,
) -> Element<'a, M> {
    let placement = container(sweeten::widget::mouse_area(panel).on_press(move |_| swallow.clone()))
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center);
    let mut backdrop = sweeten::widget::mouse_area(
        container(iced::widget::Space::new().width(Length::Fill).height(Length::Fill))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(crate::anim::backdrop_style(backdrop_alpha)),
    );
    if let Some(m) = dismiss {
        backdrop = backdrop.on_press(move |_| m.clone());
    }
    iced::widget::stack![Element::from(backdrop), Element::from(placement)].into()
}

/// The molded-plastic fill the drawn-GBA console keys share (and the
/// D-pad hub) — a step above the surrounding plate so keys read as
/// raised. Used by the settings input pane's console and the replay
/// input display, which mirrors its layout.
pub fn gba_key_plate(theme: &Theme) -> iced::Color {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    if p.is_dark {
        mix(bg, theme.palette().text, 0.16)
    } else {
        mix(bg, iced::Color::WHITE, 0.65)
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

/// The standard dropdown: sweeten's `pick_list` with the
/// [`chunky_pick_list`] chrome and [`STANDARD_PADDING`] applied.
/// Callers chain extras (`.placeholder`, `.width`, `.disabled`) on
/// the returned picker; compact in-pane variants (CONTROL_PADDING +
/// smaller text) keep hand-building.
///
/// [`STANDARD_PADDING`]: crate::style::STANDARD_PADDING
pub fn picker<'a, T, L, V, M>(
    options: L,
    selected: Option<V>,
    on_selected: impl Fn(T) -> M + 'a,
) -> sweeten::widget::PickList<'a, T, L, V, M>
where
    T: ToString + PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
    M: Clone,
{
    sweeten::widget::pick_list(options, selected, on_selected)
        .padding(crate::style::STANDARD_PADDING)
        .style(chunky_pick_list)
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
    let dim = if p.is_dark {
        mix(bg, plate_lift(theme), 0.11)
    } else {
        mix(bg, text, 0.08)
    };
    iced::widget::container::Style {
        text_color: Some(iced::Color { a: 0.35, ..text }),
        background: Some(iced::Background::Color(dim)),
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

/// Chunky slider matching the button bevel: a thicker rounded rail
/// whose filled side runs primary (brightening on hover / drag) and
/// whose empty side is the neutral plate, plus a circular handle
/// with the same white-tinted border as the CTA buttons so it reads
/// as a physical thumb rather than iced's flat default dot.
pub fn chunky_slider(theme: &Theme, status: iced::widget::slider::Status) -> iced::widget::slider::Style {
    use iced::widget::slider::{Handle, HandleShape, Rail, Status, Style};
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    let bg = theme.palette().background;
    let text = theme.palette().text;
    // Empty track: same plate recipe as the neutral button so the
    // rail reads as part of the same widget family.
    let track = if p.is_dark {
        mix(bg, plate_lift(theme), 0.18)
    } else {
        mix(bg, text, 0.18)
    };
    let (fill, grip, radius) = match status {
        Status::Hovered => (
            mix(primary, iced::Color::WHITE, 0.10),
            mix(primary, iced::Color::WHITE, 0.18),
            9.0,
        ),
        Status::Dragged => (
            mix(primary, iced::Color::WHITE, 0.18),
            mix(primary, iced::Color::WHITE, 0.28),
            9.0,
        ),
        Status::Active => (primary, primary, 8.0),
    };
    Style {
        rail: Rail {
            backgrounds: (iced::Background::Color(fill), iced::Background::Color(track)),
            width: 6.0,
            border: iced::Border {
                radius: 3.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
        },
        handle: Handle {
            shape: HandleShape::Circle { radius },
            background: iced::Background::Color(grip),
            border_width: 2.0,
            border_color: mix(primary, iced::Color::WHITE, 0.35),
        },
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

/// Slim progress bar: faint text-tinted track + primary fill with
/// pill-rounded ends. Pair with `.girth(Length::Fixed(4.0))` for
/// the thin "loading strip" look used by the replay exporter.
pub fn slim_progress_bar(theme: &Theme) -> iced::widget::progress_bar::Style {
    let text = theme.palette().text;
    iced::widget::progress_bar::Style {
        background: iced::Background::Color(iced::Color { a: 0.12, ..text }),
        bar: iced::Background::Color(theme.palette().primary),
        border: iced::Border {
            radius: 999.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
    }
}

/// The "you vs opponent" matchup pane shared by the lobby band and
/// the replay detail: the two side cards with a wide gap so the
/// diagonal cut + VS badge from [`vs_splitter`] paints through the
/// middle. The splitter canvas (which also paints the red/blue half
/// tints) is layered *under* the row, so the cards sit on top of
/// the colored plate. Top-aligned so the left card doesn't bounce
/// when the right one grows (the lobby's opponent card gains a line
/// when their settings land).
pub fn matchup_pane<'a, M: 'a>(left: Element<'a, M>, right: Element<'a, M>) -> Element<'a, M> {
    let sides_row = row![left, right].spacing(56).align_y(Alignment::Start);
    container(
        iced::widget::Stack::new()
            .push(
                container(sides_row)
                    .padding(crate::style::PANE_PADDING)
                    .width(Length::Fill),
            )
            .push_under(vs_splitter()),
    )
    .width(Length::Fill)
    .style(pane)
    .into()
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
    use iced::widget::canvas::{Canvas, Frame, LineCap, Path, Stroke, Style};
    use iced::{Point, Rectangle, Renderer};

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
    /// "V" / "S" glyph box: per-letter width and cap height of the
    /// hand-drawn letterforms. Roughly what the old 18px font-rendered
    /// glyphs occupied.
    const GLYPH_W: f32 = 10.0;
    const GLYPH_H: f32 = 12.0;
    /// Stroke weight of the letterforms — heavy, keeping the "Black"
    /// weight look of the old font-rendered glyphs.
    const GLYPH_T: f32 = 2.8;
    /// Italic shear: horizontal offset per unit of height above the
    /// glyph's vertical center (≈12°, matching Noto's italic angle).
    const SLANT: f32 = 0.21;
    /// Radius of the body-bg-colored circle that the "VS" sits
    /// inside. Sized so the glyph pair has a comfortable margin
    /// to the rim; the circle merges seamlessly with the band
    /// (same color), reading as a node bulging out of the cut.
    const BADGE_R: f32 = 18.0;
    /// Half the horizontal spread between the V and S glyph
    /// centers. Less than the glyph width so the letter boxes
    /// overlap diagonally — the pair reads as one stamped "VS"
    /// mark — but enough that a hairline channel, parallel to
    /// the cut, stays open between the V's stem and the S's
    /// top bar.
    const GLYPH_DX: f32 = 4.0;
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
            // the cut runs diagonally between them. The letterforms
            // are hand-traced filled polygons, sheared for the
            // italic lean: none of the bundled Noto faces carry a
            // Black Italic, and two letters aren't worth shipping
            // one for. Heavy and leaned-over so the pair still
            // reads as a fighting-game splash stamped on the slash.
            let cy = h / 2.0;
            let color = muted_color(theme);
            // Outline points are in glyph-local coordinates: origin
            // at the letter's center, y down. The shear leans the
            // top of each letter to the right; horizontal edges stay
            // horizontal, as in a real italic.
            let glyph = |outline: &[(f32, f32)], gx: f32, gy: f32| {
                Path::new(|p| {
                    let mut pts = outline.iter().map(|&(x, y)| Point::new(gx + x - y * SLANT, gy + y));
                    p.move_to(pts.next().unwrap());
                    for pt in pts {
                        p.line_to(pt);
                    }
                    p.close();
                })
            };
            let (gl, gr, gt, gb) = (-GLYPH_W / 2.0, GLYPH_W / 2.0, -GLYPH_H / 2.0, GLYPH_H / 2.0);

            // The V is two thick diagonal strokes meeting in a
            // point. `vt` is the stroke's horizontal cut where it
            // meets the top edge (perpendicular thickness GLYPH_T
            // over the cosine of the stroke's lean); the inner
            // edges run parallel to the outer ones and meet at
            // `apex_y`, leaving a small triangular counter.
            let vt = GLYPH_T * (GLYPH_W / 2.0).hypot(GLYPH_H) / GLYPH_H;
            let v = glyph(
                &[
                    (gl, gt),
                    (gl + vt, gt),
                    (0.0, gt + GLYPH_H * (gr - vt) / gr),
                    (gr - vt, gt),
                    (gr, gt),
                    (0.0, gb),
                ],
                cx - GLYPH_DX,
                cy - GLYPH_DY,
            );
            frame.fill(&v, color);

            // The S is the blocky three-bars-and-two-notches
            // digital form — top aperture opening right, bottom
            // aperture opening left, like the letter. Angular
            // rather than curved both because tracing a curved S
            // by hand is fiddly and because blocky suits the
            // splash style.
            let s = glyph(
                &[
                    (gr, gt),
                    (gr, gt + GLYPH_T),
                    (gl + GLYPH_T, gt + GLYPH_T),
                    (gl + GLYPH_T, -GLYPH_T / 2.0),
                    (gr, -GLYPH_T / 2.0),
                    (gr, gb),
                    (gl, gb),
                    (gl, gb - GLYPH_T),
                    (gr - GLYPH_T, gb - GLYPH_T),
                    (gr - GLYPH_T, GLYPH_T / 2.0),
                    (gl, GLYPH_T / 2.0),
                    (gl, gt),
                ],
                cx + GLYPH_DX,
                cy + GLYPH_DY,
            );
            frame.fill(&s, color);

            vec![frame.into_geometry()]
        }
    }

    Canvas::new(VsDiagonal).width(Length::Fill).height(Length::Fill).into()
}

/// Full-width inline banner for after-the-fact action failures
/// (singleplayer launch, PvP session build). Softer styling than a
/// hard-bordered chrome: a danger-tinted wash, rounded corners, an
/// AlertTriangle glyph, danger-colored body text, and a quiet × the
/// user can click to dismiss (`on_dismiss`). Callers also auto-clear
/// on the next Fight or Play retry, so the user isn't forced into
/// the × path.
pub fn error_banner<'a, M: Clone + 'a>(
    lang: &'a unic_langid::LanguageIdentifier,
    err: &'a str,
    on_dismiss: M,
) -> Element<'a, M> {
    container(
        row![
            Icon::AlertTriangle.widget(),
            text(err.to_string()).size(TEXT_BODY).style(danger_text_style),
            iced::widget::space::horizontal(),
            icon_button(Icon::X, crate::t!(lang, "save-action-cancel"), on_dismiss, [4.0, 8.0],),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([8, 16])
    .style(|theme: &Theme| {
        let p = theme.extended_palette();
        // Soft danger-tinted wash — readable against both light and
        // dark themes without the hard border that made the old
        // banner feel like an OS-level dialog.
        let alpha = if p.is_dark { 0.18 } else { 0.10 };
        container::Style {
            background: Some(iced::Background::Color(iced::Color {
                a: alpha,
                ..p.danger.base.color
            })),
            text_color: Some(theme.palette().text),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    })
    .into()
}

/// The battlefield seat colors: this side reads red, the opponent blue —
/// one pair everywhere a you-vs-opponent split is drawn (the PvP
/// telemetry header's seat dots, the HP graphs and their legends).
pub const FIELD_RED: iced::Color = iced::Color::from_rgb(0.85, 0.22, 0.28);
pub const FIELD_BLUE: iced::Color = iced::Color::from_rgb(0.18, 0.40, 0.85);

/// This side's HP-trace color (see [`FIELD_RED`]). Kept as a style fn so
/// legend chips and canvas draws share one signature.
pub fn hp_you_color(_theme: &Theme) -> iced::Color {
    FIELD_RED
}

/// The opponent's HP-trace color (see [`FIELD_BLUE`]).
pub fn hp_opponent_color(_theme: &Theme) -> iced::Color {
    FIELD_BLUE
}

/// The list-row / results-card mark for a round outcome.
pub fn outcome_mark(outcome: tango_pvp::stepper::BattleOutcome) -> (Icon, fn(&Theme) -> iced::widget::text::Style) {
    match outcome {
        tango_pvp::stepper::BattleOutcome::Win => (Icon::Check, success_text_style),
        tango_pvp::stepper::BattleOutcome::Loss => (Icon::X, danger_text_style),
        tango_pvp::stepper::BattleOutcome::Draw => (Icon::Minus, muted_text_style),
    }
}

/// One match's HP graph: every round on a single continuous timeline,
/// each round a segment whose width is proportional to its tick span,
/// separated by small gaps. Within a segment both navis' HP run as
/// step-lines (HP holds between hits — a diagonal would invent a ramp
/// that never happened) over an inset wash, a zero baseline, and a
/// slightly lighter band under each custom-screen span; a small dot in
/// the segment's top-right corner carries the round's outcome. `sweep`
/// (0..=1 of the whole timeline) reveals the chart left to right with a
/// head dot on each line while mid-sweep. Trace/custom x values are
/// 0..=1 within their round; HP values are normalized to the match-wide
/// maximum by the caller, so every segment shares one vertical scale.
pub struct HpGraphRound<'a> {
    pub trace: &'a [(f32, f32, f32)],
    pub custom: &'a [(f32, f32)],
    pub outcome: Option<tango_pvp::stepper::BattleOutcome>,
    /// Tick span of the round — its share of the timeline's width.
    pub weight: f32,
}

/// `max_hp` is the match-wide scale the traces were normalized against;
/// hovering the chart shows a crosshair with both navis' HP numbers read
/// back through it.
pub fn hp_match_graph<'a, M: 'a>(
    rounds: Vec<HpGraphRound<'a>>,
    max_hp: f32,
    sweep: f32,
    height: f32,
) -> Element<'a, M> {
    use iced::widget::canvas;

    struct HpMatchGraph<'a> {
        rounds: Vec<HpGraphRound<'a>>,
        max_hp: f32,
        sweep: f32,
        /// Whether the cursor was over the chart on the last mouse event —
        /// lets the leave-event redraw clear the crosshair.
        was_hovered: std::cell::Cell<bool>,
    }

    impl<M> canvas::Program<M> for HpMatchGraph<'_> {
        type State = ();

        fn update(
            &self,
            _state: &mut (),
            event: &iced::Event,
            bounds: iced::Rectangle,
            cursor: iced::mouse::Cursor,
        ) -> Option<canvas::Action<M>> {
            // The hover crosshair is drawn straight from the cursor in
            // `draw`, so cursor motion over (or off) the chart must trigger
            // a redraw — without this the readout only refreshes when
            // something else invalidates the view (e.g. a click).
            if !matches!(event, iced::Event::Mouse(iced::mouse::Event::CursorMoved { .. })) {
                return None;
            }
            let over = cursor.is_over(bounds);
            let was_over = self.was_hovered.replace(over);
            (over || was_over).then(canvas::Action::request_redraw)
        }

        fn draw(
            &self,
            _state: &(),
            renderer: &iced::Renderer,
            theme: &Theme,
            bounds: iced::Rectangle,
            cursor: iced::mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            use canvas::{Frame, LineCap, Path, Stroke};
            use iced::Point;

            let mut frame = Frame::new(renderer, bounds.size());
            let palette = theme.extended_palette();
            let text_color = theme.palette().text;
            let (w, h) = (bounds.width, bounds.height);
            // Inset vertically so full-HP traces keep their line width
            // on-canvas.
            const PAD: f32 = 3.0;
            const GAP: f32 = 3.0;
            let y_at = |yf: f32| PAD + (1.0 - yf.clamp(0.0, 1.0)) * (h - 2.0 * PAD);

            let total: f32 = self.rounds.iter().map(|r| r.weight.max(1.0)).sum::<f32>().max(1.0);
            let gaps = GAP * (self.rounds.len().saturating_sub(1)) as f32;
            let usable = (w - gaps).max(1.0);

            let mut segments: Vec<(f32, f32)> = Vec::with_capacity(self.rounds.len());
            let mut seg_x = 0.0f32;
            // The sweep runs over the whole timeline; convert to a px
            // cursor so segment boundaries don't distort its pace.
            let sweep_px = self.sweep.clamp(0.0, 1.0) * w;
            for round in &self.rounds {
                let seg_w = round.weight.max(1.0) / total * usable;
                segments.push((seg_x, seg_w));
                let x_at = |xf: f32| seg_x + xf.clamp(0.0, 1.0) * seg_w;
                // Local reveal fraction of this segment under the global
                // px cursor.
                let local_sweep = ((sweep_px - seg_x) / seg_w).clamp(0.0, 1.0);

                // Recessed background so each round reads as its own inset
                // panel; the gaps between them are the round dividers.
                let bg = Path::rounded_rectangle(Point::new(seg_x, 0.0), iced::Size::new(seg_w, h), 3.0.into());
                frame.fill(
                    &bg,
                    iced::Color {
                        a: if palette.is_dark { 0.10 } else { 0.05 },
                        ..text_color
                    },
                );

                // Custom-screen bands: the stretches where the battle stood
                // paused while chips were picked.
                for &(x0, x1) in round.custom {
                    let (bx0, bx1) = (x_at(x0.min(local_sweep)), x_at(x1.min(local_sweep)));
                    if bx1 > bx0 {
                        frame.fill_rectangle(
                            Point::new(bx0, 0.0),
                            iced::Size::new(bx1 - bx0, h),
                            iced::Color { a: 0.07, ..text_color },
                        );
                    }
                }

                // Zero baseline — where a KO'd navi's trace lands.
                let base_y = y_at(0.0);
                frame.stroke(
                    &Path::line(Point::new(seg_x, base_y), Point::new(seg_x + seg_w, base_y)),
                    Stroke::default()
                        .with_color(iced::Color { a: 0.22, ..text_color })
                        .with_width(1.0),
                );

                if round.trace.len() >= 2 && local_sweep > 0.0 {
                    // Draw the opponent under this side, so "you" stays
                    // legible where the traces overlap (equal HP at round
                    // start).
                    for you in [false, true] {
                        let color = if you {
                            hp_you_color(theme)
                        } else {
                            hp_opponent_color(theme)
                        };
                        let value = |p: &(f32, f32, f32)| if you { p.1 } else { p.2 };
                        let mut head = None;
                        let path = Path::new(|b| {
                            let mut prev_y = y_at(value(&round.trace[0]));
                            b.move_to(Point::new(x_at(round.trace[0].0), prev_y));
                            for point in &round.trace[1..] {
                                let x = x_at(point.0.min(local_sweep));
                                // Step-line: run flat to the new x, then
                                // drop/rise there.
                                b.line_to(Point::new(x, prev_y));
                                if point.0 > local_sweep {
                                    head = Some(Point::new(x, prev_y));
                                    break;
                                }
                                prev_y = y_at(value(point));
                                b.line_to(Point::new(x, prev_y));
                                head = Some(Point::new(x, prev_y));
                            }
                        });
                        frame.stroke(
                            &path,
                            Stroke::default()
                                .with_color(color)
                                .with_width(1.5)
                                .with_line_cap(LineCap::Round),
                        );
                        // Sweep-head dot: the "now" cursor of the miniature
                        // replay.
                        if local_sweep < 1.0 && local_sweep > 0.0 {
                            if let Some(head) = head {
                                frame.fill(&Path::circle(head, 2.0), color);
                            }
                        }
                    }
                }

                // Outcome dot, top-right of the segment, once the sweep has
                // fully crossed it.
                if local_sweep >= 1.0 {
                    if let Some(outcome) = round.outcome {
                        let color = match outcome {
                            tango_pvp::stepper::BattleOutcome::Win => palette.success.strong.color,
                            tango_pvp::stepper::BattleOutcome::Loss => palette.danger.strong.color,
                            tango_pvp::stepper::BattleOutcome::Draw => muted_color(theme),
                        };
                        frame.fill(&Path::circle(Point::new(seg_x + seg_w - 6.0, 6.0), 2.5), color);
                    }
                }

                seg_x += seg_w + GAP;
            }

            // Hover readout: a crosshair over the hovered segment with the
            // step values under the cursor, read back through the shared
            // scale — dots carry which number is whose, ink stays neutral.
            if let Some(pos) = cursor.position_in(bounds) {
                let hovered = segments
                    .iter()
                    .zip(&self.rounds)
                    .find(|((sx, sw), _)| pos.x >= *sx && pos.x < sx + sw && pos.x <= sweep_px);
                if let Some((&(sx, sw), round)) = hovered {
                    let xf = ((pos.x - sx) / sw).clamp(0.0, 1.0);
                    // Step semantics: the value in force at xf is the last
                    // point at or before it.
                    let at = round
                        .trace
                        .iter()
                        .take_while(|p| p.0 <= xf)
                        .last()
                        .or(round.trace.first());
                    if let Some(&(_, you, opp)) = at {
                        frame.stroke(
                            &Path::line(Point::new(pos.x, 0.0), Point::new(pos.x, h)),
                            Stroke::default()
                                .with_color(iced::Color { a: 0.35, ..text_color })
                                .with_width(1.0),
                        );
                        for (yf, color) in [(opp, hp_opponent_color(theme)), (you, hp_you_color(theme))] {
                            frame.fill(&Path::circle(Point::new(pos.x, y_at(yf)), 2.5), color);
                        }

                        let you_hp = (you * self.max_hp).round() as u32;
                        let opp_hp = (opp * self.max_hp).round() as u32;
                        let label = |hp: u32| hp.to_string();
                        let widest = label(you_hp).len().max(label(opp_hp).len()) as f32;
                        let box_w = 16.0 + widest * 7.0;
                        let box_h = 30.0;
                        // Flip to the cursor's left near the right edge so
                        // the readout stays on-canvas.
                        let bx = if pos.x + 10.0 + box_w > w {
                            pos.x - 10.0 - box_w
                        } else {
                            pos.x + 10.0
                        };
                        let by = (pos.y - box_h / 2.0).clamp(0.0, (h - box_h).max(0.0));
                        frame.fill(
                            &Path::rounded_rectangle(Point::new(bx, by), iced::Size::new(box_w, box_h), 4.0.into()),
                            iced::Color {
                                a: 0.92,
                                ..theme.palette().background
                            },
                        );
                        for (i, (hp, color)) in [(you_hp, hp_you_color(theme)), (opp_hp, hp_opponent_color(theme))]
                            .into_iter()
                            .enumerate()
                        {
                            let line_y = by + 8.0 + i as f32 * 14.0;
                            frame.fill(&Path::circle(Point::new(bx + 7.0, line_y), 2.5), color);
                            frame.fill_text(canvas::Text {
                                content: label(hp),
                                position: Point::new(bx + 13.0, line_y),
                                color: text_color,
                                size: 11.0.into(),
                                align_y: iced::alignment::Vertical::Center.into(),
                                ..Default::default()
                            });
                        }
                    }
                }
            }

            vec![frame.into_geometry()]
        }
    }

    iced::widget::canvas::Canvas::new(HpMatchGraph {
        rounds,
        max_hp,
        sweep,
        was_hovered: std::cell::Cell::new(false),
    })
    .width(Length::Fill)
    .height(Length::Fixed(height))
    .into()
}
