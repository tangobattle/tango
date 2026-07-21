//! The Replays tab: the OPFS `replays/` listing with download and
//! delete — the web slice of the desktop's replay browser. Each row
//! decodes the replay's own metadata (shared 0x1C reader), so the
//! listing shows the matchup rather than bare file names, with Watch
//! (linear in-browser playback), download (opens in the desktop
//! client too), and delete.

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::runtime::SAVES_REV;

/// A watch attempt's status line (booting the pair takes seconds).
static WATCH_STATUS: GlobalSignal<Option<String>> = Signal::global(|| None);

/// One listed replay, metadata decoded from the file's own header.
#[derive(Clone, PartialEq)]
struct Row {
    file: String,
    size: usize,
    summary: String,
    complete: bool,
}

/// Bumped on delete so the listing refreshes.
static REPLAYS_REV: GlobalSignal<u64> = Signal::global(|| 0);

#[component]
pub fn ReplaysScreen() -> Element {
    let Ctx {
        runtime, storage, library, ..
    } = use_ctx();

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
                let (summary, complete) = match tango_pvp::replay::Replay::decode(&bytes[..]) {
                    Ok(replay) => {
                        let meta = &replay.metadata;
                        let local = meta
                            .local_side
                            .as_ref()
                            .map(|s| s.nickname.clone())
                            .unwrap_or_default();
                        let remote = meta
                            .remote_side
                            .as_ref()
                            .map(|s| s.nickname.clone())
                            .unwrap_or_default();
                        let family = meta
                            .local_side
                            .as_ref()
                            .and_then(|s| s.game_info.as_ref())
                            .map(|g| g.rom_family.clone())
                            .unwrap_or_default();
                        (
                            format!("{family} · {local} vs {remote}"),
                            replay.is_complete,
                        )
                    }
                    Err(_) => ("unreadable replay".to_string(), false),
                };
                rows.push(Row {
                    file,
                    size,
                    summary,
                    complete,
                });
            }
            // Newest first: the file names lead with the match clock.
            rows.sort_by(|a, b| b.file.cmp(&a.file));
            rows
        }
    });
    let rows = rows.read().clone().unwrap_or_default();

    rsx! {
        section { class: "pane", style: "flex:1; min-height:0; display:flex; flex-direction:column;",
            h2 { "Replays" }
            if rows.is_empty() {
                p { class: "sub",
                    "No replays yet — finish a netplay match and it lands here. \
                     Downloaded replays open in the desktop client too."
                }
            }
            if let Some(status) = WATCH_STATUS.read().clone() {
                p { class: "sub flash ok", "{status}" }
            }
            ul { class: "rows", style: "flex:1; min-height:0;",
                for row in rows.iter().cloned() {
                    li {
                        div { class: "btn row", style: "align-items:center; gap:10px;",
                            if row.complete {
                                icons::Check {}
                            } else {
                                icons::X {}
                            }
                            div { style: "flex:1; min-width:0;",
                                div { "{row.summary}" }
                                span { class: "sub", "{row.file} · {row.size / 1024} KiB" }
                            }
                            button {
                                class: "btn primary icon-btn",
                                title: "Watch",
                                disabled: !row.complete,
                                onclick: {
                                    let file = row.file.clone();
                                    let runtime = runtime.clone();
                                    move |_| {
                                        let storage = storage.read().clone().flatten();
                                        let lib = library.read().clone().flatten();
                                        let file = file.clone();
                                        let runtime = runtime.clone();
                                        *WATCH_STATUS.write() =
                                            Some("Booting replay…".to_string());
                                        spawn(async move {
                                            let result = watch(runtime, storage, lib, file).await;
                                            match result {
                                                Ok(()) => *WATCH_STATUS.write() = None,
                                                Err(e) => {
                                                    *WATCH_STATUS.write() =
                                                        Some(format!("couldn't watch: {e:#}"));
                                                }
                                            }
                                        });
                                    }
                                },
                                icons::Play {}
                            }
                            button {
                                class: "btn icon-btn",
                                title: "Download (.tangoreplay — opens in the desktop client)",
                                onclick: {
                                    let file = row.file.clone();
                                    move |_| {
                                        let storage = storage.read().clone().flatten();
                                        let file = file.clone();
                                        spawn(async move {
                                            let Some(storage) = storage else { return };
                                            if let Ok(Some(bytes)) =
                                                crate::storage::read(storage.replays(), &file)
                                                    .await
                                            {
                                                crate::web::download_bytes(&file, &bytes);
                                            }
                                        });
                                    }
                                },
                                icons::Download {}
                            }
                            button {
                                class: "btn icon-btn danger",
                                title: "Delete",
                                onclick: {
                                    let file = row.file.clone();
                                    move |_| {
                                        let storage = storage.read().clone().flatten();
                                        let file = file.clone();
                                        spawn(async move {
                                            let Some(storage) = storage else { return };
                                            let _ = crate::storage::delete(
                                                storage.replays(),
                                                &file,
                                            )
                                            .await;
                                            *REPLAYS_REV.write() += 1;
                                        });
                                    }
                                },
                                icons::Trash2 {}
                            }
                        }
                    }
                }
            }
        }
    }
}


/// Decode the replay, resolve + read both ROMs, boot the playback.
async fn watch(
    runtime: std::rc::Rc<std::cell::RefCell<crate::runtime::Runtime>>,
    storage: Option<crate::storage::Storage>,
    lib: Option<crate::library::Library>,
    file: String,
) -> anyhow::Result<()> {
    let (storage, lib) = match (storage, lib) {
        (Some(s), Some(l)) => (s, l),
        _ => anyhow::bail!("storage unavailable"),
    };
    let bytes = crate::storage::read(storage.replays(), &file)
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
    let local_rom = crate::storage::read(storage.roms(), &lf)
        .await
        .map_err(|e| anyhow::anyhow!("read rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("ROM disappeared"))?;
    let remote_rom = crate::storage::read(storage.roms(), &rf)
        .await
        .map_err(|e| anyhow::anyhow!("read rom: {e}"))?
        .ok_or_else(|| anyhow::anyhow!("ROM disappeared"))?;
    // The Watch click is a user gesture — grab the audio sink while we
    // can.
    crate::web::ensure_audio(&runtime).await;
    runtime.borrow_mut().start_replay(replay, local_rom, remote_rom)
}
