//! The user's library — ROMs, saves, patches — and the operations the
//! UI drives it with.
//!
//! This is a thin arrangement of [`tango_library`] around the two
//! browser seams ([`crate::storage::Files`], [`crate::http::BrowserHttp`]).
//! The scanners, the patch catalog, the download-and-verify, the BPS
//! apply and the netplay tag resolution are all the desktop's, verbatim
//! — which is the point: a patched match only works if both clients
//! agree byte for byte on what "this patch" means, and the way to
//! guarantee that is to run the same code.
//!
//! The library lives in a thread-local rather than a Dioxus signal: it
//! holds every ROM's bytes, the UI reads it far more often than it
//! changes, and none of it is `Clone`. Components read through
//! [`with`] and re-render off a [`Revision`] counter.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use tango_library::config::Config;
use tango_library::rom::GameRef;
use tango_library::{game, patch, rom, save, storage::Storage as _};

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
    pub config: Config,
    pub roms: rom::Scanner,
    pub saves: save::Scanner,
    pub patches: patch::Scanner,
}

thread_local! {
    static LIBRARY: RefCell<Option<Rc<Library>>> = const { RefCell::new(None) };
    /// Bumped on every change the UI should re-render for. Components
    /// mirror it into a signal, so a scan or an import repaints without
    /// the library itself having to be reactive.
    static REVISION: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// Open the library: load the persisted files, read the config, run the
/// first scan. Called once, before the first render that needs any of it.
pub async fn open() {
    let files = Files::load().await;
    let config = Config::load_or_create(&files, Path::new(CONFIG_PATH), Path::new(DATA_ROOT));
    let library = Rc::new(Library {
        files,
        http: BrowserHttp,
        config,
        roms: rom::Scanner::new(),
        saves: save::Scanner::new(),
        patches: patch::Scanner::new(),
    });
    LIBRARY.with(|l| *l.borrow_mut() = Some(library));
    rescan().await;
}

/// Read the library. `None` before [`open`] has finished, which is the
/// only state the UI has to spell (a splash line).
pub fn with<R>(f: impl FnOnce(&Library) -> R) -> Option<R> {
    let library = LIBRARY.with(|l| l.borrow().clone())?;
    Some(f(&library))
}

/// The current revision. Any UI that reads the library should re-render
/// when this changes.
pub fn revision() -> u64 {
    REVISION.with(|r| r.get())
}

pub(crate) fn touch() {
    REVISION.with(|r| r.set(r.get() + 1));
}

/// Re-read everything from storage. Cheap here — the "disk" is memory —
/// but it still re-parses every save and re-reads every package, so it
/// runs on the operations that change what's there, not on every render.
pub async fn rescan() {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return;
    };
    let config = &library.config;

    let rom_listing = library.files.list(&rom::scan_roots(&config.roms_path())).await;
    library
        .roms
        .rescan_if_changed(&rom_listing, || Some(rom::scan_roms(&library.files, &rom_listing)));

    let save_listing = library.files.list(&[config.saves_path()]).await;
    library
        .saves
        .rescan_if_changed(&save_listing, || Some(save::scan_saves(&library.files, &save_listing)));

    let patch_listing = library.files.list(&patch::scan_roots(&config.patches_path())).await;
    library.patches.rescan_if_changed(&patch_listing, || {
        match patch::scan(&library.files, &config.patches_path(), &patch_listing) {
            Ok(catalog) => Some(catalog),
            Err(e) => {
                log::warn!("patch scan failed: {e}");
                None
            }
        }
    });

    touch();
}

/// Every game with a ROM in the library, in registry order so the list
/// doesn't reshuffle between renders.
pub fn owned_games() -> Vec<GameRef> {
    with(|library| {
        let roms = library.roms.read();
        game::GAMES.iter().copied().filter(|g| roms.contains_key(g)).collect()
    })
    .unwrap_or_default()
}

