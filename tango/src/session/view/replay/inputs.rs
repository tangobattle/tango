//! The replay input display: each side's recorded pad state at the
//! playhead, drawn as a console face.

use super::*;
// Explicit so these win over iced's prelude macros; see the view module.
use sweeten::widget::{column, row};

/// Width of one input-display pad: the D-pad cross (three 24px cells
/// + 2px seams) plus the B/A cluster, spread to the edges by the
/// shoulders' `horizontal_space`.
const PAD_W: f32 = 160.0;

/// One side's recorded pad state, drawn as the settings input pane's
/// console face ([`crate::tabs::settings`]) at ~0.7 scale, minus the
/// screen: chevron D-pad cross left with the Start/Select pills below
/// it, B/A round keys on the console's diagonal right, L/R shoulder
/// pills capping the top corners. Non-interactive twin of that pane's
/// `key_btn`/`gba_key`: every key is always drawn on the shared
/// molded plate, and a pressed key mixes toward palette primary —
/// the same lit chrome as the settings' live binding test — so the
/// chip never changes size or layout as inputs flip.
fn input_pad<'a>(joyflags: u16, ds: bool) -> Element<'a, Message> {
    use tango_session::keys;
    let cell = 24.0;
    let key = move |content: Element<'a, Message>, bit: u32, w: f32, h: f32, radius: iced::border::Radius| {
        let lit = joyflags as u32 & bit != 0;
        container(container(content).center(Fill))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .style(move |theme: &iced::Theme| {
                let plate = widgets::gba_key_plate(theme);
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(if lit {
                        widgets::mix(plate, theme.palette().primary, 0.55)
                    } else {
                        plate
                    })),
                    text_color: Some(theme.palette().text),
                    border: iced::Border {
                        radius,
                        width: 1.0,
                        color: theme.extended_palette().background.strong.color,
                    },
                    ..Default::default()
                }
            })
    };

    let arm = |icon: Icon, bit: u32, corners: [f32; 4]| {
        key(
            icon.widget().size(11.0).into(),
            bit,
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
    let corner = || iced::widget::Space::new().width(cell).height(cell);
    // Inert hub: `bit` 0 is never held, which is exactly the
    // settings hub's always-plate look.
    let hub = key(iced::widget::Space::new().into(), 0, cell, cell, 3.0.into());
    let (ro, ri) = (7.0, 3.0);
    let dpad = column![
        row![corner(), arm(Icon::ChevronUp, keys::UP, [ro, ro, ri, ri]), corner()].spacing(2),
        row![
            arm(Icon::ChevronLeft, keys::LEFT, [ro, ri, ri, ro]),
            hub,
            arm(Icon::ChevronRight, keys::RIGHT, [ri, ro, ro, ri]),
        ]
        .spacing(2),
        row![corner(), arm(Icon::ChevronDown, keys::DOWN, [ri, ri, ro, ro]), corner()].spacing(2),
    ]
    .spacing(2);

    let pill = |label: &'static str, bit: u32| key(text(label).size(8.0).into(), bit, 44.0, 14.0, 999.0.into());
    // Start/Select below the face cluster, plainly stacked — the DS
    // face arrangement, same as the settings shell draws.
    let start_select = column![pill("START", keys::START), pill("SELECT", keys::SELECT)].spacing(4);

    let ab_d = 32.0;
    let face_key =
        |label: &'static str, bit: u32| key(text(label).size(TEXT_BODY).into(), bit, ab_d, ab_d, 999.0.into());
    // The face cluster shows the recorded console's keys: a DS pad
    // gets the full diamond (the settings shell's, at chip scale), a
    // GBA pad keeps its two-key diagonal.
    let cluster: Element<'a, Message> = if ds {
        use iced::alignment::{Horizontal as Ax, Vertical as Ay};
        let diamond_box = 90.0;
        let place = |el, ax, ay| {
            container(el)
                .width(Length::Fixed(diamond_box))
                .height(Length::Fixed(diamond_box))
                .align_x(ax)
                .align_y(ay)
        };
        iced::widget::stack![
            place(face_key("X", keys::X), Ax::Center, Ay::Top),
            place(face_key("Y", keys::Y), Ax::Left, Ay::Center),
            place(face_key("A", keys::A), Ax::Right, Ay::Center),
            place(face_key("B", keys::B), Ax::Center, Ay::Bottom),
        ]
        .into()
    } else {
        row![
            column![iced::widget::Space::new().height(14.0), face_key("B", keys::B)],
            column![face_key("A", keys::A), iced::widget::Space::new().height(14.0)],
        ]
        .spacing(6)
        .into()
    };
    let right_col = column![cluster, start_select].spacing(10).align_x(Alignment::Center);

    let shoulder = |label: &'static str, bit: u32| key(text(label).size(9.0).into(), bit, 56.0, 15.0, 999.0.into());
    // A DS recording carries the mic, so the chip shows when the
    // recorder was blowing into it, on the hinge between the shoulders
    // where the console's own hole is. It is drawn whether or not it
    // was held, like every other key here.
    let shoulders = if ds {
        row![
            shoulder("L", keys::L),
            horizontal_space(),
            key(text("BLOW").size(9.0).into(), keys::MIC, 44.0, 15.0, 999.0.into()),
            horizontal_space(),
            shoulder("R", keys::R),
        ]
    } else {
        row![shoulder("L", keys::L), horizontal_space(), shoulder("R", keys::R)]
    };
    let face = row![dpad, horizontal_space(), right_col].align_y(Alignment::Center);
    // The diamond needs more shell than the GBA diagonal.
    let pad_w = if ds { 176.0 } else { PAD_W };
    column![shoulders, face].spacing(8).width(Length::Fixed(pad_w)).into()
}

