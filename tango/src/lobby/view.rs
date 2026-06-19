//! The Play tab's right-hand presence sidebar — ported onto the live
//! [`super::State`] (the prototype rendered off mock `presence` data). Your own
//! identity + status sit in a footer pane; above it the roster of players to
//! challenge (incoming challenges first, then friends, then everyone else),
//! or — Telegram-style — an open player's profile sliding in over the list.
//!
//! "Friend" is purely local: giving someone a nickname (stored in
//! `config.friends`, keyed by friend code) is what makes them one, which floats
//! them above strangers and keeps them visible even while offline.

use std::collections::BTreeSet;

use iced::widget::canvas::{self, Canvas, Frame, Path, Stroke};
use iced::widget::{button, checkbox, container, mouse_area, scrollable, text, tooltip, Space, Stack};
use iced::{mouse, Alignment, Color, Element, Fill, Length, Point, Rectangle, Renderer, Theme};
use lucide_icons::Icon;
use sweeten::widget::{column, pick_list, row, text_input};
use tango_lobby::{FriendCode, MatchProposal};
use unic_langid::LanguageIdentifier;

use super::{Connection, Message, SelfStatus, State};
use crate::i18n::t;
use crate::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION};
use crate::{game, rom, widgets};

/// Fixed width of the presence sidebar — matches the prototype's roomier pane.
pub const SIDEBAR_WIDTH: f32 = 320.0;

/// Copy-feedback keys — distinct so copying your own code doesn't flash a
/// profile's code button (and vice versa) when both are on screen.
const SELF_CODE_FLASH_KEY: &str = "lobby-self-code";
const PEER_CODE_FLASH_KEY: &str = "lobby-peer-code";

/// The long-lived state the sidebar renders from — every field is borrowed from
/// the App for the frame's lifetime `'a`. Per-frame derived inputs that the
/// elements only read during construction (the incompatible-challenger set) are
/// passed separately so they don't pin the returned element's lifetime.
pub struct Ctx<'a> {
    pub lang: &'a LanguageIdentifier,
    pub state: &'a State,
    /// Local nicknames keyed by friend-code string (`config.friends`).
    pub friends: &'a std::collections::BTreeMap<String, String>,
    pub streamer_mode: bool,
    /// A game + save are loaded (else challenge / accept are disabled).
    pub can_challenge: bool,
    /// Netplay is idle (else challenge / accept are disabled — a match is
    /// already in flight).
    pub netplay_idle: bool,
    /// A direct bring-up the sidebar kicked off is in flight: `Some(true)` while
    /// hosting (waiting for a peer to dial in), `Some(false)` while dialing.
    /// Drives the direct view's "connecting" screen.
    pub direct_connecting: Option<bool>,
    pub local_game: Option<rom::GameRef>,
    /// The match type we'll propose (mirrors `netplay.lobby.match_type`).
    pub match_type: (u8, u8),
    /// Whether we'll propose a blind setup (mirrors `netplay.lobby.blind_setup`).
    pub blind_setup: bool,
}

/// Presence of a player as the roster renders it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Online,
    InMatch,
    Offline,
}

/// Which status pip to paint — Online glows like a "ready light"; in-match is
/// tinted but quiet; offline goes dark.
#[derive(Clone, Copy)]
enum Pip {
    Online,
    Match,
    Off,
}

/// One row's worth of resolved player info — folded from the live roster, the
/// incoming-challenge table, and the local friends map.
struct Player {
    code: FriendCode,
    code_str: String,
    /// Local nickname, if any (its presence ⇔ "is a friend").
    nickname: Option<String>,
    status: Status,
    /// Arrival stamp of their incoming challenge, if they've challenged us.
    incoming_seq: Option<u64>,
    incoming_label: Option<String>,
    incoming_blind: bool,
    /// Loadout label of the match they're in, while `Status::InMatch`.
    now_playing_label: Option<String>,
}

impl Player {
    fn is_friend(&self) -> bool {
        self.nickname.is_some()
    }
    fn has_incoming(&self) -> bool {
        self.incoming_seq.is_some()
    }
    /// Lowercased display key for stable alphabetical ordering within a group.
    fn sort_name(&self) -> String {
        self.nickname
            .clone()
            .unwrap_or_else(|| self.code_str.clone())
            .to_lowercase()
    }
}

// ---------- top-level layout ----------