/// File an imported ROM under the game it turns out to be.
///
/// Returns what it was, or `None` if this build has no support for it —
/// including a recognized game whose CRC32 doesn't match, since a bad
/// dump desyncs rather than failing cleanly.
pub async fn import_rom(file_name: &str, bytes: &[u8]) -> Option<GameRef> {
    let game = game::detect(bytes)?;
    let (family, variant) = game.family_and_variant();
    let path = with(|library| library.config.roms_path().join(format!("{family}-{variant}.gba")))?;
    log::info!("importing {file_name} as {family} v{variant}");
    with(|library| library.files.write(&path, bytes))?.ok()?;
    rescan().await;
    Some(game)
}

/// Store a save. Named after the game plus whatever the user called the
/// file, so several saves per game coexist the way they do on a desktop.
pub async fn import_save(file_name: &str, bytes: &[u8]) -> bool {
    // Which game it belongs to is decided by which game can parse it —
    // the same rule `save::scan_saves` applies on the way back out.
    let Some(game) = game::GAMES.iter().copied().find(|g| g.parse_save(bytes).is_ok()) else {
        return false;
    };
    let (family, variant) = game.family_and_variant();
    let stem = Path::new(file_name)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "save".to_string());
    let path = match with(|library| library.config.saves_path().join(format!("{family}-{variant}-{stem}.sav"))) {
        Some(p) => p,
        None => return false,
    };
    if with(|library| library.files.write(&path, bytes)).is_none() {
        return false;
    }
    rescan().await;
    true
}

/// Write out a game's bundled starter save, so a first-time player can
/// get into a link battle without hunting down a `.sav` on a phone.
///
/// Named after the template in the family's own words — "Heat Guts",
/// "Saito/Normal" — because that name is the only thing distinguishing
/// one starter from another, and a file called `bn3-0-new` tells you
/// nothing about which style is in it.
pub async fn create_starter_save(game: GameRef, template_name: &str) -> bool {
    let Some((template_name, template)) = game
        .save_templates
        .iter()
        .find(|(name, _)| *name == template_name)
        .or_else(|| game.save_templates.first())
    else {
        return false;
    };
    // The checksum has to be rebuilt before the dump is a save file the
    // game — or our own scanner — will accept. Skipping it writes bytes
    // that parse back as nothing, which reads as "New save did nothing".
    let mut save = template.clone_box();
    save.rebuild_checksum();
    let Some(path) = free_save_path(game, &crate::lang::save_template_name(game, template_name)) else {
        return false;
    };
    if with(|library| library.files.write(&path, &save.to_sram_dump())).is_none() {
        return false;
    }
    rescan().await;
    true
}

/// The starter saves this game ships, as `(template, label)` — BN3
/// alone has eight, and they are only distinguishable by name.
pub fn save_templates(game: GameRef) -> Vec<(String, String)> {
    game.save_templates
        .iter()
        .map(|(name, _)| (name.to_string(), crate::lang::save_template_name(game, name)))
        .collect()
}

/// A save path for `game` called `name`, with a counter appended if that
/// is taken — pressing "New save" twice should give two saves, not
/// silently overwrite the first.
fn free_save_path(game: GameRef, name: &str) -> Option<PathBuf> {
    let (family, variant) = game.family_and_variant();
    // Saves are keyed by game in the filename, and the name comes from a
    // translation, so strip what a path can't carry.
    let name: String = name
        .chars()
        .map(|c| if "/\\?%*:|\"<>".contains(c) { '-' } else { c })
        .collect();
    let name = name.trim();
    let saves = with(|library| library.config.saves_path())?;
    for attempt in 0..100 {
        let stem = if attempt == 0 {
            format!("{family}-{variant}-{name}")
        } else {
            format!("{family}-{variant}-{name} {}", attempt + 1)
        };
        let path = saves.join(format!("{stem}.sav"));
        if !with(|library| library.files.is_file(&path)).unwrap_or(false) {
            return Some(path);
        }
    }
    None
}

