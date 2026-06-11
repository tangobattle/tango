//! The Fight tab: everything netplay. Idle, it's the link-code strip
//! + Fight CTA (the body above them is reserved for the future lobby
//! browser); once a connection attempt is in flight, the body becomes
//! the full lobby — versus cards, compat verdict, match settings,
//! Ready CTA.
//!
//! A compact loadout strip (family / save / patch pickers, see
//! [`crate::loadout::compact_row`]) sits at the top as a fixture
//! across the idle↔lobby swap, so switching what you bring — including
//! flipping your patch to match the opponent's — never requires
//! leaving the lobby.

use crate::app::Scanners;
use crate::i18n::t;
use crate::loadout::{self, Loadout};
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING, TEXT_TITLE};
use crate::widgets;
use crate::{config, rom};
use iced::widget::space::horizontal as horizontal_space;
use iced::widget::{button, container, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, pick_list, row, text_input};
use tango_pvp::battle::suggest_frame_delay;
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
            lobby_view(
                lang,
                body_lobby,
                body_phase,
                loadout.game,
                scanners,
                has_save,
                local_fallback,
                streamer_mode,
                netplay_handoff_pending,
                config.frame_delay,
            )
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

// ---------- Lobby ----------

/// Lobby body shown while netplay is in flight. Two columns — you on
/// the left, opponent on the right — plus a latency line at the top +
/// match-type + input-delay controls underneath. Settings round-trips
/// asynchronously, so either side may be `None` for a tick.
#[allow(clippy::too_many_arguments)]
fn lobby_view<'a>(
    lang: &'a LanguageIdentifier,
    lobby: &'a crate::netplay::LobbyState,
    phase: &'a crate::netplay::Phase,
    local_game: Option<rom::GameRef>,
    scanners: &'a Scanners,
    has_save: bool,
    local_fallback: crate::net::protocol::Settings,
    streamer_mode: bool,
    handoff_pending: bool,
    frame_delay: u32,
) -> Element<'a, Message> {
    let failed = matches!(phase, crate::netplay::Phase::Failed { .. });
    // `inert` collapses Failed and handoff-pending — both are
    // states where the lobby controls are still on screen but
    // shouldn't accept input (Failed because the connection is
    // gone, handoff-pending because the match is spinning up
    // and the connection has been handed to the PvP session).
    let inert = failed || handoff_pending;

    let header_line = lobby_header_line(lang, lobby, phase, streamer_mode);
    let (verdict_line, compat_ok) = lobby_verdict(lang, lobby, phase, scanners);

    // Settings stack on the left, Ready CTA floated to the right
    // of the pane and bottom-aligned against the stack — mirrors
    // the matchmaking screen's Fight button anchored to the
    // bottom-right of the hud bar.
    let mt_picker = lobby_match_type_picker(lang, local_game, lobby, inert);
    let controls = row![
        lobby_settings_rows(lang, lobby, mt_picker, frame_delay, inert),
        lobby_ready_button(lang, lobby, failed, compat_ok, has_save),
    ]
    .spacing(12)
    .align_y(Alignment::End);

    // Leave-lobby (Disconnect) button. Top-right of the header —
    // out of the way of the verdict line, and visually paired
    // with the Ready CTA in the bottom-right of the lobby pane
    // (same right edge, opposite corner). Disabled during the
    // handoff window so the user can't tear down a lobby whose
    // PvP session is already being built — clicking Disconnect
    // there wouldn't actually cancel spawn_pvp, just leave the
    // user confused when the match view pops up anyway.
    let leave_button: Element<'a, Message> = {
        let inner = row![Icon::LogOut.widget(), text(t!(lang, "play-cancel"))]
            .spacing(8)
            .align_y(Alignment::Center);
        let mut btn = button(inner).padding(STANDARD_PADDING).style(widgets::danger_button);
        if !handoff_pending {
            btn = btn.on_press(Message::Disconnect);
        }
        btn.into()
    };

    // Header row: verdict on the left, leave button on the right.
    let mut header_text_col = column![].spacing(2);
    if let Some(hl) = header_line {
        header_text_col = header_text_col.push(hl);
    }
    header_text_col = header_text_col.push(verdict_line);
    let header_row = row![header_text_col, horizontal_space(), leave_button]
        .spacing(12)
        .align_y(Alignment::Center);

    // Sides row: you / opponent cards with a wide gap so the
    // diagonal cut + VS badge from `widgets::vs_splitter` paints
    // through the middle. The splitter canvas (which also paints
    // the red/blue half tints) is layered *under* the row.
    let sides_row = row![
        lobby_side_card(
            lang,
            t!(lang, "play-you"),
            Some(lobby.local.as_ref().unwrap_or(&local_fallback)),
            lobby.local_ready,
        ),
        lobby_side_card(
            lang,
            t!(lang, "play-opponent"),
            lobby.remote.as_ref(),
            lobby.remote_ready
        ),
    ]
    .spacing(56)
    // Top-align so the YOU slot doesn't bounce upward when the
    // opponent's settings land and their card grows from a 2-line
    // placeholder to a 3-line filled card.
    .align_y(Alignment::Start);
    let matchup_pane = container(
        iced::widget::Stack::new()
            .push(container(sides_row).padding(style::PANE_PADDING).width(Fill))
            .push_under(widgets::vs_splitter()),
    )
    .width(Fill)
    .style(widgets::pane);
    let controls_pane = container(controls)
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane);
    // On failure the header pane (which carries the verdict line)
    // picks up a faint red wash so a dead lobby reads as dead at a
    // glance — quiet on purpose; the icon + danger text carry the
    // message, the wash just sets the mood.
    let header_style: fn(&iced::Theme) -> iced::widget::container::Style = if failed {
        |theme: &iced::Theme| {
            let danger = theme.extended_palette().danger.strong.color;
            iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color { a: 0.08, ..danger })),
                ..widgets::pane(theme)
            }
        }
    } else {
        widgets::pane
    };
    let header_pane = container(header_row)
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(header_style);
    container(
        column![header_pane, matchup_pane, controls_pane]
            .spacing(style::PANE_GAP)
            .padding(style::PANE_GAP),
    )
    .width(Fill)
    .into()
}

