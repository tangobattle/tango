//! Input pane: key bindings on a drawn GBA whose keys pick the binding to edit.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

/// Fixed width of the drawn console shell. Sized to fit the
/// in-session settings modal's body (~620px wide); in the full
/// settings page it centers in the pane.
const GBA_SHELL_WIDTH: f32 = 560.0;

pub(super) fn settings_input<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    state: &'a State,
    held: &input::HeldState,
) -> Element<'a, Message> {
    let mapping = &config.input_mapping;
    // A console key lights up while any of its bindings is physically
    // held — the same test-your-bindings affordance the old chip table
    // had, moved onto the drawn button itself.
    let lit = |k: input::MappedKey| mapping.slot(k).iter().any(|b| held.is_active(b));
    // One clickable console key: fixed box, centered face label,
    // selected/lit chrome. Clicking brings the key up on the screen.
    let key_btn = |content: Element<'a, Message>, k: input::MappedKey, w: f32, h: f32, radius: iced::border::Radius| {
        button(container(content).center(Fill))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .padding(0)
            .style(gba_key(state.selected_key == Some(k), lit(k), radius))
            .on_press(Message::BindingSlotSelected(k))
    };

    // D-pad: four arms around an inert hub. Outer corners take the
    // big radius so the cross reads as one molded pad; the 2px seams
    // keep each arm clickable as its own key.
    let cell = 34.0;
    let arm = |icon: Icon, k: input::MappedKey, corners: [f32; 4]| {
        key_btn(
            icon.widget().size(16.0).into(),
            k,
            cell,
            cell,
            iced::border::Radius {
                top_left: corners[0],
                top_right: corners[1],
                bottom_right: corners[2],
                bottom_left: corners[3],
            },
        )
    };
    let corner = || Space::new().width(cell).height(cell);
    let hub = container(Space::new())
        .width(Length::Fixed(cell))
        .height(Length::Fixed(cell))
        .style(|theme: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(widgets::gba_key_plate(theme))),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: theme.extended_palette().background.strong.color,
            },
            ..Default::default()
        });
    let (ro, ri) = (10.0, 4.0);
    let dpad = column![
        row![
            corner(),
            arm(Icon::ChevronUp, input::MappedKey::Up, [ro, ro, ri, ri]),
            corner()
        ]
        .spacing(2),
        row![
            arm(Icon::ChevronLeft, input::MappedKey::Left, [ro, ri, ri, ro]),
            hub,
            arm(Icon::ChevronRight, input::MappedKey::Right, [ri, ro, ro, ri]),
        ]
        .spacing(2),
        row![
            corner(),
            arm(Icon::ChevronDown, input::MappedKey::Down, [ri, ri, ro, ro]),
            corner()
        ]
        .spacing(2),
    ]
    .spacing(2);

    let left_col = column![dpad];

    // Face buttons: the DS diamond — X top, Y left, A right, B bottom —
    // which keeps the GBA's pair where it always was (A on the right, B
    // low) and adds the DS-only two in the spots the console puts them.
    use iced::alignment::{Horizontal as Ax, Vertical as Ay};
    let ab_d = 40.0;
    let face_key = |label: &'static str, k: input::MappedKey| {
        key_btn(
            text(label).size(style::TEXT_HEADING).into(),
            k,
            ab_d,
            ab_d,
            999.0.into(),
        )
    };
    // A square housing with one key pinned to the midpoint of each
    // edge. Stacked full-size layers rather than rows so X and B tuck
    // into the vertical space beside Y and A — adjacent keys sit
    // ~51px apart center-to-center on the diagonal (an ~11px gap)
    // instead of three full button-rows of height.
    let diamond_box = 112.0;
    let place = |el, ax, ay| {
        container(el)
            .width(Length::Fixed(diamond_box))
            .height(Length::Fixed(diamond_box))
            .align_x(ax)
            .align_y(ay)
    };
    let diamond = iced::widget::stack![
        place(face_key("X", input::MappedKey::X), Ax::Center, Ay::Top),
        place(face_key("Y", input::MappedKey::Y), Ax::Left, Ay::Center),
        place(face_key("A", input::MappedKey::A), Ax::Right, Ay::Center),
        place(face_key("B", input::MappedKey::B), Ax::Center, Ay::Bottom),
    ];
    // Start/Select: small pills below the face diamond, plainly
    // stacked — the DS face layout. Face labels stay literal like the
    // silkscreen (localized names appear on the bezel caption when
    // selected).
    let pill = |label: &'static str, k: input::MappedKey| {
        key_btn(text(label).size(TEXT_CAPTION).into(), k, 64.0, 20.0, 999.0.into())
    };
    let start_select = column![
        pill("START", input::MappedKey::Start),
        pill("SELECT", input::MappedKey::Select),
    ]
    .spacing(5);
    let right_col = column![diamond, start_select]
        .spacing(18)
        .align_x(iced::Alignment::Center);

    // The screen shows the selected key's bindings — chips + the
    // add/capture flow the old table rows carried — or a hint when
    // nothing is selected yet.
    let screen_body: Element<'a, Message> = if let Some(k) = state.selected_key {
        let mut chips = row![].spacing(6).align_y(iced::Alignment::Center);
        for (i, b) in mapping.slot(k).iter().enumerate() {
            chips = chips.push(binding_chip(lang, b, k, i, held.is_active(b)));
        }
        // Capture mode swaps the Add button for "press a key…" + a
        // cancel chip, same as the old per-row treatment.
        let action: Element<'a, Message> = if state.capture_target == Some(k) {
            row![
                text(t!(lang, "settings-input-press-key"))
                    .size(TEXT_BODY)
                    .style(widgets::primary_text_style),
                widgets::icon_button(
                    Icon::X,
                    t!(lang, "save-action-cancel"),
                    Message::BindingCaptureCancel,
                    STANDARD_PADDING,
                ),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
            .into()
        } else {
            widgets::icon_button(
                Icon::Plus,
                t!(lang, "settings-input-add"),
                Message::BindingCaptureStart(k),
                STANDARD_PADDING,
            )
        };
        container(
            column![chips.wrap(), action]
                .spacing(10)
                .align_x(iced::Alignment::Center),
        )
        .center(Fill)
        .into()
    } else {
        container(
            text(t!(lang, "settings-input-select-hint"))
                .size(TEXT_BODY)
                .style(widgets::muted_text_style)
                .align_x(iced::Alignment::Center),
        )
        .center(Fill)
        .into()
    };
    let screen = container(screen_body)
        .width(Length::Fixed(252.0))
        .height(Length::Fixed(168.0))
        .padding(8)
        .style(gba_screen);
    // Silkscreen line under the glass names the selected key in the
    // UI language. Fixed-height slot so selecting never reflows the
    // bezel.
    let caption = container(
        text(
            state
                .selected_key
                .map(|k| mapped_key_label(lang, k))
                .unwrap_or_default(),
        )
        .size(TEXT_CAPTION)
        .style(|_: &iced::Theme| iced::widget::text::Style {
            color: Some(iced::Color::from_rgba(0.82, 0.82, 0.86, 0.9)),
        }),
    )
    .height(Length::Fixed(16.0));
    let bezel = container(column![screen, caption].spacing(4).align_x(iced::Alignment::Center))
        .padding(iced::Padding {
            top: 12.0,
            right: 14.0,
            bottom: 4.0,
            left: 14.0,
        })
        .style(gba_bezel);

    let shoulders = row![
        key_btn(
            text("L").size(TEXT_BODY).into(),
            input::MappedKey::L,
            84.0,
            22.0,
            999.0.into()
        ),
        horizontal_space(),
        key_btn(
            text("R").size(TEXT_BODY).into(),
            input::MappedKey::R,
            84.0,
            22.0,
            999.0.into()
        ),
    ];
    let face =
        row![left_col, horizontal_space(), bezel, horizontal_space(), right_col].align_y(iced::Alignment::Center);
    let shell = container(column![shoulders, face].spacing(10))
        .width(Length::Fixed(GBA_SHELL_WIDTH))
        .padding(iced::Padding {
            top: 10.0,
            right: 18.0,
            bottom: 24.0,
            left: 18.0,
        })
        .style(gba_shell);

    // Neither fast-forward nor the DS's mic is a key on the shell: one
    // is the host's own knob and the other is a hole in the hinge. Both
    // sit under the shell as pills sharing the key chrome, with Reset on
    // the opposite edge.
    let wide_pill = |icon: Icon, label: String, k: input::MappedKey| {
        button(
            row![icon.widget().size(TEXT_BODY), text(label).size(TEXT_BODY)]
                .spacing(6)
                .align_y(iced::Alignment::Center),
        )
        .padding([5.0, 12.0])
        .style(gba_key(state.selected_key == Some(k), lit(k), 999.0.into()))
        .on_press(Message::BindingSlotSelected(k))
    };
    let speed = wide_pill(
        Icon::FastForward,
        t!(lang, "input-key-speed-up"),
        input::MappedKey::SpeedUp,
    );
    let mic = wide_pill(Icon::Wind, t!(lang, "input-key-mic"), input::MappedKey::Mic);
    let reset = widgets::labeled_icon_button(
        Icon::RefreshCw,
        t!(lang, "settings-input-reset"),
        Message::BindingsReset,
        STANDARD_PADDING,
        widgets::neutral,
    );
    let below = row![speed, mic, horizontal_space(), reset]
        .spacing(8)
        .width(Length::Fixed(GBA_SHELL_WIDTH))
        .align_y(iced::Alignment::Center);

    container(column![shell, below].spacing(14))
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Center)
        .padding(style::PANE_PADDING)
        .into()
}

