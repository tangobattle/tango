//! The Patches tab: the synced patch list (title, versions, their
//! netplay-compatibility tags and supported games) and the Update
//! button that syncs the configured repo into OPFS.

use dioxus::prelude::*;

use super::{use_ctx, Ctx};
use crate::library;
use crate::t;

/// Bumped after a sync so the scan re-runs (shared with the Play
/// tab's patch picker).
pub static PATCHES_REV: GlobalSignal<u64> = Signal::global(|| 0);

#[derive(Clone, PartialEq)]
enum SyncState {
    Idle,
    Running,
    Done(usize),
    Failed(String),
}

#[component]
pub fn PatchesScreen() -> Element {
    let Ctx {
        config, storage, patches, ..
    } = use_ctx();
    let mut sync_state = use_signal(|| SyncState::Idle);
    let lang = crate::i18n::LANG.read().clone();
    let list = patches.read().clone().unwrap_or_default();
    let running = matches!(*sync_state.read(), SyncState::Running);

    rsx! {
        section { class: "pane", style: "flex:1; min-height:0; display:flex; flex-direction:column;",
            div { class: "option-row",
                h2 { {t!(&lang, "tab-patches")} }
                div { style: "flex:1" }
                button {
                    class: "btn primary",
                    disabled: running,
                    onclick: move |_| {
                        let storage = storage.read().clone().flatten();
                        let repo = config.peek().patch_repo.clone();
                        sync_state.set(SyncState::Running);
                        spawn(async move {
                            let Some(storage) = storage else {
                                sync_state.set(SyncState::Failed("storage unavailable".into()));
                                return;
                            };
                            match crate::patches::sync(&storage, &repo).await {
                                Ok(n) => {
                                    sync_state.set(SyncState::Done(n));
                                    *PATCHES_REV.write() += 1;
                                }
                                Err(e) => sync_state.set(SyncState::Failed(format!("{e:#}"))),
                            }
                        });
                    },
                    if running {
                        {t!(&lang, "patches-updating")}
                    } else {
                        {t!(&lang, "patches-update")}
                    }
                }
            }
            match sync_state.read().clone() {
                SyncState::Failed(e) => rsx! {
                    p { class: "sub flash bad", {t!(&lang, "patches-update-failed")} " — {e}" }
                },
                SyncState::Done(n) => rsx! {
                    p { class: "sub flash ok", "{n} file(s) updated" }
                },
                _ => rsx! {},
            }
            if list.is_empty() {
                p { class: "sub", {t!(&lang, "patches-select-prompt")} }
            }
            ul { class: "rows", style: "flex:1; min-height:0;",
                for patch in list.iter().cloned() {
                    li {
                        div { class: "btn row", style: "flex-direction:column; align-items:flex-start;",
                            div { "{patch.title}" }
                            for (version, v) in patch.versions.iter() {
                                span { class: "sub",
                                    "v{version} · "
                                    {t!(&lang, "patches-netplay-compatibility")}
                                    " {v.netplay_compatibility} · "
                                    {
                                        v.supported
                                            .iter()
                                            .map(|g| library::short_name(*g))
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