pub fn sidebar<'a>(ctx: &Ctx<'a>, incompatible: &BTreeSet<FriendCode>) -> Element<'a, Message> {
    let now = iced::time::Instant::now();
    let lang = ctx.lang;

    // The profile takes over the list area (master→detail) while it's in the
    // tree — entering, settled, or gliding out (profile_vis still visible) —
    // otherwise the roster list shows.
    let open = ctx
        .state
        .profile_vis
        .visible(now)
        .then(|| ctx.state.open_peer)
        .flatten();
    let content: Element<'a, Message> = if ctx.state.direct_connect {
        // The direct-connect form takes over the list area (entered from the ⋮
        // menu); a back arrow returns to the roster.
        container(direct_connect_view(ctx))
            .padding(style::PANE_PADDING)
            .width(Fill)
            .height(Fill)
            .into()
    } else {
        match open {
            Some(code) => {
                let prog = ctx.state.profile_vis.progress(now);
                let profile = container(profile_panel(ctx, code, incompatible))
                    .padding(style::PANE_PADDING)
                    .width(Fill)
                    .height(Fill);
                let mut el: Element<'a, Message> = profile.into();
                if prog < 1.0 {
                    el = crate::anim::slide_in(el, prog, iced::Vector::new(48.0, 0.0));
                }
                el
            }
            None => roster_list(ctx, incompatible),
        }
    };

    let list_pane = container(content).width(Fill).height(Fill).style(widgets::pane);
    let you_pane = container(you_chip(ctx))
        .padding([10, 12])
        .width(Fill)
        .style(widgets::pane);

    let body = column![widgets::zone_title(&t!(lang, "roster-zone")), list_pane, you_pane]
        .spacing(style::PANE_GAP)
        .padding(iced::Padding {
            top: style::PANE_GAP,
            right: style::PANE_GAP,
            left: 0.0,
            bottom: style::PANE_GAP,
        })
        .width(Length::Fixed(SIDEBAR_WIDTH))
        .height(Fill);

    // At most one popover is open at a time. Each opens upward as an overlay so
    // it never shifts the layout, anchored over the bottom strip it belongs to,
    // with a sidebar-scoped scrim for click-away.
    let popover: Option<(Element<'a, Message>, Message)> = if ctx.state.status_menu_open {
        // Flush on top of the You pane, left-aligned with the avatar. YOU_REGION
        // is the You pane's footprint from the sidebar bottom (the body's bottom
        // PANE_GAP + the chip pane's ~56 px) so the card's bottom meets the
        // pane's top with no gap.
        const YOU_REGION: f32 = style::PANE_GAP + 56.0;
        let card = container(status_menu_card(ctx))
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Left)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: 0.0,
                left: style::PANE_PADDING,
                bottom: YOU_REGION,
            });
        Some((card.into(), Message::ToggleStatusMenu))
    } else if ctx.state.menu_open {
        // Flush on top of the You pane, right-aligned with the ⋮ button on the
        // user plate (mirror of the status menu, which hangs off the avatar at
        // the other end). Same YOU_REGION footprint.
        const YOU_REGION: f32 = style::PANE_GAP + 56.0;
        let card = container(overflow_menu_card(ctx))
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Bottom)
            .padding(iced::Padding {
                top: 0.0,
                right: style::PANE_PADDING,
                left: 0.0,
                bottom: YOU_REGION,
            });
        Some((card.into(), Message::ToggleMenu))
    } else {
        None
    };

    if let Some((card, dismiss)) = popover {
        let scrim = mouse_area(Space::new().width(Fill).height(Fill)).on_press(dismiss);
        Stack::new()
            .push(body)
            .push(scrim)
            .push(card)
            .width(Length::Fixed(SIDEBAR_WIDTH))
            .height(Fill)
            .into()
    } else {
        body.into()
    }
}

// ---------- roster list ----------

/// Resolve every code we know about (roster ∪ named friends ∪ challengers),
/// minus ourselves, into a [`Player`].
fn players(ctx: &Ctx) -> Vec<Player> {
    let me = ctx.state.friend_code();
    let mut codes: BTreeSet<FriendCode> = ctx.state.roster.keys().copied().collect();
    codes.extend(ctx.state.incoming.keys().copied());
    for key in ctx.friends.keys() {
        if let Ok(fc) = key.parse::<FriendCode>() {
            codes.insert(fc);
        }
    }
    codes
        .into_iter()
        .filter(|fc| Some(*fc) != me)
        .map(|code| player(ctx, code))
        .collect()
}

fn player(ctx: &Ctx, code: FriendCode) -> Player {
    let code_str = code.to_string();
    let nickname = ctx
        .friends
        .get(&code_str)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let (status, now_playing_label) = match ctx.state.roster.get(&code) {
        Some(Some(p)) => (Status::InMatch, Some(proposal_label(ctx.lang, p))),
        Some(None) => (Status::Online, None),
        None => (Status::Offline, None),
    };
    let incoming = ctx.state.incoming.get(&code);
    Player {
        code,
        code_str,
        nickname,
        status,
        incoming_seq: incoming.map(|c| c.seq),
        incoming_label: incoming.map(|c| proposal_label(ctx.lang, &c.proposal)),
        incoming_blind: incoming.map(|c| c.proposal.blind_setup).unwrap_or(false),
        now_playing_label,
    }
}

fn roster_list<'a>(ctx: &Ctx<'a>, incompatible: &BTreeSet<FriendCode>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let connected = matches!(ctx.state.connection, Connection::Connected { .. });

    // Not connected: explain why instead of an empty roster.
    if !connected {
        let label = match &ctx.state.connection {
            Connection::NoIdentity => t!(lang, "lobby-no-identity"),
            Connection::Connecting => t!(lang, "lobby-connecting"),
            _ => t!(lang, "lobby-offline"),
        };
        return column![hint_row(label)]
            .padding(iced::Padding {
                top: style::PANE_GAP,
                ..iced::Padding::ZERO
            })
            .width(Fill)
            .height(Fill)
            .into();
    }

    let all = players(ctx);
    // Two sections — Friends (anyone you've nicknamed, online or offline) and
    // Online (present strangers). Within each, pending challenges float to the
    // top (earliest first), then free players, then by name. Offline strangers
    // aren't shown.
    let mut friends: Vec<&Player> = all.iter().filter(|p| p.is_friend()).collect();
    let mut online: Vec<&Player> = all
        .iter()
        .filter(|p| !p.is_friend() && p.status != Status::Offline)
        .collect();
    let order =
        |p: &&Player| (!p.has_incoming(), p.incoming_seq.unwrap_or(u64::MAX), status_rank(p.status), p.sort_name());
    friends.sort_by_key(order);
    online.sort_by_key(order);

    // Friends keep showing while offline (so you can still challenge them — an
    // "offline" friend may just be invisible), but the header counts only the
    // ones actually present.
    let friends_online = friends.iter().filter(|p| p.status != Status::Offline).count();
    let mut list = column![].spacing(style::PANE_GAP).width(Fill);
    if !friends.is_empty() {
        list = list.push(section(
            t!(lang, "roster-friends", count = friends_online as i64),
            &friends,
            ctx,
            incompatible,
        ));
    }
    if !online.is_empty() {
        list = list.push(section(
            t!(lang, "roster-online", count = online.len() as i64),
            &online,
            ctx,
            incompatible,
        ));
    }
    if friends.is_empty() && online.is_empty() {
        list = list.push(hint_row(t!(lang, "roster-empty")));
    }
    let body: Element<'a, Message> = scrollable(list)
        .height(Fill)
        .width(Fill)
        .style(widgets::chunky_scrollable)
        .into();

    // The look-up-by-code bar pins to the bottom; it keeps the pane inset the
    // flush rows drop. A small top gap keeps the first row off the edge.
    column![
        container(body).width(Fill).height(Fill),
        container(add_contact_bar(ctx)).padding(iced::Padding {
            top: 0.0,
            right: style::PANE_PADDING,
            left: style::PANE_PADDING,
            bottom: style::PANE_PADDING,
        }),
    ]
    .spacing(8)
    .padding(iced::Padding {
        top: style::PANE_GAP,
        ..iced::Padding::ZERO
    })
    .width(Fill)
    .height(Fill)
    .into()
}

