//! The app's own widgets, over the shared toolkit in
//! [`tango_ui::widgets`] — re-exported here, so `crate::ui::widgets::*`
//! covers both without call sites caring which side of the gamesupport
//! boundary a widget lives on. The HUD chrome (`hud_bar`,
//! `hud_scanline_top`, `cyber_backdrop`, `panel`), the nav tabs, the
//! ⋮ [`MenuButton`], and the match-analysis chart are all app-only.

pub use tango_ui::widgets::*;

use crate::ui::style::{PANE_GAP, TEXT_BODY, TEXT_CAPTION};
use iced::widget::{button, container, text, tooltip};
use iced::{Alignment, Element, Length, Theme};
use lucide_icons::Icon;
use sweeten::widget::{column, row};

mod match_graph;
pub use match_graph::*;

/// The stats-to-chart cooking [`hp_match_graph`] draws from.
pub use super::matchup::*;

mod menu_button;
pub use menu_button::{MenuButton, MenuItem};

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
        crate::ui::style::STANDARD_PADDING,
        neutral,
    );
    // Tooltip above, not below — below is where the dropdown lands,
    // and the bubble lingers while the cursor rests on the trigger.
    tooltip(btn, tooltip_bubble(label), tooltip::Position::Top)
        .gap(4)
        .into()
}

/// The replay renderer's shared quality/scale picker. Full replay
/// exports and marked clips both edit the same setting, so they use
/// this one control too: `0` is lossless at native resolution and
/// `1..=10` is a lossy integer upscale.
pub fn replay_export_scale_picker<'a, M: Clone + 'a>(
    lang: &'a unic_langid::LanguageIdentifier,
    scale: u8,
    on_select: impl Fn(u8) -> M,
    on_toggle: Option<fn(bool) -> M>,
) -> Element<'a, M> {
    let scale = scale.min(10);
    let value_label = |scale: u8| {
        if scale == 0 {
            crate::i18n::t!(lang, "replays-export-scale-lossless").to_string()
        } else {
            format!("{scale}×")
        }
    };
    let current_label = value_label(scale);
    let items = (0..=10)
        .map(|candidate| MenuItem::toggle(value_label(candidate), on_select(candidate), candidate == scale))
        .collect();
    let mut picker = MenuButton::new(
        row![
            Icon::Scaling.widget().size(14.0),
            text(format!(
                "{}: {}",
                crate::i18n::t!(lang, "replays-export-scale"),
                current_label
            ))
            .size(TEXT_CAPTION),
            Icon::ChevronDown.widget().size(12.0),
        ]
        .spacing(6)
        .align_y(Alignment::Center),
        items,
        true,
        [4.0, 8.0],
        crate::ui::style::STANDARD_PADDING,
        neutral,
    )
    .menu_width(144.0);
    if let Some(on_toggle) = on_toggle {
        picker = picker.on_toggle(on_toggle);
    }
    tooltip(
        picker,
        tooltip_bubble(format!(
            "{}: {}",
            crate::i18n::t!(lang, "replays-export-scale"),
            current_label
        )),
        tooltip::Position::Top,
    )
    .gap(4)
    .into()
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

/// Larger pill for the global top nav (Play / Replays).
/// TEXT_HEADING-sized icon + label so the chrome reads as the
/// primary navigation for the whole app.
pub fn nav_tab_button<'a, M: Clone + 'a>(icon: Icon, label: String, msg: M, active: bool) -> Element<'a, M> {
    pill_tab(icon, Some(label), msg, active, true)
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
    let pill = pill_tab(icon, Some(label), msg, active, true);
    if !badge {
        return pill;
    }
    pill_tab_badge(pill, |theme| theme.palette().primary)
}

/// Icon-only variant of [`nav_tab_button`] for the right-aligned
/// utility tabs (Patches, Settings).
pub fn nav_icon_tab_button<'a, M: Clone + 'a>(
    icon: Icon,
    tooltip_label: String,
    msg: M,
    active: bool,
) -> Element<'a, M> {
    let stacked = pill_tab(icon, None, msg, active, true);
    tooltip(stacked, tooltip_bubble(tooltip_label), tooltip::Position::Bottom)
        .gap(4)
        .into()
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
    hud_scanline(crate::ui::theme::is_gay_time().then(|| flag_background(&crate::ui::theme::rainbow_flag_stops())))
}

/// The bottom-edge accent strip.
pub fn hud_scanline_bottom<'a, M: 'a>() -> Element<'a, M> {
    hud_scanline(crate::ui::theme::is_gay_time().then(|| flag_background(&crate::ui::theme::trans_flag_stops())))
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
            .style(crate::ui::anim::backdrop_style(backdrop_alpha)),
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

/// The standard dropdown: sweeten's `pick_list` with the
/// [`chunky_pick_list`] chrome and [`STANDARD_PADDING`] applied.
/// Callers chain extras (`.placeholder`, `.width`, `.disabled`) on
/// the returned picker; compact in-pane variants (CONTROL_PADDING +
/// smaller text) keep hand-building.
///
/// [`STANDARD_PADDING`]: crate::ui::style::STANDARD_PADDING
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
        .padding(crate::ui::style::STANDARD_PADDING)
        .style(chunky_pick_list)
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
        .padding(crate::ui::style::STANDARD_PADDING)
        .style(disabled_pick_list_style)
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

/// One patch download, as the same row wherever it shows up: the
/// patches tab, the play strip, the lobby band and the replay detail
/// all fetch the same packages, and used to each say so differently.
///
/// Deliberately small — a short bar and a caption on one line, not a
/// full-width meter. Running draws a determinate bar (flat until the
/// server tells us the size); failed drops the bar for the caption in
/// danger colour. `retry` and `cancel` are the surface's own messages;
/// pass `None` to leave the affordance out. Captions arrive
/// pre-resolved: this decides layout, not wording.
pub fn download_row<'a, M: Clone + 'a>(
    caption: String,
    fraction: Option<f32>,
    failed: bool,
    retry: Option<(String, M)>,
    cancel: Option<(String, M)>,
) -> Element<'a, M> {
    let mut controls = row![].spacing(6).align_y(Alignment::Center);
    if !failed {
        controls = controls.push(
            iced::widget::progress_bar(0.0..=1.0, fraction.unwrap_or(0.0))
                .girth(Length::Fixed(3.0))
                .length(Length::Fixed(56.0))
                .style(slim_progress_bar),
        );
    }
    let style: fn(&Theme) -> iced::widget::text::Style = if failed { danger_text_style } else { muted_text_style };
    controls = controls.push(text(caption).size(TEXT_CAPTION).style(style));
    for (icon, action) in [(Icon::RefreshCw, retry), (Icon::X, cancel)] {
        if let Some((label, msg)) = action {
            controls = controls.push(icon_button(icon, label, msg, [1.0, 1.0]));
        }
    }
    controls.into()
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
                    .padding(crate::ui::style::PANE_PADDING)
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

/// The battlefield seat colors: this side reads red, the opponent blue —
/// one pair everywhere a you-vs-opponent split is drawn (the PvP
/// telemetry header's seat dots, the HP graphs and their legends).
pub const FIELD_RED: iced::Color = iced::Color::from_rgb(0.85, 0.22, 0.28);
pub const FIELD_BLUE: iced::Color = iced::Color::from_rgb(0.18, 0.40, 0.85);
