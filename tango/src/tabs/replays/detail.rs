//! The selected replay's detail panel: matchup and build selector, HP
//! chart, patch download line, and actions.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

/// The replay's own patch download, when it has one in flight or
/// failed: the same row the patches tab, play strip and lobby use.
/// Absent the rest of the time, so the detail carries no empty slot.
fn patch_download_line<'a>(
    lang: &'a LanguageIdentifier,
    r: &replays::ScannedReplay,
    scanners: &'a Catalog,
    downloads: &'a crate::library::patch::Downloads,
) -> Option<Element<'a, Message>> {
    // Same pick App makes when Watch fires: the first side's patch we
    // don't have. Both sides matter -- playback runs both games.
    let patches = scanners.patches.read();
    let key = [r.metadata.side(0), r.metadata.side(1)]
        .into_iter()
        .flatten()
        .filter_map(|s| s.game_info.as_ref()?.patch.as_ref())
        .find_map(|p| {
            let version = semver::Version::parse(&p.version).ok()?;
            (!patches.is_installed(&p.name, &version)).then_some((p.name.clone(), version))
        });
    let key = key?;
    match downloads.get(&key) {
        Some(download) if download.is_running() => {
            let caption = match download.percent() {
                Some(percent) => t!(lang, "replays-patch-downloading-progress", percent = percent as i64),
                None => t!(lang, "replays-patch-downloading"),
            };
            Some(widgets::download_row(
                caption,
                download.fraction(),
                false,
                None,
                Some((t!(lang, "patches-cancel"), Message::CancelPatchDownload(key))),
            ))
        }
        Some(crate::library::patch::Download::Failed) => Some(widgets::download_row(
            t!(lang, "replays-patch-download-failed"),
            None,
            true,
            // Retrying is just watching again: App re-runs the fetch and
            // queues the playback behind it.
            Some((t!(lang, "patches-retry"), Message::Watch(r.path.clone()))),
            None,
        )),
        _ => None,
    }
}

