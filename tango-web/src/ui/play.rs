//! The Play tab, laid out like the desktop's (`tabs/play/mod.rs`):
//! a selector strip up top (game row over save row), the game body in
//! the middle (stands in for the desktop's save-view until that port
//! lands — logo + title + the Play button in its header), and the
//! bottom link-code band (the Fight button arms with the netplay port
//! at M3).

use base64::Engine as _;
use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::library;
use crate::runtime::SAVES_REV;

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
fn logo_data_url(game: library::GameRef) -> Option<String> {
    let mut png = std::io::Cursor::new(Vec::new());
    game.logo_image
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

#[component]
pub fn PlayScreen() -> Element {
    let Ctx {
        runtime,
        mut config,
        storage,
        mut library_rev,
        library,
        mut selected_game,
        mut selected_save,
        ..
    } = use_ctx();
    let launch_flash = use_signal(|| Option::<Flash>::None);

    let lib = library.read().clone().flatten().unwrap_or_default();

    // Prune a stale remembered pick once the scan disagrees (the game
    // was deleted between loads).
    {
        let lib = lib.clone();
        use_effect(move || {
            let picked = selected_game.peek().clone();
            if let Some(slug) = picked {
                if library.read().clone().flatten().is_some() && lib.by_slug(&slug).is_none() {
                    selected_game.set(None);
                    selected_save.set(None);
                }
            }
        });
    }

    // The selected game's compatible saves: list the flat saves/
    // directory and keep the files this game's own parser accepts
    // (content detection, same as the desktop scanner).
    let saves = use_resource(move || {
        let _ = SAVES_REV.read();
        let storage = storage.read().clone().flatten();
        let slug = selected_game.read().clone();
        async move {
            let (Some(storage), Some(slug)) = (storage, slug) else {
                return Vec::new();
            };
            let Some(game) = library::find_by_slug(&slug) else {
                return Vec::new();
            };
            let Ok(files) = crate::storage::list_files(storage.saves()).await else {
                return Vec::new();
            };
            let mut out = Vec::new();
            for (name, handle) in files {
                let Ok(bytes) = crate::storage::read_handle(&handle).await else {
                    continue;
                };
                if game.parse_save(&bytes).is_ok() {
                    out.push(name);
                }
            }
            out
        }
    });
    let save_names = saves.read().clone().unwrap_or_default();

    // A remembered save pick that the listing no longer shows is
    // stale — drop back to the fresh row. (Only once the listing has
    // actually resolved, and no write-back is still in flight.)
    {
        let save_names = save_names.clone();
        use_effect(move || {
            if saves.read().is_some() && *crate::runtime::SAVES_IN_FLIGHT.read() == 0 {
                let picked = selected_save.peek().clone();
                if let Some(pick) = picked {
                    if !save_names.contains(&pick) {
                        selected_save.set(None);
                    }
                }
            }
        });
    }

    let selected_slug = selected_game.read().clone();
    let selected_entry = selected_slug.as_ref().and_then(|s| lib.by_slug(s)).cloned();
    let pick = selected_save.read().clone();
    let logo = selected_entry.as_ref().and_then(|e| logo_data_url(e.game));

    // The launch handler, shared by the Play button.
    let launch = {
        let runtime = runtime.clone();
        let selected_entry = selected_entry.clone();
        move |_| {
            let runtime = runtime.clone();
            let storage = storage.read().clone().flatten();
            let entry = selected_entry.clone();
            let pick = selected_save.peek().clone();
            async move {
                let (Some(storage), Some(entry)) = (storage, entry) else {
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
                let save = match &pick {
                    Some(name) => match crate::storage::read(storage.saves(), name).await {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            flash(launch_flash, format!("couldn't read save: {e}"), false, 5000);
                            return;
                        }
                    },
                    None => None,
                };
                // A fresh boot persists into a new file named for the
                // game; a picked save writes back to itself.
                let save_file = pick
                    .clone()
                    .unwrap_or_else(|| format!("{}.sav", library::game_slug(entry.game)));
                if let Err(e) =
                    crate::web::boot(runtime, entry.game, rom, save, Some(save_file)).await
                {
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
                            selected_game.set(None);
                            selected_save.set(None);
                        } else {
                            selected_save.set(config.peek().last_saves.get(&v).cloned());
                            config.with_mut(|c| c.last_game = Some(v.clone()));
                            selected_game.set(Some(v));
                        }
                    },
                    option { value: "", selected: selected_slug.is_none(), "no game" }
                    for entry in lib.entries.iter() {
                        option {
                            value: "{library::game_slug(entry.game)}",
                            selected: Some(library::game_slug(entry.game)) == selected_slug,
                            "{library::display_name(entry.game)}"
                        }
                    }
                }
                // Patches join at M5; the slots hold the desktop's
                // geometry meanwhile.
                select { class: "patch", disabled: true,
                    option { "No patch" }
                }
                select { class: "version", disabled: true,
                    option { "—" }
                }
            }
            div { class: "save-row",
                select {
                    disabled: selected_entry.is_none(),
                    onchange: move |evt: FormEvent| {
                        let v = evt.value();
                        let slug = selected_game.peek().clone();
                        if v.is_empty() {
                            selected_save.set(None);
                            if let Some(slug) = slug {
                                config.with_mut(|c| { c.last_saves.remove(&slug); });
                            }
                        } else {
                            selected_save.set(Some(v.clone()));
                            if let Some(slug) = slug {
                                config.with_mut(|c| { c.last_saves.insert(slug, v); });
                            }
                        }
                    },
                    option { value: "", selected: pick.is_none(), "(fresh save)" }
                    for name in save_names.iter() {
                        option {
                            value: "{name}",
                            selected: pick.as_deref() == Some(name.as_str()),
                            "{name}"
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
            if let Some(entry) = selected_entry.clone() {
                div { class: "head",
                    span { class: "title", "{library::display_name(entry.game)}" }
                    div { class: "grow" }
                    button {
                        class: "btn primary",
                        onclick: launch,
                        icons::Play {}
                        "Play"
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
                        if let Some(p) = pick.as_deref() {
                            "save: {p}"
                        } else {
                            "starting from a fresh save"
                        }
                    }
                }
            } else if lib.entries.is_empty() {
                div { class: "game-card",
                    span { class: "title", "No games yet" }
                    p { class: "sub",
                        "Import a Mega Man Battle Network ROM (.gba) — drop it anywhere \
                         or use the Import button above. Files stay in private browser \
                         storage on this device."
                    }
                }
            } else {
                div { class: "game-card",
                    span { class: "sub", "Pick a game above." }
                }
            }
        }

        // --- bottom band: the link-code strip (arms at M3) ---
        div { class: "bottom-band",
            input {
                r#type: "text",
                placeholder: "link code",
                spellcheck: "false",
                autocomplete: "off",
                disabled: true,
                title: "Netplay arrives with the crossplay milestone",
            }
            button {
                class: "btn primary",
                disabled: true,
                title: "Netplay arrives with the crossplay milestone",
                icons::Swords {}
                "Fight"
            }
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
            "Import…"
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