/// A titled section of flush rows: highlighted accept/decline rows for pending
/// challengers (floated to the top by the caller's sort), plain rows otherwise.
fn section<'a>(
    label: String,
    group: &[&Player],
    ctx: &Ctx<'a>,
    incompatible: &BTreeSet<FriendCode>,
) -> Element<'a, Message> {
    let mut flush = column![].width(Fill);
    for (zebra, p) in group.iter().copied().enumerate() {
        let row_el = if p.has_incoming() {
            incoming_row(ctx, p, incompatible)
        } else {
            roster_row(p, zebra)
        };
        flush = flush.push(row_el);
    }
    column![section_label(label)].spacing(6).push(flush).into()
}

/// One roster entry — the whole row is a list button opening that player's
/// profile. Avatar + name (nickname, or code when unnamed), with what they're
/// playing beneath while in a match.
fn roster_row<'a>(p: &Player, zebra: usize) -> Element<'a, Message> {
    button(identity_lines(p))
        .padding([6, 12])
        .width(Fill)
        .style(widgets::list_item(false, zebra))
        .on_press(Message::OpenPeer(p.code))
        .into()
}

/// Name (nickname, or the monospaced code when unnamed) stacked over an
/// optional caption line, in a fixed-height slot. Every roster row — plain,
/// in-match, and incoming-challenge — routes through this one fixed height, so
/// the second line (or a challenge's accept/decline pair) appearing never
/// reflows the row. Keep it at this height; both row builders rely on it.
fn name_slot<'a>(p: &Player, secondary: Option<Element<'a, Message>>) -> Element<'a, Message> {
    const SLOT_H: f32 = 32.0;
    let primary = match &p.nickname {
        Some(n) => text(n.clone()).size(TEXT_BODY),
        None => text(p.code_str.clone()).size(TEXT_BODY).font(style::MONOSPACE_FONT),
    };
    let mut info = column![primary].spacing(1).width(Fill);
    if let Some(sub) = secondary {
        info = info.push(sub);
    }
    container(info)
        .height(Length::Fixed(SLOT_H))
        .width(Fill)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}

/// A player's row identity: avatar, then the name slot — with what they're
/// playing beneath the name while they're in a match.
fn identity_lines<'a>(p: &Player) -> Element<'a, Message> {
    let secondary = match (p.status, &p.now_playing_label) {
        (Status::InMatch, Some(playing)) => Some(clipped_line(playing.clone())),
        _ => None,
    };
    row![avatar(&p.code_str, pip_of(p.status), 26.0), name_slot(p, secondary)]
        .spacing(8)
        .width(Fill)
        .align_y(Alignment::Center)
        .into()
}

/// One incoming-challenge row — clickable to open the challenger's profile,
/// with reject (✕) / accept (✓) on the right. Accept is blocked on a netplay
/// mismatch (the profile spells out the fix).
fn incoming_row<'a>(ctx: &Ctx<'a>, p: &Player, incompatible: &BTreeSet<FriendCode>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let mismatch = incompatible.contains(&p.code);
    let can_accept = !mismatch && ctx.can_challenge && ctx.netplay_idle;
    let reject = widgets::icon_button_styled(
        Icon::X,
        t!(lang, "roster-decline"),
        Some(Message::DeclineIncoming(p.code)),
        STANDARD_PADDING,
        widgets::neutral,
    );
    let accept = widgets::icon_button_styled(
        Icon::Check,
        if mismatch {
            t!(lang, "roster-mismatch")
        } else {
            t!(lang, "roster-accept")
        },
        can_accept.then_some(Message::AcceptIncoming(p.code)),
        STANDARD_PADDING,
        widgets::primary_button,
    );
    let label = p.incoming_label.clone().unwrap_or_default();
    let info = name_slot(p, Some(challenge_line(lang, label, p.incoming_blind)));

    button(
        row![avatar(&p.code_str, pip_of(p.status), 26.0), info, reject, accept]
            .spacing(8)
            .width(Fill)
            .align_y(Alignment::Center),
    )
    .padding([6, 12])
    .width(Fill)
    .style(challenge_highlight)
    .on_press(Message::OpenPeer(p.code))
    .into()
}

/// Highlight plate for an incoming-challenge row — a strong wash in the Legacy
/// Collection's selection yellow (the same register the replay list paints a
/// picked row), so a pending challenge stands out from the plain rows. Just the
/// row wash: a plain rounded plate, no tech-frame border.
fn challenge_highlight(theme: &iced::Theme, status: iced::widget::button::Status) -> iced::widget::button::Style {
    let sel = crate::theme::SELECT_YELLOW;
    let bg = theme.palette().background;
    let wash = match status {
        iced::widget::button::Status::Hovered | iced::widget::button::Status::Pressed => 0.50,
        _ => 0.38,
    };
    iced::widget::button::Style {
        background: Some(iced::Background::Color(widgets::mix(bg, sel, wash))),
        text_color: theme.palette().text,
        border: iced::Border {
            radius: 4.0.into(),
            width: 0.0,
            color: iced::Color::TRANSPARENT,
        },
        ..Default::default()
    }
}

/// The "find by friend code" bar, pinned at the bottom: a code input + add
/// button. Submitting opens that code's profile so you can nickname it.
fn add_contact_bar<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    // Only a well-formed friend code (Crockford + valid check digit) can be
    // looked up — the button stays disabled until the draft parses.
    let valid = ctx.state.add_draft.trim().parse::<FriendCode>().is_ok();
    row![
        text_input(&t!(lang, "roster-lookup-placeholder"), &ctx.state.add_draft)
            .on_input(Message::AddDraftChanged)
            .on_submit(Message::ConfirmAddContact)
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::chunky_text_input),
        widgets::icon_button_styled(
            Icon::UserRoundSearch,
            t!(lang, "roster-lookup"),
            valid.then_some(Message::ConfirmAddContact),
            STANDARD_PADDING,
            widgets::primary_button,
        ),
    ]
    .spacing(6)
    .width(Fill)
    .align_y(Alignment::Center)
    .into()
}

