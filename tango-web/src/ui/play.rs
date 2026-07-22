//! The Play tab, laid out like the desktop's (`tabs/play/mod.rs`):
//! a selector strip up top (game row over the managed save row —
//! [New save][picker][Import][⋮ actions], with the rename / duplicate
//! / delete / create-from-template forms swapping in), the save view
//! in the middle, and the bottom link-code band.
//!
//! Selection is per *family*, mirroring the desktop loadout: the
//! family picker lists every supported family (un-owned ones
//! disabled), ordered like the desktop's `loadout::family_options`;
//! the save picker intermingles the family's variants, each save
//! resolving to its concrete game. A fresh-save pick (web-only: boot
//! without a save, persist on first run) falls back to the game card.

use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::t;
use crate::library::{self, GameRef};
use crate::runtime::SAVES_REV;
use crate::save_view::{SaveHandle, SaveView};

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
        crate::compat::sleep_ms(ms).await;
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
pub(crate) fn note_import(counts: &crate::host::ImportCounts) {
    let crate::host::ImportCounts {
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

/// The row the next boot uses: the explicit pick, else the first
/// available row (saves lead, so a family with saves defaults to its
/// first save; a family without defaults to fresh).
fn resolve_active_row(rows: &[SaveRow], pick: Option<&str>) -> Option<SaveRow> {
    pick.and_then(|p| rows.iter().find(|r| r.value == p))
        .or_else(|| rows.iter().find(|r| r.available))
        .cloned()
}

/// The synced patches with a version supporting `game`.
fn eligible_patches(all: &[crate::patches::Patch], game: GameRef) -> Vec<crate::patches::Patch> {
    all.iter()
        .filter(|p| p.versions.values().any(|v| v.supported.contains(&game)))
        .cloned()
        .collect()
}

/// The remembered patch pick for `family`, validated against
/// eligibility; an empty/stale remembered version resolves to the
/// newest available.
fn resolve_patch_pick(
    config: &crate::config::Config,
    family: Option<&str>,
    eligible: &[crate::patches::Patch],
) -> Option<(String, semver::Version)> {
    family
        .and_then(|f| config.last_patches.get(f).cloned())
        .and_then(|(name, ver)| {
            let p = eligible.iter().find(|p| p.name == name)?;
            match semver::Version::parse(&ver) {
                Ok(v) if p.versions.contains_key(&v) => Some((name, v)),
                _ => p.versions.keys().next_back().cloned().map(|v| (name, v)),
            }
        })
}

/// Same-key cache for the loaded-save resource: the desktop's
/// `refresh_loaded` dedupe. Without it, any unrelated reactive churn
/// (e.g. a config write) would rebuild the `Loaded` and wipe staged
/// edits. The trailing `u64` is [`SAVES_REV`] — a bump (session
/// write-back, import) forces a rebuild even when the key matches,
/// mirroring the desktop's `ForceRebuildLoaded`.
type LoadedKey = (String, String, Option<(String, String)>, u64);

/// In-flight save-management form state (the desktop's `SaveAction`):
/// the save row swaps to whichever rename / duplicate / delete / create
/// form is open.
#[derive(Clone, PartialEq, Default)]
enum SaveAction {
    #[default]
    None,
    Renaming {
        draft: String,
    },
    /// `draft` prefills with the next free "<stem> (copy)" suggestion
    /// so a plain Enter behaves like a one-click duplicate.
    Duplicating {
        draft: String,
    },
    ConfirmDelete,
    /// Creating a new save from a template. `pick` indexes `options`;
    /// `auto_default` tracks the generated name suggestion so switching
    /// templates regenerates it until the user types their own.
    NewSave {
        draft: String,
        pick: Option<usize>,
        auto_default: Option<String>,
        options: Vec<TemplateOption>,
        /// Save files already in OPFS, for collision-free suggestions.
        existing: std::collections::HashSet<String>,
    },
}

/// One entry in the new-save template picker: a concrete owned-ROM
/// variant × template, labeled "Variant – Template". Patch-shipped
/// templates carry their SRAM; bundled ones resolve through the game's
/// own `save_templates` at confirm.
#[derive(Clone, PartialEq)]
struct TemplateOption {
    slug: String,
    /// Raw template name (empty string = the default template).
    template: String,
    display: String,
    /// A patch-shipped template's raw SRAM; `None` = bundled.
    sram: Option<Vec<u8>>,
}

fn sanitize_filename(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if (c as u32) < 0x20 => ' ',
            c => c,
        })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Appends ` 2`, ` 3`, ... to `base` until `<name>.sav` isn't taken.
fn disambiguate_save_name(existing: &std::collections::HashSet<String>, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !existing.contains(&format!("{draft}.sav")) {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// Next free "<stem> (copy)" / "<stem> (copy N)" name for a duplicate.
fn suggest_duplicate_stem(existing: &std::collections::HashSet<String>, stem: &str) -> String {
    for n in 1..1000 {
        let suffix = if n == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {n})")
        };
        let candidate = format!("{stem}{suffix}");
        if !existing.contains(&format!("{candidate}.sav")) {
            return candidate;
        }
    }
    format!("{stem} (copy)")
}

/// A save file's display stem (the name without the `.sav`).
fn save_stem(file: &str) -> &str {
    file.strip_suffix(".sav").unwrap_or(file)
}

/// Every file currently in OPFS `saves/` — the whole flat directory,
/// not just the visible family's, since all families share it.
async fn existing_save_files(storage: &crate::storage::Storage) -> std::collections::HashSet<String> {
    crate::storage::list_files(storage.saves())
        .await
        .map(|v| v.into_iter().map(|(n, _)| n).collect())
        .unwrap_or_default()
}

/// Localized "<game-variant> - <template>" suggestion for a new save's
/// name, filesystem-sanitized.
fn suggest_save_name(lang: &unic_langid::LanguageIdentifier, game: GameRef, template: &str) -> String {
    let game_name = library::display_name(game);
    let label = template_label(lang, game, template);
    sanitize_filename(&format!("{game_name} - {label}"))
}

/// Localized template label via the family's own `save-<name>` string,
/// falling back to the shared "(default)" key or the raw name.
fn template_label(lang: &unic_langid::LanguageIdentifier, game: GameRef, raw: &str) -> String {
    library::save_template_label(game, raw).unwrap_or_else(|| {
        if raw.is_empty() {
            crate::i18n::t(lang, "save-template-default")
        } else {
            raw.to_string()
        }
    })
}

/// Every (owned-ROM variant × template) creation option for `family`:
/// patch-shipped templates (when a patch is picked) override bundled
/// ones of the same name, and variants the patch doesn't support drop
/// out — the desktop's `creation_template_options`.
async fn creation_template_options(
    lang: &unic_langid::LanguageIdentifier,
    storage: &crate::storage::Storage,
    lib: &library::Library,
    family: &str,
    patch_pick: Option<&(String, semver::Version)>,
    all_patches: &[crate::patches::Patch],
) -> Vec<TemplateOption> {
    let patch_supported: Option<Vec<GameRef>> = patch_pick.and_then(|(name, ver)| {
        let p = all_patches.iter().find(|p| p.name == *name)?;
        Some(p.versions.get(ver)?.supported.clone())
    });
    let mut out = Vec::new();
    for game in library::games_in_family(family) {
        if lib.by_game(game).is_none() {
            continue;
        }
        if let Some(supported) = &patch_supported {
            if !supported.contains(&game) {
                continue;
            }
        }
        let slug = library::game_slug(game);
        let variant = library::variant_short_name(game);
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        // Patch templates first (they override same-named bundled ones).
        if let Some((name, ver)) = patch_pick {
            for (template, sram) in crate::patches::save_templates_for(storage, name, ver, game).await {
                // Only offer templates the game can actually parse.
                if game.parse_save(&sram).is_err() {
                    continue;
                }
                if !seen.insert(template.clone()) {
                    continue;
                }
                let display = format!("{variant} \u{2013} {}", template_label(lang, game, &template));
                out.push(TemplateOption {
                    slug: slug.clone(),
                    template,
                    display,
                    sram: Some(sram),
                });
            }
        }
        for (template, _) in game.save_templates.iter() {
            if !seen.insert(template.to_string()) {
                continue;
            }
            let display = format!("{variant} \u{2013} {}", template_label(lang, game, template));
            out.push(TemplateOption {
                slug: slug.clone(),
                template: template.to_string(),
                display,
                sram: None,
            });
        }
    }
    out
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

    let active_row = resolve_active_row(&save_rows, pick.as_deref());
    let active_game = active_row.as_ref().and_then(|r| library::find_by_slug(&r.slug));
    let logo = active_game.and_then(logo_data_url);

    let eligible: Vec<crate::patches::Patch> = active_game
        .map(|g| eligible_patches(&all_patches, g))
        .unwrap_or_default();
    let patch_pick = resolve_patch_pick(&config.read(), family.as_deref(), &eligible);
    let netplay_idle = matches!(&*crate::netplay::PHASE.read(), crate::netplay::PhaseView::Idle);

    // The loaded save (parsed SRAM + ROM assets + baked icons) behind
    // the save view — the web analog of the desktop's `selection::Loaded`,
    // rebuilt only when the selection key actually changes.
    let loaded_cache: Signal<Option<(LoadedKey, SaveHandle)>> = use_signal(|| None);
    let loaded = use_resource(move || {
        let saves_rev = *SAVES_REV.read();
        let storage = storage.read().clone().flatten();
        let lib = library.read().clone().flatten().unwrap_or_default();
        let family = selected_family.read().clone();
        let pick = selected_save.read().clone();
        let rows = saves.read().clone().unwrap_or_default();
        let all_patches = patches.read().clone().unwrap_or_default();
        let cfg = config.read().clone();
        let mut cache = loaded_cache;
        async move {
            let storage = storage?;
            let row = resolve_active_row(&rows, pick.as_deref())?;
            if parse_fresh(&row.value).is_some() {
                return None;
            }
            let game = library::find_by_slug(&row.slug)?;
            let entry = lib.by_game(game)?.clone();
            let eligible = eligible_patches(&all_patches, game);
            let patch_pick = resolve_patch_pick(&cfg, family.as_deref(), &eligible);
            let key: LoadedKey = (
                row.slug.clone(),
                row.value.clone(),
                patch_pick.as_ref().map(|(n, v)| (n.clone(), v.to_string())),
                saves_rev,
            );
            if let Some((k, h)) = cache.peek().clone() {
                if k == key {
                    return Some(h);
                }
            }
            let rom = crate::storage::read(storage.roms(), &entry.file).await.ok().flatten()?;
            let (rom, overrides) = match patch_pick.as_ref() {
                Some((name, ver)) => {
                    let rom = crate::patches::apply(&storage, &rom, game, name, ver).await.ok()?;
                    let ov = crate::patches::version_overrides(&storage, name, ver).await;
                    (rom, ov)
                }
                None => (rom, Default::default()),
            };
            let save_bytes = crate::storage::read(storage.saves(), &row.value).await.ok().flatten()?;
            let l = crate::save_view::Loaded::build(game, &rom, row.value.clone(), &save_bytes, patch_pick, overrides)
                .map_err(|e| log::warn!("couldn't load save {}: {e:#}", row.value))
                .ok()?;
            let handle = SaveHandle(Rc::new(RefCell::new(l)));
            cache.set(Some((key, handle.clone())));
            Some(handle)
        }
    });
    let loaded_handle = loaded.read().clone().flatten();

    // The launch handler — a cloneable closure returning the boot
    // future, so both the save view's Play button and the fresh-save
    // fallback card share one path.
    let do_launch = {
        let runtime = runtime.clone();
        let active_row = active_row.clone();
        let patch_pick2 = patch_pick.clone();
        move || {
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
                if let Err(e) = crate::host::boot(runtime, game, rom, save, Some(save_file)).await {
                    flash(launch_flash, format!("couldn't start: {e:#}"), false, 5000);
                }
            }
        }
    };

    // ---- save management (the desktop's save_manage flows) ----
    let mut save_action = use_signal(SaveAction::default);
    let mut menu_open = use_signal(|| false);
    // The file the manage actions operate on: a real (non-fresh) pick.
    let managed_save: Option<String> = active_row
        .as_ref()
        .filter(|r| parse_fresh(&r.value).is_none())
        .map(|r| r.value.clone());
    // New-save is offered whenever the family has an owned variant with
    // bundled templates (or a patch is picked, which may ship its own —
    // the form's picker ends up empty if it doesn't).
    let can_new = family.as_deref().is_some_and(|f| {
        library::games_in_family(f)
            .any(|g| lib.by_game(g).is_some() && (!g.save_templates.is_empty() || patch_pick.is_some()))
    });

    // Select a freshly created/renamed file and remember it for the family.
    let mut adopt_save = {
        move |file: String| {
            let fam = selected_family.peek().clone();
            selected_save.set(Some(file.clone()));
            if let Some(fam) = fam {
                config.with_mut(|c| {
                    c.last_saves.insert(fam, file);
                });
            }
            save_action.set(SaveAction::None);
            *SAVES_REV.write() += 1;
        }
    };

    let start_rename = {
        let managed = managed_save.clone();
        move |_| {
            menu_open.set(false);
            if let Some(f) = &managed {
                save_action.set(SaveAction::Renaming {
                    draft: save_stem(f).to_string(),
                });
            }
        }
    };

    let start_duplicate = {
        let managed = managed_save.clone();
        move |_| {
            menu_open.set(false);
            let Some(f) = managed.clone() else { return };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let existing = existing_save_files(&storage).await;
                let draft = suggest_duplicate_stem(&existing, save_stem(&f));
                save_action.set(SaveAction::Duplicating { draft });
            });
        }
    };

    let start_delete = move |_| {
        menu_open.set(false);
        save_action.set(SaveAction::ConfirmDelete);
    };

    // Export = the web stand-in for the desktop's reveal-in-folder: the
    // save downloads through the browser.
    let do_export = {
        let managed = managed_save.clone();
        move |_| {
            menu_open.set(false);
            let Some(f) = managed.clone() else { return };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                if let Ok(Some(bytes)) = crate::storage::read(storage.saves(), &f).await {
                    crate::host::download_bytes(&f, &bytes);
                }
            });
        }
    };

    let start_new = {
        move |_| {
            menu_open.set(false);
            let storage = storage.read().clone().flatten();
            let lib = library.read().clone().flatten().unwrap_or_default();
            let fam = selected_family.peek().clone();
            let all_patches = patches.read().clone().unwrap_or_default();
            let eligible_for_new: Vec<crate::patches::Patch> = fam
                .as_deref()
                .map(|f| {
                    all_patches
                        .iter()
                        .filter(|p| {
                            p.versions
                                .values()
                                .any(|v| v.supported.iter().any(|g| g.family_and_variant().0 == f))
                        })
                        .cloned()
                        .collect()
                })
                .unwrap_or_default();
            let patch_pick = resolve_patch_pick(&config.peek(), fam.as_deref(), &eligible_for_new);
            let lang = crate::i18n::LANG.peek().clone();
            spawn(async move {
                let (Some(storage), Some(fam)) = (storage, fam) else { return };
                let existing = existing_save_files(&storage).await;
                let options =
                    creation_template_options(&lang, &storage, &lib, &fam, patch_pick.as_ref(), &all_patches).await;
                let pick = (options.len() == 1).then_some(0);
                let draft = match pick.and_then(|i| options.get(i)) {
                    Some(o) => {
                        let base = library::find_by_slug(&o.slug)
                            .map(|g| suggest_save_name(&lang, g, &o.template))
                            .unwrap_or_else(|| "new save".to_string());
                        disambiguate_save_name(&existing, &base)
                    }
                    // No single default yet — seed with the variant-neutral
                    // family name while the user picks a template.
                    None => disambiguate_save_name(&existing, &sanitize_filename(&library::family_display_name(&fam))),
                };
                save_action.set(SaveAction::NewSave {
                    auto_default: Some(draft.clone()),
                    draft,
                    pick,
                    options,
                    existing,
                });
            });
        }
    };

    let cancel_action = move |_| save_action.set(SaveAction::None);

    let confirm_rename = {
        let managed = managed_save.clone();
        move |_| {
            let SaveAction::Renaming { draft } = save_action.peek().clone() else {
                return;
            };
            let new_stem = sanitize_filename(draft.trim());
            let Some(old) = managed.clone() else { return };
            if new_stem.is_empty() {
                return;
            }
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let new_file = format!("{new_stem}.sav");
                if new_file == old {
                    save_action.set(SaveAction::None);
                    return;
                }
                if crate::storage::read(storage.saves(), &new_file).await.ok().flatten().is_some() {
                    flash(IMPORT_FLASH.signal(), format!("{new_file} already exists"), false, 4000);
                    return;
                }
                match crate::storage::rename(storage.saves(), &old, &new_file).await {
                    Ok(()) => adopt_save(new_file),
                    Err(e) => flash(IMPORT_FLASH.signal(), format!("rename failed: {e}"), false, 5000),
                }
            });
        }
    };

    let confirm_duplicate = {
        let managed = managed_save.clone();
        move |_| {
            let SaveAction::Duplicating { draft } = save_action.peek().clone() else {
                return;
            };
            let new_stem = sanitize_filename(draft.trim());
            let Some(src) = managed.clone() else { return };
            if new_stem.is_empty() {
                return;
            }
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let new_file = format!("{new_stem}.sav");
                if crate::storage::read(storage.saves(), &new_file).await.ok().flatten().is_some() {
                    flash(IMPORT_FLASH.signal(), format!("{new_file} already exists"), false, 4000);
                    return;
                }
                let Ok(Some(bytes)) = crate::storage::read(storage.saves(), &src).await else {
                    flash(IMPORT_FLASH.signal(), "couldn't read the save", false, 4000);
                    return;
                };
                match crate::storage::write(storage.saves(), &new_file, &bytes).await {
                    Ok(()) => adopt_save(new_file),
                    Err(e) => flash(IMPORT_FLASH.signal(), format!("duplicate failed: {e}"), false, 5000),
                }
            });
        }
    };

    let confirm_delete = {
        let managed = managed_save.clone();
        move |_| {
            let Some(f) = managed.clone() else { return };
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                match crate::storage::delete(storage.saves(), &f).await {
                    Ok(()) => {
                        let fam = selected_family.peek().clone();
                        selected_save.set(None);
                        if let Some(fam) = fam {
                            config.with_mut(|c| {
                                c.last_saves.remove(&fam);
                            });
                        }
                        save_action.set(SaveAction::None);
                        *SAVES_REV.write() += 1;
                    }
                    Err(e) => flash(IMPORT_FLASH.signal(), format!("delete failed: {e}"), false, 5000),
                }
            });
        }
    };

    let confirm_new = {
        move |_| {
            let SaveAction::NewSave { draft, pick, options, .. } = save_action.peek().clone() else {
                return;
            };
            let Some(opt) = pick.and_then(|i| options.get(i).cloned()) else {
                return;
            };
            let Some(game) = library::find_by_slug(&opt.slug) else { return };
            let name = sanitize_filename(draft.trim());
            if name.is_empty() {
                return;
            }
            let storage = storage.read().clone().flatten();
            spawn(async move {
                let Some(storage) = storage else { return };
                let file = format!("{name}.sav");
                if crate::storage::read(storage.saves(), &file).await.ok().flatten().is_some() {
                    flash(IMPORT_FLASH.signal(), format!("{file} already exists"), false, 4000);
                    return;
                }
                // Materialize the template: a patch-shipped SRAM re-parses
                // through the game (and both paths re-checksum — the
                // template's stored checksum predates this clone).
                let sram = match &opt.sram {
                    Some(bytes) => match game.parse_save(bytes) {
                        Ok(mut save) => {
                            save.rebuild_checksum();
                            save.to_sram_dump()
                        }
                        Err(e) => {
                            flash(IMPORT_FLASH.signal(), format!("bad template: {e}"), false, 5000);
                            return;
                        }
                    },
                    None => {
                        let Some(template) = game
                            .save_templates
                            .iter()
                            .find(|(n, _)| *n == opt.template)
                            .map(|(_, s)| *s)
                        else {
                            return;
                        };
                        let mut save = template.clone_box();
                        save.rebuild_checksum();
                        save.to_sram_dump()
                    }
                };
                match crate::storage::write(storage.saves(), &file, &sram).await {
                    Ok(()) => adopt_save(file),
                    Err(e) => flash(IMPORT_FLASH.signal(), format!("create failed: {e}"), false, 5000),
                }
            });
        }
    };

    // Template switch inside the New form: refresh the auto-suggested
    // name unless the user already typed their own.
    let on_template_pick = {
        move |evt: FormEvent| {
            let Ok(i) = evt.value().parse::<usize>() else { return };
            let lang = crate::i18n::LANG.peek().clone();
            save_action.with_mut(|a| {
                if let SaveAction::NewSave {
                    draft,
                    pick,
                    auto_default,
                    options,
                    existing,
                } = a
                {
                    *pick = Some(i);
                    if auto_default.as_deref() == Some(draft.as_str()) {
                        if let Some(o) = options.get(i) {
                            if let Some(g) = library::find_by_slug(&o.slug) {
                                let new_draft = disambiguate_save_name(existing, &suggest_save_name(&lang, g, &o.template));
                                *draft = new_draft.clone();
                                *auto_default = Some(new_draft);
                            }
                        }
                    }
                }
            });
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
                match save_action() {
                    // At rest: [New save][picker][Import][⋮ actions] —
                    // the desktop's row shape, Import being web-only.
                    SaveAction::None => rsx! {
                        button {
                            class: "btn icon-btn",
                            title: t!(&lang, "save-new"),
                            disabled: !can_new,
                            onclick: start_new,
                            icons::FilePlus {}
                        }
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
                        div { class: "menu-anchor",
                            button {
                                class: "btn icon-btn",
                                title: t!(&lang, "save-actions"),
                                disabled: managed_save.is_none(),
                                onclick: move |_| menu_open.set(!menu_open()),
                                icons::EllipsisVertical {}
                            }
                            if menu_open() {
                                div { class: "menu-backdrop", onclick: move |_| menu_open.set(false) }
                                div { class: "save-menu",
                                    button { class: "menu-item", onclick: do_export,
                                        icons::Download {}
                                        {t!(&lang, "web-export")}
                                    }
                                    button { class: "menu-item", onclick: start_duplicate,
                                        icons::Files {}
                                        {t!(&lang, "save-duplicate")}
                                    }
                                    button { class: "menu-item", onclick: start_rename,
                                        icons::Pencil {}
                                        {t!(&lang, "save-rename")}
                                    }
                                    button { class: "menu-item danger", onclick: start_delete,
                                        icons::Trash2 {}
                                        {t!(&lang, "save-delete")}
                                    }
                                }
                            }
                        }
                    },
                    // Every form ends [× cancel][confirm], the confirm
                    // repeating the icon of the action that opened it.
                    SaveAction::Renaming { draft } => rsx! {
                        input {
                            class: "save-name",
                            r#type: "text",
                            placeholder: t!(&lang, "save-name-placeholder"),
                            value: "{draft}",
                            oninput: move |evt: FormEvent| {
                                save_action.with_mut(|a| {
                                    if let SaveAction::Renaming { draft } = a {
                                        *draft = evt.value();
                                    }
                                });
                            },
                            onkeydown: {
                                let confirm = confirm_rename.clone();
                                move |evt: KeyboardEvent| {
                                    if evt.key() == Key::Enter {
                                        confirm(());
                                    }
                                }
                            },
                        }
                        button { class: "btn icon-btn", title: t!(&lang, "save-action-cancel"), onclick: cancel_action,
                            icons::X {}
                        }
                        button { class: "btn primary", onclick: move |_| confirm_rename.clone()(()),
                            icons::Pencil {}
                            {t!(&lang, "save-rename-confirm")}
                        }
                    },
                    SaveAction::Duplicating { draft } => rsx! {
                        input {
                            class: "save-name",
                            r#type: "text",
                            placeholder: t!(&lang, "save-name-placeholder"),
                            value: "{draft}",
                            oninput: move |evt: FormEvent| {
                                save_action.with_mut(|a| {
                                    if let SaveAction::Duplicating { draft } = a {
                                        *draft = evt.value();
                                    }
                                });
                            },
                            onkeydown: {
                                let confirm = confirm_duplicate.clone();
                                move |evt: KeyboardEvent| {
                                    if evt.key() == Key::Enter {
                                        confirm(());
                                    }
                                }
                            },
                        }
                        button { class: "btn icon-btn", title: t!(&lang, "save-action-cancel"), onclick: cancel_action,
                            icons::X {}
                        }
                        button { class: "btn primary", onclick: move |_| confirm_duplicate.clone()(()),
                            icons::Files {}
                            {t!(&lang, "save-duplicate")}
                        }
                    },
                    SaveAction::ConfirmDelete => rsx! {
                        span { class: "sub grow",
                            {t!(&lang, "save-delete-prompt", name = managed_save.as_deref().map(save_stem).unwrap_or_default())}
                        }
                        button { class: "btn icon-btn", title: t!(&lang, "save-action-cancel"), onclick: cancel_action,
                            icons::X {}
                        }
                        button { class: "btn danger", onclick: confirm_delete,
                            icons::Trash2 {}
                            {t!(&lang, "save-delete-confirm")}
                        }
                    },
                    SaveAction::NewSave { draft, pick, options, .. } => rsx! {
                        select {
                            class: "template-pick",
                            onchange: on_template_pick,
                            option { value: "", selected: pick.is_none(), disabled: true, {t!(&lang, "save-template-pick")} }
                            for (i, o) in options.iter().enumerate() {
                                option { value: "{i}", selected: pick == Some(i), "{o.display}" }
                            }
                        }
                        input {
                            class: "save-name",
                            r#type: "text",
                            placeholder: t!(&lang, "save-name-placeholder"),
                            value: "{draft}",
                            oninput: move |evt: FormEvent| {
                                save_action.with_mut(|a| {
                                    if let SaveAction::NewSave { draft, auto_default, .. } = a {
                                        let v = evt.value();
                                        if auto_default.as_deref() != Some(v.as_str()) {
                                            *auto_default = None;
                                        }
                                        *draft = v;
                                    }
                                });
                            },
                            onkeydown: {
                                let confirm = confirm_new.clone();
                                move |evt: KeyboardEvent| {
                                    if evt.key() == Key::Enter {
                                        confirm(());
                                    }
                                }
                            },
                        }
                        button { class: "btn icon-btn", title: t!(&lang, "save-action-cancel"), onclick: cancel_action,
                            icons::X {}
                        }
                        button {
                            class: "btn primary",
                            disabled: pick.is_none() || draft.trim().is_empty(),
                            onclick: move |_| confirm_new.clone()(()),
                            icons::FilePlus {}
                            {t!(&lang, "save-new-confirm")}
                        }
                    },
                }
            }
            if let Some(f) = IMPORT_FLASH.read().clone() {
                p { class: "sub", FlashText { flash: f } }
            }
        }

        // --- middle body: the save view; a fresh-save pick (no SRAM to
        // inspect yet) falls back to the game card, whose Play button
        // creates the save on first boot ---
        if let Some(f) = launch_flash.read().clone() {
            p { class: "sub", FlashText { flash: f } }
        }
        if let Some(handle) = loaded_handle {
            SaveView {
                handle,
                // Mirrors the desktop: Play only fights an idle netplay
                // phase (a lobby or match owns the session otherwise).
                play_enabled: Some(netplay_idle),
                on_play: {
                    let do_launch = do_launch.clone();
                    move |_| {
                        spawn(do_launch());
                    }
                },
            }
        } else {
            section { class: "pane play-body",
                if let Some(game) = active_game {
                    div { class: "head",
                        span { class: "title", "{library::display_name(game)}" }
                        div { class: "grow" }
                        button {
                            class: "btn primary",
                            disabled: !netplay_idle || !active_row.as_ref().is_some_and(|r| r.available),
                            onclick: {
                                let do_launch = do_launch.clone();
                                move |_| do_launch()
                            },
                            icons::Play {}
                            {t!(&lang, "play-play")}
                        }
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
                // The octet-stream entry is iOS's: Safari maps accept
                // to UTI types and greys out extensions it doesn't
                // recognize (.gba/.srl/.sav are all unknown to it), so
                // without a recognized type the Files picker lets
                // nothing be selected. application/octet-stream =
                // public.data = everything; import_files validates the
                // picks anyway.
                accept: ".gba,.srl,.sav,application/octet-stream",
                onchange: move |evt: FormEvent| {
                    let storage = storage.read().clone().flatten();
                    let files = evt.files();
                    crate::host::reset_file_input(&evt);
                    async move {
                        let Some(storage) = storage else { return };
                        let counts = crate::host::import_files(&storage, files).await;
                        note_import(&counts);
                        *library_rev.write() += 1;
                        *SAVES_REV.write() += 1;
                    }
                },
            }
        }
    }
}
