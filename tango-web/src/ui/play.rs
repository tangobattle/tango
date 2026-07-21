//! The Play tab, laid out like the desktop's (`tabs/play/mod.rs`):
//! a selector strip up top (game row over save row), the game body in
//! the middle (stands in for the desktop's save-view until that port
//! lands — logo + title + the Play button in its header), and the
//! bottom link-code band (the Fight button arms with the netplay port
//! at M3).
//!
//! Selection is per *family*, mirroring the desktop loadout: the
//! family picker lists every supported family (un-owned ones
//! disabled), ordered like the desktop's `loadout::family_options`;
//! the save picker intermingles the family's variants, each save
//! resolving to its concrete game.

use base64::Engine as _;
use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::t;
use crate::library::{self, GameRef};
use crate::runtime::SAVES_REV;

/// The save picker's fresh-row sentinel: `//fresh/<variant>`. `/` is
/// illegal in stored file names, so it can't collide with a save.
const FRESH_PREFIX: &str = "//fresh/";

fn fresh_sentinel(game: GameRef) -> String {
    format!("{FRESH_PREFIX}{}", game.family_and_variant().1)
}

fn parse_fresh(pick: &str) -> Option<u8> {
    pick.strip_prefix(FRESH_PREFIX)?.parse().ok()
}

#[derive(Clone, PartialEq)]
pub(crate) struct Flash {
    text: String,
    ok: bool,
}

/// Show a message in an inline feedback slot, clearing it after `ms`
/// unless something newer landed meanwhile. Feedback lives next to the
/// control that produced it — there is no global notice bar.
pub(crate) fn flash(mut slot: Signal<Option<Flash>>, text: impl Into<String>, ok: bool, ms: u32) {
    let text = text.into();
    slot.set(Some(Flash {
        text: text.clone(),
        ok,
    }));
    spawn(async move {
        gloo_timers::future::TimeoutFuture::new(ms).await;
        if slot.peek().as_ref().is_some_and(|f| f.text == text) {
            slot.set(None);
        }
    });
}

/// The import feedback slot — global so the shell-level drop handler
/// (the whole content area is one drop target) and the Welcome screen
/// can flash the outcome.
pub(crate) static IMPORT_FLASH: GlobalSignal<Option<Flash>> = Signal::global(|| None);

/// Flash an import's outcome.
pub(crate) fn note_import(counts: &crate::web::ImportCounts) {
    let crate::web::ImportCounts {
        roms,
        saves,
        skipped,
        ..
    } = counts;
    let mut parts = Vec::new();
    if *roms > 0 {
        parts.push(format!("{roms} ROM(s)"));
    }
    if *saves > 0 {
        parts.push(format!("{saves} save(s)"));
    }
    let msg = if parts.is_empty() {
        if *skipped > 0 {
            format!("Nothing imported ({skipped} skipped)")
        } else {
            "Nothing to import".to_string()
        }
    } else if *skipped > 0 {
        format!("Imported {} · {skipped} skipped", parts.join(" + "))
    } else {
        format!("Imported {}", parts.join(" + "))
    };
    flash(IMPORT_FLASH.signal(), msg, *skipped == 0, 4000);
}

/// The rendered form of a [`Flash`].
#[component]
pub(crate) fn FlashText(flash: Flash) -> Element {
    rsx! {
        span { class: if flash.ok { "flash ok" } else { "flash bad" }, "{flash.text}" }
    }
}