/// Compact "you / opponent" card — 2 lines max so the lobby pane
/// reads at a glance. `ready` paints a green dot when that side has
/// committed.
fn lobby_side_card(
    lang: &LanguageIdentifier,
    label: String,
    settings: Option<&crate::net::protocol::Settings>,
    ready: bool,
) -> Element<'static, Message> {
    // 14 px dot with a soft primary-tinted glow when the
    // side is committed — reads as a "ready light" on a
    // console panel rather than a flat status pip.
    // Padded so the dot lines up with the nickname row of
    // the column to its right — the inner side row is
    // top-aligned (Alignment::Start) so the dot doesn't
    // drift when the card grows from a 2-line placeholder
    // to a 3-line populated card.
    let dot_color = |ready: bool| -> Element<'static, Message> {
        container(
            container(
                iced::widget::Space::new()
                    .width(Length::Fixed(14.0))
                    .height(Length::Fixed(14.0)),
            )
            .style(move |theme: &iced::Theme| {
                let bg = if ready {
                    theme.palette().primary
                } else {
                    // Theme-aware "off" gray — a hardcoded mid-gray
                    // disappears against the light theme's parchment.
                    widgets::muted_color(theme)
                };
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: iced::Border {
                        radius: 7.0.into(),
                        ..Default::default()
                    },
                    shadow: if ready {
                        iced::Shadow {
                            color: iced::Color {
                                a: 0.7,
                                ..theme.palette().primary
                            },
                            offset: iced::Vector::new(0.0, 0.0),
                            blur_radius: 10.0,
                        }
                    } else {
                        iced::Shadow::default()
                    },
                    ..Default::default()
                }
            }),
        )
        .padding(iced::Padding {
            top: 20.0,
            right: 0.0,
            bottom: 0.0,
            left: 0.0,
        })
        .into()
    };
    let Some(settings) = settings else {
        return container(
            row![
                dot_color(false),
                column![
                    text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                    text(t!(lang, "lobby-waiting"))
                        .size(TEXT_TITLE)
                        .style(widgets::muted_text_style),
                ]
                .spacing(2),
            ]
            .spacing(10)
            .align_y(Alignment::Start),
        )
        .width(Length::Fill)
        .into();
    };
    let nickname = settings.nickname.clone();
    let game_label = settings
        .game_info
        .as_ref()
        .map(|gi| {
            let family = gi.family_and_variant.0.as_str();
            // Dynamic key (one per gamedb family) — bypass the
            // literal-only macro and hit the Fluent loader directly.
            use fluent_templates::Loader;
            crate::i18n::LOCALES
                .try_lookup(lang, &format!("game-{family}"))
                .unwrap_or_else(|| format!("{} v{}", gi.family_and_variant.0, gi.family_and_variant.1))
        })
        .unwrap_or_else(|| t!(lang, "lobby-no-game"));
    let patch = settings
        .game_info
        .as_ref()
        .and_then(|gi| gi.patch.as_ref())
        .map(|p| format!(" · {} v{}", p.name, p.version));
    // Game line: "<game name> · <patch> · <match-type>" packed
    // onto a single caption row so the card stays 2 lines tall.
    // Match-type is meaningless without a game (no Game::match_types
    // table to look the name up against), so omit it then.
    let mut subline = game_label;
    if let Some(p) = patch {
        subline.push_str(&p);
    }
    if let Some(gi) = settings.game_info.as_ref() {
        let mt = crate::game::match_type_name(
            lang,
            gi.family_and_variant.0.as_str(),
            settings.match_type.0,
            settings.match_type.1,
        );
        subline.push_str(&format!(" · {mt}"));
    }
    // Nickname is the marquee — title-sized, primary
    // tinted when this side is ready so the card lights
    // up visibly as commitment lands.
    let nickname_style: fn(&iced::Theme) -> iced::widget::text::Style = if ready {
        |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(theme.palette().primary),
        }
    } else {
        |_theme: &iced::Theme| iced::widget::text::Style { color: None }
    };
    container(
        row![
            dot_color(ready),
            column![
                text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                text(nickname).size(TEXT_TITLE).style(nickname_style),
                text(subline).size(TEXT_CAPTION),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_y(Alignment::Start),
    )
    .width(Length::Fill)
    .into()
}

/// The lobby header's status line: latency once Pongs are landing,
/// otherwise the connection identifier (matchmaking code / direct
/// host / direct target) so the user sees what they're matched on.
/// Streamer privacy mode suppresses the identifier so a viewer of the
/// stream can't scrape it off the screen and crash the lobby — and
/// that's the only path to the "no latency, no identifier" `None`,
/// so the caller just skips the line there.
fn lobby_header_line<'a>(
    lang: &LanguageIdentifier,
    lobby: &'a crate::netplay::LobbyState,
    phase: &'a crate::netplay::Phase,
    streamer_mode: bool,
) -> Option<Element<'a, Message>> {
    let ident: Option<&crate::netplay::LinkIdent> = if streamer_mode {
        None
    } else {
        match phase {
            crate::netplay::Phase::Connecting { ident, .. }
            | crate::netplay::Phase::Negotiating { ident }
            | crate::netplay::Phase::Lobby { ident } => Some(ident),
            _ => None,
        }
    };
    if let Some(d) = lobby.latency_counter.latest() {
        let ms = d.as_millis() as i64;
        let label = match lobby.connection_kind {
            Some(crate::netplay::ConnectionKind::Direct) => t!(lang, "lobby-latency-direct", ms = ms),
            Some(crate::netplay::ConnectionKind::Relayed) => t!(lang, "lobby-latency-relayed", ms = ms),
            None => t!(lang, "lobby-latency", ms = ms),
        };
        Some(text(label).size(TEXT_BODY).style(widgets::muted_text_style).into())
    } else if let Some(ident) = ident {
        use crate::netplay::{DirectRole, LinkIdent};
        let label = match ident {
            LinkIdent::Matchmaking(code) => t!(lang, "lobby-link-code", code = code.clone()),
            LinkIdent::Direct(DirectRole::Host { port }) => {
                t!(lang, "lobby-direct-host", port = port.to_string())
            }
            LinkIdent::Direct(DirectRole::Connect { addr }) => {
                t!(lang, "lobby-direct-connect", target = addr.clone())
            }
        };
        Some(text(label).size(TEXT_BODY).style(widgets::muted_text_style).into())
    } else {
        None
    }
}

