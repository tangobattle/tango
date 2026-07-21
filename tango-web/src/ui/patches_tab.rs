//! The Patches tab, laid out like the desktop's (`tabs/patches.rs`): a
//! top strip (search / sync status / Update) over the fixed 280px list
//! — favorites first, then alphabetical — beside the detail pane: the
//! meta pane (favorite star, title, version picker, authors / license /
//! source / supported games / netplay compatibility) and the synced
//! README rendered as markdown.

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
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
        mut config,
        storage,
        patches,
        ..
    } = use_ctx();
    let mut sync_state = use_signal(|| SyncState::Idle);
    let mut selected = use_signal(|| Option::<String>::None);
    let mut search = use_signal(String::new);
    // Per-patch version pick; unset = the newest.
    let mut version_pick = use_signal(|| Option::<(String, String)>::None);

    let lang = crate::i18n::LANG.read().clone();
    let list = patches.read().clone().unwrap_or_default();
    let running = matches!(*sync_state.read(), SyncState::Running);
    let favorites = config.read().patch_favorites.clone();

    // Favorites first, then alphabetical (the scan is already
    // title-sorted); filter = case-insensitive substring on name+title.
    let needle = search.read().to_lowercase();
    let mut visible: Vec<crate::patches::Patch> = list
        .iter()
        .filter(|p| {
            needle.is_empty() || p.name.to_lowercase().contains(&needle) || p.title.to_lowercase().contains(&needle)
        })
        .cloned()
        .collect();
    visible.sort_by(|a, b| {
        let fa = !favorites.contains(&a.name);
        let fb = !favorites.contains(&b.name);
        fa.cmp(&fb).then_with(|| a.title.cmp(&b.title))
    });

    let selected_patch = selected
        .read()
        .clone()
        .and_then(|n| list.iter().find(|p| p.name == n).cloned());

    // The selected patch's synced README.md.
    let readme = use_resource(move || {
        let name = selected.read().clone();
        let storage = storage.read().clone().flatten();
        async move {
            let (Some(storage), Some(name)) = (storage, name) else {
                return None;
            };
            crate::patches::readme(&storage, &name).await
        }
    });
    let readme_html = readme.read().clone().flatten().map(|md| {
        // Raw HTML blocks are dropped (not passed through): the repo URL
        // is user-configurable, so README markdown is untrusted input.
        let parser = pulldown_cmark::Parser::new(&md).filter(|ev| {
            !matches!(
                ev,
                pulldown_cmark::Event::Html(_) | pulldown_cmark::Event::InlineHtml(_)
            )
        });
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, parser);
        html
    });

    let on_update = move |_| {
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
    };

    rsx! {
        // --- top strip: search / status / Update ---
        section { class: "pane filter-strip",
            input {
                class: "search patches",
                r#type: "text",
                placeholder: t!(&lang, "patches-search-placeholder"),
                value: "{search}",
                oninput: move |evt: FormEvent| search.set(evt.value()),
            }
            match sync_state.read().clone() {
                SyncState::Failed(e) => rsx! {
                    span { class: "sub flash bad", {t!(&lang, "patches-update-failed", error = e)} }
                },
                SyncState::Done(n) => rsx! {
                    span { class: "sub flash ok", "{n} file(s) updated" }
                },
                SyncState::Running => rsx! {
                    span { class: "sub", {t!(&lang, "patches-updating")} }
                },
                SyncState::Idle => rsx! {},
            }
            div { class: "grow" }
            button {
                class: "btn",
                disabled: running,
                onclick: on_update,
                icons::RefreshCw {}
                {t!(&lang, "patches-update")}
            }
        }
        // --- fixed list beside the detail pane ---
        div { class: "patches-split",
            div { class: "pane patch-list",
                if visible.is_empty() {
                    p { class: "sub empty", {t!(&lang, "patches-select-prompt")} }
                }
                for patch in visible.iter().cloned() {
                    {
                        let is_selected = selected.read().as_deref() == Some(patch.name.as_str());
                        let is_fav = favorites.contains(&patch.name);
                        let name = patch.name.clone();
                        rsx! {
                            button {
                                class: if is_selected { "patch-row selected" } else { "patch-row" },
                                onclick: move |_| selected.set(Some(name.clone())),
                                span { class: "title-line",
                                    if is_fav {
                                        span { class: "fav-star", "★" }
                                    }
                                    "{patch.title}"
                                }
                                span { class: "caption", "{patch.name}" }
                            }
                        }
                    }
                }
            }
            if let Some(patch) = selected_patch.as_ref() {
                div { class: "patch-detail",
                    div { class: "pane detail-title",
                        div { class: "head",
                            // Favorite toggle: gold star when set.
                            button {
                                class: "btn ghost fav-btn",
                                title: if favorites.contains(&patch.name) { t!(&lang, "patches-unfavorite") } else { t!(&lang, "patches-favorite") },
                                onclick: {
                                    let name = patch.name.clone();
                                    move |_| {
                                        config.with_mut(|c| {
                                            if let Some(i) = c.patch_favorites.iter().position(|f| *f == name) {
                                                c.patch_favorites.remove(i);
                                            } else {
                                                c.patch_favorites.push(name.clone());
                                            }
                                        });
                                    }
                                },
                                if favorites.contains(&patch.name) {
                                    span { class: "fav-star", "★" }
                                } else {
                                    span { class: "fav-star off", "☆" }
                                }
                            }
                            span { class: "title", "{patch.title}" }
                            div { class: "grow" }
                            // Version picker (semver descending).
                            select {
                                onchange: {
                                    let name = patch.name.clone();
                                    move |evt: FormEvent| {
                                        version_pick.set(Some((name.clone(), evt.value())));
                                    }
                                },
                                for v in patch.versions.keys().rev() {
                                    option {
                                        value: "{v}",
                                        selected: version_pick.read().as_ref()
                                            .is_some_and(|(n, pv)| *n == patch.name && *pv == v.to_string())
                                            || (version_pick.read().as_ref().is_none_or(|(n, _)| *n != patch.name)
                                                && Some(v) == patch.versions.keys().next_back()),
                                        "v{v}"
                                    }
                                }
                            }
                        }
                        div { class: "meta",
                            if !patch.authors.is_empty() {
                                span { class: "sub",
                                    {t!(&lang, "patches-details-authors")}
                                    " {patch.authors.join(\", \")}"
                                }
                            }
                            if let Some(license) = patch.license.as_ref() {
                                span { class: "sub",
                                    {t!(&lang, "patches-details-license")}
                                    " {license}"
                                }
                            }
                            if let Some(source) = patch.source.as_ref() {
                                span { class: "sub",
                                    {t!(&lang, "patches-details-source")}
                                    " "
                                    a { href: "{source}", target: "_blank", rel: "noreferrer noopener", "{source}" }
                                }
                            }
                            {
                                // The shown version's supported games +
                                // netplay compatibility.
                                let shown = version_pick
                                    .read()
                                    .as_ref()
                                    .filter(|(n, _)| *n == patch.name)
                                    .and_then(|(_, v)| semver::Version::parse(v).ok())
                                    .and_then(|v| patch.versions.get_key_value(&v).map(|(k, x)| (k.clone(), x.clone())))
                                    .or_else(|| {
                                        patch
                                            .versions
                                            .iter()
                                            .next_back()
                                            .map(|(k, x)| (k.clone(), x.clone()))
                                    });
                                match shown {
                                    Some((_, v)) => rsx! {
                                        span { class: "sub",
                                            {t!(&lang, "patches-details-games")}
                                            " "
                                            {
                                                v.supported
                                                    .iter()
                                                    .map(|g| library::display_name(*g))
                                                    .collect::<Vec<_>>()
                                                    .join(", ")
                                            }
                                        }
                                        span { class: "sub",
                                            {t!(&lang, "patches-netplay-compatibility")}
                                            " {v.netplay_compatibility}"
                                        }
                                    },
                                    None => rsx! {},
                                }
                            }
                        }
                    }
                    div { class: "pane readme",
                        if let Some(html) = readme_html.as_ref() {
                            div { class: "md", dangerous_inner_html: "{html}" }
                        } else {
                            p { class: "sub", {t!(&lang, "patches-readme-placeholder")} }
                        }
                    }
                }
            } else {
                div { class: "pane select-prompt",
                    {t!(&lang, "patches-select-prompt")}
                }
            }
        }
    }
}