/// Localized display name for a mapped key — shown on the bezel
/// caption under the screen.
fn mapped_key_label(lang: &LanguageIdentifier, k: input::MappedKey) -> String {
    match k {
        input::MappedKey::Up => t!(lang, "input-key-up"),
        input::MappedKey::Down => t!(lang, "input-key-down"),
        input::MappedKey::Left => t!(lang, "input-key-left"),
        input::MappedKey::Right => t!(lang, "input-key-right"),
        input::MappedKey::A => t!(lang, "input-key-a"),
        input::MappedKey::B => t!(lang, "input-key-b"),
        input::MappedKey::X => t!(lang, "input-key-x"),
        input::MappedKey::Y => t!(lang, "input-key-y"),
        input::MappedKey::L => t!(lang, "input-key-l"),
        input::MappedKey::R => t!(lang, "input-key-r"),
        input::MappedKey::Start => t!(lang, "input-key-start"),
        input::MappedKey::Select => t!(lang, "input-key-select"),
        input::MappedKey::Mic => t!(lang, "input-key-mic"),
        input::MappedKey::SpeedUp => t!(lang, "input-key-speed-up"),
    }
}

/// Chrome for one console key. `selected` = the screen is showing
/// this key (primary ring); `lit` = a bound physical input is held
/// right now (primary flush, the live binding test). Both are
/// color-only so live input never shifts the layout.
fn gba_key(
    selected: bool,
    lit: bool,
    radius: iced::border::Radius,
) -> impl Fn(&iced::Theme, button::Status) -> button::Style {
    move |theme: &iced::Theme, status: button::Status| {
        let p = theme.extended_palette();
        let primary = theme.palette().primary;
        let plate = widgets::gba_key_plate(theme);
        let base = match status {
            button::Status::Hovered => widgets::mix(plate, iced::Color::WHITE, if p.is_dark { 0.12 } else { 0.2 }),
            button::Status::Pressed => widgets::mix(plate, iced::Color::BLACK, 0.10),
            _ => plate,
        };
        let background = if lit { widgets::mix(base, primary, 0.55) } else { base };
        button::Style {
            background: Some(iced::Background::Color(background)),
            text_color: theme.palette().text,
            border: iced::Border {
                radius,
                width: if selected { 2.0 } else { 1.0 },
                color: if selected {
                    primary
                } else if matches!(status, button::Status::Hovered) {
                    iced::Color { a: 0.7, ..primary }
                } else {
                    p.background.strong.color
                },
            },
            shadow: iced::Shadow {
                color: iced::Color {
                    a: if p.is_dark { 0.35 } else { 0.12 },
                    ..iced::Color::BLACK
                },
                offset: iced::Vector::new(
                    0.0,
                    if matches!(status, button::Status::Pressed) {
                        1.0
                    } else {
                        2.0
                    },
                ),
                blur_radius: 6.0,
            },
            snap: false,
        }
    }
}