/// Match-type pick_list — options pulled from the current local game's
/// Game::match_types() table (mode + subtype counts), labeled with the
/// per-game Fluent strings via game::match_type_name. Renders an empty
/// disabled pick_list when no game is selected (Game::match_types()
/// can't be queried until we know the game) — gives the row a stable
/// shape so the surrounding layout doesn't jump once the user picks a
/// game. When `inert`, picks reroute to Noop without touching layout.
fn lobby_match_type_picker<'a>(
    lang: &LanguageIdentifier,
    local_game: Option<rom::GameRef>,
    lobby: &'a crate::netplay::LobbyState,
    inert: bool,
) -> Element<'a, Message> {
    let Some(g) = local_game else {
        let empty: Vec<MatchTypeOption> = Vec::new();
        return pick_list(empty, None::<MatchTypeOption>, |o: MatchTypeOption| {
            Message::SetMatchType((o.mode, o.subtype))
        })
        .padding(STANDARD_PADDING)
        .style(crate::widgets::chunky_pick_list)
        .into();
    };
    let game_impl = crate::game::from_gamedb_entry(g);
    let mt_table = game_impl.map(|gi| gi.match_types).unwrap_or(&[]);
    let mut options = Vec::new();
    for (mode, subtype_count) in mt_table.iter().enumerate() {
        for sub in 0..*subtype_count {
            options.push(MatchTypeOption {
                mode: mode as u8,
                subtype: sub as u8,
                label: crate::game::match_type_name(lang, g.family_and_variant().0, mode as u8, sub as u8),
            });
        }
    }
    let selected = options
        .iter()
        .find(|o| o.mode == lobby.match_type.0 && o.subtype == lobby.match_type.1)
        .cloned();
    let on_change: fn((u8, u8)) -> Message = if inert {
        |_| Message::Noop
    } else {
        Message::SetMatchType
    };
    if options.is_empty() {
        text(t!(lang, "lobby-no-match-types"))
            .style(widgets::muted_text_style)
            .into()
    } else {
        pick_list(options, selected, move |o| on_change((o.mode, o.subtype)))
            .padding(STANDARD_PADDING)
            .style(crate::widgets::chunky_pick_list)
            .into()
    }
}

