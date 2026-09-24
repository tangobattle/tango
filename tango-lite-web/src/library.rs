//! The user's library — ROMs, saves, patches — and the operations the
//! UI drives it with.
//!
//! This is a thin arrangement of [`tango_library`] around the two
//! browser seams ([`crate::storage::Files`], [`crate::http::BrowserHttp`]).
//! The scanners, the patch catalog, the save-file rules, the
//! download-and-verify, the BPS apply and the netplay tag resolution are
//! all the desktop's, verbatim — which is the point: a patched match
//! only works if both clients agree byte for byte on what "this patch"
//! means, and the way to guarantee that is to run the same code.
//!
//! An explicit [`Handle`] retains the library and its revision counter.
//! Components read through [`with`] and re-render when [`revision`]
//! changes, without cloning the library's ROM bytes.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tango_library::config::Config;
use tango_library::loadout::Selection;
use tango_library::rom::GameRef;
use tango_library::{game, patch, save, storage::Storage as _, Catalog};

use crate::http::BrowserHttp;
use crate::storage::Files;

/// Where everything lives. A browser build has one root and no notion of
/// a user's documents folder, so the config's data path is simply `/`
/// and the derived `roms/`, `saves/`, `patches/` sit under it exactly as
/// they do on a desktop.
const DATA_ROOT: &str = "/";
const CONFIG_PATH: &str = "/config.json";

pub struct Library {
    pub files: Files,
    pub http: BrowserHttp,
    /// Mutable for the selection memory [`remember_selection`] records.
    pub config: RefCell<Config>,
    pub catalog: Catalog,
}

/// An independently owned library, cheaply cloned by browser tasks.
#[derive(Clone, Default)]
pub struct Handle {
    inner: Rc<RefCell<Option<Rc<Library>>>>,
    revision: Rc<std::cell::Cell<u64>>,
    download: Rc<std::cell::Cell<Option<patch::Progress>>>,
}

/// Open the library: load the persisted files, read the config, run the
/// first scan. Called once, before the first render that needs any of it.
pub async fn open(handle: &Handle) {
    let files = Files::load().await;
    let config = Config::load_or_create(&files, Path::new(CONFIG_PATH), Path::new(DATA_ROOT));
    let library = Rc::new(Library {
        files,
        http: BrowserHttp,
        config: RefCell::new(config),
        catalog: Catalog::new(),
    });
    *handle.inner.borrow_mut() = Some(library);
    rescan(handle).await;
}

/// Read the library. `None` before [`open`] has finished, which is the
/// only state the UI has to spell (a splash line).
pub fn with<R>(handle: &Handle, f: impl FnOnce(&Library) -> R) -> Option<R> {
    let library = handle.inner.borrow().clone()?;
    Some(f(&library))
}

/// The current revision. Any UI that reads the library should re-render
/// when this changes.
pub fn revision(handle: &Handle) -> u64 {
    handle.revision.get()
}

pub(crate) fn touch(handle: &Handle) {
    handle.revision.set(handle.revision.get() + 1);
}

/// Re-read everything from storage. Cheap here — the "disk" is memory —
/// but it still re-parses every save and re-reads every package, so it
/// runs on the operations that change what's there, not on every render.
pub async fn rescan(handle: &Handle) {
    let Some(library) = handle.inner.borrow().clone() else {
        return;
    };
    let config = library.config.borrow().clone();
    let listings = Catalog::list(&library.files, &config).await;
    library.catalog.rescan(&library.files, &config, &listings);

    touch(handle);
}

/// Every game with a ROM in the library, in registry order so the list
/// doesn't reshuffle between renders.
pub fn owned_games(handle: &Handle) -> Vec<GameRef> {
    with(handle, |library| {
        let roms = library.catalog.roms.read();
        game::GAMES.iter().copied().filter(|g| roms.contains_key(g)).collect()
    })
    .unwrap_or_default()
}

/// The stored ROM file's extension. Cosmetic — detection is by content
/// — but a DS dump filed as `.gba` would be a lie. The console is read
/// off the input word: only a DS reads X/Y.
fn rom_extension(game: GameRef) -> &'static str {
    use tango_session::keys;
    if game.pvp.keys_mask() & (keys::X | keys::Y) != 0 {
        "nds"
    } else {
        "gba"
    }
}