/// The console shell — one lifted plate with the GBA's silhouette
/// (bottom grip corners fuller than the top). The accent stays on
/// the keys; the shell frames quietly.
fn gba_shell(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    iced::widget::container::Style {
        background: Some(iced::Background::Color(widgets::plate_color(theme))),
        text_color: Some(theme.palette().text),
        border: iced::Border {
            radius: iced::border::Radius {
                top_left: 24.0,
                top_right: 24.0,
                bottom_right: 34.0,
                bottom_left: 34.0,
            },
            width: 1.5,
            color: p.background.strong.color,
        },
        ..Default::default()
    }
}

/// The glass bezel around the screen — near-black in both themes,
/// like the real console's glass regardless of shell color.
fn gba_bezel(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let glass = if p.is_dark {
        widgets::mix(theme.palette().background, iced::Color::BLACK, 0.55)
    } else {
        iced::Color::from_rgb(0.15, 0.15, 0.18)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(glass)),
        border: iced::Border {
            radius: 14.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.5,
                ..iced::Color::BLACK
            },
        },
        ..Default::default()
    }
}

/// The LCD inside the bezel. Follows the theme (pale lit panel on
/// light, near-black on dark) so the binding chips and hint text
/// drawn "on screen" keep their normal contrast.
fn gba_screen(theme: &iced::Theme) -> iced::widget::container::Style {
    let p = theme.extended_palette();
    let bg = theme.palette().background;
    let lcd = if p.is_dark {
        widgets::mix(bg, iced::Color::BLACK, 0.3)
    } else {
        widgets::mix(bg, iced::Color::WHITE, 0.25)
    };
    iced::widget::container::Style {
        background: Some(iced::Background::Color(lcd)),
        text_color: Some(theme.palette().text),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color {
                a: 0.35,
                ..iced::Color::BLACK
            },
        },
        ..Default::default()
    }
}

