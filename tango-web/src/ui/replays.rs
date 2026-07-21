//! The Replays tab, laid out like the desktop's (`tabs/replays`): a
//! filter strip (game / date / search / show-incomplete) over a fixed
//! 360px list beside the detail pane — title + actions (Watch /
//! Download / Delete), metadata, the You-vs-Opponent matchup pane, and
//! the local side's save embedded through the read-only save view.
//! (The desktop's HP-analysis chart and video render ride the native
//! analysis/export pipelines and stay desktop-only for now.)

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::save_view::{SaveHandle, SaveView};
use crate::t;
use crate::runtime::SAVES_REV;

/// A watch attempt's status line (booting the pair takes seconds).
static WATCH_STATUS: GlobalSignal<Option<String>> = Signal::global(|| None);

/// Bumped on delete so the listing refreshes.
static REPLAYS_REV: GlobalSignal<u64> = Signal::global(|| 0);

/// One listed replay, its metadata decoded from the file's own header
/// (the shared 0x1C reader) — everything the list + detail chrome needs
/// without simulating anything.
#[derive(Clone, PartialEq)]
struct Row {
    file: String,
    size: usize,
    /// The match clock, ms since epoch.
    ts: u64,
    link_code: String,
    local_nick: String,
    remote_nick: String,
    /// Short game tag for the row caption (e.g. "BN6").
    game_short: String,
    /// The local side's family string, for the game filter.
    family: Option<String>,
    /// Per-side "Variant (+patch vX)" description for the matchup pane.
    local_desc: String,
    remote_desc: String,
    /// Localized match-type label, when the local game resolves.
    match_type: Option<String>,
    /// The local side's game, for Watch gating + the save preview.
    slug: Option<String>,
    complete: bool,
}

/// The date-range filter, mirroring the desktop's choices.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum DateFilter {
    #[default]
    Any,
    PastDay,
    PastWeek,
    PastMonth,
    PastYear,
}

impl DateFilter {
    const ALL: [DateFilter; 5] = [
        DateFilter::Any,
        DateFilter::PastDay,
        DateFilter::PastWeek,
        DateFilter::PastMonth,
        DateFilter::PastYear,
    ];

    fn label(self, lang: &unic_langid::LanguageIdentifier) -> String {
        match self {
            DateFilter::Any => t!(lang, "replays-filter-any-time"),
            DateFilter::PastDay => t!(lang, "replays-filter-past-day"),
            DateFilter::PastWeek => t!(lang, "replays-filter-past-week"),
            DateFilter::PastMonth => t!(lang, "replays-filter-past-month"),
            DateFilter::PastYear => t!(lang, "replays-filter-past-year"),
        }
    }

    fn cutoff_ms(self) -> Option<f64> {
        let day = 24.0 * 60.0 * 60.0 * 1000.0;
        match self {
            DateFilter::Any => None,
            DateFilter::PastDay => Some(day),
            DateFilter::PastWeek => Some(7.0 * day),
            DateFilter::PastMonth => Some(30.0 * day),
            DateFilter::PastYear => Some(365.0 * day),
        }
    }
}

/// `ts` (ms since epoch) in the browser's local time, the desktop's
/// "%Y-%m-%d %H:%M:%S".
fn fmt_ts(ms: u64) -> String {
    let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        d.get_full_year(),
        d.get_month() + 1,
        d.get_date(),
        d.get_hours(),
        d.get_minutes(),
        d.get_seconds()
    )
}

/// One side's "Variant (+patch vX)" description for the matchup pane.
fn side_desc(side: Option<&tango_pvp::replay::metadata::Side>) -> String {
    let Some(gi) = side.and_then(|s| s.game_info.as_ref()) else {
        return String::new();
    };
    let game = u8::try_from(gi.rom_variant)
        .ok()
        .and_then(|v| crate::library::find_by_family_and_variant(&gi.rom_family, v));
    let mut out = match game {
        Some(g) => crate::library::display_name(g),
        None => gi.rom_family.clone(),
    };
    if let Some(p) = gi.patch.as_ref() {
        out.push_str(&format!(" + {} v{}", p.name, p.version));
    }
    out
}