/// The overflow (⋮) popover card hanging off the user plate: a floating plate of
/// menu items. Today just "Direct connect", disabled while an outgoing challenge
/// is pending (you'd be juggling two bring-ups). Sized like the status menu card.
fn overflow_menu_card<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    // An outstanding outgoing challenge owns the netplay bring-up; don't let a
    // direct connect race it.
    let busy = ctx.state.outgoing_peer().is_some();
    let mut item = button(
        row![
            Icon::Cable.widget().size(TEXT_BODY),
            text(t!(lang, "roster-direct-connect")).size(TEXT_BODY),
        ]
        .spacing(8)
        .width(Fill)
        .align_y(Alignment::Center),
    )
    .padding([6, 8])
    .width(Fill)
    .style(widgets::flat);
    if !busy {
        item = item.on_press(Message::OpenDirectConnect);
    }
    container(column![item].width(Fill))
        .width(Length::Fixed(190.0))
        .padding(6)
        .style(floating_card)
        .into()
}

/// The direct-connect view that takes over the list area: host a link on the
/// default port, or dial a peer by address — the signaling-free path that needs
/// no lobby (see [`crate::net::direct_rtc`]). Both require a game + save loaded
/// (same as a challenge), so the actions disable until ready.
fn direct_connect_view<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;

    // Bring-up in flight: a waiting screen with Cancel (the only way out — a
    // plain "back" would orphan the connection). Host waits for a dialer;
    // the dialer is actively connecting.
    if let Some(waiting) = ctx.direct_connecting {
        let title = text(t!(lang, "direct-title")).size(style::TEXT_TITLE);
        let status = text(if waiting {
            t!(lang, "direct-waiting")
        } else {
            t!(lang, "direct-dialing")
        })
        .size(TEXT_BODY);
        let cancel = widgets::labeled_icon_button(
            Icon::X,
            t!(lang, "direct-cancel"),
            Message::CancelDirect,
            STANDARD_PADDING,
            widgets::neutral,
        );
        return column![title, status, cancel].spacing(16).width(Fill).into();
    }

    // Idle (or a just-failed attempt): the host/join form. Starting a connect
    // resets any prior failure, so the only gate is a loaded game + save.
    let ready = ctx.can_challenge;

    let back = widgets::icon_button_styled(
        Icon::ArrowLeft,
        t!(lang, "roster-back"),
        Some(Message::CloseDirectConnect),
        [2.0, 6.0],
        widgets::flat,
    );
    let header = row![back, text(t!(lang, "direct-title")).size(style::TEXT_TITLE)]
        .spacing(8)
        .align_y(Alignment::Center);

    let explainer = text(t!(lang, "direct-explainer"))
        .size(TEXT_CAPTION)
        .style(widgets::muted_text_style);

    // Host: listen on the default UDP port and wait for a dialer.
    let host_btn = widgets::labeled_icon_button_maybe(
        Icon::RadioTower,
        t!(lang, "direct-host"),
        ready.then_some(Message::DirectHost),
        STANDARD_PADDING,
        widgets::primary_button,
    );
    let host_hint = text(t!(lang, "direct-host-hint", port = crate::net::DEFAULT_LOCAL_PORT as i64))
        .size(TEXT_CAPTION)
        .style(widgets::muted_text_style);
    let host = column![host_btn, host_hint].spacing(6).width(Fill);

    // Join: dial a typed address (`host` or `host:port`).
    let addr_valid = !ctx.state.direct_addr.trim().is_empty();
    let join_msg = (ready && addr_valid).then_some(Message::DirectJoin);
    let addr_input = text_input(&t!(lang, "direct-addr-placeholder"), &ctx.state.direct_addr)
        .on_input(Message::DirectAddrChanged)
        .on_submit(Message::DirectJoin)
        .padding(STANDARD_PADDING)
        .width(Fill)
        .style(widgets::chunky_text_input);
    let join_btn = widgets::icon_button_styled(
        Icon::Cable,
        t!(lang, "direct-join"),
        join_msg,
        STANDARD_PADDING,
        widgets::primary_button,
    );
    let join = column![
        text(t!(lang, "direct-join-label")).size(TEXT_CAPTION).style(widgets::muted_text_style),
        row![addr_input, join_btn].spacing(6).width(Fill).align_y(Alignment::Center),
    ]
    .spacing(6)
    .width(Fill);

    let mut col = column![header, explainer, host, join].spacing(16).width(Fill);
    // A peer reached us but their settings didn't match — show the localized
    // reason (matched off the Verdict here, at the point of display).
    if let Some(verdict) = &ctx.state.direct_error {
        col = col.push(
            text(compat_verdict_text(lang, verdict))
                .size(TEXT_CAPTION)
                .style(widgets::danger_text_style),
        );
    }
    if !ready {
        col = col.push(
            text(t!(lang, "direct-need-game"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        );
    }
    col.into()
}

/// The localized reason a direct peer was rejected, matched off the compat
/// [`Verdict`](crate::netplay::compat::Verdict) at the point of display so it
/// reuses the lobby pane's compatibility strings.
fn compat_verdict_text(lang: &LanguageIdentifier, verdict: &crate::netplay::compat::Verdict) -> String {
    use crate::netplay::compat::Verdict;
    match verdict {
        Verdict::Compatible => t!(lang, "lobby-compat-ok"),
        Verdict::MissingGame => t!(lang, "lobby-compat-missing-game"),
        Verdict::DifferentVersions => t!(lang, "lobby-compat-version-mismatch"),
        Verdict::DifferentMatchTypes => t!(lang, "lobby-compat-match-mismatch"),
    }
}

// ---------- profile (master→detail) ----------

fn profile_panel<'a>(ctx: &Ctx<'a>, code: FriendCode, incompatible: &BTreeSet<FriendCode>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let p = player(ctx, code);
    // Looking yourself up: it's not a peer you can friend or challenge, so the
    // card just shows "You" like the footer chip. Your pip comes from
    // self-status — you're never in your own roster, so `player` reads offline.
    let is_me = ctx.state.friend_code() == Some(code);
    let challenging = ctx.state.outgoing_peer() == Some(code);

    let back = widgets::icon_button_styled(
        Icon::ArrowLeft,
        t!(lang, "roster-back"),
        (!challenging).then_some(Message::ClosePeer),
        [2.0, 6.0],
        widgets::flat,
    );

    let pip = if is_me {
        self_pip(ctx.state.self_status())
    } else {
        pip_of(p.status)
    };
    // Identity: avatar + (for others) a rename field that *is* the name, with an
    // X to clear it (un-friend); for yourself, a static "You" label. The field
    // value borrows straight from `ctx.friends` so it lives as long as the element.
    let id_row = if is_me {
        row![
            avatar(&p.code_str, pip, 40.0),
            text(t!(lang, "roster-you")).size(TEXT_BODY).width(Fill),
        ]
        .spacing(10)
        .align_y(Alignment::Center)
    } else {
        let nick_value: &'a str = ctx.friends.get(&p.code_str).map(String::as_str).unwrap_or("");
        let code_for_rename = p.code_str.clone();
        let rename = text_input(&t!(lang, "roster-name-placeholder"), nick_value)
            .on_input(move |name| Message::SetNickname {
                code: code_for_rename.clone(),
                name,
            })
            .padding(STANDARD_PADDING)
            .width(Fill)
            .style(widgets::chunky_text_input);
        let mut id_row = row![avatar(&p.code_str, pip, 40.0), rename]
            .spacing(10)
            .align_y(Alignment::Center);
        if p.is_friend() {
            let code_clear = p.code_str.clone();
            id_row = id_row.push(widgets::icon_button_styled(
                Icon::X,
                t!(lang, "roster-remove-friend"),
                Some(Message::SetNickname {
                    code: code_clear,
                    name: String::new(),
                }),
                STANDARD_PADDING,
                widgets::flat,
            ));
        }
        id_row
    };

    // Mask your own code on stream, like the footer chip does (copy still grabs
    // the real one).
    let shown_code = if is_me && ctx.streamer_mode {
        "••••••".to_string()
    } else {
        p.code_str.clone()
    };
    let code_line = row![
        text(shown_code)
            .size(TEXT_CAPTION)
            .font(style::MONOSPACE_FONT)
            .style(widgets::muted_text_style),
        widgets::copy_icon_button(
            PEER_CODE_FLASH_KEY,
            Icon::ClipboardCopy,
            TEXT_CAPTION,
            t!(lang, "save-copy"),
            t!(lang, "copied"),
            Some(Message::CopyText {
                text: p.code_str.clone(),
                flash: PEER_CODE_FLASH_KEY,
            }),
            [2.0, 5.0],
        ),
    ]
    .spacing(6)
    .width(Fill)
    .align_y(Alignment::Center);

    let col = column![row![back].width(Fill), id_row, code_line]
        .spacing(8)
        .width(Fill)
        .height(Fill);
    let col = col.push(Space::new().height(Fill));

    // Your own card stops here: nothing to accept, send, or cancel.
    if is_me {
        return col.into();
    }

    // The challenge subsection, pinned to the bottom — its shape depends on the
    // relationship: they've challenged you → accept/decline; you've challenged
    // them → waiting + cancel; otherwise → set up + Challenge.
    let mut sub = column![].spacing(8).width(Fill);
    if p.has_incoming() {
        let mismatch = incompatible.contains(&code);
        sub = sub.push(section_label(t!(lang, "roster-incoming")));
        sub = sub.push(challenge_line(
            lang,
            p.incoming_label.clone().unwrap_or_default(),
            p.incoming_blind,
        ));
        if mismatch {
            sub = sub.push(
                row![
                    Icon::AlertTriangle
                        .widget()
                        .size(TEXT_CAPTION)
                        .style(widgets::danger_text_style),
                    text(t!(lang, "roster-mismatch"))
                        .size(TEXT_CAPTION)
                        .style(widgets::danger_text_style),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        sub = sub.push(
            row![
                widgets::labeled_icon_button(
                    Icon::X,
                    t!(lang, "roster-decline"),
                    Message::DeclineIncoming(code),
                    STANDARD_PADDING,
                    widgets::neutral,
                ),
                widgets::labeled_icon_button_maybe(
                    Icon::Check,
                    t!(lang, "roster-accept"),
                    (!mismatch && ctx.can_challenge && ctx.netplay_idle).then_some(Message::AcceptIncoming(code)),
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
            ]
            .spacing(8),
        );
    } else if challenging {
        sub = sub.push(section_label(t!(lang, "roster-challenge-sent")));
        sub = sub.push(
            text(challenge_summary(ctx))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        );
        sub = sub.push(
            row![
                Icon::Swords
                    .widget()
                    .size(TEXT_CAPTION)
                    .style(widgets::primary_text_style),
                text(t!(lang, "roster-waiting"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        );
        sub = sub.push(widgets::labeled_icon_button(
            Icon::X,
            t!(lang, "roster-cancel-challenge"),
            Message::CancelChallenge,
            STANDARD_PADDING,
            widgets::danger_button,
        ));
    } else {
        sub = sub.push(challenge_settings(ctx));
        // Allow challenging anyone who isn't already in a match — including
        // someone who reads as offline, since an invisible user shows that way
        // but can still receive (and answer) a challenge.
        let can = p.status != Status::InMatch && ctx.netplay_idle && ctx.can_challenge;
        sub = sub.push(widgets::labeled_icon_button_maybe(
            Icon::Swords,
            t!(lang, "roster-challenge"),
            can.then_some(Message::IssueChallenge(code)),
            STANDARD_PADDING,
            widgets::primary_button,
        ));
    }
    col.push(container(sub).padding([10, 12]).width(Fill).style(widgets::panel))
        .into()
}

/// The match-settings controls shown before proposing a challenge: a match-type
/// picker + the blind-setup toggle. (The game/patch is the active loadout;
/// frame delay lives in Settings.)
fn challenge_settings<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let blind = checkbox(ctx.blind_setup)
        .label(t!(lang, "lobby-blind-mine"))
        .on_toggle(Message::SetBlindSetup)
        .size(TEXT_BODY)
        .style(widgets::chunky_checkbox);
    column![
        text(t!(lang, "lobby-match-type"))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style),
        match_type_picker(ctx),
        blind,
    ]
    .spacing(6)
    .width(Fill)
    .into()
}

/// A static one-line summary of what we proposed, shown while waiting.
fn challenge_summary(ctx: &Ctx) -> String {
    let lang = ctx.lang;
    let mt = ctx
        .local_game
        .map(|g| game::match_type_name(lang, g.family_and_variant().0, ctx.match_type.0, ctx.match_type.1))
        .unwrap_or_default();
    if ctx.blind_setup {
        format!("{mt} · {}", t!(lang, "lobby-blind-mine"))
    } else {
        mt
    }
}

/// Match-type pick_list off the local game's `match_types` table — an empty
/// disabled picker when no game is selected, so the row keeps a stable shape.
fn match_type_picker<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let Some(g) = ctx.local_game else {
        let empty: Vec<MatchTypeOption> = Vec::new();
        return pick_list(empty, None::<MatchTypeOption>, |o: MatchTypeOption| {
            Message::SetMatchType((o.mode, o.subtype))
        })
        .padding(STANDARD_PADDING)
        .style(widgets::chunky_pick_list)
        .into();
    };
    let mt_table = game::from_gamedb_entry(g).map(|gi| gi.match_types).unwrap_or(&[]);
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
        .find(|o| o.mode == ctx.match_type.0 && o.subtype == ctx.match_type.1)
        .cloned();
    pick_list(options, selected, |o: MatchTypeOption| {
        Message::SetMatchType((o.mode, o.subtype))
    })
    .padding(STANDARD_PADDING)
    .style(widgets::chunky_pick_list)
    .into()
}

// ---------- "You" footer + status menu ----------

/// Your identity in its own footer pane — your emblem (with your presence dot)
/// beside **You** over your friend code (with a copy button). The avatar is the
/// button that drops the status menu upward.
fn you_chip<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let status = ctx.state.self_status();
    let code = ctx.state.friend_code().map(|c| c.to_string());

    let avatar_el: Element<'a, Message> = match &code {
        Some(c) => avatar(c, self_pip(status), 26.0),
        // No code yet (connecting / offline / no identity): a neutral emblem,
        // centered in the same 26×26 footprint as the identicon so the row
        // doesn't shift when a code appears/disappears. (The glyph's line box is
        // taller than the disc, so size it down and center rather than fill.)
        None => container(Icon::CircleUserRound.widget().size(22.0))
            .width(Length::Fixed(26.0))
            .height(Length::Fixed(26.0))
            .align_x(iced::alignment::Horizontal::Center)
            .align_y(iced::alignment::Vertical::Center)
            .into(),
    };
    let avatar_btn = tooltip(
        button(avatar_el)
            .padding(0)
            .style(widgets::flat)
            .on_press(Message::ToggleStatusMenu),
        widgets::tooltip_bubble(t!(lang, "roster-set-status")),
        tooltip::Position::Top,
    )
    .gap(4);

    let code_line: Element<'a, Message> = match &code {
        Some(c) => {
            let shown = if ctx.streamer_mode {
                "••••••".to_string()
            } else {
                c.clone()
            };
            row![
                text(shown)
                    .size(TEXT_CAPTION)
                    .font(style::MONOSPACE_FONT)
                    .style(widgets::muted_text_style),
                widgets::copy_icon_button(
                    SELF_CODE_FLASH_KEY,
                    Icon::ClipboardCopy,
                    TEXT_CAPTION,
                    t!(lang, "save-copy"),
                    t!(lang, "copied"),
                    Some(Message::CopyText {
                        text: c.clone(),
                        flash: SELF_CODE_FLASH_KEY,
                    }),
                    [2.0, 5.0],
                ),
            ]
            .spacing(6)
            .align_y(Alignment::Center)
            .into()
        }
        None => text(self_status_word(lang, status))
            .size(TEXT_CAPTION)
            .style(widgets::muted_text_style)
            .into(),
    };

    // Pin the second line to a fixed-height slot: the connected line carries a
    // copy button (taller) and the offline line is plain text, so without this
    // the whole user bar would jump height when you go offline.
    const YOU_LINE_H: f32 = 22.0;
    let code_slot = container(code_line)
        .height(Length::Fixed(YOU_LINE_H))
        .align_y(iced::alignment::Vertical::Center);
    let info = column![text(t!(lang, "roster-you")).size(TEXT_BODY), code_slot]
        .spacing(1)
        .width(Fill);
    // Overflow (⋮) on the user plate: opens the menu popover upward (see
    // `sidebar`). Top-positioned tooltip so it doesn't clip past the window edge.
    let menu_btn = tooltip(
        button(Icon::EllipsisVertical.widget().size(TEXT_BODY))
            .padding([4, 6])
            .style(widgets::flat)
            .on_press(Message::ToggleMenu),
        widgets::tooltip_bubble(t!(lang, "roster-menu")),
        tooltip::Position::Top,
    )
    .gap(4);
    row![avatar_btn, info, menu_btn]
        .spacing(8)
        .align_y(Alignment::Center)
        .into()
}

fn status_menu_card<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    container(status_menu(ctx))
        .width(Length::Fixed(190.0))
        .padding(6)
        .style(floating_card)
        .into()
}