/// File an imported ROM under the game it turns out to be.
///
/// Returns what it was, or `None` if this build has no support for it —
/// including a recognized game whose CRC32 doesn't match, since a bad
/// dump desyncs rather than failing cleanly.
pub async fn import_rom(handle: &Handle, file_name: &str, bytes: &[u8]) -> Option<GameRef> {
    // Detection grows a trimmed dump back to the full image, so that —
    // not the dropped file — is what gets stored.
    let mut bytes = bytes.to_vec();
    let game = game::detect(&mut bytes)?;
    let (family, variant) = game.family_and_variant();
    let ext = rom_extension(game);
    let path = with(handle, |library| {
        library
            .config
            .borrow()
            .roms_path()
            .join(format!("{family}-{variant}.{ext}"))
    })?;
    log::info!("importing {file_name} as {family} v{variant}");
    with(handle, |library| library.files.write(&path, &bytes))?.ok()?;
    rescan(handle).await;
    Some(game)
}

/// Store a save. Named after the game plus whatever the user called the
/// file, so several saves per game coexist the way they do on a desktop.
pub async fn import_save(handle: &Handle, file_name: &str, bytes: &[u8]) -> bool {
    // Which game it belongs to is decided by which game can parse it —
    // the same rule `save::scan_saves` applies on the way back out.
    let Some(game) = save::detect_game(bytes) else {
        return false;
    };
    let (family, variant) = game.family_and_variant();
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "save".to_string());
    let path = match with(handle, |library| {
        library
            .config
            .borrow()
            .saves_path()
            .join(format!("{family}-{variant}-{stem}.sav"))
    }) {
        Some(p) => p,
        None => return false,
    };
    if with(handle, |library| library.files.write(&path, bytes)).is_none() {
        return false;
    }
    rescan(handle).await;
    true
}

/// Write out a starter save from one of the game's templates — the
/// patch's own first, then the game's bundled ones — so a first-time
/// player can get into a link battle without hunting down a `.sav` on a
/// phone. Named the way the desktop names one: the game, then the
/// template in the family's own words ("Heat Guts", "Saito/Normal"),
/// with a counter when that's taken. Returns the new file.
pub async fn create_starter_save(
    handle: &Handle,
    game: GameRef,
    patch: Option<(String, semver::Version)>,
    template_name: &str,
) -> Option<PathBuf> {
    let patch = patch.as_ref().map(|(name, version)| (name.as_str(), version));
    let created = with(handle, |library| {
        let template = save::template(game, &library.catalog.patches.read(), patch, template_name)?;
        let saves_path = library.config.borrow().saves_path();
        let label = crate::lang::save_template_name(game, template_name);
        let name = save::free_name(
            &library.files,
            &saves_path,
            &save::suggest_name(&crate::lang::game_name(game), Some(&label)),
        );
        save::create(&library.files, &saves_path, &name, template.as_ref())
            .inspect_err(|e| log::warn!("creating a starter save: {e}"))
            .ok()
    })??;
    rescan(handle).await;
    Some(created)
}

