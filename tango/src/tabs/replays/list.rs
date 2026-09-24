//! The replay list: filters, the filter strip, list rows, and the
//! playback queue under them.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

/// Date-dropdown filter: the replay's timestamp must fall within
/// the window ending now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DateFilter {
    #[default]
    Any,
    PastDay,
    PastWeek,
    PastMonth,
    PastYear,
}

impl DateFilter {
    fn matches(self, ts_ms: u64) -> bool {
        let window_secs: u64 = match self {
            DateFilter::Any => return true,
            DateFilter::PastDay => 60 * 60 * 24,
            DateFilter::PastWeek => 60 * 60 * 24 * 7,
            DateFilter::PastMonth => 60 * 60 * 24 * 30,
            DateFilter::PastYear => 60 * 60 * 24 * 365,
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        ts_ms >= now_ms.saturating_sub(window_secs * 1000)
    }
}

impl ReplaysState {
    /// The playback queue, its own pane under the replay list — `None` when
    /// nothing is queued, so the tab looks exactly as it did for anyone who
    /// never uses it. A header (count + Play + Clear) over one row per
    /// waiting replay, each carrying the same two lines the list row it came
    /// from carries. Play is disabled for the same reason Watch is: a
    /// playback session can't start while netplay holds the emulator.
    pub(super) fn queue_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        replays: &[replays::ScannedReplay],
        netplay_active: bool,
    ) -> Option<Element<'a, Message>> {
        if self.queue.is_empty() {
            return None;
        }
        // A glyph and the count lead; the action pair rides the right edge
        // with Clear immediately left of Play. Play carries the primary
        // treatment — starting the queue is the header's one real action, and
        // it reads as the counterpart to the detail panel's Watch button.
        let header = row![
            // The pane sits directly under the replay list with nothing but a
            // count to say what it is, so it gets the list-of-videos glyph —
            // the same family as the ListPlus on the button that fills it.
            // Body-sized against the caption beside it: a glyph shrunk to
            // caption size reads as a smudge, and muted keeps it from
            // outweighing the header's buttons.
            Icon::ListVideo
                .widget()
                .size(TEXT_BODY)
                .style(widgets::muted_text_style),
            text(t!(lang, "replays-queue-count", n = self.queue.len() as i64))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
            Space::new().width(Fill),
            widgets::icon_button(
                Icon::Trash2,
                t!(lang, "replays-queue-clear"),
                Message::ClearQueue,
                style::ROW_PADDING,
            ),
            widgets::icon_button_styled(
                Icon::Play,
                t!(lang, "replays-queue-play"),
                (!netplay_active).then_some(Message::PlayQueue),
                style::ROW_PADDING,
                if netplay_active {
                    widgets::neutral
                } else {
                    widgets::primary_button
                },
            ),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        // Vertical breathing room only — horizontal inset is each row's own,
        // so the scrollable spans the pane edge to edge and its bar sits
        // flush with the right edge, matching the replay list above.
        let mut rows = column![].spacing(2).padding([4, 0]);
        for (i, path) in self.queue.iter().enumerate() {
            // A queued replay that has since left the scan (deleted on disk)
            // still gets a row, so it can be taken out by hand rather than
            // just failing when its turn comes.
            let scanned = replays.iter().find(|r| &r.path == path);
            let (line, caption) = match scanned {
                Some(r) => Self::replay_caption(lang, r),
                None => (
                    path.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    t!(lang, "replays-queue-missing"),
                ),
            };
            // The position number is the whole point of a queue, so it leads
            // each row in a fixed-width gutter — the rows stay aligned as the
            // count crosses into double digits.
            rows = rows.push(
                container(
                    row![
                        container(
                            text(format!("{}", i + 1))
                                .size(TEXT_CAPTION)
                                .style(widgets::muted_text_style)
                        )
                        .width(Length::Fixed(18.0)),
                        column![
                            text(line).size(TEXT_CAPTION).wrapping(text::Wrapping::None),
                            text(caption)
                                .size(TEXT_CAPTION)
                                .style(widgets::muted_text_style)
                                .wrapping(text::Wrapping::None),
                        ]
                        .spacing(1)
                        .width(Fill),
                        // Understated on purpose: removing one entry is
                        // incidental next to the header's Play and Clear, so
                        // it's a borderless muted glyph that only picks up a
                        // plate on hover.
                        widgets::icon_button_styled(
                            Icon::X,
                            t!(lang, "replays-queue-remove"),
                            Some(Message::Dequeue(i)),
                            [2.0, 6.0],
                            |theme: &iced::Theme, status| {
                                let mut st = widgets::flat(theme, status);
                                if matches!(status, iced::widget::button::Status::Active) {
                                    st.text_color = widgets::muted_color(theme);
                                }
                                st
                            },
                        ),
                    ]
                    .spacing(6)
                    .align_y(Alignment::Center),
                )
                .padding(style::ROW_PADDING)
                .width(Fill)
                .clip(true),
            );
        }

        // Height-capped and scrollable: a long queue is the normal case for
        // this feature, and it must not push the list it was built from off
        // the tab.
        const QUEUE_MAX_H: f32 = 168.0;
        Some(
            container(
                column![
                    // Only the header is inset, and by the same 8px the
                    // list's own column uses at its top — a full PANE_PADDING
                    // here left the queue looking loose against the tight
                    // list above it. Horizontally it lines the count up with
                    // the row text below. The pane itself carries no padding,
                    // so the scrollable can run to the edges.
                    container(header).padding([8.0, style::ROW_PADDING[1]]).width(Fill),
                    scrollable(rows)
                        .style(widgets::chunky_scrollable)
                        .height(Length::Shrink)
                        .width(Fill),
                ]
                .width(Fill),
            )
            .max_height(QUEUE_MAX_H)
            .width(Fill)
            .style(widgets::pane)
            .into(),
        )
    }

