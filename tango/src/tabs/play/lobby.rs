//! The lobby — the Play tab's bottom band while a netplay attempt is
//! in flight, standing in for the link-code strip while the save view
//! above stays visible. Two stacked panes, hero then console: the
//! matchup (you / opponent cards over the VS splitter) and one
//! command bar that reads left to right in the order the user thinks
//! — what's happening (status / verdict, with the connection detail
//! tucked under it), what the terms are (the match settings cluster),
//! then back out / commit (Leave + Ready). One home per kind of
//! information; nothing in opposite corners.
//!
//! Everything renders off [`Lobby`], the per-frame bundle the Play
//! tab assembles in [`super::State::view`], and [`Status`], the one
//! derived lifecycle fact that keeps the verdict line and the Ready
//! gate in agreement.

use crate::app::Scanners;
use crate::game;
use crate::i18n::t;
use crate::net::protocol::Settings;
use crate::netplay::{self, Phase};
use crate::rom;
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_HEADING, TEXT_TITLE};
use crate::widgets;
use iced::widget::{button, container, text};
use iced::{Alignment, Element, Fill, Length};
use lucide_icons::Icon;
use sweeten::widget::{column, pick_list, row};
use tango_pvp::battle::suggest_frame_delay;
use unic_langid::LanguageIdentifier;

use super::{ready_button_style, Message, ReadyPalette};

/// Everything the lobby needs to paint one frame. Settings round-trip
/// asynchronously, so either of `state.local` / `state.remote` may be
/// `None` for a tick.
pub(super) struct Lobby<'a> {
    pub(super) lang: &'a LanguageIdentifier,
    /// May be the App's exit snapshot rather than the live state while
    /// the lobby body is animating out — see `lobby_exit_snapshot` in
    /// [`super::State::view`]. Same goes for `phase`.
    pub(super) state: &'a netplay::LobbyState,
    pub(super) phase: &'a netplay::Phase,
    pub(super) local_game: Option<rom::GameRef>,
    pub(super) scanners: &'a Scanners,
    pub(super) has_save: bool,
    /// Local-side Settings synthesized from the current loadout, used
    /// to fill the "You" card before `state.local` lands.
    pub(super) local_fallback: Settings,
    pub(super) streamer_mode: bool,
    pub(super) handoff_pending: bool,
    pub(super) frame_delay: u32,
}