/// A plain popover menu — one flat row per status (pip + label), no zebra and
/// no selected-row plate; the current status is the one whose pip is lit on the
/// You chip's avatar.
fn status_menu<'a>(ctx: &Ctx<'a>) -> Element<'a, Message> {
    let lang = ctx.lang;
    let row_btn = |status: SelfStatus, label: String| -> Element<'a, Message> {
        button(
            row![status_dot(self_pip(status), 10.0), text(label).size(TEXT_BODY)]
                .spacing(8)
                .width(Fill)
                .align_y(Alignment::Center),
        )
        .padding([6, 8])
        .width(Fill)
        .style(widgets::flat)
        .on_press(Message::SetSelfStatus(status))
        .into()
    };
    column![
        row_btn(SelfStatus::Online, t!(lang, "roster-self-online")),
        row_btn(SelfStatus::Invisible, t!(lang, "roster-self-invisible")),
        row_btn(SelfStatus::Offline, t!(lang, "roster-self-offline")),
    ]
    .spacing(2)
    .width(Fill)
    .into()
}

fn self_status_word(lang: &LanguageIdentifier, status: SelfStatus) -> String {
    match status {
        SelfStatus::Online => t!(lang, "roster-self-online"),
        SelfStatus::Invisible => t!(lang, "roster-self-invisible"),
        SelfStatus::Offline => t!(lang, "roster-self-offline"),
    }
}