/// Persist a session's savedata back over the file it was loaded from.
/// Single-player only — a PvP match runs entirely off the committed
/// in-memory image and never writes anyone's save.
pub fn write_save(path: &Path, bytes: &[u8]) {
    if with(|library| library.files.write(path, bytes)).is_none() {
        return;
    }
    touch();
}

pub async fn delete_file(path: PathBuf) {
    let _ = with(|library| library.files.remove_file(&path));
    rescan().await;
}

/// Forget a game entirely: its ROM and every save filed under it. The
/// one destructive action here, and the reason the library screen asks
/// before running it — a phone is where you notice you're out of space,
/// and also where an accidental tap is easiest.
pub async fn delete_game(game: GameRef) {
    let (family, variant) = game.family_and_variant();
    let paths = with(|library| {
        let mut paths = vec![library.config.roms_path().join(format!("{family}-{variant}.gba"))];
        if let Some(saves) = library.saves.read().get(&game) {
            paths.extend(saves.iter().map(|s| s.path.clone()));
        }
        paths
    })
    .unwrap_or_default();
    for path in paths {
        let _ = with(|library| library.files.remove_file(&path));
    }
    rescan().await;
}

/// Bytes the library is holding, for the storage line. The browser's own
/// quota accounting is asynchronous and origin-wide; this is the part of
/// it we put there.
pub fn bytes_used() -> u64 {
    with(|library| library.files.bytes_used()).unwrap_or(0)
}

// ---------------------------------------------------------------------
// Replays

/// Where recordings live. Not a config path like the others, because
/// `Config` derives its own from the data root and this one has to
/// agree with it.
pub fn replays_path() -> PathBuf {
    PathBuf::from(DATA_ROOT).join("replays")
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

/// Everything recorded, newest first. Reads each file's header — the
/// metadata sits at the front, so this doesn't decode the input stream.
pub fn replays() -> Vec<ReplayEntry> {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return Vec::new();
    };
    let mut out: Vec<ReplayEntry> = Vec::new();
    for path in library.files.paths_under(&replays_path()) {
        let Ok(raw) = library.files.read(&path) else { continue };
        let mut cursor = std::io::Cursor::new(&raw);
        let Ok((_, local_player_index, metadata)) = tango_replay::read_metadata(&mut cursor) else {
            log::warn!("replay scan: {}: unreadable", path.display());
            continue;
        };
        let side = |s: Option<&tango_replay::metadata::Side>| s.map(|s| s.nickname.clone()).unwrap_or_default();
        let (p1, p2) = (side(metadata.p1_side.as_ref()), side(metadata.p2_side.as_ref()));
        let mine = local_player_index == 0;
        let game = metadata
            .p1_side
            .as_ref()
            .and_then(|s| s.game_info.as_ref())
            .map(|g| crate::lang::game_name_of(&g.rom_family, g.rom_variant as u8))
            .unwrap_or_default();
        out.push(ReplayEntry {
            name: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            sides: if mine { (p1, p2) } else { (p2, p1) },
            game,
            ts: metadata.ts,
            bytes: raw.len() as u64,
            path,
        });
    }
    // Newest first: the one you just played is the one you want.
    out.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| b.name.cmp(&a.name)));
    out
}

/// Decode a recording into something playable.
pub fn read_replay(path: &Path) -> Result<tango_replay::Replay, String> {
    let raw = with(|library| library.files.read(path))
        .ok_or_else(|| "library not open".to_string())?
        .map_err(|e| e.to_string())?;
    tango_replay::Replay::decode(std::io::Cursor::new(raw)).map_err(|e| e.to_string())
}

/// Take in a `.tangoreplay` from the device — one recorded on a
/// desktop, or sent by an opponent.
pub async fn import_replay(file_name: &str, bytes: &[u8]) -> bool {
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
    let path = replays_path().join(format!("{stem}.{}", tango_replay::EXTENSION));
    if with(|library| library.files.write(&path, bytes)).is_none() {
        return false;
    }
    touch();
    true
}

/// The bytes behind a recording, for handing to a download.
pub fn replay_bytes(path: &Path) -> Option<Vec<u8>> {
    with(|library| library.files.read(path))?.ok()
}