impl<'a> Lobby<'a> {
    pub(super) fn view(self) -> Element<'a, Message> {
        let status = self.status();
        let compat_ok = status.compat_ok();
        let matchup_pane = self.matchup_pane();
        let command_pane = self.command_pane(&status, compat_ok);
        container(
            column![matchup_pane, command_pane]
                .spacing(style::PANE_GAP)
                .padding(style::PANE_GAP),
        )
        .width(Fill)
        .into()
    }

    /// Terminal failure — the connection is gone but the lobby chrome
    /// stays on screen until the user cancels out of it.
    fn failed(&self) -> bool {
        matches!(self.phase, Phase::Failed { .. })
    }

    /// Whether the controls should refuse input without changing
    /// layout: the connection is dead ([`Self::failed`]), or the match
    /// is spinning up and the connection has been handed to the PvP
    /// session (`handoff_pending`).
    fn inert(&self) -> bool {
        self.failed() || self.handoff_pending
    }

    /// Derive this frame's lifecycle [`Status`] from the netplay phase
    /// + lobby state.
    fn status(&self) -> Status<'a> {
        match self.phase {
            Phase::Failed { error } => Status::Failed { error },
            // Matchmaking codes hit the server first ("Connecting to
            // matchmaking server…"); direct `/connect` codes dial
            // straight at the peer, so the matchmaking copy is wrong —
            // the status carries the distinction.
            Phase::Connecting {
                ident,
                waiting_for_opponent: false,
            } => Status::Connecting {
                direct: matches!(ident, netplay::LinkIdent::Direct(netplay::DirectRole::Connect { .. })),
            },
            Phase::Connecting {
                waiting_for_opponent: true,
                ..
            } => Status::WaitingForOpponent,
            Phase::Negotiating { .. } => Status::Negotiating,
            _ => match (self.state.local.as_ref(), self.state.remote.as_ref()) {
                (Some(l), Some(r)) => {
                    let patches = self.scanners.patches.read();
                    Status::Verdict(netplay::compat::check(l, r, &*patches))
                }
                _ => Status::Handshake,
            },
        }
    }

    /// The command bar under the matchup: the one pane that gathers
    /// everything the user reads and operates, laid out left to right
    /// in reading order — status stack (verdict line + connection
    /// detail), match settings cluster, then the Leave / Ready action
    /// pair. On failure the pane picks up a faint red wash so a dead
    /// lobby reads as dead at a glance — quiet on purpose; the icon +
    /// danger text of the status line carry the message, the wash
    /// just sets the mood.
    fn command_pane(&self, status: &Status<'_>, compat_ok: bool) -> Element<'a, Message> {
        let mut status_col = column![self.status_line(status)].spacing(4);
        if let Some(line) = self.connection_line() {
            status_col = status_col.push(line);
        }
        // Leave before Ready — cancel before primary, same order as
        // the save-action forms and the modal dialogs, so the commit
        // action always sits at the row's end (right where the idle
        // strip parks its Fight CTA).
        let actions = row![self.leave_button(), self.ready_button(compat_ok)]
            .spacing(10)
            .align_y(Alignment::Center);
        let bar = row![
            // The status stack soaks up the slack so the settings +
            // actions stay packed against the right edge.
            container(status_col).width(Length::Fill),
            self.settings_cluster(),
            actions,
        ]
        .spacing(24)
        .align_y(Alignment::Center);
        let pane_style: fn(&iced::Theme) -> iced::widget::container::Style = if self.failed() {
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
        container(bar)
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(pane_style)
            .into()
    }

    /// Small muted line under the status: the connection identifier
    /// (matchmaking code / direct host / direct target) plus, once
    /// Pongs are landing, the measured latency — joined on one line so
    /// the code doesn't vanish the moment the first ping lands.
    /// Streamer privacy mode suppresses the identifier so a viewer of
    /// the stream can't scrape it off the screen and crash the lobby;
    /// `None` only when neither half has anything to say yet.
    fn connection_line(&self) -> Option<Element<'a, Message>> {
        let lang = self.lang;
        let mut parts: Vec<String> = Vec::new();
        if !self.streamer_mode {
            use netplay::{DirectRole, LinkIdent};
            let ident = match self.phase {
                Phase::Connecting { ident, .. } | Phase::Negotiating { ident } | Phase::Lobby { ident } => Some(ident),
                _ => None,
            };
            if let Some(ident) = ident {
                parts.push(match ident {
                    LinkIdent::Matchmaking(code) => t!(lang, "lobby-link-code", code = code.clone()),
                    LinkIdent::Direct(DirectRole::Host { port }) => {
                        t!(lang, "lobby-direct-host", port = port.to_string())
                    }
                    LinkIdent::Direct(DirectRole::Connect { addr }) => {
                        t!(lang, "lobby-direct-connect", target = addr.clone())
                    }
                });
            }
        }
        if let Some(d) = self.state.latency_counter.latest() {
            let ms = d.as_millis() as i64;
            parts.push(match self.state.connection_kind {
                Some(netplay::ConnectionKind::Direct) => t!(lang, "lobby-latency-direct", ms = ms),
                Some(netplay::ConnectionKind::Relayed) => t!(lang, "lobby-latency-relayed", ms = ms),
                None => t!(lang, "lobby-latency", ms = ms),
            });
        }
        if parts.is_empty() {
            None
        } else {
            Some(
                text(parts.join("  ·  "))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style)
                    .into(),
            )
        }
    }

    /// The command bar's status / verdict line — the first thing in
    /// reading order, since it answers "can we fight yet". The
    /// in-flight statuses (connecting / waiting / negotiating /
    /// handshake) breathe between muted and primary-tinted so the
    /// line reads as "still working" rather than frozen — the App's
    /// subscription keeps per-frame redraws coming while one of those
    /// is on screen; terminal states (verdicts, failures) stay static.
    fn status_line(&self, status: &Status<'_>) -> Element<'a, Message> {
        let lang = self.lang;
        let pulse = crate::anim::pulse();
        let pulsing_style = move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(widgets::mix(
                widgets::muted_color(theme),
                theme.palette().primary,
                0.45 * pulse,
            )),
        };
        let in_flight =
            move |label: String| -> Element<'a, Message> { text(label).size(TEXT_BODY).style(pulsing_style).into() };
        match status {
            Status::Failed { error } => {
                // Route the netplay error tag through Fluent so each
                // failure mode can carry its own translated copy.
                // Anything we don't have a dedicated key for falls
                // back to the generic "Connection failed: <raw>".
                let label = match *error {
                    "peer-disconnected" => t!(lang, "play-status-peer-disconnected"),
                    "negotiate-expected-hello" => t!(lang, "play-status-negotiate-expected-hello"),
                    "negotiate-version-too-old" => t!(lang, "play-status-negotiate-version-too-old"),
                    "negotiate-version-too-new" => t!(lang, "play-status-negotiate-version-too-new"),
                    other if other.starts_with("negotiate-other: ") => t!(
                        lang,
                        "play-status-negotiate-failed",
                        error = other.trim_start_matches("negotiate-other: ").to_string(),
                    ),
                    _ => t!(lang, "play-status-failed", error = error.to_string()),
                };
                // The lobby is dead at this point but its chrome is
                // still on screen — cue it with an alert icon next to
                // the danger text and a faint red wash on the command
                // bar (see [`Self::command_pane`]). Deliberately
                // gentle: a loud border + oversized text read as more
                // alarming than a failed handshake warrants.
                row![
                    Icon::AlertTriangle
                        .widget()
                        .size(TEXT_BODY)
                        .style(widgets::danger_text_style),
                    text(label).size(TEXT_BODY).style(widgets::danger_text_style),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            Status::Connecting { direct: true } => in_flight(t!(lang, "play-status-direct-connecting")),
            Status::Connecting { direct: false } => in_flight(t!(lang, "play-status-connecting")),
            Status::WaitingForOpponent => in_flight(t!(lang, "play-status-waiting-opponent")),
            Status::Negotiating => in_flight(t!(lang, "play-status-negotiating")),
            Status::Handshake => in_flight(t!(lang, "lobby-handshake")),
            Status::Verdict(verdict) => {
                use netplay::compat::Verdict;
                let label = match verdict {
                    Verdict::Compatible => t!(lang, "lobby-compat-ok"),
                    Verdict::MissingGame => t!(lang, "lobby-compat-missing-game"),
                    Verdict::MissingRomOrPatch => t!(lang, "lobby-compat-missing-rom"),
                    Verdict::DifferentVersions => t!(lang, "lobby-compat-version-mismatch"),
                    Verdict::DifferentMatchTypes => t!(lang, "lobby-compat-match-mismatch"),
                };
                let style: fn(&iced::Theme) -> iced::widget::text::Style = if status.compat_ok() {
                    widgets::success_text_style
                } else {
                    widgets::danger_text_style
                };
                text(label).size(TEXT_BODY).style(style).into()
            }
        }
    }

    /// Leave-lobby (Disconnect) button — the back-out half of the
    /// action pair at the command bar's right end, just left of
    /// Ready. Disabled during the handoff window so the user can't
    /// tear down a lobby whose PvP session is already being built —
    /// clicking Disconnect there wouldn't actually cancel spawn_pvp,
    /// just leave the user confused when the match view pops up
    /// anyway.
    fn leave_button(&self) -> Element<'a, Message> {
        let inner = row![Icon::LogOut.widget(), text(t!(self.lang, "play-cancel"))]
            .spacing(8)
            .align_y(Alignment::Center);
        let mut btn = button(inner).padding(STANDARD_PADDING).style(widgets::danger_button);
        if !self.handoff_pending {
            btn = btn.on_press(Message::Disconnect);
        }
        btn.into()
    }

    /// Matchup pane: you / opponent cards with a wide gap so the
    /// diagonal cut + VS badge from `widgets::vs_splitter` paints
    /// through the middle. The splitter canvas (which also paints the
    /// red/blue half tints) is layered *under* the row.
    fn matchup_pane(&self) -> Element<'a, Message> {
        let lang = self.lang;
        let sides_row = row![
            side_card(
                lang,
                t!(lang, "play-you"),
                Some(self.state.local.as_ref().unwrap_or(&self.local_fallback)),
                self.state.local_ready,
            ),
            side_card(
                lang,
                t!(lang, "play-opponent"),
                self.state.remote.as_ref(),
                self.state.remote_ready
            ),
        ]
        .spacing(56)
        // Top-align so the YOU slot doesn't bounce upward when the
        // opponent's settings land and their card grows from a 2-line
        // placeholder to a 3-line filled card.
        .align_y(Alignment::Start);
        container(
            iced::widget::Stack::new()
                .push(container(sides_row).padding(style::PANE_PADDING).width(Fill))
                .push_under(widgets::vs_splitter()),
        )
        .width(Fill)
        .style(widgets::pane)
        .into()
    }

    /// The match settings, clustered as labeled columns — a muted
    /// caption over each control, every setting the same shape so the
    /// group reads as one coherent block between the status and the
    /// actions. (These were stacked label/control rows when the lobby
    /// filled a whole tab body; the band wants its height back, and
    /// the caption-over-control shape keeps the settings scannable in
    /// a single horizontal pass.)
    fn settings_cluster(&self) -> Element<'a, Message> {
        let lang = self.lang;
        let inert = self.inert();
        let labeled = |label: String, control: Element<'a, Message>| -> Element<'a, Message> {
            column![text(label).size(TEXT_CAPTION).style(widgets::muted_text_style), control]
                .spacing(4)
                .into()
        };

        let match_col = labeled(t!(lang, "lobby-match-type"), self.match_type_picker());

        // Frame delay slider — 2..=10 frames. Set here before the
        // match; it's this side's local frame delay (how far the
        // display trails the netcode frontier), purely local with no
        // negotiation. Each increment is one GBA frame (~16.7 ms) of
        // added display latency.
        let slider = iced::widget::slider(
            tango_pvp::battle::MIN_FRAME_DELAY..=tango_pvp::battle::MAX_FRAME_DELAY,
            self.frame_delay,
            gated(inert, Message::SetFrameDelay),
        )
        .width(Length::Fixed(160.0));
        // "Suggest" button: one-way frames + 1, clamped to the slider
        // range. Reads the median window rather than the raw `latest()`
        // shown on the connection line, so the recommendation doesn't
        // jump with a single spiky Pong. Disabled when the controls are
        // inert, and until the first Pong lands (`latest()` is `Some`)
        // so the counter has a real reading to take the median of.
        let suggest_msg = if inert || self.state.latency_counter.latest().is_none() {
            None
        } else {
            let rtt = self.state.latency_counter.median();
            Some(Message::SetFrameDelay(suggest_frame_delay(rtt)))
        };
        let suggest = widgets::icon_button_maybe(
            Icon::Wand,
            t!(lang, "lobby-frame-delay-suggest"),
            suggest_msg,
            STANDARD_PADDING,
        );
        let delay_col = labeled(
            t!(lang, "settings-netplay-frame-delay"),
            row![
                slider,
                text(format!("{}", self.frame_delay))
                    .size(TEXT_BODY)
                    .width(Length::Fixed(18.0)),
                suggest,
            ]
            .spacing(10)
            .align_y(Alignment::Center)
            .into(),
        );

        // Reveal-setup checkbox. Mirrors the legacy app's
        // `play-details-reveal-setup` checkbox — each side picks
        // independently. The peer's current flag is surfaced as a
        // colored sentence next to the checkbox (green when the peer
        // is sharing, muted/red when not / unknown) so the
        // parens-stuffed copy doesn't have to be locale-jammed into
        // the checkbox text.
        let (peer_label, peer_style): (String, fn(&iced::Theme) -> iced::widget::text::Style) =
            match self.state.remote.as_ref() {
                Some(r) if r.reveal_setup => (t!(lang, "lobby-reveal-peer-on"), widgets::success_text_style),
                Some(_) => (t!(lang, "lobby-reveal-peer-off"), widgets::danger_text_style),
                None => (t!(lang, "lobby-reveal-peer-unknown"), widgets::muted_text_style),
            };
        // Unlike the picker and slider, the checkbox does accept a
        // `None` handler, so inert gets the real disabled rendering
        // instead of the `gated` reroute.
        let toggle = if inert {
            None
        } else {
            Some(Message::SetRevealSetup as fn(bool) -> Message)
        };
        let reveal_col = labeled(
            t!(lang, "lobby-reveal-mine"),
            row![
                iced::widget::checkbox(self.state.reveal_setup)
                    .on_toggle_maybe(toggle)
                    .size(TEXT_HEADING)
                    .style(widgets::chunky_checkbox),
                text(peer_label).size(TEXT_CAPTION).style(peer_style),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
        );

        // Top-align so the captions sit on one line like a table
        // header row, whatever each control's height is.
        row![match_col, delay_col, reveal_col]
            .spacing(20)
            .align_y(Alignment::Start)
            .into()
    }

    /// Match-type pick_list — options pulled from the current local
    /// game's Game::match_types() table (mode + subtype counts),
    /// labeled with the per-game Fluent strings via
    /// game::match_type_name. Renders an empty disabled pick_list when
    /// no game is selected (Game::match_types() can't be queried until
    /// we know the game) — gives the row a stable shape so the
    /// surrounding layout doesn't jump once the user picks a game.
    fn match_type_picker(&self) -> Element<'a, Message> {
        let lang = self.lang;
        let Some(g) = self.local_game else {
            let empty: Vec<MatchTypeOption> = Vec::new();
            return pick_list(empty, None::<MatchTypeOption>, |o: MatchTypeOption| {
                Message::SetMatchType((o.mode, o.subtype))
            })
            .padding(STANDARD_PADDING)
            .style(widgets::chunky_pick_list)
            .into();
        };
        let game_impl = game::from_gamedb_entry(g);
        let mt_table = game_impl.map(|gi| gi.match_types).unwrap_or(&[]);
        let mut options = Vec::new();
        for (mode, subtype_count) in mt_table.iter().enumerate() {
            for sub in 0..*subtype_count {
                options.push(MatchTypeOption {
                    mode: mode as u8,
                    subtype: sub as u8,
                    label: game::match_type_name(lang, g.family_and_variant().0, mode as u8, sub as u8),
                });
            }
        }
        if options.is_empty() {
            return text(t!(lang, "lobby-no-match-types"))
                .style(widgets::muted_text_style)
                .into();
        }
        let selected = options
            .iter()
            .find(|o| o.mode == self.state.match_type.0 && o.subtype == self.state.match_type.1)
            .cloned();
        let on_change = gated(self.inert(), Message::SetMatchType);
        pick_list(options, selected, move |o| on_change((o.mode, o.subtype)))
            .padding(STANDARD_PADDING)
            .style(widgets::chunky_pick_list)
            .into()
    }

    /// Big single toggle: Ready → Unready → Starting…, switching label
    /// + icon + color on click. Same button, same position; clicking
    /// it always does the obvious next thing (ready up, unready, or
    /// wait for match-start). A touch chunkier than the regular CTAs
    /// in the strip, but not so big that it blows the lobby layout —
    /// the glow shadow does the work of "look at me" instead.
    fn ready_button(&self, compat_ok: bool) -> Element<'a, Message> {
        const READY_TEXT: f32 = 16.0;
        const READY_PAD: [f32; 2] = [10.0, 22.0];
        let lang = self.lang;
        let (icon, label, msg, palette): (Icon, String, Option<Message>, ReadyPalette) = if self.state.match_ready {
            // Both committed — match is spinning up. Button is purely
            // a status indicator; no click target until the session
            // actually opens.
            (
                Icon::Play,
                t!(lang, "lobby-match-starting"),
                None,
                ReadyPalette::Starting,
            )
        } else if self.state.local_ready {
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
            let can_ready = compat_ok && self.has_save;
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
        let msg = if self.failed() { None } else { msg };
        let label_widget = row![icon.widget().size(READY_TEXT), text(label).size(READY_TEXT)]
            .spacing(8)
            .align_y(Alignment::Center);
        let mut btn = button(label_widget)
            .padding(READY_PAD)
            .style(move |theme: &iced::Theme, status| ready_button_style(theme, status, palette));
        if let Some(m) = msg {
            btn = btn.on_press(m);
        }
        btn.into()
    }
}