/// The selected game's logo as a PNG data URL, freshly encoded per
/// selection (the registry keeps logos as decoded images).
fn logo_data_url(game: GameRef) -> Option<String> {
    let mut png = std::io::Cursor::new(Vec::new());
    game.logo_image
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

/// One row of the save picker: a real save file (resolved to its
/// concrete game) or a fresh-start row for one variant.
#[derive(Clone, PartialEq)]
struct SaveRow {
    /// The select's value: a file name or a fresh sentinel.
    value: String,
    label: String,
    /// The game this row boots.
    slug: String,
    /// Whether that game's ROM is imported (un-owned rows disable,
    /// like the desktop picker greys them).
    available: bool,
}

#[component]
pub fn PlayScreen() -> Element {
    let Ctx {
        runtime,
        mut config,
        storage,
        mut library_rev,
        library,
        patches,
        mut selected_family,
        mut selected_save,
        ..
    } = use_ctx();
    let launch_flash = use_signal(|| Option::<Flash>::None);
    let lang = crate::i18n::LANG.read().clone();

    let lib = library.read().clone().flatten().unwrap_or_default();
    let scanned = library.read().clone().flatten().is_some();
    let family_options = library::family_options(&lib);
    let any_owned = family_options.iter().any(|f| f.available);

    // Prune a stale remembered family once the registry disagrees
    // (persisted config from an older build).
    {
        use_effect(move || {
            let picked = selected_family.peek().clone();
            if let Some(fam) = picked {
                if library::families().iter().all(|f| *f != fam) {
                    selected_family.set(None);
                    selected_save.set(None);
                }
            }
        });
    }

    // The family's save rows: real saves (content-detected across the
    // family's variants, like the desktop's intermingled picker) then
    // one fresh row per variant.
    let saves = use_resource(move || {
        let _ = SAVES_REV.read();
        let storage = storage.read().clone().flatten();
        let family = selected_family.read().clone();
        let lib = library.read().clone().flatten().unwrap_or_default();
        async move {
            let (Some(storage), Some(family)) = (storage, family) else {
                return Vec::new();
            };
            let games: Vec<GameRef> = library::games_in_family(&family).collect();
            let mut rows = Vec::new();
            if let Ok(files) = crate::storage::list_files(storage.saves()).await {
                for (name, handle) in files {
                    let Ok(bytes) = crate::storage::read_handle(&handle).await else {
                        continue;
                    };
                    // A save resolves to exactly one variant within
                    // its family (each parser validates variant).
                    let Some(game) = games.iter().copied().find(|g| g.parse_save(&bytes).is_ok())
                    else {
                        continue;
                    };
                    rows.push(SaveRow {
                        value: name.clone(),
                        label: if games.len() > 1 {
                            format!("{name} · {}", library::variant_short_name(game))
                        } else {
                            name.clone()
                        },
                        slug: library::game_slug(game),
                        available: lib.by_game(game).is_some(),
                    });
                }
            }
            for game in games {
                rows.push(SaveRow {
                    value: fresh_sentinel(game),
                    label: {
                        let no_save =
                            crate::i18n::t(&crate::i18n::LANG.peek().clone(), "play-no-save");
                        if library::games_in_family(&family).count() > 1 {
                            format!("{no_save} · {}", library::variant_short_name(game))
                        } else {
                            no_save
                        }
                    },
                    slug: library::game_slug(game),
                    available: lib.by_game(game).is_some(),
                });
            }
            rows
        }
    });
    let save_rows = saves.read().clone().unwrap_or_default();

    // A remembered pick the listing no longer offers is stale — drop
    // back to the default row. (Only once the listing has resolved,
    // and no write-back is still in flight.)
    {
        let save_rows = save_rows.clone();
        use_effect(move || {
            if saves.read().is_some() && *crate::runtime::SAVES_IN_FLIGHT.read() == 0 {
                let picked = selected_save.peek().clone();
                if let Some(pick) = picked {
                    if !save_rows.iter().any(|r| r.value == pick) {
                        selected_save.set(None);
                    }
                }
            }
        });
    }

    // The synced patches, for the picker + eligibility.
    let all_patches = patches.read().clone().unwrap_or_default();
    let family = selected_family.read().clone();
    let pick = selected_save.read().clone();

    // The row the next boot uses: the explicit pick, else the first
    // available row (saves lead, so a family with saves defaults to
    // its first save; a family without defaults to fresh).
    let active_row = pick
        .as_ref()
        .and_then(|p| save_rows.iter().find(|r| &r.value == p))
        .or_else(|| save_rows.iter().find(|r| r.available))
        .cloned();
    let active_game = active_row.as_ref().and_then(|r| library::find_by_slug(&r.slug));
    let logo = active_game.and_then(logo_data_url);

    let eligible: Vec<crate::patches::Patch> = active_game
        .map(|g| {
            all_patches
                .iter()
                .filter(|p| p.versions.values().any(|v| v.supported.contains(&g)))
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    // The remembered pick, validated against eligibility.
    let patch_pick: Option<(String, semver::Version)> = family
        .as_ref()
        .and_then(|f| config.read().last_patches.get(f).cloned())
        .and_then(|(name, ver)| {
            let p = eligible.iter().find(|p| p.name == name)?;
            match semver::Version::parse(&ver) {
                Ok(v) if p.versions.contains_key(&v) => Some((name, v)),
                // Empty/stale version: newest available.
                _ => p.versions.keys().next_back().cloned().map(|v| (name, v)),
            }
        });

    // The launch handler.
    let launch = {
        let runtime = runtime.clone();
        let active_row = active_row.clone();
        let patch_pick2 = patch_pick.clone();
        move |_| {
            let runtime = runtime.clone();
            let storage = storage.read().clone().flatten();
            let row = active_row.clone();
            let lib = library.read().clone().flatten().unwrap_or_default();
            let patch_pick = patch_pick2.clone();
            async move {
                let (Some(storage), Some(row)) = (storage, row) else {
                    return;
                };
                let Some(game) = library::find_by_slug(&row.slug) else {
                    return;
                };
                let Some(entry) = lib.by_game(game).cloned() else {
                    flash(launch_flash, "That game's ROM isn't imported", false, 5000);
                    return;
                };
                let rom = match crate::storage::read(storage.roms(), &entry.file).await {
                    Ok(Some(rom)) => rom,
                    _ => {
                        flash(launch_flash, "ROM disappeared — re-import it", false, 5000);
                        *library_rev.write() += 1;
                        return;
                    }
                };
                let rom = if let Some((name, ver)) = patch_pick.as_ref() {
                    match crate::patches::apply(&storage, &rom, entry.game, name, ver).await {
                        Ok(r) => r,
                        Err(e) => {
                            flash(launch_flash, format!("patch failed: {e:#}"), false, 5000);
                            return;
                        }
                    }
                } else {
                    rom
                };
                let fresh = parse_fresh(&row.value).is_some();
                let save = if fresh {
                    None
                } else {
                    match crate::storage::read(storage.saves(), &row.value).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            flash(launch_flash, format!("couldn't read save: {e}"), false, 5000);
                            return;
                        }
                    }
                };
                // A fresh boot persists into a new file named for the
                // game; a picked save writes back to itself.
                let save_file = if fresh {
                    format!("{}.sav", row.slug)
                } else {
                    row.value.clone()
                };
                if let Err(e) = crate::web::boot(runtime, game, rom, save, Some(save_file)).await {
                    flash(launch_flash, format!("couldn't start: {e:#}"), false, 5000);
                }
            }
        }
    };

    rsx! {
        // --- selector strip: game row over save row, one pane ---
        section { class: "pane selector-strip",
            div { class: "game-row",
                select {
                    class: "family",
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        if v.is_empty() {
                            selected_family.set(None);
                            selected_save.set(None);
                        } else {
                            selected_save.set(config.peek().last_saves.get(&v).cloned());
                            config.with_mut(|c| c.last_game = Some(v.clone()));
                            selected_family.set(Some(v));
                        }
                    },
                    option { value: "", selected: family.is_none(), {t!(&lang, "play-no-game")} }
                    for opt in family_options.iter() {
                        option {
                            value: "{opt.family}",
                            selected: family.as_deref() == Some(opt.family),
                            disabled: !opt.available,
                            "{opt.display}"
                        }
                    }
                }
                select {
                    class: "patch",
                    disabled: eligible.is_empty(),
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        let fam = selected_family.peek().clone();
                        let Some(fam) = fam else { return };
                        if v.is_empty() {
                            config.with_mut(|c| {
                                c.last_patches.remove(&fam);
                            });
                        } else {
                            // Default to the newest version on pick.
                            config.with_mut(|c| {
                                c.last_patches.insert(fam, (v.clone(), String::new()));
                            });
                        }
                    },
                    option { value: "", selected: patch_pick.is_none(), {t!(&lang, "play-no-patch")} }
                    for p in eligible.iter() {
                        option {
                            value: "{p.name}",
                            selected: patch_pick.as_ref().is_some_and(|(n, _)| *n == p.name),
                            "{p.title}"
                        }
                    }
                }
                select {
                    class: "version",
                    disabled: patch_pick.is_none(),
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        let fam = selected_family.peek().clone();
                        let Some(fam) = fam else { return };
                        config.with_mut(|c| {
                            if let Some(entry) = c.last_patches.get_mut(&fam) {
                                entry.1 = v.clone();
                            }
                        });
                    },
                    if let Some((name, ver)) = patch_pick.as_ref() {
                        for v in eligible
                            .iter()
                            .find(|p| p.name == *name)
                            .map(|p| p.versions.keys().rev().cloned().collect::<Vec<_>>())
                            .unwrap_or_default()
                        {
                            option { value: "{v}", selected: *ver == v, "v{v}" }
                        }
                    } else {
                        option { {t!(&lang, "play-version-placeholder")} }
                    }
                }
            }
            div { class: "save-row",
                select {
                    disabled: family.is_none(),
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        let fam = selected_family.peek().clone();
                        selected_save.set(Some(v.clone()));
                        if let Some(fam) = fam {
                            config.with_mut(|c| {
                                c.last_saves.insert(fam, v);
                            });
                        }
                    },
                    for row in save_rows.iter() {
                        option {
                            value: "{row.value}",
                            selected: active_row.as_ref().is_some_and(|a| a.value == row.value),
                            disabled: !row.available,
                            "{row.label}"
                        }
                    }
                }
                ImportButton {}
            }
            if let Some(f) = IMPORT_FLASH.read().clone() {
                p { class: "sub", FlashText { flash: f } }
            }
        }

        // --- middle body: the game card (save-view stand-in) ---
        section { class: "pane play-body",
            if let Some(game) = active_game {
                div { class: "head",
                    span { class: "title", "{library::display_name(game)}" }
                    div { class: "grow" }
                    button {
                        class: "btn primary",
                        disabled: !active_row.as_ref().is_some_and(|r| r.available),
                        onclick: launch,
                        icons::Play {}
                        {t!(&lang, "play-play")}
                    }
                }
                if let Some(f) = launch_flash.read().clone() {
                    p { class: "sub", FlashText { flash: f } }
                }
                div { class: "game-card",
                    if let Some(url) = logo {
                        img { class: "logo", src: "{url}", alt: "" }
                    }
                    span { class: "sub",
                        if let Some(row) = active_row.as_ref() {
                            if parse_fresh(&row.value).is_some() {
                                {t!(&lang, "play-no-save")}
                            } else {
                                "{row.value}"
                            }
                        }
                    }
                }
            } else if scanned && !any_owned {
                div { class: "game-card",
                    span { class: "title", {t!(&lang, "empty-no-roms-title")} }
                    p { class: "sub", {t!(&lang, "empty-no-roms-body")} }
                    p { class: "sub", {t!(&lang, "web-import-privacy")} }
                }
            } else {
                div { class: "game-card",
                    span { class: "sub", {t!(&lang, "play-no-selection")} }
                }
            }
        }

        // --- bottom band: the link-code strip / live lobby.
        // Netplay commits real save bytes, so a fresh-save row doesn't
        // count as a fightable pick (matching the desktop, whose
        // new-save flow creates the file first).
        super::lobby_band::BottomBand {
            active_game,
            active_save: active_row
                .as_ref()
                .map(|r| r.value.clone())
                .filter(|v| !v.starts_with("//fresh/")),
            active_patch: patch_pick
                .as_ref()
                .map(|(n, v)| (n.clone(), v.to_string())),
        }
    }
}

/// The explicit import picker (drag-and-drop anywhere also works).
#[component]
fn ImportButton() -> Element {
    let Ctx {
        storage,
        mut library_rev,
        ..
    } = use_ctx();
    rsx! {
        label { class: "btn file-btn",
            icons::Upload {}
            {t!(&crate::i18n::LANG.read().clone(), "web-import")}
            input {
                r#type: "file",
                multiple: true,
                accept: ".gba,.srl,.sav",
                onchange: move |evt: FormEvent| {
                    let storage = storage.read().clone().flatten();
                    let files = evt.files();
                    crate::web::reset_file_input(&evt);
                    async move {
                        let Some(storage) = storage else { return };
                        let counts = crate::web::import_files(&storage, files).await;
                        note_import(&counts);
                        *library_rev.write() += 1;
                        *SAVES_REV.write() += 1;
                    }
                },
            }
        }
    }
}