    /// Top strip: game + date filter dropdowns, the free-text
    /// search box, and the show-incomplete toggle. Game options are
    /// derived from the distinct families seen across the scanned
    /// replays; "All …" is always the first option.
    pub(super) fn filter_strip<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        replays: &[replays::ScannedReplay],
    ) -> Element<'a, Message> {
        let all_games = t!(lang, "replays-filter-all-games");
        let mut game_options = vec![widgets::Choice::new(None, all_games.clone())];
        {
            use itertools::Itertools;
            // Dedupe by family only — the filter ignores variant,
            // so listing "BN6" once covers both Gregar and Falzar.
            let mut seen: Vec<String> = replays
                .iter()
                .filter_map(|r| {
                    let gi = r.local_side()?.game_info.as_ref()?;
                    Some(gi.rom_family.clone())
                })
                .unique()
                .collect();
            seen.sort();
            for family in seen {
                let display = family_display_name_or_raw(lang, &family, 0);
                game_options.push(widgets::Choice::new(Some(family), display));
            }
        }
        let selected_game = game_options
            .iter()
            .find(|o| o.value == self.game_filter)
            .cloned()
            .unwrap_or_else(|| game_options[0].clone());
        let date_options = vec![
            widgets::Choice::new(DateFilter::Any, t!(lang, "replays-filter-any-time")),
            widgets::Choice::new(DateFilter::PastDay, t!(lang, "replays-filter-past-day")),
            widgets::Choice::new(DateFilter::PastWeek, t!(lang, "replays-filter-past-week")),
            widgets::Choice::new(DateFilter::PastMonth, t!(lang, "replays-filter-past-month")),
            widgets::Choice::new(DateFilter::PastYear, t!(lang, "replays-filter-past-year")),
        ];
        let selected_date = date_options
            .iter()
            .find(|o| o.value == self.date_filter)
            .cloned()
            .unwrap_or_else(|| date_options[0].clone());
        let show_incomplete_toggle = iced::widget::checkbox(self.show_incomplete)
            .on_toggle(Message::ShowIncompleteToggled)
            .label(t!(lang, "replays-show-incomplete"))
            .size(TEXT_BODY)
            .text_size(TEXT_BODY)
            .style(widgets::chunky_checkbox);
        container(
            row![
                widgets::picker(
                    game_options,
                    Some(selected_game),
                    |o: widgets::Choice<Option<String>>| { Message::GameFilterSelected(o.value) }
                ),
                widgets::picker(date_options, Some(selected_date), |o: widgets::Choice<DateFilter>| {
                    Message::DateFilterSelected(o.value)
                }),
                text_input(&t!(lang, "replays-filter-search-placeholder"), &self.search,)
                    .on_input(Message::SearchChanged)
                    .padding(STANDARD_PADDING)
                    .width(Length::Fixed(220.0))
                    .style(widgets::chunky_text_input),
                show_incomplete_toggle,
            ]
            .spacing(8)
            .align_y(Alignment::Center),
        )
        .padding(style::PANE_PADDING)
        .width(Fill)
        .style(widgets::pane)
        .into()
    }

    /// AND of the game + date + search + completeness filters.
    /// Search splits into whitespace-separated terms, each of which
    /// must appear (case-insensitive) somewhere in the replay's
    /// haystack — see [`search_haystack`]. Completeness only drops a
    /// row once its stats have actually loaded — unloaded entries
    /// pass through so a freshly-scanned replay isn't hidden during
    /// the lazy stats-worker window.
    pub(super) fn matches_filters(
        &self,
        lang: &LanguageIdentifier,
        replays_path: &std::path::Path,
        r: &replays::ScannedReplay,
    ) -> bool {
        let g_ok = self
            .game_filter
            .as_ref()
            .map(|family| {
                r.local_side()
                    .and_then(|s| s.game_info.as_ref())
                    .map(|gi| gi.rom_family == *family)
                    .unwrap_or(false)
            })
            .unwrap_or(true);
        let d_ok = self.date_filter.matches(r.metadata.ts);
        let query = self.search.trim().to_lowercase();
        let s_ok = query.is_empty() || {
            // Haystack is only built for a non-empty query, so the
            // idle (no-search) view pays nothing per row.
            let hay = search_haystack(lang, replays_path, r);
            query.split_whitespace().all(|term| hay.contains(term))
        };
        let c_ok = self.show_incomplete || self.stats.get(&r.path).map(|s| s.is_complete).unwrap_or(false);
        g_ok && d_ok && s_ok && c_ok
    }

    /// The two lines that name a replay in the list: its timestamp, and the
    /// "game @ code · nicknames" caption under it. Shared with the playback
    /// queue, so a queued entry reads exactly like the row it was added from.
    pub(super) fn replay_caption(lang: &LanguageIdentifier, r: &replays::ScannedReplay) -> (String, String) {
        let md = &r.metadata;
        let local_nick = r.local_side().map(|s| s.nickname.clone()).unwrap_or_default();
        let remote_nick = r.remote_side().map(|s| s.nickname.clone()).unwrap_or_default();
        let local_gi = r.local_side().and_then(|s| s.game_info.as_ref());
        let game_label = local_gi
            .and_then(|g| u8::try_from(g.rom_variant).ok().map(|v| (g.rom_family.as_str(), v)))
            .and_then(|(family, variant)| crate::library::game::find_by_family_and_variant(family, variant))
            .map(|g| crate::library::game::short_name(lang, g))
            .or_else(|| local_gi.map(|g| g.rom_family.clone()))
            .unwrap_or_default();
        let nick_pair = if remote_nick.is_empty() && local_nick.is_empty() {
            link_code_display(lang, &md.link_code).into_owned()
        } else {
            format!("{local_nick} vs {remote_nick}")
        };
        (
            format_ts(md.ts, "%Y-%m-%d %H:%M:%S"),
            format!(
                "{game_label} @ {}  ·  {nick_pair}",
                link_code_display(lang, &md.link_code)
            ),
        )
    }

    /// One row of the replay list: timestamp + status glyph, the
    /// "game @ code · nicknames" line, an optional stats line once the
    /// lazy stats worker gets here, and a bottom progress strip while
    /// an export render is in flight.
    pub(super) fn replay_list_row<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        r: &replays::ScannedReplay,
        idx: usize,
    ) -> Element<'a, Message> {
        let md = &r.metadata;
        let (ts_str, caption) = Self::replay_caption(lang, r);
        let local_gi = r.local_side().and_then(|s| s.game_info.as_ref());

        let selected = self.selected.as_ref() == Some(&r.path);
        // Right-edge status glyph: a clapperboard while a render
        // is in flight, a green check on success, a red X on
        // failure. In-flight renders additionally get a progress
        // bar flush along the row's bottom edge (see
        // `progress_strip`).
        let job_state = self.job(&r.path);
        let rendering = matches!(job_state, Some(j) if j.result.is_none());
        let render_done_ok = matches!(job_state, Some(j) if matches!(&j.result, Some(Ok(_))));
        let render_done_err = matches!(job_state, Some(j) if matches!(&j.result, Some(Err(_))));
        let status_badge = |icon: Icon, color: fn(&iced::Theme) -> iced::Color| -> Element<'a, Message> {
            container(
                icon.widget()
                    .style(move |theme: &iced::Theme| iced::widget::text::Style {
                        color: Some(color(theme)),
                    }),
            )
            .padding([0, 4])
            .into()
        };
        let badge: Element<'_, Message> = if rendering {
            status_badge(Icon::Clapperboard, |theme| theme.palette().primary)
        } else if render_done_ok {
            status_badge(Icon::Check, |theme| theme.palette().success)
        } else if render_done_err {
            status_badge(Icon::X, |theme| theme.palette().danger)
        } else {
            Space::new().width(Length::Fixed(0.0)).into()
        };
        // Bottom progress strip. While an export is in flight we
        // draw a full-width bar flush with the row's bottom edge
        // (no label — the detail panel carries the percentage);
        // otherwise we reserve the same height with an empty
        // spacer so toggling the bar on never shifts row height.
        let progress_strip: Element<'_, Message> = if rendering {
            let pct = match job_state.filter(|j| j.total > 0) {
                Some(j) => (j.completed as f32 / j.total as f32).clamp(0.0, 1.0),
                None => 0.0,
            };
            iced::widget::progress_bar(0.0..=1.0, pct)
                .girth(Length::Fixed(4.0))
                .style(|theme: &iced::Theme| {
                    iced::widget::progress_bar::Style {
                        // Transparent track lets the row's own
                        // background show through — so it matches
                        // exactly, including the zebra stripe on
                        // alternating rows. Only the filled portion
                        // reads as progress. Square corners, no
                        // border — a flush bottom-edge accent.
                        background: iced::Background::Color(iced::Color::TRANSPARENT),
                        bar: iced::Background::Color(theme.palette().primary),
                        border: iced::Border {
                            radius: 0.0.into(),
                            width: 0.0,
                            color: iced::Color::TRANSPARENT,
                        },
                    }
                })
                .into()
        } else {
            Space::new().height(Length::Fixed(4.0)).into()
        };
        // Match-type name (e.g. "Triple") for the stats line.
        let family = local_gi.map(|g| g.rom_family.clone()).unwrap_or_default();
        let type_name =
            crate::library::game::match_type_name(lang, &family, md.match_type as u8, md.match_subtype as u8);
        // Stats line: "Triple (2 rounds) · 0:42" once the lazy
        // stats worker gets here, with " · incomplete" tacked on
        // when the recorded stream didn't reach END_OF_REPLAY.
        // Composed from the per-locale match-type-value + the
        // shared "incomplete" string so we don't carry a
        // dedicated stats-line template just to glue them.
        // The round count is only there for a replay whose telemetry
        // analysis is cached — nothing else counts rounds.
        let stats = self.stats.get(&r.path);
        let stats_line = stats.map(|s| {
            let mut parts = vec![type_name.clone()];
            parts.extend(s.round_count.map(|n| t!(lang, "replays-round-count", count = n as i64)));
            parts.push(crate::session::format_tick(s.tick_count));
            if !s.is_complete {
                parts.push(t!(lang, "replays-incomplete"));
            }
            parts.join(" · ")
        });
        let is_complete = stats.map(|s| s.is_complete).unwrap_or(true);
        // Two static caption lines, optionally a third with
        // duration / rounds / incomplete (only when stats
        // have loaded for this row).
        let mut text_col = column![
            // Title line carries the status glyph pinned to its
            // right. Keeping it on this fixed first line (rather
            // than vertically centered across the whole row) means
            // it never moves as the optional stats line loads or
            // the glyph changes.
            row![text(ts_str).size(TEXT_BODY), Space::new().width(Fill), badge].align_y(Alignment::Center),
            text(caption)
                .size(TEXT_CAPTION)
                .style(widgets::list_caption_style(selected)),
        ]
        .spacing(2)
        .width(Fill);
        if let Some(line) = stats_line {
            text_col = text_col.push(text(line).size(TEXT_CAPTION).style(move |theme: &iced::Theme| {
                if !is_complete {
                    widgets::danger_text_style(theme)
                } else {
                    widgets::list_caption_style(selected)(theme)
                }
            }));
        }
        button(
            column![
                container(text_col).padding(style::ROW_PADDING).width(Fill),
                progress_strip,
            ]
            .width(Fill),
        )
        .padding(0)
        .width(Fill)
        .style(widgets::list_item(selected, idx))
        .on_press(Message::Selected(r.path.clone()))
        .into()
    }
}