// ---------- shared bits: labels, avatars, identicon ----------

/// Compact "<game> · <patch> · <match-type>" label for a proposal.
fn proposal_label(lang: &LanguageIdentifier, p: &MatchProposal) -> String {
    let Some(gi) = p.game_info.as_ref() else {
        return t!(lang, "lobby-no-game");
    };
    let mut s = game::family_str(&gi.family, lang, "short").unwrap_or_else(|| gi.family.clone());
    if let Some(patch) = gi.patch.as_ref() {
        s.push_str(&format!(" · {} v{}", patch.name, patch.version));
    }
    if let Some(mt) = p.match_type.as_ref() {
        let name = game::match_type_name(lang, &gi.family, mt.mode as u8, mt.subtype as u8);
        s.push_str(&format!(" · {name}"));
    }
    s
}

/// A "loadout · match type" line with an optional red blind eye on the right,
/// truncating to a tooltip when it's too long for the sidebar.
fn challenge_line<'a>(lang: &LanguageIdentifier, label: String, blind: bool) -> Element<'a, Message> {
    let line = clipped_line(label);
    if !blind {
        return line;
    }
    let eye = tooltip(
        Icon::EyeOff
            .widget()
            .size(TEXT_CAPTION)
            .style(widgets::danger_text_style),
        widgets::tooltip_bubble(t!(lang, "lobby-blind-peer-on")),
        tooltip::Position::Top,
    )
    .gap(4);
    // The line fills, pinning the blind eye to the right edge.
    row![line, eye].spacing(6).align_y(Alignment::Center).into()
}