/// Everything the detail pane derives from the full replay decode:
/// duration/rounds plus the local side's save baked for the embedded
/// save view (`None` when that game's ROM isn't imported).
#[derive(Clone)]
struct Detail {
    duration_secs: u32,
    rounds: usize,
    handle: Option<SaveHandle>,
}

impl PartialEq for Detail {
    fn eq(&self, other: &Self) -> bool {
        self.duration_secs == other.duration_secs && self.rounds == other.rounds && self.handle == other.handle
    }
}

#[component]
pub fn ReplaysScreen() -> Element {
    let Ctx {
        runtime, storage, library, ..
    } = use_ctx();
    let mut selected = use_signal(|| Option::<String>::None);
    let mut game_filter = use_signal(|| Option::<String>::None);
    let mut date_filter = use_signal(DateFilter::default);
    let mut search = use_signal(String::new);
    let mut show_incomplete = use_signal(|| false);

    let rows = use_resource(move || {
        let _ = REPLAYS_REV.read();
        let _ = SAVES_REV.read();
        let storage = storage.read().clone().flatten();
        async move {
            let Some(storage) = storage else {
                return Vec::new();
            };
            let Ok(files) = crate::storage::list_files(storage.replays()).await else {
                return Vec::new();
            };
            let mut rows = Vec::new();
            for (file, handle) in files {
                let Ok(bytes) = crate::storage::read_handle(&handle).await else {
                    continue;
                };
                let size = bytes.len();
                match tango_pvp::replay::Replay::decode(&bytes[..]) {
                    Ok(replay) => {
                        let meta = &replay.metadata;
                        let local = meta.local_side.as_ref();
                        let remote = meta.remote_side.as_ref();
                        let game = local.and_then(|s| s.game_info.as_ref()).and_then(|gi| {
                            u8::try_from(gi.rom_variant)
                                .ok()
                                .and_then(|v| crate::library::find_by_family_and_variant(&gi.rom_family, v))
                        });
                        rows.push(Row {
                            file,
                            size,
                            ts: meta.ts,
                            link_code: meta.link_code.clone(),
                            local_nick: local.map(|s| s.nickname.clone()).unwrap_or_default(),
                            remote_nick: remote.map(|s| s.nickname.clone()).unwrap_or_default(),
                            game_short: game
                                .map(crate::library::short_name)
                                .or_else(|| {
                                    local
                                        .and_then(|s| s.game_info.as_ref())
                                        .map(|gi| gi.rom_family.clone())
                                })
                                .unwrap_or_default(),
                            family: local.and_then(|s| s.game_info.as_ref()).map(|gi| gi.rom_family.clone()),
                            local_desc: side_desc(local),
                            remote_desc: side_desc(remote),
                            match_type: game.map(|g| {
                                crate::library::match_type_name(
                                    g,
                                    meta.match_type as u8,
                                    meta.match_subtype as u8,
                                )
                            }),
                            slug: game.map(crate::library::game_slug),
                            complete: replay.is_complete,
                        });
                    }
                    Err(_) => rows.push(Row {
                        file: file.clone(),
                        size,
                        ts: 0,
                        link_code: String::new(),
                        local_nick: String::new(),
                        remote_nick: String::new(),
                        game_short: "?".to_string(),
                        family: None,
                        local_desc: String::new(),
                        remote_desc: String::new(),
                        match_type: None,
                        slug: None,
                        complete: false,
                    }),
                }
            }
            // Newest first: the file names lead with the match clock.
            rows.sort_by(|a, b| b.file.cmp(&a.file));
            rows
        }
    });
    let rows = rows.read().clone().unwrap_or_default();

    // The full decode for the selected replay: duration / rounds + the
    // local save baked for the embedded save view.
    let detail = use_resource(move || {
        let file = selected.read().clone();
        let storage = storage.read().clone().flatten();
        let lib = library.read().clone().flatten().unwrap_or_default();
        async move {
            let (Some(storage), Some(file)) = (storage, file) else {
                return None;
            };
            let bytes = crate::storage::read(storage.replays(), &file).await.ok().flatten()?;
            let replay = tango_pvp::replay::Replay::decode(&bytes[..]).ok()?;
            let duration_secs = (replay.inputs.len() as u32) / 60;
            let rounds = replay.round_starts.len().max(1);
            // The local side's save, viewed through the same Loaded the
            // play tab uses — needs the ROM (for assets) + its patch.
            let meta = &replay.metadata;
            let gi = meta.local_side.as_ref().and_then(|s| s.game_info.as_ref());
            let handle = 'handle: {
                let Some(gi) = gi else { break 'handle None };
                let Some(game) = u8::try_from(gi.rom_variant)
                    .ok()
                    .and_then(|v| crate::library::find_by_family_and_variant(&gi.rom_family, v))
                else {
                    break 'handle None;
                };
                let Some(entry) = lib.by_game(game) else { break 'handle None };
                let Ok(Some(rom)) = crate::storage::read(storage.roms(), &entry.file).await else {
                    break 'handle None;
                };
                let patch = gi
                    .patch
                    .as_ref()
                    .and_then(|p| Some((p.name.clone(), semver::Version::parse(&p.version).ok()?)));
                let (rom, overrides) = match patch.as_ref() {
                    Some((name, ver)) => {
                        let Ok(rom) = crate::patches::apply(&storage, &rom, game, name, ver).await else {
                            break 'handle None;
                        };
                        let ov = crate::patches::version_overrides(&storage, name, ver).await;
                        (rom, ov)
                    }
                    None => (rom, Default::default()),
                };
                crate::save_view::Loaded::build(game, &rom, String::new(), &replay.local_sram, patch, overrides)
                    .ok()
                    .map(|l| SaveHandle(std::rc::Rc::new(std::cell::RefCell::new(l))))
            };
            Some(Detail {
                duration_secs,
                rounds,
                handle,
            })
        }
    });

    let lang = crate::i18n::LANG.read().clone();
    let lib = library.read().clone().flatten().unwrap_or_default();

    // Families present in the listing, for the game filter.
    let mut families: Vec<String> = rows.iter().filter_map(|r| r.family.clone()).collect();
    families.sort();
    families.dedup();

    // AND of the four filters, like the desktop's `matches_filters`.
    let now_ms = js_sys::Date::now();
    let needle = search.read().to_lowercase();
    let visible: Vec<&Row> = rows
        .iter()
        .filter(|r| {
            if let Some(f) = &*game_filter.read() {
                if r.family.as_deref() != Some(f.as_str()) {
                    return false;
                }
            }
            if let Some(cutoff) = date_filter.read().cutoff_ms() {
                if (now_ms - r.ts as f64) > cutoff {
                    return false;
                }
            }
            if !show_incomplete() && !r.complete {
                return false;
            }
            if !needle.is_empty() {
                let hay = format!(
                    "{} {} {} {} {}",
                    r.file, r.local_nick, r.remote_nick, r.game_short, r.link_code
                )
                .to_lowercase();
                if !hay.contains(&needle) {
                    return false;
                }
            }
            true
        })
        .collect();

    let selected_row = selected
        .read()
        .clone()
        .and_then(|f| rows.iter().find(|r| r.file == f).cloned());
    let detail_data = detail.read().clone().flatten();

    let watch_missing_rom = selected_row
        .as_ref()
        .is_some_and(|r| r.slug.as_deref().and_then(crate::library::find_by_slug).and_then(|g| lib.by_game(g)).is_none());
    let netplay_idle = matches!(&*crate::netplay::PHASE.read(), crate::netplay::PhaseView::Idle);

    let on_watch = {
        let runtime = runtime.clone();
        let selected_row = selected_row.clone();
        move |_| {
            let Some(row) = selected_row.clone() else { return };
            let storage = storage.read().clone().flatten();
            let lib = library.read().clone().flatten();
            let runtime = runtime.clone();
            *WATCH_STATUS.write() = Some(crate::i18n::t(&crate::i18n::LANG.peek().clone(), "web-booting-replay"));
            spawn(async move {
                match watch(runtime, storage, lib, row.file).await {
                    Ok(()) => *WATCH_STATUS.write() = None,
                    Err(e) => *WATCH_STATUS.write() = Some(format!("couldn't watch: {e:#}")),
                }
            });
        }
    };

    let on_download = {
        let selected_row = selected_row.clone();
        move |_| {
            let Some(row) = selected_row.clone() else { return };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                if let Ok(Some(bytes)) = crate::storage::read(storage.replays(), &row.file).await {
                    crate::web::download_bytes(&row.file, &bytes);
                }
            });
        }
    };

    // Render the replay to a WebM through WebCodecs, streaming to disk;
    // progress + cancel ride the export signals.
    let exporting = crate::export::EXPORT_PROGRESS.read().is_some();
    let on_export = {
        let selected_row = selected_row.clone();
        move |_| {
            let Some(row) = selected_row.clone() else { return };
            if crate::export::EXPORT_PROGRESS.peek().is_some() {
                return;
            }
            let storage = storage.read().clone().flatten();
            let lib = library.read().clone().flatten();
            spawn(async move {
                let lang = crate::i18n::LANG.peek().clone();
                let stem = save_stem_of(&row.file);
                let result = async {
                    // Where supported, ask for the destination first —
                    // the click's user activation is still live, and the
                    // whole render then streams straight to the user's
                    // file. Elsewhere: stream into an OPFS temp.
                    let target = if crate::export::save_picker_available() {
                        match crate::export::pick_save_file(&format!("{stem}.webm")).await? {
                            Some(handle) => crate::export::ExportTarget::Picked(handle),
                            // Dismissing the picker is a quiet cancel.
                            None => return Ok(false),
                        }
                    } else {
                        let Some(storage) = storage.clone() else {
                            anyhow::bail!("storage unavailable");
                        };
                        crate::export::ExportTarget::OpfsTemp(storage)
                    };
                    let (replay, local_rom, remote_rom) = load_pair(storage, lib, &row.file).await?;
                    crate::export::export_replay(replay, local_rom, remote_rom, stem, target, None).await?;
                    Ok(true)
                }
                .await;
                match result {
                    Ok(true) => *WATCH_STATUS.write() = Some(t!(&lang, "replays-export-success")),
                    Ok(false) => {}
                    Err(e) => {
                        let e: anyhow::Error = e;
                        *WATCH_STATUS.write() = Some(t!(&lang, "replays-export-error", error = format!("{e:#}")));
                    }
                }
            });
        }
    };

    let on_delete = {
        let selected_row = selected_row.clone();
        move |_| {
            let Some(row) = selected_row.clone() else { return };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let _ = crate::storage::delete(storage.replays(), &row.file).await;
                selected.set(None);
                *REPLAYS_REV.write() += 1;
            });
        }
    };

    rsx! {
        // --- filter strip: game / date / search / show-incomplete ---
        section { class: "pane filter-strip",
            select {
                onchange: move |evt: FormEvent| {
                    let v = evt.value();
                    game_filter.set((!v.is_empty()).then_some(v));
                },
                option { value: "", selected: game_filter.read().is_none(), {t!(&lang, "replays-filter-all-games")} }
                for f in families.iter() {
                    option {
                        value: "{f}",
                        selected: game_filter.read().as_deref() == Some(f.as_str()),
                        {crate::library::family_display_name(f)}
                    }
                }
            }
            select {
                onchange: move |evt: FormEvent| {
                    if let Ok(i) = evt.value().parse::<usize>() {
                        if let Some(d) = DateFilter::ALL.get(i) {
                            date_filter.set(*d);
                        }
                    }
                },
                for (i, d) in DateFilter::ALL.iter().enumerate() {
                    option { value: "{i}", selected: *d == date_filter(), {d.label(&lang)} }
                }
            }
            input {
                class: "search",
                r#type: "text",
                placeholder: t!(&lang, "replays-filter-search-placeholder"),
                value: "{search}",
                oninput: move |evt: FormEvent| search.set(evt.value()),
            }
            label { class: "check",
                input {
                    r#type: "checkbox",
                    checked: show_incomplete(),
                    onchange: move |evt: FormEvent| show_incomplete.set(evt.checked()),
                }
                {t!(&lang, "replays-show-incomplete")}
            }
            if let Some(p) = *crate::export::EXPORT_PROGRESS.read() {
                span { class: "sub",
                    {t!(&lang, "replays-export-progress")}
                    " {p.frame * 100 / p.total.max(1)}%"
                }
                button {
                    class: "btn",
                    onclick: move |_| *crate::export::EXPORT_CANCEL.write() = true,
                    {t!(&lang, "replays-export-cancel")}
                }
            }
            if let Some(status) = WATCH_STATUS.read().clone() {
                span { class: "sub flash ok", "{status}" }
            }
        }
        // --- fixed list beside the detail pane ---
        div { class: "replays-split",
            div { class: "pane replay-list",
                if visible.is_empty() {
                    p { class: "sub empty", {t!(&lang, "web-replays-empty")} }
                }
                for row in visible.iter().map(|r| (*r).clone()) {
                    {
                        let is_selected = selected.read().as_deref() == Some(row.file.as_str());
                        let ts = fmt_ts(row.ts);
                        let code = if row.link_code.is_empty() {
                            t!(&lang, "replays-direct-marker")
                        } else {
                            format!("@ {}", row.link_code)
                        };
                        let caption = format!(
                            "{} {}  ·  {} vs {}",
                            row.game_short, code, row.local_nick, row.remote_nick
                        );
                        let file = row.file.clone();
                        let incomplete = !row.complete;
                        rsx! {
                            button {
                                class: if is_selected { "replay-row selected" } else { "replay-row" },
                                onclick: move |_| selected.set(Some(file.clone())),
                                div { class: "line",
                                    span { class: "ts", "{ts}" }
                                    div { class: "grow" }
                                    if incomplete {
                                        span { class: "status bad", icons::X {} }
                                    }
                                }
                                span { class: "caption", "{caption}" }
                            }
                        }
                    }
                }
            }
            // --- detail: title pane, matchup, the embedded save view ---
            if let Some(row) = selected_row.as_ref() {
                div { class: "replay-detail",
                    div { class: "pane detail-title",
                        div { class: "head",
                            span { class: "title", {save_stem_of(&row.file)} }
                            div { class: "grow" }
                            button {
                                class: "btn icon-btn",
                                title: t!(&lang, "replays-export"),
                                disabled: exporting || watch_missing_rom,
                                onclick: on_export,
                                icons::Clapperboard {}
                            }
                            button {
                                class: "btn icon-btn",
                                title: t!(&lang, "web-download"),
                                onclick: on_download,
                                icons::Download {}
                            }
                            button {
                                class: "btn icon-btn danger",
                                title: t!(&lang, "save-delete"),
                                onclick: on_delete,
                                icons::Trash2 {}
                            }
                            button {
                                class: "btn primary",
                                // Incomplete replays are watchable — the
                                // desktop only gates on the ROM and live
                                // netplay; the truncated stream just ends
                                // playback early.
                                disabled: watch_missing_rom || !netplay_idle,
                                title: if watch_missing_rom { t!(&lang, "replays-watch-missing-rom") } else { t!(&lang, "replays-watch") },
                                onclick: on_watch,
                                icons::Play {}
                                {t!(&lang, "replays-watch")}
                            }
                        }
                        div { class: "meta",
                            span { class: "sub mono", "{row.file} · {row.size / 1024} KiB" }
                            span { class: "sub", {fmt_ts(row.ts)} }
                            if let Some(mt) = row.match_type.as_ref() {
                                span { class: "sub",
                                    {t!(&lang, "replays-match-type")}
                                    " {mt}"
                                    if !row.complete {
                                        " · "
                                        {t!(&lang, "replays-incomplete")}
                                    }
                                }
                            }
                            if let Some(d) = detail_data.as_ref() {
                                span { class: "sub",
                                    {t!(&lang, "replays-duration")}
                                    " {d.duration_secs / 60}:{d.duration_secs % 60:02} · "
                                    {t!(&lang, "replays-round-count", count = d.rounds as i64)}
                                }
                            }
                        }
                    }
                    div { class: "pane replay-matchup",
                        div { class: "side",
                            span { class: "sub", {t!(&lang, "play-you")} }
                            span { class: "nick", "{row.local_nick}" }
                            span { class: "sub", "{row.local_desc}" }
                        }
                        span { class: "vs", "VS" }
                        div { class: "side them",
                            span { class: "sub", {t!(&lang, "play-opponent")} }
                            span { class: "nick", "{row.remote_nick}" }
                            span { class: "sub", "{row.remote_desc}" }
                        }
                    }
                    // The local side's save, through the read-only save
                    // view (same embedding as the desktop's detail pane).
                    if let Some(handle) = detail_data.as_ref().and_then(|d| d.handle.clone()) {
                        SaveView { handle, editable: false }
                    }
                }
            } else {
                div { class: "pane select-prompt",
                    {t!(&lang, "replays-select-prompt")}
                }
            }
        }
    }
}

