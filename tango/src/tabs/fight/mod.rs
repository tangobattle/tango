//! The Fight tab: everything netplay. Idle, it's the link-code strip
//! + Fight CTA (the body above them is reserved for the future lobby
//! browser); once a connection attempt is in flight, the body becomes
//! the full lobby — versus cards, compat verdict, match settings,
//! Ready CTA. The lobby body lives in [`lobby`].
//!
//! A compact loadout strip (family / save / patch pickers, see
//! [`crate::loadout::compact_row`]) sits at the top as a fixture
//! across the idle↔lobby swap, so switching what you bring — including
//! flipping your patch to match the opponent's — never requires
//! leaving the lobby.

mod lobby;

use crate::app::Scanners;
use crate::config;
use crate::i18n::t;
use crate::loadout::{self, Loadout};
use crate::style::{self, TEXT_CAPTION, TEXT_TITLE};
use crate::widgets;
use iced::widget::{button, container, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use unic_langid::LanguageIdentifier;

// ---------- Messages ----------

#[derive(Debug, Clone)]
pub enum Message {
    /// Loadout strip interaction. Routed by the App to the shared
    /// [`Loadout`] state — never reaches [`State::update`].
    Loadout(loadout::Message),
    LinkCodeChanged(String),
    /// Fill the link-code input with a fresh random
    /// adjective-word-noun handle from `randomcode::generate`.
    LinkCodeRandom,
    FightPressed,
    Disconnect,
    /// Lobby UI: user picked a different match type. App routes
    /// this through netplay::Message::SetMatchType so the resend
    /// machinery picks it up.
    SetMatchType((u8, u8)),
    /// Lobby UI: user dragged the frame-delay slider, OR pressed
    /// the "suggest" button (which dispatches a value computed from the
    /// `lobby.latency_counter` median). Routes to the shared `config.frame_delay`
    /// (same store the Settings-tab slider writes), not lobby-local state.
    SetFrameDelay(u32),
    /// Lobby UI: user toggled the reveal-setup checkbox.
    SetRevealSetup(bool),
    /// Lobby UI: user pressed Ready. App loads the local
    /// save's raw SRAM, builds a NegotiatedState, and
    /// dispatches netplay::Message::Commit.
    Ready,
    /// Lobby UI: user pressed Unready (Ready button while
    /// already committed). Sends an Uncommit packet.
    Unready,
    /// User clicked × on the inline error banner; clears
    /// `State::last_error`.
    DismissError,
    /// Soft-disable sentinel for widgets that don't accept a
    /// `None` handler in iced 0.14 (pick_list, slider). The
    /// lobby reroutes match-type / frame-delay changes here in
    /// Phase::Failed (and the loadout strip during handoff) so
    /// the controls render inert without touching layout. The
    /// update handler drops it.
    Noop,
}

/// Side-effects bubble-up — see [`crate::tabs::replays::Effect`] for
/// the convention.
#[derive(Debug, Clone)]
pub enum Effect {
    /// Kick off netplay. The `LinkIdent` variant tells the app
    /// handler whether to route via matchmaking signaling or direct
    /// TCP transport.
    Connect(crate::netplay::LinkIdent),
    /// Forward verbatim to the netplay subsystem.
    Netplay(crate::netplay::Message),
    /// Lobby frame-delay slider moved. App persists `config.frame_delay`; it's
    /// this side's local frame delay (snapshotted into the match at
    /// start, not negotiated with the peer), so there's nothing live to update.
    SetFrameDelay(u32),
    /// Lobby Ready — App reads the local save SRAM and
    /// dispatches `netplay::Message::Commit`.
    ReadyWithSave,
    /// Copy plain text to the clipboard.
    CopyText(String),
}

// ---------- Fight tab state ----------

#[derive(Default)]
pub struct State {
    pub link_code: String,
    /// Last after-the-fact action failure (PvP session build
    /// failed, …) — rendered as a dismissable banner at the top of
    /// the tab. Pre-condition errors are handled by view-time
    /// button gating instead.
    pub last_error: Option<String>,
}

impl State {
    pub fn update(&mut self, msg: Message, config: &config::Config) -> Option<Effect> {
        match msg {
            // Routed to the shared Loadout at App level before this
            // dispatch is reached.
            Message::Loadout(_) => None,
            Message::LinkCodeChanged(s) => {
                // Direct-TCP commands (/host, /connect) need slashes,
                // spaces, dots, colons, brackets — pass them through.
                let filtered: String = if s.starts_with('/') {
                    s
                } else {
                    s.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
                };
                self.link_code = filtered.chars().take(100).collect();
                None
            }
            Message::LinkCodeRandom => {
                self.link_code = crate::randomcode::generate(&config.language);
                // Drop the freshly-generated code straight onto the
                // clipboard so the user can paste it into chat
                // without an extra select+copy round-trip.
                Some(Effect::CopyText(self.link_code.clone()))
            }
            Message::FightPressed => {
                // The Fight CTA is gated at the view layer to require
                // a submittable link code, so reaching this handler
                // without one is a stale message + safe to ignore.
                let ident = resolve_link_ident(self.link_code.trim())?;
                // Clear any leftover after-the-fact error from a prior
                // attempt — the new attempt's outcome will replace it.
                self.last_error = None;
                Some(Effect::Connect(ident))
            }
            Message::DismissError => {
                self.last_error = None;
                None
            }
            Message::Noop => None,
            Message::Disconnect => Some(Effect::Netplay(crate::netplay::Message::Disconnect)),
            Message::SetMatchType(mt) => Some(Effect::Netplay(crate::netplay::Message::SetMatchType(mt))),
            Message::SetFrameDelay(d) => Some(Effect::SetFrameDelay(d)),
            Message::SetRevealSetup(v) => Some(Effect::Netplay(crate::netplay::Message::SetRevealSetup(v))),
            Message::Ready => Some(Effect::ReadyWithSave),
            Message::Unready => Some(Effect::Netplay(crate::netplay::Message::Uncommit)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        loadout: &'a Loadout,
        scanners: &'a Scanners,
        config: &'a config::Config,
        has_save: bool,
        netplay_phase: &'a crate::netplay::Phase,
        netplay_lobby: &'a crate::netplay::LobbyState,
        netplay_handoff_pending: bool,
        streamer_mode: bool,
        // Two-phase swap between the idle body (browser-to-be +
        // link-code strip) and the lobby body, driven by the App
        // (which sees the netplay phase flip): first half sinks +
        // dissolves the outgoing body, second half rises the
        // incoming one out of the page surface.
        lobby_swap: &'a crate::anim::Transition,
        // The lobby's last live state, frozen by the App on the
        // frame the lobby left — the exiting body renders from
        // this so the verdict (e.g. the failure banner) doesn't
        // flash to the idle handshake line mid-dissolve.
        lobby_exit_snapshot: Option<&'a (crate::netplay::Phase, crate::netplay::LobbyState)>,
    ) -> Element<'a, Message> {
        let mut col = column![].width(Fill).height(Fill);
        if let Some(err) = &self.last_error {
            col = col.push(widgets::error_banner(lang, err, Message::DismissError));
        }

        // Compact loadout strip — the one fixture across the
        // idle↔lobby swap, so switching saves (or patches) mid-lobby
        // is always one click away. Inert during the handoff window:
        // the PvP session is being built from the committed state and
        // selection changes would only confuse.
        let inert_strip = netplay_handoff_pending;
        let strip: Element<'a, Message> = Element::from(
            container(loadout::compact_row(loadout, lang, scanners, config))
                .padding(style::PANE_PADDING)
                .width(Fill)
                .style(widgets::pane),
        )
        .map(move |m| {
            if inert_strip {
                Message::Noop
            } else {
                Message::Loadout(m)
            }
        });
        col = col.push(container(strip).padding(iced::Padding {
            top: style::PANE_GAP,
            right: style::PANE_GAP,
            bottom: 0.0,
            left: style::PANE_GAP,
        }));

        // Body below the strip: the idle screen (future lobby
        // browser + link-code strip) or the lobby, swapped on
        // `lobby_swap`'s unified timeline — the outgoing body sinks
        // while dissolving into the page surface, then the incoming
        // one rises out of it.
        let now = iced::time::Instant::now();
        let (render_lobby, swap) = crate::anim::swap_phase(lobby_swap, now);
        let mut body: Element<'a, Message> = if render_lobby {
            // While the lobby is on its way OUT, the live phase has
            // already gone Idle (and the lobby may be wiped) — use
            // the snapshot the App froze on its last live frame so
            // the verdict doesn't flash mid-dissolve.
            let (body_phase, body_lobby) = if !lobby_swap.shown() {
                lobby_exit_snapshot
                    .map(|(p, l)| (p, l))
                    .unwrap_or((netplay_phase, netplay_lobby))
            } else {
                (netplay_phase, netplay_lobby)
            };
            // Synthesize the local side's Settings from the current
            // loadout so the "You" slot fills in immediately —
            // pre-Lobby phases haven't populated `lobby.local` yet,
            // but everything it needs is already on hand locally.
            // Same builder the netplay loop uses to ship settings on
            // the wire, so the visible info during the handshake
            // exactly matches what gets sent.
            let local_fallback = loadout.make_local_settings(config, netplay_lobby, scanners);
            lobby::Lobby {
                lang,
                state: body_lobby,
                phase: body_phase,
                local_game: loadout.game,
                scanners,
                has_save,
                local_fallback,
                streamer_mode,
                handoff_pending: netplay_handoff_pending,
                frame_delay: config.frame_delay,
            }
            .view()
        } else {
            self.idle_view(lang)
        };
        if let Some(phase) = swap {
            let dist = if render_lobby { 48.0 } else { 24.0 };
            body = crate::anim::swap_transform(body, phase, iced::Vector::new(0.0, dist), |theme: &iced::Theme| {
                theme.palette().background
            });
        }
        col = col.push(container(body).width(Fill).height(Fill));
        col.into()
    }

    /// The pre-connection body: a placeholder card where the lobby
    /// browser will eventually live, with the link-code strip + Fight
    /// CTA pinned to the bottom.
    fn idle_view<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        let hint = container(
            container(
                column![
                    Icon::Swords.widget().size(28.0),
                    text(t!(lang, "fight-idle-title")).size(TEXT_TITLE),
                    text(t!(lang, "fight-idle-body"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                ]
                .spacing(10)
                .align_x(Alignment::Center)
                .padding(28)
                .max_width(520),
            )
            .style(widgets::panel),
        )
        .padding(24)
        .center(Fill);
        column![hint, widgets::hud_scanline_bottom(), self.bottom_strip(lang)]
            .width(Fill)
            .height(Fill)
            .into()
    }

    fn bottom_strip<'a>(&'a self, lang: &'a LanguageIdentifier) -> Element<'a, Message> {
        // Only rendered in Idle phase — the lobby body replaces this
        // strip for every in-flight netplay phase, so this strip is
        // pure "enter a link code and fight". Singleplayer lives in
        // the Saves tab's save view.
        const BOTTOM_SIZE: f32 = 15.0;
        const BOTTOM_PAD: [f32; 2] = [10.0, 16.0];
        const BOTTOM_CTA_PAD: [f32; 2] = [10.0, 22.0];
        let can_submit = resolve_link_ident(self.link_code.trim()).is_some();
        let fight_button: Element<'a, Message> = {
            // Same chrome as the lobby's Ready button — both are
            // "commit to a match" CTAs. ready_button_style for
            // ReadyPalette::Idle falls back to neutral when the
            // button is disabled, so the empty-link-code case
            // renders as a plain greyed-out pill without a
            // separate branch here.
            let label = row![
                Icon::Swords.widget().size(BOTTOM_SIZE),
                text(t!(lang, "play-fight")).size(BOTTOM_SIZE),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            let mut btn = button(label)
                .padding(BOTTOM_CTA_PAD)
                .height(Length::Fixed(crate::style::BAR_CONTROL_HEIGHT))
                .style(|theme: &iced::Theme, status| ready_button_style(theme, status, ReadyPalette::Idle));
            if can_submit {
                btn = btn.on_press(Message::FightPressed);
            }
            btn.into()
        };
        // Link-code input fills all the slack between the dice
        // button on its right and the row's left edge.
        // text_input doesn't expose a `.height()` method, so we
        // wrap it in a fixed-height container to match the
        // surrounding controls.
        let link_input: Element<'a, Message> = container(
            text_input(&t!(lang, "play-link-code"), &self.link_code)
                .on_input(Message::LinkCodeChanged)
                .on_submit(Message::FightPressed)
                .size(BOTTOM_SIZE)
                .padding(BOTTOM_PAD)
                .width(Length::Fill)
                .style(widgets::chunky_text_input),
        )
        .height(Length::Fixed(crate::style::BAR_CONTROL_HEIGHT))
        .width(Length::Fill)
        .into();
        let dice_button: Element<'a, Message> = iced::widget::tooltip(
            button(Icon::Dice5.widget().size(BOTTOM_SIZE))
                .padding(BOTTOM_PAD)
                .height(Length::Fixed(crate::style::BAR_CONTROL_HEIGHT))
                .style(widgets::neutral)
                .on_press(Message::LinkCodeRandom),
            container(text(t!(lang, "play-link-code-random")).size(TEXT_CAPTION))
                .padding(6)
                .style(|theme: &iced::Theme| {
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
                }),
            iced::widget::tooltip::Position::Top,
        )
        .gap(4)
        .into();

        container(
            row![link_input, dice_button, fight_button]
                .spacing(10)
                .align_y(Alignment::Center)
                .padding([10, 8]),
        )
        .width(Fill)
        .style(widgets::hud_bar)
        .into()
    }
}