/// Where the lobby is in its lifecycle — derived once per frame by
/// [`Lobby::status`] so the status line ([`Lobby::status_line`]) and
/// the Ready gate ([`Status::compat_ok`]) can't drift apart. While the
/// netplay attempt is still pre-Lobby it reports connection progress,
/// so the user has something to read through the handshake; once both
/// sides' settings are on hand it's the compat verdict.
enum Status<'a> {
    /// Terminal: the connection is gone. Carries the netplay error
    /// tag, sticky until the user cancels out via the leave button.
    Failed { error: &'a str },
    /// Dialing out. `direct` = a `/connect` dial straight at the peer,
    /// as opposed to the matchmaking server.
    Connecting { direct: bool },
    /// Connected to matchmaking; the peer hasn't shown up yet.
    WaitingForOpponent,
    Negotiating,
    /// In the lobby, but settings haven't round-tripped both ways yet.
    Handshake,
    /// Both sides' settings on hand — the compat verdict between them.
    Verdict(netplay::compat::Verdict),
}

impl Status<'_> {
    /// Whether compat allows readying up — only a fully-compatible
    /// verdict opens the Ready button.
    fn compat_ok(&self) -> bool {
        matches!(self, Status::Verdict(netplay::compat::Verdict::Compatible))
    }
}