/// A replay file's display stem (without the `.tangoreplay`).
fn save_stem_of(file: &str) -> String {
    file.strip_suffix(".tangoreplay").unwrap_or(file).to_string()
}

/// Decode the replay and read both sides' patched ROMs — everything a
/// playback pair boots from. Shared by Watch, the video exporter, and
/// the in-session clip export.
pub(crate) async fn load_pair(
    storage: Option<crate::storage::Storage>,
    lib: Option<crate::library::Library>,
    file: &str,
) -> anyhow::Result<(tango_pvp::replay::Replay, Vec<u8>, Vec<u8>)> {
    let (storage, lib) = match (storage, lib) {
        (Some(s), Some(l)) => (s, l),
        _ => anyhow::bail!("storage unavailable"),
    };
    let bytes = crate::storage::read(storage.replays(), file)
        .await
        .map_err(|e| anyhow::anyhow!("read replay: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("replay disappeared"))?;
    let replay = tango_pvp::replay::Replay::decode(&bytes[..])
        .map_err(|e| anyhow::anyhow!("decode replay: {e}"))?;
    let (local_game, remote_game) = crate::session::replay::resolve_games(&replay)?;
    let rom_of = |game| -> anyhow::Result<String> {
        lib.by_game(game)
            .map(|e| e.file.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "{}'s ROM isn't imported",
                    crate::library::display_name(game)
                )
            })
    };
    let (lf, rf) = (rom_of(local_game)?, rom_of(remote_game)?);
    let mut local_rom = crate::storage::read(storage.roms(), &lf)
        .await
        .map_err(|e| anyhow::anyhow!("read rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("ROM disappeared"))?;
    let mut remote_rom = crate::storage::read(storage.roms(), &rf)
        .await
        .map_err(|e| anyhow::anyhow!("read rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("ROM disappeared"))?;
    // Recorded patches re-apply from the synced tree.
    let patch_of = |side: Option<&tango_pvp::replay::metadata::Side>| {
        side.and_then(|s| s.game_info.as_ref())
            .and_then(|g| g.patch.as_ref())
            .and_then(|p| Some((p.name.clone(), semver::Version::parse(&p.version).ok()?)))
    };
    if let Some((name, ver)) = patch_of(replay.metadata.local_side.as_ref()) {
        local_rom = crate::patches::apply(&storage, &local_rom, local_game, &name, &ver)
            .await
            .map_err(|e| anyhow::anyhow!("apply local patch: {e:#}"))?;
    }
    if let Some((name, ver)) = patch_of(replay.metadata.remote_side.as_ref()) {
        remote_rom = crate::patches::apply(&storage, &remote_rom, remote_game, &name, &ver)
            .await
            .map_err(|e| anyhow::anyhow!("apply remote patch: {e:#}"))?;
    }
    Ok((replay, local_rom, remote_rom))
}

/// Decode the replay, resolve + read both ROMs, boot the playback.
async fn watch(
    runtime: std::rc::Rc<std::cell::RefCell<crate::runtime::Runtime>>,
    storage: Option<crate::storage::Storage>,
    lib: Option<crate::library::Library>,
    file: String,
) -> anyhow::Result<()> {
    let (replay, local_rom, remote_rom) = load_pair(storage, lib, &file).await?;
    // The Watch click is a user gesture — grab the audio sink while we
    // can.
    crate::web::ensure_audio(&runtime).await;
    runtime.borrow_mut().start_replay(replay, local_rom, remote_rom, file)
}