// ---------- "Commit to a match" CTA chrome ----------
//
// Shared between the idle screen's Fight button and the lobby's Ready
// toggle — both are the same "slam this to fight" affordance, so they
// wear the same chrome.

/// Which ready-button state we're painting. Drives
/// [`ready_button_style`]'s color choice.
#[derive(Clone, Copy)]
enum ReadyPalette {
    /// Pre-commit; the action is "ready up". Accent (primary) so
    /// it reads as the call-to-action in the strip.
    Idle,
    /// Locally committed; the action is "unready". Neutral / gray —
    /// the commitment isn't a celebration to surface in green;
    /// what matters is the user can un-commit.
    Committed,
    /// Both committed; match is spinning up. Rendered as a passive
    /// indicator: muted background, no click target, no border.
    /// Caller sets `on_press = None` to match the disabled look.
    Starting,
}

/// Custom style for the lobby's Ready toggle. Three discrete
/// moods — each one its own visual register so a glance at the
/// button tells the whole story of "where are we in the
/// handshake".
///
/// * Idle      — primary_button on steroids: brighter gradient,
///               huge primary glow, chunky 2 px border. This is
///               the moment the user is supposed to slam the
///               button, so it has to feel hot.
/// * Committed — neutral beveled plate. We've ack'd locally and
///               are waiting on the peer; the only useful action
///               is to take it back, which is not a celebration.
/// * Starting  — flat muted badge. Both sides committed; the
///               button is now purely a status indicator with no
///               click target.
fn ready_button_style(theme: &iced::Theme, status: button::Status, palette: ReadyPalette) -> button::Style {
    let p = theme.extended_palette();
    let primary = theme.palette().primary;
    match palette {
        ReadyPalette::Starting => button::Style {
            background: Some(iced::Background::Color(p.background.weak.color)),
            text_color: widgets::muted_color(theme),
            border: iced::Border {
                radius: 10.0.into(),
                width: 1.0,
                color: p.background.strong.color,
            },
            ..Default::default()
        },
        ReadyPalette::Committed => {
            // Defer to the shared beveled neutral so the
            // un-ready toggle looks like a sibling of the other
            // chunky neutral buttons in the lobby strip.
            crate::widgets::neutral(theme, status)
        }
        ReadyPalette::Idle => {
            // Disabled state defers to the standard neutral
            // button so it reads as a plainly-greyed-out button
            // — the dim-primary-fill version this used to
            // render looked like a corrupted variant of the
            // lit-up state rather than a disabled affordance.
            if matches!(status, button::Status::Disabled) {
                return crate::widgets::neutral(theme, status);
            }
            // Inline expansion of the battle-button kernel with
            // every dial cranked: bigger glow, brighter top stop,
            // 2 px border so the button reads as a console
            // affordance rather than a CSS rectangle.
            let lighter = widgets::mix(primary, iced::Color::WHITE, 0.30);
            let darker = widgets::mix(primary, iced::Color::BLACK, 0.25);
            let (top, bottom, glow_alpha, offset_y, blur) = match status {
                button::Status::Hovered => (
                    widgets::mix(lighter, iced::Color::WHITE, 0.18),
                    widgets::mix(primary, iced::Color::WHITE, 0.05),
                    0.95,
                    8.0,
                    28.0,
                ),
                button::Status::Pressed => (darker, widgets::mix(darker, iced::Color::BLACK, 0.12), 0.35, 2.0, 14.0),
                button::Status::Disabled => unreachable!("handled above"),
                button::Status::Active => (lighter, darker, 0.75, 6.0, 22.0),
            };
            button::Style {
                background: Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, top)
                        .add_stop(1.0, bottom),
                ))),
                text_color: iced::Color::WHITE,
                border: iced::Border {
                    radius: 10.0.into(),
                    width: 2.0,
                    color: widgets::mix(primary, iced::Color::WHITE, 0.45),
                },
                shadow: iced::Shadow {
                    color: iced::Color {
                        a: glow_alpha,
                        ..primary
                    },
                    offset: iced::Vector::new(0.0, offset_y),
                    blur_radius: blur,
                },
                snap: false,
            }
        }
    }
}