fn binding_chip<'a>(
    lang: &'a LanguageIdentifier,
    binding: &input::PhysicalInput,
    key: input::MappedKey,
    idx: usize,
    lit: bool,
) -> Element<'a, Message> {
    let (kind, label) = input::describe(lang, binding);
    let kind_glyph = match kind {
        input::DescribeKind::Keyboard => Icon::Keyboard,
        input::DescribeKind::Gamepad => Icon::Gamepad2,
    };
    // Primary-tinted rounded pill matching the rest of the
    // app's chip + badge chrome. × button is borderless (flat
    // chrome) so it doesn't shout against the small label.
    container(
        row![
            kind_glyph.widget().size(TEXT_BODY),
            text(label).size(TEXT_BODY),
            button(Icon::X.widget().size(TEXT_CAPTION))
                .padding([2, 4])
                .style(widgets::flat)
                .on_press(Message::BindingRemove(key, idx)),
        ]
        .spacing(6)
        .align_y(iced::Alignment::Center),
    )
    .padding([3, 8])
    .style(move |theme: &iced::Theme| {
        let primary = theme.palette().primary;
        // `lit` = the bound key/button is physically held right now —
        // the chip brightens so the user can test their bindings from
        // this screen. Colors only; the geometry never moves.
        let (bg_a, border_a) = if lit { (0.45, 1.0) } else { (0.12, 0.45) };
        iced::widget::container::Style {
            background: Some(iced::Background::Color(iced::Color { a: bg_a, ..primary })),
            text_color: Some(theme.palette().text),
            border: iced::Border {
                radius: 999.0.into(),
                width: 1.0,
                color: iced::Color { a: border_a, ..primary },
            },
            ..Default::default()
        }
    })
    .into()
}