/// The starter saves this game can be created from under `patch`, as
/// `(template, label)` — BN3 alone has eight, and they are only
/// distinguishable by name.
pub fn save_templates(
    handle: &Handle,
    game: GameRef,
    patch: Option<(&str, &semver::Version)>,
) -> Vec<(String, String)> {
    with(handle, |library| {
        save::templates(game, &library.catalog.patches.read(), patch)
            .into_iter()
            .map(|(name, _)| {
                let label = crate::lang::save_template_name(game, &name);
                (name, label)
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Persist a session's savedata back over the file it was loaded from.
/// Single-player only — a PvP match runs entirely off the committed
/// in-memory image and never writes anyone's save. `true` once the
/// bytes are in storage.
pub fn write_save(handle: &Handle, path: &Path, bytes: &[u8]) -> bool {
    match with(handle, |library| library.files.write(path, bytes)) {
        Some(Ok(())) => {}
        Some(Err(e)) => {
            log::warn!("writing {}: {e}", path.display());
            return false;
        }
        None => return false,
    }
    touch(handle);
    true
}

pub async fn delete_save(handle: &Handle, path: PathBuf) {
    if let Some(Err(e)) = with(handle, |library| save::delete(&library.files, &path)) {
        log::warn!("deleting {}: {e}", path.display());
    }
    rescan(handle).await;
}

/// Forget a game entirely: its ROM and every save filed under it. The
/// one destructive action here, and the reason the library screen asks
/// before running it — a phone is where you notice you're out of space,
/// and also where an accidental tap is easiest.
pub async fn delete_game(handle: &Handle, game: GameRef) {
    let (family, variant) = game.family_and_variant();
    let ext = rom_extension(game);
    let paths = with(handle, |library| {
        let mut paths = vec![library
            .config
            .borrow()
            .roms_path()
            .join(format!("{family}-{variant}.{ext}"))];
        if let Some(saves) = library.catalog.saves.read().get(&game) {
            paths.extend(saves.iter().map(|s| s.path.clone()));
        }
        paths
    })
    .unwrap_or_default();
    for path in paths {
        let _ = with(handle, |library| library.files.remove_file(&path));
    }
    rescan(handle).await;
}

/// Bytes the library is holding, for the storage line. The browser's own
/// quota accounting is asynchronous and origin-wide; this is the part of
/// it we put there.
pub fn bytes_used(handle: &Handle) -> u64 {
    with(handle, |library| library.files.bytes_used()).unwrap_or(0)
}

// ---------------------------------------------------------------------
// Replays

/// Where recordings live. Not a config path like the others, because
/// `Config` derives its own from the data root and this one has to
/// agree with it.
pub fn replays_path(handle: &Handle) -> PathBuf {
    with(handle, |library| library.config.borrow().replays_path())
        .unwrap_or_else(|| PathBuf::from(DATA_ROOT).join("replays"))
}

/// A recorded match: its key, and enough of the metadata to list it
/// without decoding the whole file.
#[derive(Clone, PartialEq)]
pub struct ReplayEntry {
    pub path: PathBuf,
    pub name: String,
    /// Both sides' nicknames, recorder first.
    pub sides: (String, String),
    /// Which game it was, already localized.
    pub game: String,
    /// Milliseconds since the epoch, from the match clock.
    pub ts: u64,
    pub bytes: u64,
}

/// Everything recorded, newest first, off the library's replay index —
/// the same one the desktop lists, so a recording neither build can
/// replay is hidden here too.
pub fn replays(handle: &Handle) -> Vec<ReplayEntry> {
    with(handle, |library| {
        library
            .catalog
            .replays
            .read()
            .iter()
            .map(|replay| {
                let side = |s: Option<&tango_replay::metadata::Side>| s.map(|s| s.nickname.clone()).unwrap_or_default();
                let game = replay
                    .local_side()
                    .and_then(|s| s.game_info.as_ref())
                    .map(|g| crate::lang::game_name_of(&g.rom_family, g.rom_variant as u8))
                    .unwrap_or_default();
                ReplayEntry {
                    name: replay
                        .path
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    sides: (side(replay.local_side()), side(replay.remote_side())),
                    game,
                    ts: replay.metadata.ts,
                    bytes: replay.len,
                    path: replay.path.clone(),
                }
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Decode a recording with both sides' games and the exact ROMs they
/// were played on — the same resolution the desktop runs, which
/// playback and the exporter share so they re-simulate the identical
/// pair.
pub fn open_replay(
    handle: &Handle,
    path: &Path,
) -> Result<(tango_replay::Replay, tango_library::replays::ResolvedRoms), String> {
    with(handle, |library| {
        library
            .catalog
            .open_replay(&library.files, &library.config.borrow(), path)
    })
    .ok_or_else(|| "library not open".to_owned())?
    .map_err(|e| e.to_string())
}

/// Take in a `.tangoreplay` from the device — one recorded on a
/// desktop, or sent by an opponent.
pub async fn import_replay(handle: &Handle, file_name: &str, bytes: &[u8]) -> bool {
    // Decode before storing: a file that won't parse is one the
    // replays list would show and then fail to open.
    if tango_replay::Replay::decode(std::io::Cursor::new(bytes)).is_err() {
        log::warn!("{file_name}: not a readable replay");
        return false;
    }
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "imported".to_string());
    let path = replays_path(handle).join(format!("{stem}.{}", tango_replay::EXTENSION));
    if with(handle, |library| library.files.write(&path, bytes)).is_none() {
        return false;
    }
    rescan(handle).await;
    true
}

/// The bytes behind a recording, for handing to a download.
pub fn replay_bytes(handle: &Handle, path: &Path) -> Option<Vec<u8>> {
    with(handle, |library| library.files.read(path))?.ok()
}

/// File a finished recording, handed over by the sink's `Drop` (see
/// [`crate::recording`]), and index it.
pub async fn write_replay(handle: &Handle, path: &Path, bytes: &[u8]) {
    if with(handle, |library| library.files.write(path, bytes)).is_none() {
        return;
    }
    rescan(handle).await;
}

// ---------------------------------------------------------------------
// Patches

/// Pull the repo index. Runs once at startup and on the Patches screen's
/// refresh; see [`crate::http`] for why it isn't polled on a timer here
/// the way the desktop polls it.
pub async fn fetch_index(handle: &Handle) -> Result<(), String> {
    let Some(library) = handle.inner.borrow().clone() else {
        return Err("library not open".into());
    };
    let (repo, patches_path) = {
        let config = library.config.borrow();
        (config.patch_repo_url(), config.patches_path())
    };
    let changed = patch::fetch_index(&library.http, &library.files, &repo, &patches_path)
        .await
        .map_err(|e| e.to_string())?;
    if changed {
        rescan(handle).await;
    }
    Ok(())
}

/// Download and install one patch version, hash-verified against the
/// index before anything is written.
pub async fn install_patch(handle: &Handle, name: String, version: semver::Version) -> Result<(), String> {
    let Some(library) = handle.inner.borrow().clone() else {
        return Err("library not open".into());
    };
    let entry = library
        .catalog
        .patches
        .read()
        .entry(&name, &version)
        .cloned()
        .ok_or_else(|| format!("{name} {version} is not in the index"))?;

    let (repo, patches_path) = {
        let config = library.config.borrow();
        (config.patch_repo_url(), config.patches_path())
    };
    let outcome = patch::download(
        &library.http,
        &library.files,
        &repo,
        &patches_path,
        &name,
        &version,
        &entry,
        |progress| {
            set_download(handle, Some(progress));
            true
        },
    )
    .await;
    set_download(handle, None);
    match outcome.map_err(|e| e.to_string())? {
        patch::Outcome::Installed => {
            rescan(handle).await;
            Ok(())
        }
        patch::Outcome::Cancelled => Err("cancelled".into()),
    }
}

pub async fn uninstall_patch(handle: &Handle, name: String, version: semver::Version) {
    let Some(library) = handle.inner.borrow().clone() else {
        return;
    };
    let patches_path = library.config.borrow().patches_path();
    if let Err(e) = patch::uninstall(&library.files, &patches_path, &name, &version) {
        log::warn!("uninstall {name} {version}: {e}");
    }
    rescan(handle).await;
}

fn set_download(handle: &Handle, progress: Option<patch::Progress>) {
    handle.download.set(progress);
    touch(handle);
}

/// Bytes of the in-flight package download, for the progress bar.
pub fn download_progress(handle: &Handle) -> Option<patch::Progress> {
    handle.download.get()
}

/// Every `(name, newest version)` in the catalog that supports `game` —
/// what the patch picker offers for the current pick, favorites first.
pub fn patches_for(handle: &Handle, game: GameRef) -> Vec<(String, semver::Version, bool)> {
    with(handle, |library| {
        let catalog = library.catalog.patches.read();
        let favorites = library.config.borrow().favorite_patches.clone();
        tango_library::loadout::patch_names_for(&catalog, &[game], &favorites)
            .into_iter()
            .filter_map(|name| {
                let version = catalog.newest_version(&name, Some(game))?;
                let installed = catalog.is_installed(&name, &version);
                Some((name, version, installed))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// Record `selection` in the config and write it out, so the next visit
/// comes back to the same game, save, and patch. A no-op before the
/// library has opened, and when nothing changed.
pub fn remember_selection(handle: &Handle, selection: &Selection) {
    with(handle, |library| {
        let mut config = library.config.borrow_mut();
        let before = (
            config.last_family.clone(),
            config.last_game.clone(),
            config.last_save_per_family.clone(),
            config.last_patch_per_save.clone(),
        );
        selection.persist(&mut config);
        let after = (
            &config.last_family,
            &config.last_game,
            &config.last_save_per_family,
            &config.last_patch_per_save,
        );
        if (&before.0, &before.1, &before.2, &before.3) != after {
            if let Err(e) = config.save(&library.files, Path::new(CONFIG_PATH)) {
                log::warn!("saving config: {e}");
            }
        }
    });
}

/// Remember `match_type` as the one `family` was last picked in, so the
/// lobby offers it again next time. A no-op before the library has
/// opened, and when nothing changed.
pub fn remember_match_type(handle: &Handle, family: &str, match_type: (u8, u8)) {
    with(handle, |library| {
        let mut config = library.config.borrow_mut();
        if config.last_match_type_per_family.get(family) == Some(&match_type) {
            return;
        }
        config.last_match_type_per_family.insert(family.to_owned(), match_type);
        if let Err(e) = config.save(&library.files, Path::new(CONFIG_PATH)) {
            log::warn!("saving config: {e}");
        }
    });
}