// ---------- Link-code parsing ----------

/// Resolve a trimmed link-code input into a submittable
/// [`crate::netplay::LinkIdent`], or `None` if the input isn't
/// submittable (empty, or a malformed `/`-prefixed direct command).
fn resolve_link_ident(input: &str) -> Option<crate::netplay::LinkIdent> {
    if input.is_empty() {
        return None;
    }
    if input.starts_with('/') {
        parse_direct_command(input).map(crate::netplay::LinkIdent::Direct)
    } else {
        Some(crate::netplay::LinkIdent::Matchmaking(input.to_string()))
    }
}

/// Recognise the direct-TCP link-code commands the user can type
/// in place of a matchmaking code:
///
/// - `/host`            — listen on [`crate::net::DEFAULT_LOCAL_PORT`]
/// - `/host <port>`     — listen on the given port
/// - `/connect <addr>`  — dial `<addr>`, appending the default
///                        port if the user didn't specify one
fn parse_direct_command(input: &str) -> Option<crate::netplay::DirectRole> {
    // The leading slash is the disambiguator — without it, any
    // input is a matchmaking link code (which can legitimately
    // contain letters, digits, and the random-code separators).
    if !input.starts_with('/') {
        return None;
    }
    let mut parts = input.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("");
    let arg = parts.next().map(str::trim).unwrap_or("");
    match cmd {
        "/host" => {
            let port = if arg.is_empty() {
                crate::net::DEFAULT_LOCAL_PORT
            } else {
                arg.parse::<u16>().ok()?
            };
            Some(crate::netplay::DirectRole::Host { port })
        }
        "/connect" => {
            if arg.is_empty() {
                return None;
            }
            // Heuristic: if the user gave no colon (bare IP) or
            // their input ends with the IPv6 closing bracket
            // without a trailing colon, append the default port.
            // We deliberately don't try to validate the address
            // itself — TcpStream::connect's error surfaces well.
            let addr = if arg.contains(':') && !arg.ends_with(']') {
                arg.to_string()
            } else {
                format!("{arg}:{}", crate::net::DEFAULT_LOCAL_PORT)
            };
            Some(crate::netplay::DirectRole::Connect { addr })
        }
        _ => None,
    }
}