/// The lobby settings table — one stacked row per setting (match type /
/// frame delay / reveal setup), each shaped `[fixed-width muted label]
/// [control fills the rest]`. The identical row shape is what makes the
/// block read as a single coherent settings group; visual weight
/// differences between picker / slider / checkbox stop mattering
/// because every control hangs off the same label column.
fn lobby_settings_rows<'a>(
    lang: &LanguageIdentifier,
    lobby: &'a crate::netplay::LobbyState,
    mt_picker: Element<'a, Message>,
    frame_delay: u32,
    inert: bool,
) -> Element<'a, Message> {
    let label_style: fn(&iced::Theme) -> iced::widget::text::Style = widgets::muted_text_style;
    let setting_row = |label_el: Element<'a, Message>, control: Element<'a, Message>| -> Element<'a, Message> {
        row![
            container(label_el).width(Length::Fixed(140.0)),
            container(control).width(Length::Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into()
    };

    let match_row = setting_row(
        text(t!(lang, "lobby-match-type"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        mt_picker,
    );

    // Frame delay slider — 2..=10 frames. Set here before the match; it's this
    // side's local frame delay (how far the display trails the netcode
    // frontier), purely local with no negotiation. Each increment is one GBA
    // frame (~16.7 ms) of added display latency.
    // Reroute through Noop when inert so dragging it doesn't do anything.
    let slider_on_change: fn(u32) -> Message = if inert {
        |_| Message::Noop
    } else {
        Message::SetFrameDelay
    };
    let id_slider = iced::widget::slider(
        tango_pvp::battle::MIN_FRAME_DELAY..=tango_pvp::battle::MAX_FRAME_DELAY,
        frame_delay,
        slider_on_change,
    )
    .width(Length::Fixed(160.0));

    // "Suggest" button: one-way frames + 1, clamped to the slider range. Reads
    // the median window rather than the raw `latest()` shown on the line, so the
    // recommendation doesn't jump with a single spiky Pong. Disabled when the
    // controls are inert, and until the first Pong lands (`latest()` is `Some`)
    // so the counter has a real reading to take the median of.
    let suggest_msg = if inert || lobby.latency_counter.latest().is_none() {
        None
    } else {
        let rtt = lobby.latency_counter.median();
        Some(Message::SetFrameDelay(suggest_frame_delay(rtt)))
    };
    let id_suggest = widgets::icon_button_maybe(
        Icon::Wand,
        t!(lang, "lobby-frame-delay-suggest"),
        suggest_msg,
        STANDARD_PADDING,
    );

    let delay_row = setting_row(
        text(t!(lang, "settings-netplay-frame-delay"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        row![
            id_slider,
            text(format!("{}", frame_delay))
                .size(TEXT_BODY)
                .width(Length::Fixed(18.0)),
            id_suggest,
        ]
        .spacing(10)
        .align_y(Alignment::Center)
        .into(),
    );

    // Reveal-setup checkbox. Mirrors the legacy app's
    // `play-details-reveal-setup` checkbox — each side picks
    // independently; the peer can see (read-only) what we picked
    // via the remote status next to the checkbox.
    // Peer's current "reveal my setup" flag — surfaced as a
    // standalone sentence under the checkbox so the parens-stuffed
    // label doesn't have to be locale-jammed into the checkbox text.
    // Color follows the state: green when peer is sharing,
    // muted/red when not / unknown.
    let (reveal_label, reveal_style): (String, fn(&iced::Theme) -> iced::widget::text::Style) =
        if let Some(r) = lobby.remote.as_ref() {
            if r.reveal_setup {
                (t!(lang, "lobby-reveal-peer-on"), widgets::success_text_style)
            } else {
                (t!(lang, "lobby-reveal-peer-off"), widgets::danger_text_style)
            }
        } else {
            (t!(lang, "lobby-reveal-peer-unknown"), widgets::muted_text_style)
        };
    let reveal_toggle = if inert {
        None
    } else {
        Some(Message::SetRevealSetup as fn(bool) -> Message)
    };
    let reveal_row = setting_row(
        text(t!(lang, "lobby-reveal-mine"))
            .size(TEXT_BODY)
            .style(label_style)
            .into(),
        row![
            iced::widget::checkbox(lobby.reveal_setup)
                .on_toggle_maybe(reveal_toggle)
                .size(TEXT_HEADING)
                .style(widgets::chunky_checkbox),
            text(reveal_label).size(TEXT_CAPTION).style(reveal_style),
        ]
        .spacing(12)
        .align_y(Alignment::Center)
        .into(),
    );

    column![match_row, delay_row, reveal_row]
        .spacing(8)
        .width(Length::Fill)
        .into()
}

/// Status / verdict line + whether compat allows readying up. While
/// the netplay attempt is still pre-Lobby (Connecting / Negotiating),
/// this shows the connection progress so the user has something to
/// read through the handshake. Once we're in Lobby with both sides'
/// settings on hand, it switches to the compat verdict and gates the
/// Ready button. Failed = sticky banner with the cause, dismissed by
/// the Cancel button in the header.
fn lobby_verdict<'a>(
    lang: &LanguageIdentifier,
    lobby: &'a crate::netplay::LobbyState,
    phase: &'a crate::netplay::Phase,
    scanners: &Scanners,
) -> (Element<'a, Message>, bool) {
    use crate::netplay::Phase;
    // The in-flight statuses (connecting / waiting / negotiating /
    // handshake) breathe between muted and primary-tinted so the
    // line reads as "still working" rather than frozen. The App's
    // subscription keeps per-frame redraws coming while one of
    // these is on screen; terminal states (verdicts, failures)
    // stay static.
    let pulse = crate::anim::pulse();
    let pulsing_style = move |theme: &iced::Theme| iced::widget::text::Style {
        color: Some(widgets::mix(
            widgets::muted_color(theme),
            theme.palette().primary,
            0.45 * pulse,
        )),
    };
    match phase {
        Phase::Failed { error } => {
            // Route the netplay error tag through Fluent so each
            // failure mode can carry its own translated copy.
            // Anything we don't have a dedicated key for falls
            // back to the generic "Connection failed: <raw>".
            let label = match error.as_str() {
                "peer-disconnected" => t!(lang, "play-status-peer-disconnected"),
                "negotiate-expected-hello" => t!(lang, "play-status-negotiate-expected-hello"),
                "negotiate-version-too-old" => t!(lang, "play-status-negotiate-version-too-old"),
                "negotiate-version-too-new" => t!(lang, "play-status-negotiate-version-too-new"),
                other if other.starts_with("negotiate-other: ") => t!(
                    lang,
                    "play-status-negotiate-failed",
                    error = other.trim_start_matches("negotiate-other: ").to_string(),
                ),
                _ => t!(lang, "play-status-failed", error = error.clone()),
            };
            // The lobby is dead at this point but its chrome is
            // still on screen — cue it with an alert icon next to
            // the danger text and a faint red wash on the header
            // pane (see `lobby_view`). Deliberately gentle: a loud
            // border + oversized text read as more alarming than a
            // failed handshake warrants.
            let line = row![
                Icon::AlertTriangle
                    .widget()
                    .size(TEXT_BODY)
                    .style(widgets::danger_text_style),
                text(label).size(TEXT_BODY).style(widgets::danger_text_style),
            ]
            .spacing(8)
            .align_y(Alignment::Center);
            (line.into(), false)
        }
        Phase::Connecting {
            ident,
            waiting_for_opponent: false,
        } => {
            // Matchmaking codes hit the server first ("Connecting
            // to matchmaking server…"); direct `/connect` codes
            // dial straight at the peer, so the matchmaking copy
            // is wrong — use the opponent-targeted string instead.
            let label = match ident {
                crate::netplay::LinkIdent::Direct(crate::netplay::DirectRole::Connect { .. }) => {
                    t!(lang, "play-status-direct-connecting")
                }
                _ => t!(lang, "play-status-connecting"),
            };
            (text(label).size(TEXT_BODY).style(pulsing_style).into(), false)
        }
        Phase::Connecting {
            waiting_for_opponent: true,
            ..
        } => (
            text(t!(lang, "play-status-waiting-opponent"))
                .size(TEXT_BODY)
                .style(pulsing_style)
                .into(),
            false,
        ),
        Phase::Negotiating { .. } => (
            text(t!(lang, "play-status-negotiating"))
                .size(TEXT_BODY)
                .style(pulsing_style)
                .into(),
            false,
        ),
        _ => match (lobby.local.as_ref(), lobby.remote.as_ref()) {
            (Some(l), Some(r)) => {
                use crate::netplay::compat::Verdict;
                let patches = scanners.patches.read();
                let verdict = crate::netplay::compat::check(l, r, &*patches);
                let label = match verdict {
                    Verdict::Compatible => t!(lang, "lobby-compat-ok"),
                    Verdict::MissingGame => t!(lang, "lobby-compat-missing-game"),
                    Verdict::MissingRomOrPatch => t!(lang, "lobby-compat-missing-rom"),
                    Verdict::DifferentVersions => t!(lang, "lobby-compat-version-mismatch"),
                    Verdict::DifferentMatchTypes => t!(lang, "lobby-compat-match-mismatch"),
                };
                let ok = matches!(verdict, Verdict::Compatible);
                let style: fn(&iced::Theme) -> iced::widget::text::Style = if ok {
                    widgets::success_text_style
                } else {
                    widgets::danger_text_style
                };
                (text(label).size(TEXT_BODY).style(style).into(), ok)
            }
            _ => (
                text(t!(lang, "lobby-handshake"))
                    .size(TEXT_BODY)
                    .style(pulsing_style)
                    .into(),
                false,
            ),
        },
    }
}

/// Big single toggle: Ready → Unready → Starting…, switching label +
/// icon + color on click. Same button, same position; clicking it
/// always does the obvious next thing (ready up, unready, or wait for
/// match-start). A touch chunkier than the regular CTAs in the strip,
/// but not so big that it blows the lobby layout — the glow shadow
/// does the work of "look at me" instead.
fn lobby_ready_button<'a>(
    lang: &LanguageIdentifier,
    lobby: &crate::netplay::LobbyState,
    failed: bool,
    compat_ok: bool,
    has_save: bool,
) -> Element<'a, Message> {
    const READY_TEXT: f32 = 16.0;
    const READY_PAD: [f32; 2] = [10.0, 22.0];
    let (ready_icon, ready_label, ready_msg, ready_palette): (Icon, String, Option<Message>, ReadyPalette) =
        if lobby.match_ready {
            // Both committed — match is spinning up. Button is purely
            // a status indicator; no click target until the session
            // actually opens.
            (
                Icon::Play,
                t!(lang, "lobby-match-starting"),
                None,
                ReadyPalette::Starting,
            )
        } else if lobby.local_ready {
            // Locally committed, waiting on peer. Action = unready.
            // Gray / neutral so it doesn't masquerade as a primary CTA.
            (
                Icon::X,
                t!(lang, "lobby-unready"),
                Some(Message::Unready),
                ReadyPalette::Committed,
            )
        } else {
            // Compat OK + a save loaded → click sends Commit. Either
            // missing → button disabled (the user can see WHY: the
            // verdict text covers compat, and the side card / save
            // selector covers "no save").
            let can_ready = compat_ok && has_save;
            (
                Icon::Check,
                t!(lang, "lobby-ready"),
                if can_ready { Some(Message::Ready) } else { None },
                ReadyPalette::Idle,
            )
        };
    // Failed lobby: the only action is to dismiss via Cancel.
    // Force the Ready button off regardless of how the
    // pre-failure state looked.
    let ready_msg = if failed { None } else { ready_msg };
    let label_widget = row![ready_icon.widget().size(READY_TEXT), text(ready_label).size(READY_TEXT),]
        .spacing(8)
        .align_y(Alignment::Center);
    let mut btn = button(label_widget)
        .padding(READY_PAD)
        .style(move |theme: &iced::Theme, status| ready_button_style(theme, status, ready_palette));
    if let Some(m) = ready_msg {
        btn = btn.on_press(m);
    }
    btn.into()
}

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct MatchTypeOption {
    mode: u8,
    subtype: u8,
    label: String,
}
impl std::fmt::Display for MatchTypeOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
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