/// What stands in for the HP chart while streamer mode has it masked: the
/// reason and the button that unmasks it, on one row so the whole thing fits
/// the height the chart would have taken. Same [`DETAIL_HP_GRAPH_H`] as the
/// chart, so revealing swaps the pane's contents without moving the panes
/// below it.
fn streamer_masked_chart<'a>(lang: &'a LanguageIdentifier) -> Element<'a, Message> {
    container(
        row![
            text(t!(lang, "replays-streamer-hidden"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
            widgets::labeled_icon_button(
                lucide_icons::Icon::Eye,
                t!(lang, "replays-streamer-show"),
                Message::RevealChart,
                [4.0, 10.0],
                widgets::neutral,
            ),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .center(Fill)
    .height(Length::Fixed(DETAIL_HP_GRAPH_H))
    .width(Fill)
    .style(widgets::pane)
    .into()
}

/// Quiet, non-CTA styling for the two build choices in the matchup pane.
/// The checked dot uses the app's selection gold; the ring and hover remain
/// neutral so this small selector does not read as another green action.
fn build_selector_radio(theme: &iced::Theme, status: iced::widget::radio::Status) -> iced::widget::radio::Style {
    let hovered = matches!(status, iced::widget::radio::Status::Hovered { .. });
    iced::widget::radio::Style {
        background: if hovered {
            iced::Color {
                a: 0.08,
                ..theme.palette().text
            }
            .into()
        } else {
            iced::Color::TRANSPARENT.into()
        },
        dot_color: widgets::SELECT_YELLOW,
        border_width: 1.0,
        border_color: iced::Color {
            a: if hovered { 0.75 } else { 0.45 },
            ..theme.palette().text
        },
        text_color: None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn replay_detail<'a>(
    lang: &'a LanguageIdentifier,
    r: &replays::ScannedReplay,
    replays_path: &std::path::Path,
    state: &'a ReplaysState,
    scanners: &'a Catalog,
    netplay_active: bool,
    streamer_mode: bool,
    downloads: &'a crate::library::patch::Downloads,
) -> Element<'a, Message> {
    // Playback needs a scanned ROM for the local-side game; without
    // one the emulator session would error on construction. Resolve
    // now so the Watch button can disable + explain.
    let local_rom_present = r
        .local_side()
        .and_then(|s| s.game_info.as_ref())
        .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
        .and_then(|(family, variant)| crate::library::game::find_by_family_and_variant(family, variant))
        .map(|g| scanners.roms.read().contains_key(&g))
        .unwrap_or(false);
    let md = &r.metadata;
    let ts_str = format_ts(md.ts, "%Y-%m-%d %H:%M:%S %z");

    let row_for_side = |label: String,
                        side: Option<&tango_replay::metadata::Side>,
                        build: BuildSide,
                        available: bool|
     -> Element<'static, Message> {
        let nick = side.map(|s| s.nickname.clone()).unwrap_or_default();
        let gi = side.and_then(|s| s.game_info.as_ref());
        let game_line = gi
            .map(|g| {
                let mut s = family_display_name(lang, &g.rom_family, g.rom_variant);
                if let Some(p) = g.patch.as_ref() {
                    s.push_str(&format!(" · {} v{}", p.name, p.version));
                }
                s
            })
            .unwrap_or_default();
        let selector: Element<'static, Message> = if available {
            iced::widget::radio(label, build, Some(state.viewed_build), Message::BuildSelected)
                .size(14.0)
                .spacing(6.0)
                .text_size(TEXT_CAPTION)
                .style(build_selector_radio)
                .into()
        } else {
            text(label).size(TEXT_CAPTION).style(widgets::muted_text_style).into()
        };
        let col = column![
            selector,
            text(nick).size(TEXT_TITLE),
            text(game_line).size(TEXT_CAPTION)
        ]
        .spacing(2);
        container(col).width(Length::Fill).into()
    };

    let parent_str = r
        .path
        .parent()
        .map(|p| replays::format_rel_path(replays_path, p))
        .unwrap_or_else(|| "/".to_string());
    let filename = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    let game_short = r
        .local_side()
        .and_then(|s| s.game_info.as_ref())
        .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
        .and_then(|(family, variant)| crate::library::game::find_by_family_and_variant(family, variant))
        .map(|g| crate::library::game::short_name(lang, g))
        .unwrap_or_else(|| "?".to_string());
    let title = format!("{game_short} @ {}", link_code_display(lang, &md.link_code));

    // Title + metadata pane: title row with action buttons, then
    // timestamp and file path. Render settings float above this
    // detail stack as a popover rather than changing its layout.
    let title_pane = container(
        column![
            row![
                // Title in a Fill container so a long link code
                // wraps naturally without squashing the action
                // buttons on the right.
                container(text(title).size(18)).width(Fill),
                {
                    // Per-replay toggle. Disabled outright while a
                    // render for this replay is in flight, so the
                    // user can't even attempt to close the panel
                    // mid-render (which would otherwise be a
                    // no-op, but a dead button is more honest).
                    let msg = if state.is_rendering(&r.path) {
                        None
                    } else if state.is_panel_open(&r.path) {
                        Some(Message::Export(ExportMessage::PanelClose(r.path.clone())))
                    } else {
                        Some(Message::Export(ExportMessage::PanelOpen(r.path.clone())))
                    };
                    widgets::icon_button_maybe(Icon::Clapperboard, t!(lang, "replays-export"), msg, STANDARD_PADDING)
                },
                widgets::icon_button(
                    Icon::FolderOpen,
                    t!(lang, "patches-open-folder"),
                    Message::RevealReplay(r.path.clone()),
                    STANDARD_PADDING,
                ),
                // Queue: line this replay up to play when the current one
                // runs out. Repeatable — queueing the same replay twice
                // plays it twice.
                widgets::icon_button(
                    Icon::ListPlus,
                    t!(lang, "replays-queue-add"),
                    Message::Enqueue(r.path.clone()),
                    STANDARD_PADDING,
                ),
                // Watch is the main action of the detail view —
                // promote to primary with a text label so it's
                // visually obvious. Disabled while netplay is in any
                // non-Idle phase: starting a playback session would
                // race with the live emulator. Also disabled when the
                // local-side ROM isn't scanned (playback can't build
                // a core without it); a tooltip carries the reason in
                // that case, since the label alone can't say why.
                {
                    let watch_disabled = netplay_active || !local_rom_present;
                    let btn = widgets::labeled_icon_button_maybe(
                        Icon::Play,
                        t!(lang, "replays-watch"),
                        if watch_disabled {
                            None
                        } else {
                            Some(Message::Watch(r.path.clone()))
                        },
                        STANDARD_PADDING,
                        if watch_disabled {
                            widgets::neutral
                        } else {
                            widgets::primary_button
                        },
                    );
                    if local_rom_present {
                        btn
                    } else {
                        iced::widget::tooltip(
                            btn,
                            widgets::tooltip_bubble(t!(lang, "replays-watch-missing-rom")),
                            iced::widget::tooltip::Position::Top,
                        )
                        .gap(4)
                        .into()
                    }
                },
            ]
            .spacing(6)
            // Top-align so the action buttons stay anchored when
            // a long title wraps to a second line.
            .align_y(Alignment::Start),
            // Watch on a replay whose patch we lack starts a download;
            // without this the click looked like it did nothing at all
            // until playback appeared, and a failure looked like
            // nothing happening forever. Only present while there is
            // one -- no reserved gap the rest of the time.
            Element::from(
                patch_download_line(lang, r, scanners, downloads)
                    .unwrap_or_else(|| iced::widget::space::vertical().height(Length::Fixed(0.0)).into())
            ),
            // Metadata rows: file path, timestamp, match type,
            // duration. Stacked tight in a sub-column so the rows
            // read as one block (matches the patches detail-card
            // density at .spacing(3)), with the outer column still
            // breathing at .spacing(6) between sections.
            column![
                text(format!("{parent_str}{filename}"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
                text(ts_str).size(TEXT_CAPTION).style(widgets::muted_text_style),
                {
                    let family = r
                        .local_side()
                        .and_then(|s| s.game_info.as_ref())
                        .map(|g| g.rom_family.clone())
                        .unwrap_or_default();
                    let type_name = crate::library::game::match_type_name(
                        lang,
                        &family,
                        md.match_type as u8,
                        md.match_subtype as u8,
                    );
                    // "Triple (2 rounds)" for a replay whose telemetry
                    // analysis is cached — a recording doesn't say how
                    // many rounds it holds. Just "Triple" otherwise.
                    let value = match state.stats.get(&r.path).and_then(|s| s.round_count) {
                        Some(n) => {
                            let rounds = t!(lang, "replays-round-count", count = n as i64);
                            format!("{type_name} · {rounds}")
                        }
                        None => type_name,
                    };
                    row![
                        text(t!(lang, "replays-match-type"))
                            .size(TEXT_CAPTION)
                            .style(widgets::muted_text_style),
                        text(value).size(TEXT_CAPTION),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center)
                },
                // Duration row, matching the match-type styling.
                // Value fills in once the lazy stats worker has
                // tallied the tick count for this path; em-dash
                // placeholder until then so the row doesn't pop
                // into existence.
                row![
                    text(t!(lang, "replays-duration"))
                        .size(TEXT_CAPTION)
                        .style(widgets::muted_text_style),
                    text(
                        state
                            .stats
                            .get(&r.path)
                            .map(|s| crate::session::format_tick(s.tick_count))
                            .unwrap_or_else(|| "—".to_string())
                    )
                    .size(TEXT_CAPTION),
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            ]
            .spacing(3),
        ]
        .spacing(6),
    )
    .width(Fill)
    .padding(style::PANE_PADDING)
    .style(widgets::pane);

    let matchup_pane = widgets::matchup_pane(
        row_for_side(
            t!(lang, "play-you"),
            r.local_side(),
            BuildSide::You,
            state.loaded.is_some(),
        ),
        row_for_side(
            t!(lang, "play-opponent"),
            r.remote_side(),
            BuildSide::Opponent,
            state.opponent_loaded.is_some(),
        ),
    );

    // HP-over-time pane: the match graph, at a fixed height with the
    // chip-event lanes always present. During a first-focus analysis the
    // chart exists from the start (empty segments at their final widths,
    // seeded on selection) and the re-simulation draws into it live — no
    // placeholder state.
    //
    // Streamer mode masks it instead: the trace reads out how a match went
    // round by round, and the event lanes name every chip both players used.
    // The mask is per-selection (see `ReplaysState::revealed`).
    let hp_pane: Element<'_, Message> = if streamer_mode && state.revealed.as_ref() != Some(&r.path) {
        streamer_masked_chart(lang)
    } else {
        // The pane renders whether or not a chart exists yet (a missing
        // entry — e.g. a failed analysis — draws as an empty frame), so
        // the detail column's layout never shifts with analysis state.
        let chart = state.hp_charts.get(&r.path);
        let chart_rounds: Vec<widgets::HpGraphRound<'_>> = chart
            .map(|c| c.rounds.as_slice())
            .unwrap_or(&[])
            .iter()
            .map(|r| widgets::HpGraphRound {
                trace: &r.trace,
                custom: &r.custom,
                chip_uses: [&r.chip_uses[0], &r.chip_uses[1]],
                outcome: r.outcome,
                weight: r.weight,
            })
            .collect();
        let body = widgets::hp_match_graph(
            chart_rounds,
            chart.map(|c| c.max_hp).unwrap_or(1.0),
            1.0,
            DETAIL_HP_GRAPH_H,
            // Zoomable, keyed on the replay path so switching replays
            // resets the view.
            Some({
                use std::hash::{Hash, Hasher};
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                r.path.hash(&mut hasher);
                hasher.finish()
            }),
        );
        // No pane padding: the chart's own per-round inset panels are the
        // content, so the canvas runs edge to edge and the pane background
        // only peeks through the round dividers.
        container(body).width(Fill).style(widgets::pane).into()
    };

    // Save view contributes its own pane pair (tab strip + body)
    // when a save is loaded; otherwise a single placeholder pane
    // explaining the empty state.
    let selected_loaded = match state.viewed_build {
        BuildSide::You => state.loaded.as_ref(),
        BuildSide::Opponent => state.opponent_loaded.as_ref(),
    };
    let preview: Element<'_, Message> = if let Some(loaded) = selected_loaded {
        let viewed_build = state.viewed_build;
        loaded
            .editor
            .view(lang, loaded, streamer_mode, None, true, false)
            .map(move |msg| Message::SaveEditor(viewed_build, msg))
    } else {
        container(
            text(t!(lang, "save-empty"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        )
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane)
        .into()
    };

    let panes = column![title_pane, hp_pane, matchup_pane]
        .spacing(style::PANE_GAP)
        .width(Fill);
    let content: Element<'a, Message> = panes.push(preview).height(Fill).into();
    if !state.is_panel_open(&r.path) {
        return content;
    }

    // Transparent click-away layer, then the popover itself. It is
    // aligned beneath the top-right replay actions and floats over the
    // detail panes without reflowing them. An in-flight render stays
    // pinned: its progress/cancel controls must remain reachable.
    let dismiss = if state.is_rendering(&r.path) {
        Message::NoOp
    } else {
        Message::Export(ExportMessage::PanelClose(r.path.clone()))
    };
    let click_away =
        iced::widget::mouse_area(iced::widget::Space::new().width(Length::Fill).height(Length::Fill)).on_press(dismiss);
    let popover = export::export_popover(
        lang,
        &state.export_settings,
        state.rounds_for(&r.path),
        state.hp_charts.get(&r.path).is_some_and(|c| c.has_setup),
        state.hp_pending.contains(&r.path),
        state.job(&r.path),
        &r.path,
    );
    // Swallow presses on unused parts of the panel so they do not hit
    // the click-away layer. Child controls capture their own presses.
    let popover = iced::widget::mouse_area(popover).on_press(Message::NoOp);
    let positioned = container(popover)
        .width(Fill)
        .height(Fill)
        .align_x(iced::alignment::Horizontal::Right)
        .align_y(iced::alignment::Vertical::Top)
        .padding(iced::Padding {
            top: 48.0,
            right: style::PANE_PADDING,
            bottom: 0.0,
            left: 0.0,
        });
    iced::widget::stack![content, click_away, positioned].into()
}

/// Height of the HP graph in the detail panel: a 54 px trace field plus
/// the widget's two per-side chip-event lanes (18 px, always present),
/// which also leaves room for the four-line icon hover readout — the
/// canvas clips anything that hangs past its bounds. One fixed height
/// for every chart state, so the layout never jerks as an analysis
/// renders in.
const DETAIL_HP_GRAPH_H: f32 = 72.0;