/// Replay-only: the input display overlay — one pad chip per side,
/// the recorder bottom-left and their opponent bottom-right (matching
/// the battle screen, which renders the recording side's navi on the
/// left), each captioned with the side's nickname and lit with the
/// recorded buttons at the playhead. Sampled through [`playhead_tick`]
/// so scrubbing previews inputs along with the readout. Anchored at
/// the transport bar's popover lift so it never moves — the bar
/// auto-hides beneath it, the chips stay. Pure presentation: no mouse
/// handlers anywhere in the chain.
pub(super) fn input_display_overlay<'a>(
    r: &'a ReplaySession,
    state: &'a State,
    show_replay_inputs: bool,
) -> Option<Element<'a, Message>> {
    if !show_replay_inputs {
        return None;
    }
    let (mut local, mut remote) = r.input_at(playhead_tick(r, state));
    let (mut local_nick, mut remote_nick) = r.nicknames();
    // While the perspective is swapped, the main screen is the opponent's
    // — the pads follow it, so the left chip always belongs to whoever is
    // on the big screen.
    if r.swap_perspective() {
        std::mem::swap(&mut local, &mut remote);
        std::mem::swap(&mut local_nick, &mut remote_nick);
    }
    // X and Y mean a DS, and the pads draw the full face diamond for
    // one. Asked of the console rather than counted off the screens:
    // a session composes only the screens its mode uses, so a DS can
    // present one.
    let ds = {
        use tango_session::keys;
        r.local_game().pvp.keys_mask() & (keys::X | keys::Y) != 0
    };
    let chip = |joyflags: u16, nick: &str| -> Element<'a, Message> {
        // The caption renders even when the nickname is empty so the
        // two chips always match heights.
        let name = text(nick.to_string())
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style);
        container(
            column![input_pad(joyflags, ds), name]
                .spacing(4)
                .align_x(Alignment::Center),
        )
        .padding([8, 10])
        .style(hud_chip_plate)
        .into()
    };
    Some(
        container(row![
            chip(local, local_nick),
            horizontal_space(),
            chip(remote, remote_nick)
        ])
        .width(Fill)
        .height(Fill)
        .align_y(iced::alignment::Vertical::Bottom)
        .padding(iced::Padding {
            top: 0.0,
            right: 12.0,
            // Rides up with the clip strip so the expanded bar's
            // taller plate never slides underneath the pads.
            bottom: POPOVER_LIFT + clip_lift(state),
            left: 12.0,
        })
        .into(),
    )
}