/// One caption line that never overflows: measured against the width it's
/// actually allotted (via `responsive`) and truncated to a real ellipsis when
/// it won't fit, with a full-text tooltip. The width comes from the layout, not
/// a per-character guess, so the "…" lands wherever the text genuinely runs out.
fn clipped_line<'a>(label: String) -> Element<'a, Message> {
    let body = iced::widget::responsive(move |size| {
        let (shown, long) = ellipsize_to(&label, size.width);
        let line: Element<'_, Message> = container(
            text(shown)
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style)
                .wrapping(iced::widget::text::Wrapping::None),
        )
        .width(Fill)
        .clip(true)
        .into();
        if long {
            tooltip(line, widgets::tooltip_bubble(label.clone()), tooltip::Position::Top)
                .gap(4)
                .into()
        } else {
            line
        }
    });
    // `responsive` fills its allotment in both axes; pin the height to one
    // caption line so it doesn't stretch the row.
    container(body)
        .width(Fill)
        .height(Length::Fixed((TEXT_CAPTION * 1.3).ceil()))
        .into()
}

/// Logical width a caption-sized run of `s` occupies, measured by the real
/// shaping engine (the global font system) so truncation matches the rendered
/// glyphs rather than assuming a fixed per-character width.
///
/// Crucially this measures in `DEFAULT_FONT`, the same font the caption renders
/// in: an unstyled `text(..)` falls back to the renderer's default font, not to
/// `Font::default()` (a bare `SansSerif` that resolves to a different system
/// face with different metrics) — measuring in the latter under/over-shoots the
/// real width and the ellipsis lands in the wrong place.
fn caption_width(s: &str) -> f32 {
    use iced::advanced::text::Paragraph as _;
    let para = iced_graphics::text::Paragraph::with_text(iced::advanced::text::Text {
        content: s,
        bounds: iced::Size::INFINITE,
        size: iced::Pixels(TEXT_CAPTION),
        line_height: iced::advanced::text::LineHeight::default(),
        font: style::DEFAULT_FONT,
        align_x: iced::advanced::text::Alignment::Default,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::default(),
        wrapping: iced::advanced::text::Wrapping::None,
    });
    para.min_width()
}