/// Everything the free-text search matches against, joined into one
/// lowercased blob: both sides' nicknames, game names (raw family
/// plus the localized display/short names, so "exe6" and "battle
/// network" both hit), patch name + version, the link code, the
/// date as `YYYY-MM-DD` (so "2026-07" matches a month), and the
/// path relative to the replays root.
pub(super) fn search_haystack(
    lang: &LanguageIdentifier,
    replays_path: &std::path::Path,
    r: &replays::ScannedReplay,
) -> String {
    let md = &r.metadata;
    let mut parts: Vec<String> = Vec::new();
    for side in [md.side(0), md.side(1)].into_iter().flatten() {
        parts.push(side.nickname.clone());
        if let Some(gi) = side.game_info.as_ref() {
            parts.push(gi.rom_family.clone());
            parts.push(family_display_name_or_raw(lang, &gi.rom_family, gi.rom_variant));
            if let Some(g) = u8::try_from(gi.rom_variant)
                .ok()
                .and_then(|v| crate::library::game::find_by_family_and_variant(&gi.rom_family, v))
            {
                parts.push(crate::library::game::short_name(lang, g));
            }
            if let Some(p) = gi.patch.as_ref() {
                parts.push(format!("{} v{}", p.name, p.version));
            }
        }
    }
    parts.push(md.link_code.clone());
    parts.push(format_ts(md.ts, "%Y-%m-%d"));
    let parent = r
        .path
        .parent()
        .map(|p| replays::format_rel_path(replays_path, p))
        .unwrap_or_default();
    let filename = r
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    parts.push(format!("{parent}{filename}"));
    parts.join("\n").to_lowercase()
}
