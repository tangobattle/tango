//! The Replays tab: the OPFS `replays/` listing with download and
//! delete — the web slice of the desktop's replay browser. Each row
//! decodes the replay's own metadata (shared 0x1C reader), so the
//! listing shows the matchup rather than bare file names. In-browser
//! playback rides a later milestone; downloaded files open in the
//! desktop client.

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::runtime::SAVES_REV;

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
    let Ctx { storage, .. } = use_ctx();

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