/// Soft-disable helper: when the lobby is inert, reroute a control's
/// message constructor to [`Message::Noop`] so the control renders
/// unchanged but drops input — pick_list and slider don't accept a
/// `None` handler in iced 0.14, so swapping the handler is how they go
/// inert without touching layout.
fn gated<T>(inert: bool, live: fn(T) -> Message) -> fn(T) -> Message {
    if inert {
        |_| Message::Noop
    } else {
        live
    }
}

/// Compact "you / opponent" card — a 2-line waiting placeholder that
/// grows to 3 lines once that side's settings land. `ready` lights the
/// dot and tints the nickname when that side has committed.
fn side_card(
    lang: &LanguageIdentifier,
    label: String,
    settings: Option<&Settings>,
    ready: bool,
) -> Element<'static, Message> {
    let Some(settings) = settings else {
        return container(
            row![
                ready_dot(false),
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
    // Nickname is the marquee — title-sized, primary tinted when this
    // side is ready so the card lights up visibly as commitment lands.
    let nickname_style: fn(&iced::Theme) -> iced::widget::text::Style = if ready {
        |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(theme.palette().primary),
        }
    } else {
        |_theme: &iced::Theme| iced::widget::text::Style { color: None }
    };
    container(
        row![
            ready_dot(ready),
            column![
                text(label).size(TEXT_CAPTION).style(widgets::muted_text_style),
                text(settings.nickname.clone()).size(TEXT_TITLE).style(nickname_style),
                text(side_card_subline(lang, settings)).size(TEXT_CAPTION),
            ]
            .spacing(2),
        ]
        .spacing(10)
        .align_y(Alignment::Start),
    )
    .width(Length::Fill)
    .into()
}

/// 14 px dot with a soft primary-tinted glow when the side is
/// committed — reads as a "ready light" on a console panel rather than
/// a flat status pip. Padded so the dot lines up with the nickname row
/// of the column to its right — the card row is top-aligned
/// (Alignment::Start) so the dot doesn't drift when the card grows
/// from the 2-line placeholder to the 3-line populated card.
fn ready_dot(ready: bool) -> Element<'static, Message> {
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
}

/// The card's caption line: "<game name> · <patch> · <match-type>"
/// packed onto a single row so the card stays compact. Match-type is
/// meaningless without a game (no Game::match_types table to look the
/// name up against), so it's omitted then.
fn side_card_subline(lang: &LanguageIdentifier, settings: &Settings) -> String {
    let mut subline = settings
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
    if let Some(p) = settings.game_info.as_ref().and_then(|gi| gi.patch.as_ref()) {
        subline.push_str(&format!(" · {} v{}", p.name, p.version));
    }
    if let Some(gi) = settings.game_info.as_ref() {
        let mt = game::match_type_name(
            lang,
            gi.family_and_variant.0.as_str(),
            settings.match_type.0,
            settings.match_type.1,
        );
        subline.push_str(&format!(" · {mt}"));
    }
    subline
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