/// Largest character-boundary prefix of `label` whose shaped width plus an
/// ellipsis fits within `avail` px, paired with whether it had to truncate.
/// Binary search over the measured width — a fixed character budget is wrong
/// for proportional fonts (an `m` is far wider than an `i`).
fn ellipsize_to(label: &str, avail: f32) -> (String, bool) {
    if avail <= 0.0 || caption_width(label) <= avail {
        return (label.to_string(), false);
    }
    let chars: Vec<char> = label.chars().collect();
    // `lo` = chars that fit with the ellipsis appended; `hi` = upper bound.
    let (mut lo, mut hi) = (0usize, chars.len());
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if caption_width(&candidate) <= avail {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    (chars[..lo].iter().collect::<String>() + "…", true)
}

fn section_label<'a>(label: String) -> Element<'a, Message> {
    // Left padding matches the rows' (`[6, 12]`) so the header lines up with the
    // avatars below it rather than sitting a few px to their left.
    container(text(label).size(TEXT_CAPTION).style(widgets::muted_text_style))
        .padding([2, 12])
        .into()
}

fn hint_row<'a>(message: String) -> Element<'a, Message> {
    container(text(message).size(TEXT_CAPTION).style(widgets::muted_text_style))
        .padding([8, style::PANE_PADDING as u16])
        .into()
}

fn status_rank(status: Status) -> u8 {
    match status {
        Status::Online => 0,
        Status::InMatch => 1,
        Status::Offline => 2,
    }
}

fn pip_of(status: Status) -> Pip {
    match status {
        Status::Online => Pip::Online,
        Status::InMatch => Pip::Match,
        Status::Offline => Pip::Off,
    }
}

fn self_pip(status: SelfStatus) -> Pip {
    match status {
        SelfStatus::Online => Pip::Online,
        SelfStatus::Invisible | SelfStatus::Offline => Pip::Off,
    }
}

/// Lifted-card chrome — plate fill, primary border, drop shadow — for the
/// floating status menu.
fn floating_card(theme: &iced::Theme) -> iced::widget::container::Style {
    // Plain popover: solid plate fill, a neutral hairline, square-ish corners,
    // and just a whisper of shadow to lift it off the roster — no accent border
    // or heavy drop shadow.
    iced::widget::container::Style {
        background: Some(iced::Background::Color(widgets::plate_color(theme))),
        border: iced::Border {
            color: theme.extended_palette().background.strong.color,
            width: 1.0,
            radius: 4.0.into(),
        },
        shadow: iced::Shadow {
            color: iced::Color {
                a: 0.2,
                ..iced::Color::BLACK
            },
            offset: iced::Vector::new(0.0, 2.0),
            blur_radius: 6.0,
        },
        ..Default::default()
    }
}

/// An identity's avatar: its [`Identicon`] emblem with a presence dot on a
/// little round plate in the bottom-right corner.
fn avatar<'a>(code: &str, pip: Pip, size: f32) -> Element<'a, Message> {
    let face = identicon(code, size);
    let badge = container(status_dot(pip, (size * 0.28).max(7.0)))
        .padding((size * 0.05).max(1.0))
        .style(|t: &iced::Theme| iced::widget::container::Style {
            background: Some(iced::Background::Color(widgets::plate_color(t))),
            border: iced::Border {
                radius: 999.0.into(),
                width: 0.0,
                color: iced::Color::TRANSPARENT,
            },
            ..Default::default()
        });
    let overlay = container(badge)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Bottom);
    Stack::new()
        .push(face)
        .push(overlay)
        .width(Length::Fixed(size))
        .height(Length::Fixed(size))
        .into()
}

/// A deterministic, Battle-Network-flavored identicon for a friend code: a
/// glowing Navi-emblem disc — bright core in a neon hoop, with concentric rings
/// of two-tone neon nodes laid out radially and mirror-symmetric. Same code →
/// same emblem. Drawn on a [`Canvas`] so it can be a true circle.
fn identicon<'a>(code: &str, size: f32) -> Element<'a, Message> {
    Canvas::new(Identicon {
        hash: identicon_hash(code),
    })
    .width(Length::Fixed(size))
    .height(Length::Fixed(size))
    .into()
}

struct Identicon {
    hash: u64,
}

impl<M> canvas::Program<M> for Identicon {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let s = bounds.width.min(bounds.height);
        let center = Point::new(s / 2.0, s / 2.0);
        let r = s * 0.47;
        let ring_w = (s * 0.08).max(1.5);
        let inner = r - ring_w;
        let h = self.hash;
        let hue = (h >> 56) as f32 / 256.0 * 360.0;
        let disc = hsl_to_rgb(hue, 0.6, 0.15);
        let neon = hsl_to_rgb((hue + 8.0) % 360.0, 0.9, 0.62);
        let neon2 = hsl_to_rgb((hue + 42.0) % 360.0, 0.92, 0.66);

        frame.fill(&Path::circle(center, r), disc);
        frame.fill(&Path::circle(center, inner * 0.17), neon);

        let rings: [(usize, f32, f32, u32, u32); 2] = [(6, 0.42, 0.15, 0, 5), (8, 0.70, 0.13, 10, 15)];
        for (k, ring_r, node_r, pat_base, col_base) in rings {
            for i in 0..k {
                // Reflect the right half onto the left so the emblem reads
                // balanced; each mirrored pair shares a bit.
                let g = i.min((k - i) % k) as u32;
                if (h >> (pat_base + g)) & 1 == 0 {
                    continue;
                }
                let ang = (-90.0 + i as f32 * 360.0 / k as f32).to_radians();
                let p = Point::new(
                    center.x + inner * ring_r * ang.cos(),
                    center.y + inner * ring_r * ang.sin(),
                );
                let color = if (h >> (col_base + g)) & 1 == 0 { neon } else { neon2 };
                frame.fill(&Path::circle(p, inner * node_r), color);
            }
        }

        frame.stroke(
            &Path::circle(center, r - ring_w / 2.0),
            Stroke::default()
                .with_color(Color { a: 0.3, ..neon2 })
                .with_width(ring_w * 2.0),
        );
        frame.stroke(
            &Path::circle(center, r - ring_w / 2.0),
            Stroke::default().with_color(neon).with_width(ring_w),
        );

        vec![frame.into_geometry()]
    }
}

/// FNV-1a 64-bit — a tiny, fully deterministic string hash (std's hasher isn't
/// guaranteed stable, and this is the only thing we need it for).
fn identicon_hash(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// HSL → RGB for the identicon hue. `h` in degrees [0,360), `s`/`l` in [0,1].
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> iced::Color {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r, g, b) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    iced::Color::from_rgb(r + m, g + m, b + m)
}

/// A rounded status pip in the [`Pip`] color, with a soft glow for online — the
/// same "ready light" register the matchup cards use.
fn status_dot<'a>(pip: Pip, size: f32) -> Element<'a, Message> {
    container(
        container(Space::new().width(Length::Fixed(size)).height(Length::Fixed(size))).style(
            move |theme: &iced::Theme| {
                let bg = match pip {
                    Pip::Online => theme.palette().primary,
                    Pip::Match => theme.extended_palette().danger.strong.color,
                    Pip::Off => widgets::muted_color(theme),
                };
                let glow = matches!(pip, Pip::Online);
                iced::widget::container::Style {
                    background: Some(iced::Background::Color(bg)),
                    border: iced::Border {
                        radius: (size / 2.0).into(),
                        ..Default::default()
                    },
                    shadow: if glow {
                        iced::Shadow {
                            color: iced::Color { a: 0.7, ..bg },
                            offset: iced::Vector::new(0.0, 0.0),
                            blur_radius: 10.0,
                        }
                    } else {
                        iced::Shadow::default()
                    },
                    ..Default::default()
                }
            },
        ),
    )
    .into()
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