/// File a finished recording. Synchronous because it is called from the
/// sink's `Drop` (see [`crate::recording`]), which can't await — the
/// storage mirror's write-back is asynchronous on its own.
pub fn write_replay(path: &Path, bytes: &[u8]) {
    if with(|library| library.files.write(path, bytes)).is_none() {
        return;
    }
    touch();
}

// ---------------------------------------------------------------------
// Patches

/// Pull the repo index. Runs once at startup and on the Patches screen's
/// refresh; see [`crate::http`] for why it isn't polled on a timer here
/// the way the desktop polls it.
pub async fn fetch_index() -> Result<(), String> {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return Err("library not open".into());
    };
    let changed = patch::fetch_index(
        &library.http,
        &library.files,
        &library.config.patch_repo_url(),
        &library.config.patches_path(),
    )
    .await
    .map_err(|e| e.to_string())?;
    if changed {
        rescan().await;
    }
    Ok(())
}

/// Download and install one patch version, hash-verified against the
/// index before anything is written.
pub async fn install_patch(name: String, version: semver::Version) -> Result<(), String> {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return Err("library not open".into());
    };
    let entry = library
        .patches
        .read()
        .entry(&name, &version)
        .cloned()
        .ok_or_else(|| format!("{name} {version} is not in the index"))?;

    let outcome = patch::download(
        &library.http,
        &library.files,
        &library.config.patch_repo_url(),
        &library.config.patches_path(),
        &name,
        &version,
        &entry,
        |progress| {
            set_download(Some(progress));
            true
        },
    )
    .await;
    set_download(None);
    match outcome.map_err(|e| e.to_string())? {
        patch::Outcome::Installed => {
            rescan().await;
            Ok(())
        }
        patch::Outcome::Cancelled => Err("cancelled".into()),
    }
}

pub async fn uninstall_patch(name: String, version: semver::Version) {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return;
    };
    if let Err(e) = patch::uninstall(&library.files, &library.config.patches_path(), &name, &version) {
        log::warn!("uninstall {name} {version}: {e}");
    }
    rescan().await;
}

thread_local! {
    static DOWNLOAD: std::cell::Cell<Option<patch::Progress>> = const { std::cell::Cell::new(None) };
}

fn set_download(progress: Option<patch::Progress>) {
    DOWNLOAD.with(|d| d.set(progress));
    touch();
}

/// Bytes of the in-flight package download, for the progress bar.
pub fn download_progress() -> Option<patch::Progress> {
    DOWNLOAD.with(|d| d.get())
}

/// Every `(name, newest version)` in the catalog that supports `game` —
/// what the patch picker offers for the current pick.
pub fn patches_for(game: GameRef) -> Vec<(String, semver::Version, bool)> {
    with(|library| {
        let catalog = library.patches.read();
        catalog
            .names()
            .into_iter()
            .filter_map(|name| {
                let version = catalog.newest_version(name, Some(game))?;
                let installed = catalog.is_installed(name, &version);
                Some((name.to_string(), version, installed))
            })
            .collect()
    })
    .unwrap_or_default()
}

/// The ROM to actually run for `(game, patch)`: the stored dump with the
/// patch's BPS applied, or the dump itself when unpatched.
pub fn patched_rom(game: GameRef, patch_pick: Option<&(String, semver::Version)>) -> Result<Vec<u8>, String> {
    let Some(library) = LIBRARY.with(|l| l.borrow().clone()) else {
        return Err("library not open".into());
    };
    let raw = library
        .roms
        .read()
        .get(&game)
        .cloned()
        .ok_or_else(|| format!("no rom for {}", game.family_and_variant().0))?;
    let Some((name, version)) = patch_pick else {
        return Ok(raw);
    };
    patch::apply_patch(
        &library.files,
        &raw,
        game,
        &library.config.patches_path(),
        name,
        version,
    )
    .map_err(|e| format!("apply {name} {version}: {e}"))
}
