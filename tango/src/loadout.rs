//! The local cartridge, save and gamemode selection shared by the Play tab and lobby.
//! Package cartridges use content identity; legacy families still resolve their
//! variant from the selected save and remember patch overlays per save.

use crate::config;
use crate::i18n::t;
use crate::library::Scanners;
use crate::library::{game, rom};
use crate::ui::style::TEXT_CAPTION;
use crate::ui::widgets;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length};
use unic_langid::LanguageIdentifier;

#[derive(Default)]
pub struct Loadout {
    pub gamemodes: crate::package::gamemode::Selection,
    pub editors: crate::package::editor::selection::Selection,
    pub package_rom: Option<rom::Id>,
    pub package_save: crate::package::selection::Selection,
    /// Selected game *family* (region-specific gamedb family string).
    /// The family picker drives the intermingled save list; the
    /// concrete `game` below is resolved from whichever save is chosen.
    pub family: Option<&'static str>,
    pub game: Option<rom::GameRef>,
    pub save: Option<std::path::PathBuf>,
    /// Active patch overlay. NOT part of the loadout's identity —
    /// see the module docs; persisted per save, not globally.
    pub patch: Option<String>,
    pub patch_version: Option<semver::Version>,
}

#[derive(Debug, Clone)]
pub enum Message {
    GameModeSelected(crate::library::package::ExportRef),
    EditorSelected(crate::library::package::ExportRef),
    GameSelected(GameOption),
    SaveSelected(SaveOption),
    /// Real patch name; empty string is the "no patch" sentinel.
    PatchSelected(String),
    PatchVersionSelected(semver::Version),
    /// Start a failed patch download again / stop one in flight. Act on
    /// the fetch, not on the selection.
    RetryPatchDownload(crate::library::patch::VersionKey),
    CancelPatchDownload(crate::library::patch::VersionKey),
}

/// Side-effects bubble-up, mirroring the tab modules' convention:
/// pure state mutations happen inside [`Loadout::update`]; anything
/// that needs App-level collaborators comes back as an `Effect`.
#[derive(Debug, Clone, Copy)]
pub enum Effect {
    /// Selection (family / game / save / patch / version) changed.
    /// App should rebuild its `LoadedSave` cache, persist config, and
    /// resend lobby settings if one is live.
    SelectionChanged,
    /// Start a failed patch download again.
    RetryDownload,
    /// Stop the download the play strip is reporting on.
    CancelDownload,
}

impl Loadout {
    pub fn rom(&self, scanners: &Scanners) -> Option<rom::Id> {
        self.package_rom
            .or_else(|| self.game.and_then(|game| scanners.roms.read().native_id(game)))
    }

    pub fn choice(&self) -> Option<GameChoice> {
        self.package_rom
            .map(GameChoice::Package)
            .or_else(|| self.family.map(GameChoice::Family))
    }

    pub fn select_package(&mut self, rom: rom::Id, scanners: &Scanners, config: &config::Config) {
        self.package_rom = Some(rom);
        self.package_save = Default::default();
        self.editors = Default::default();
        self.family = None;
        self.game = None;
        self.patch = None;
        self.patch_version = None;
        let remembered = config.package_loadouts.iter().find(|entry| entry.rom == rom);
        self.save = remembered
            .and_then(|entry| entry.save.as_ref())
            .map(|path| config.data_relative_to_absolute(path));
        self.gamemodes = Default::default();
        self.gamemodes
            .refresh(scanners, Some(rom), false, config.disable_bgm_in_pvp);
        if let Some(mode) = remembered.and_then(|entry| entry.gamemode.as_ref()) {
            self.gamemodes.restore(
                mode.reference(tango_script::ExportKind::GameMode),
                scanners,
                Some(rom),
                config.disable_bgm_in_pvp,
            );
        }
        if let Some(editor) = crate::package::editor::selection::remembered(config, rom) {
            self.editors.restore(editor);
        }
    }

    pub fn remember_package(&self, config: &mut config::Config) {
        config.last_package_rom = self.package_rom;
        if let Some(rom) = self.package_rom {
            let entry = tango_library::config::PackageLoadout {
                rom,
                save: self.save.as_ref().and_then(|path| config.data_relative_string(path)),
                gamemode: self.gamemodes.selected.as_ref().map(Into::into),
                editor: self.editors.selected.as_ref().map(Into::into),
            };
            config.package_loadouts.retain(|entry| entry.rom != rom);
            config.package_loadouts.push(entry);
        }
    }

    pub fn refresh_package(
        &mut self,
        scanners: &Scanners,
        config: &config::Config,
        loaded: &mut Option<crate::selection::LoadedSave>,
    ) {
        let Some(id) = self.package_rom else { return };
        if !scanners.package_roms.read().unwrap().roms().contains(&id) {
            self.editors.unavailable();
            self.package_save
                .invalidate("selected package game is no longer available".into(), loaded);
            return;
        }
        let roms = scanners.roms.read();
        let Some(image) = roms.image(id) else {
            self.editors.unavailable();
            self.package_save
                .invalidate("selected ROM is no longer available".into(), loaded);
            return;
        };
        if let Some(error) = &self.gamemodes.error {
            self.editors.unavailable();
            self.package_save.invalidate(error.clone(), loaded);
            return;
        }
        let rom = self
            .gamemodes
            .prepared
            .as_ref()
            .map(|prepared| prepared.rom())
            .unwrap_or(&image.bytes);
        {
            let packages = scanners.packages.read();
            self.editors
                .refresh(&packages, packages.revision(), rom, &config.language.to_string());
        }
        self.package_save
            .refresh(scanners, rom, &self.editors, self.save.as_deref(), loaded);
        if self.save.is_none() {
            self.save = self.package_save.saves().next().cloned();
            self.package_save
                .refresh(scanners, rom, &self.editors, self.save.as_deref(), loaded);
        }
    }

    pub fn has_save(&self, loaded: Option<&crate::selection::LoadedSave>) -> bool {
        let Some(path) = self.save.as_ref() else { return false };
        if self.package_rom.is_some() && self.package_save.error.is_some() {
            return false;
        }
        loaded.is_some_and(|loaded| &loaded.save_path == path) || self.package_save.raw_sram_for(path).is_some()
    }

    pub fn save_sram(&self, loaded: Option<&crate::selection::LoadedSave>) -> Result<Vec<u8>, String> {
        if !self.has_save(loaded) {
            return Err("no valid save selected".into());
        }
        if let Some(loaded) = loaded {
            return loaded.editor.sram(loaded);
        }
        self.package_save
            .raw_sram()
            .map(<[u8]>::to_vec)
            .ok_or_else(|| "no save selected".into())
    }

    pub fn update(&mut self, msg: Message, scanners: &Scanners, config: &config::Config) -> Option<Effect> {
        match msg {
            Message::EditorSelected(reference) => {
                self.editors.select(reference);
                Some(Effect::SelectionChanged)
            }
            Message::GameModeSelected(reference) => {
                self.gamemodes
                    .select(reference, scanners, self.rom(scanners), config.disable_bgm_in_pvp);
                Some(Effect::SelectionChanged)
            }
            Message::GameSelected(f) => {
                let family = match f.choice {
                    GameChoice::Family(family) => family,
                    GameChoice::Package(rom) => {
                        self.select_package(rom, scanners, config);
                        return Some(Effect::SelectionChanged);
                    }
                };
                self.package_rom = None;
                self.package_save = Default::default();
                self.editors = Default::default();
                self.family = Some(family);
                // Auto-land on the family's remembered (or first
                // available) save, which also fixes the concrete game.
                match resolve_family_save(config, scanners, family) {
                    Some((game, path)) => {
                        self.game = Some(game);
                        self.save = Some(path);
                    }
                    None => {
                        self.game = None;
                        self.save = None;
                    }
                }
                // A family switch resets the overlay baseline; the
                // landed save's own patch memory then has the last word.
                self.patch = None;
                self.patch_version = None;
                self.restore_patch_memory(config, scanners);
                Some(Effect::SelectionChanged)
            }
            Message::SaveSelected(s) => {
                // The save carries the concrete game it resolves to;
                // selecting it dynamically switches `game`. The patch
                // follows the save: its remembered overlay applies, and
                // only saves with no memory inherit the current patch
                // (kept only if it supports the new variant).
                self.game = s.game;
                self.family = s.game.map(|game| game.family_and_variant().0);
                self.save = Some(s.path);
                if self.package_rom.is_none() {
                    self.restore_patch_memory(config, scanners);
                }
                Some(Effect::SelectionChanged)
            }
            Message::PatchSelected(name) => {
                if name.is_empty() {
                    self.patch = None;
                    self.patch_version = None;
                } else {
                    let mut version = newest_supporting_version(scanners, &name, self.game);
                    // Patches are no longer hidden by the selected save, so
                    // the user can pick one the current save can't run. The
                    // actively-chosen patch wins: deselect the save (and the
                    // game it resolved to) rather than the patch, then pick a
                    // version unconstrained by the dropped game.
                    if self.game.is_some() && version.is_none() {
                        self.save = None;
                        self.game = None;
                        version = newest_supporting_version(scanners, &name, None);
                    }
                    self.patch_version = version;
                    self.patch = Some(name);
                }
                // With no save selected yet, land on the game's
                // remembered/first save (without applying that save's
                // patch memory — the user just picked this patch
                // explicitly; it'll be recorded for the save instead).
                if self.save.is_none() {
                    if let Some(g) = self.game {
                        self.save = remembered_save_for_game(config, scanners, g);
                    }
                }
                Some(Effect::SelectionChanged)
            }
            Message::RetryPatchDownload(_) => return Some(Effect::RetryDownload),
            Message::CancelPatchDownload(_) => return Some(Effect::CancelDownload),
            Message::PatchVersionSelected(v) => {
                // The version list is filtered to versions supporting
                // the current variant, so nothing else needs fixing up.
                self.patch_version = Some(v);
                Some(Effect::SelectionChanged)
            }
        }
    }

    /// Programmatic save selection (post-delete auto-pick, etc.) —
    /// same semantics as the user picking the save in the strip,
    /// including restoring the save's remembered patch overlay.
    pub fn select_save(
        &mut self,
        game: rom::GameRef,
        path: std::path::PathBuf,
        config: &config::Config,
        scanners: &Scanners,
    ) {
        self.package_rom = None;
        self.package_save = Default::default();
        self.editors = Default::default();
        self.game = Some(game);
        self.family = Some(game.family_and_variant().0);
        self.save = Some(path);
        self.restore_patch_memory(config, scanners);
    }

    /// Apply the selected save's remembered patch overlay:
    /// * recorded patch → restore it (if it still exists and supports
    ///   the save's variant);
    /// * recorded "explicitly unpatched" → clear the patch;
    /// * no record (brand-new save) → keep the current patch if it
    ///   supports the new variant, else clear it.
    fn restore_patch_memory(&mut self, config: &config::Config, scanners: &Scanners) {
        let rel = self.save.as_ref().and_then(|p| config.data_relative_string(p));
        match rel.and_then(|r| config.last_patch_per_save.get(&r).cloned()) {
            Some(Some((name, version))) => {
                let supported = {
                    let patches = scanners.patches.read();
                    self.game
                        .map(|g| patches.supported_games(&name, &version).contains(&g))
                        .unwrap_or(false)
                };
                if supported {
                    self.patch = Some(name);
                    self.patch_version = Some(version);
                } else {
                    self.retain_patch_for_game(scanners);
                }
            }
            Some(None) => {
                self.patch = None;
                self.patch_version = None;
            }
            None => self.retain_patch_for_game(scanners),
        }
    }

    /// Keep the active patch only if it can run the current game:
    /// prefer the already-selected version, fall back to the newest
    /// version that supports the variant, clear the patch entirely
    /// when none does.
    fn retain_patch_for_game(&mut self, scanners: &Scanners) {
        let Some(name) = self.patch.clone() else {
            return;
        };
        let Some(g) = self.game else {
            return;
        };
        let current_ok = {
            let patches = scanners.patches.read();
            self.patch_version
                .as_ref()
                .map(|v| patches.supported_games(&name, v).contains(&g))
                .unwrap_or(false)
        };
        if current_ok {
            return;
        }
        match newest_supporting_version(scanners, &name, Some(g)) {
            Some(v) => self.patch_version = Some(v),
            None => {
                self.patch = None;
                self.patch_version = None;
            }
        }
    }

    /// Single source of truth for the local side's
    /// `protocol::Settings`. App calls this when actually sending
    /// settings on the wire; the lobby view calls it as the "You"
    /// slot fallback during Connecting/Negotiating (before
    /// `lobby.local` has been populated by the netplay loop).
    pub fn make_local_settings(
        &self,
        config: &config::Config,
        lobby: &crate::netplay::LobbyState,
    ) -> tango_net_protocol::control::Settings {
        use tango_net_protocol::control::{GameInfo, PatchInfo, Settings};
        Settings {
            gamemode: self
                .gamemodes
                .prepared
                .as_ref()
                .map(|p| p.configuration().expect("prepared configuration")),
            nickname: config.nickname.clone().unwrap_or_default(),
            match_type: if self.gamemodes.selected.is_some() {
                0
            } else {
                lobby.match_type
            },
            game_info: self.game.filter(|_| self.gamemodes.error.is_none()).map(|game| {
                let (family, variant) = game.family_and_variant();
                GameInfo {
                    family_and_variant: (family.to_string(), variant),
                    patch: match (&self.patch, &self.patch_version) {
                        (Some(name), Some(version)) => Some(PatchInfo {
                            name: name.clone(),
                            version: version.clone(),
                        }),
                        _ => None,
                    },
                    sim_version: game.pvp.sim_version(),
                }
            }),
            blind_setup: lobby.blind_setup,
        }
    }
}

// ---------- Game / Save pick_list options ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GameChoice {
    Family(&'static str),
    Package(rom::Id),
}

#[derive(Clone)]
pub struct GameOption {
    pub choice: GameChoice,
    pub display: String,
    /// `false` unless *every* game in this family has a ROM in the scan
    /// results. Drives sweeten's `.disabled()` closure on the picker so
    /// the row renders greyed out and refuses clicks.
    pub available: bool,
}

impl PartialEq for GameOption {
    fn eq(&self, o: &Self) -> bool {
        self.choice == o.choice
    }
}
impl Eq for GameOption {}
impl std::hash::Hash for GameOption {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.choice.hash(s);
    }
}
impl std::fmt::Display for GameOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}
impl std::fmt::Debug for GameOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

#[derive(Clone, Debug)]
pub struct SaveOption {
    pub path: std::path::PathBuf,
    /// Pre-computed display label: the save's path relative to the
    /// saves dir, forward-slash separated (so nested folders show up
    /// in the picker), behind the short variant tag when the family has
    /// more than one variant to tell apart. Built when the option list
    /// is constructed because `Display::fmt` gets neither the saves root
    /// nor the language as input.
    pub display: String,
    /// Optional native variant; package saves have no Rust save model.
    pub game: Option<rom::GameRef>,
    /// Unavailable ROMs and missing selected saves are disabled.
    pub available: bool,
}

// Identity is the path: a save is the same option regardless of which
// game/availability the family aggregation tagged it with, so picker
// selection-matching and de-dup stay path-based.
impl PartialEq for SaveOption {
    fn eq(&self, o: &Self) -> bool {
        self.path == o.path
    }
}
impl Eq for SaveOption {}
impl std::hash::Hash for SaveOption {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.path.hash(s);
    }
}

impl SaveOption {
    /// `variant` is the save's short variant tag (e.g. "Blue Moon"),
    /// `None` for families with only one variant — nothing to tell apart
    /// there, so the row stays bare.
    pub fn new(
        saves_path: &std::path::Path,
        path: std::path::PathBuf,
        game: Option<rom::GameRef>,
        available: bool,
        variant: Option<&str>,
    ) -> Self {
        let name = path
            .strip_prefix(saves_path)
            .ok()
            .map(|rel| {
                rel.components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .or_else(|| path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_else(|| path.display().to_string());
        // Variant first: the list intermingles a family's variants, and a
        // save's file name seldom says which one it belongs to. Same
        // "<variant> – <name>" shape the new-save template picker uses.
        let display = match variant {
            Some(variant) => format!("{variant} \u{2013} {name}"),
            None => name,
        };
        Self {
            path,
            display,
            game,
            available,
        }
    }
}

impl std::fmt::Display for SaveOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

// ---------- Option builders ----------

/// Every supported family — not just the ones we have ROMs for, so
/// users can see what tango knows about. sweeten's `.disabled()` greys
/// out families that don't have every game's ROM owned; available
/// families stable-sort to the top (then own-region first) so the live
/// ones lead.
///
/// What survives both of those is the order the library itself lists
/// the games in, because the sort is stable and that is the order they
/// were collected in. It is the series' own order, which is the one a
/// player already knows them by — alphabetical on the family string
/// sorted BN1..BN6 by accident and would have sorted the next family
/// wherever its letters happened to fall.
pub fn game_options(lang: &LanguageIdentifier, scanners: &Scanners) -> Vec<GameOption> {
    let roms = scanners.roms.read();
    let mut families: Vec<&'static str> = Vec::new();
    for g in crate::library::game::GAMES.iter() {
        let fam = g.family_and_variant().0;
        if !families.contains(&fam) {
            families.push(fam);
        }
    }
    let mut game_options: Vec<GameOption> = families
        .iter()
        .map(|fam| GameOption {
            choice: GameChoice::Family(fam),
            display: game::family_display_name(lang, fam),
            available: game::games_in_family(fam).all(|g| roms.contains_key(&g)),
        })
        .collect();
    let packages = scanners.package_roms.read().unwrap();
    for id in packages.roms() {
        let Some(image) = roms.image(id) else { continue };
        let title = packages
            .label(id, &lang.to_string())
            .unwrap_or_else(|| t!(lang, "play-package"));
        let file = image
            .paths
            .first()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned());
        let suffix = file.or_else(|| image.native_game.map(|game| game::variant_short_name(lang, game)));
        game_options.push(GameOption {
            choice: GameChoice::Package(id),
            display: suffix.map(|suffix| format!("{title} · {suffix}")).unwrap_or(title),
            available: true,
        });
    }
    game_options.sort_by(|a, b| {
        (!a.available).cmp(&(!b.available)).then_with(|| {
            let ar = match a.choice {
                GameChoice::Family(family) => !game::family_matches_language(lang, family),
                GameChoice::Package(_) => false,
            };
            let br = match b.choice {
                GameChoice::Family(family) => !game::family_matches_language(lang, family),
                GameChoice::Package(_) => false,
            };
            ar.cmp(&br)
        })
    });
    game_options
}

/// Every save across the selected family's color variants, grouped by
/// variant. Each save is tagged with the concrete game it resolves to
/// and whether that game's ROM is owned (so the row can grey out), and —
/// for families with more than one variant — labelled with that
/// variant's short name. A path appears under exactly one variant within
/// a family, but de-dup defensively. The list itself isn't trimmed by
/// the active patch — `save_picker` instead greys out (disables) saves
/// the active patch can't run, so the set stays stable while the
/// patch comes and goes.
pub fn save_options(
    loadout: &Loadout,
    lang: &LanguageIdentifier,
    scanners: &Scanners,
    config: &config::Config,
) -> Vec<SaveOption> {
    let saves_path = config.saves_path();
    let roms = scanners.roms.read();
    let saves = scanners.saves.read();
    let mut save_options: Vec<SaveOption> = Vec::new();
    if loadout.package_rom.is_some() {
        save_options.extend(
            loadout
                .package_save
                .saves()
                .map(|path| SaveOption::new(&saves_path, path.clone(), None, true, None)),
        );
        if let Some(path) = loadout
            .save
            .as_ref()
            .filter(|path| !save_options.iter().any(|option| &option.path == *path))
        {
            save_options.push(SaveOption::new(&saves_path, path.clone(), None, false, None));
        }
    }
    if let Some(family) = loadout.family {
        // Single-variant families (bn1, bn2, exe45) have nothing to tell
        // apart, so their rows carry no tag.
        let multi_variant = game::games_in_family(family).count() > 1;
        let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
        for g in game::games_in_family(family) {
            let available = roms.contains_key(&g);
            let variant = multi_variant.then(|| game::variant_short_name(lang, g));
            if let Some(saves_for_game) = saves.get(&g) {
                for s in saves_for_game {
                    if seen.insert(s.path.clone()) {
                        save_options.push(SaveOption::new(
                            &saves_path,
                            s.path.clone(),
                            Some(g),
                            available,
                            variant.as_deref(),
                        ));
                    }
                }
            }
        }
    }
    // Variant first, so the list reads as one block per variant in the
    // order the games themselves are numbered — matching the tag every
    // row now carries. Within a variant, a folder-first recursive sort:
    // at the first differing path component, whichever side still has
    // components after it (i.e. is "inside a folder at this level")
    // wins. Files at a given level sort below any subfolders at that
    // level, and order among themselves by their extensionless name — so
    // "Blue.sav" sits next to "Blue Moon.sav" instead of wherever the
    // '.' happens to fall against spaces and digits — with the raw name
    // breaking stem ties.
    save_options.sort_by(|a, b| {
        let variant = |o: &SaveOption| o.game.map(|game| game.family_and_variant().1);
        variant(a).cmp(&variant(b)).then_with(|| {
            let av: Vec<&std::ffi::OsStr> = a.path.strip_prefix(&saves_path).unwrap_or(&a.path).iter().collect();
            let bv: Vec<&std::ffi::OsStr> = b.path.strip_prefix(&saves_path).unwrap_or(&b.path).iter().collect();
            for i in 0..av.len().min(bv.len()) {
                if av[i] != bv[i] {
                    let a_is_dir = i + 1 < av.len();
                    let b_is_dir = i + 1 < bv.len();
                    return match (a_is_dir, b_is_dir) {
                        (true, false) => std::cmp::Ordering::Less,
                        (false, true) => std::cmp::Ordering::Greater,
                        (true, true) => av[i].cmp(bv[i]),
                        (false, false) => std::path::Path::new(av[i])
                            .file_stem()
                            .cmp(&std::path::Path::new(bv[i]).file_stem())
                            .then_with(|| av[i].cmp(bv[i])),
                    };
                }
            }
            av.len().cmp(&bv.len())
        })
    });
    save_options
}

/// Patch picker options (with the "no patch" sentinel first) and the
/// currently-selected entry. Filtered to patches that support *any*
/// variant in the selected family — but NOT narrowed to the specific
/// save's variant, so a patch for the family's other variant still
/// shows. Within-family incompatibility is resolved by *deselection*
/// at pick time (selecting a save drops an incompatible patch;
/// selecting a patch drops an incompatible save), never by hiding.
/// With no family selected, the list is empty. Favorites sort first
/// (and get a "★ " label prefix), alphabetical within each group.
pub fn patch_options(
    loadout: &Loadout,
    lang: &LanguageIdentifier,
    scanners: &Scanners,
    config: &config::Config,
) -> (Vec<widgets::Choice<String>>, Option<widgets::Choice<String>>) {
    let patches = scanners.patches.read();
    let family_games: Vec<rom::GameRef> = loadout
        .family
        .map(|f| game::games_in_family(f).collect())
        .unwrap_or_default();
    let mut names: Vec<String> = patches
        .names()
        .into_iter()
        .filter(|name| {
            patches.versions(name).keys().any(|v| {
                family_games
                    .iter()
                    .any(|g| patches.supported_games(name, v).contains(g))
            })
        })
        .map(|n| n.to_owned())
        .collect();
    names.sort_by(|a, b| {
        let fa = config.favorite_patches.contains(a);
        let fb = config.favorite_patches.contains(b);
        fb.cmp(&fa).then_with(|| a.cmp(b))
    });
    let no_patch_option = widgets::Choice::new(String::new(), t!(lang, "play-no-patch"));
    let patch_options: Vec<widgets::Choice<String>> = std::iter::once(no_patch_option.clone())
        .chain(names.into_iter().map(|n| {
            // The list offers everything the repo has, so say which ones
            // aren't here yet — picking one downloads it.
            let mut display = String::new();
            if config.favorite_patches.contains(&n) {
                display.push_str("\u{2605} ");
            }
            if !patches.installed.contains_key(&n) {
                display.push_str("\u{2193} ");
            }
            display.push_str(&n);
            widgets::Choice::new(n, display)
        }))
        .collect();
    let selected_patch = match loadout.patch.as_ref() {
        Some(n) => patch_options.iter().find(|o| &o.value == n).cloned(),
        None => Some(no_patch_option),
    };
    (patch_options, selected_patch)
}

/// Versions of the selected patch that support the current game, newest
/// first. Empty when no patch is selected. Includes versions that aren't
/// downloaded — the repo keeps every release forever, and an old one is
/// exactly what a replay or an opponent may need — marked with a ↓, same
/// as the patch list.
pub fn version_options(loadout: &Loadout, scanners: &Scanners) -> Vec<widgets::Choice<semver::Version>> {
    let patches = scanners.patches.read();
    loadout
        .patch
        .as_ref()
        .map(|name| {
            let game = loadout.game;
            let mut vs: Vec<semver::Version> = patches
                .versions(name)
                .into_keys()
                .filter(|v| {
                    game.map(|g| patches.supported_games(name, v).contains(&g))
                        .unwrap_or(true)
                })
                .collect();
            vs.sort_by(|a, b| b.cmp(a));
            vs.into_iter()
                .map(|v| {
                    let label = if patches.is_installed(name, &v) {
                        v.to_string()
                    } else {
                        format!("\u{2193} {v}")
                    };
                    widgets::Choice::new(v, label)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Whether the selected patch is actually on disk.
///
/// False while its package is downloading, after a failed download, and
/// for a patch with no resolvable version — in every one of those the
/// session would run unpatched, so the entry points stay shut until it
/// lands. `true` when no patch is selected, which is a valid setup.
pub fn patch_ready(loadout: &Loadout, scanners: &Scanners) -> bool {
    match (&loadout.patch, &loadout.patch_version) {
        (None, _) => true,
        (Some(name), Some(version)) => scanners.patches.read().is_installed(name, version),
        (Some(_), None) => false,
    }
}

// ---------- Resolution helpers ----------

/// Newest version of `patch_name` that supports `game` (any version
/// when `game` is `None`).
fn newest_supporting_version(
    scanners: &Scanners,
    patch_name: &str,
    game: Option<rom::GameRef>,
) -> Option<semver::Version> {
    let patches = scanners.patches.read();
    patches.newest_version(patch_name, game)
}

/// The remembered save for `game` if it's still in the scan,
/// otherwise the first save listed for it.
fn remembered_save_for_game(
    config: &config::Config,
    scanners: &Scanners,
    game: rom::GameRef,
) -> Option<std::path::PathBuf> {
    let saves_map = scanners.saves.read();
    let saves_for_game = saves_map.get(&game);
    let remembered = config
        .last_save_per_family
        .get(game.family_and_variant().0)
        .map(|rel| config.data_relative_to_absolute(rel))
        .filter(|p| saves_for_game.map(|v| v.iter().any(|s| s.path == *p)).unwrap_or(false));
    remembered.or_else(|| saves_for_game.and_then(|v| v.first().map(|s| s.path.clone())))
}

/// Pick the (game, save) to land on after a *family* selection.
/// Prefers the remembered save of any owned-ROM game in the family;
/// otherwise the first available save. Grayed (un-owned) saves are
/// never auto-selected.
fn resolve_family_save(
    config: &config::Config,
    scanners: &Scanners,
    family: &str,
) -> Option<(rom::GameRef, std::path::PathBuf)> {
    // One remembered save for the family, and it names the version:
    // whichever of the family's owned games actually has that save is
    // the game to land on.
    if let Some(rel) = config.last_save_per_family.get(family) {
        let roms = scanners.roms.read();
        let saves = scanners.saves.read();
        let abs = config.data_relative_to_absolute(rel);
        for g in game::games_in_family(family) {
            if !roms.contains_key(&g) {
                continue;
            }
            if saves.get(&g).map(|v| v.iter().any(|s| s.path == abs)).unwrap_or(false) {
                return Some((g, abs));
            }
        }
    }
    first_available_family_save(scanners, family)
}

/// First owned-ROM save across every game in `family`, ordered by
/// extensionless name like the picker. Used as the family auto-pick
/// fallback (and by the App's post-delete auto-pick). Returns the
/// concrete game alongside the path so callers can set `game` without
/// re-sniffing the save.
pub fn first_available_family_save(scanners: &Scanners, family: &str) -> Option<(rom::GameRef, std::path::PathBuf)> {
    let roms = scanners.roms.read();
    let saves = scanners.saves.read();
    let mut candidates: Vec<(rom::GameRef, std::path::PathBuf)> = Vec::new();
    for g in game::games_in_family(family) {
        if !roms.contains_key(&g) {
            continue;
        }
        if let Some(v) = saves.get(&g) {
            for s in v {
                candidates.push((g, s.path.clone()));
            }
        }
    }
    candidates.sort_by(|a, b| a.1.file_stem().cmp(&b.1.file_stem()).then_with(|| a.1.cmp(&b.1)));
    candidates.into_iter().next()
}

/// The set of games the currently-selected patch+version supports, or
/// None when no patch (or no version) is selected — meaning "don't
/// filter". Used by the new-save template flow so patch-incompatible
/// variants don't offer templates under an active patch.
pub fn patch_supported_games(
    loadout: &Loadout,
    scanners: &Scanners,
) -> Option<std::collections::HashSet<rom::GameRef>> {
    let name = loadout.patch.as_ref()?;
    let version = loadout.patch_version.as_ref()?;
    let games = scanners.patches.read().supported_games(name, version);
    (!games.is_empty()).then_some(games)
}

/// Whether the currently-selected patch+version supports `game`. True
/// when no patch (or no version) is selected — there's nothing for the
/// save to be incompatible with.
pub fn patch_supports(loadout: &Loadout, scanners: &Scanners, game: rom::GameRef) -> bool {
    patch_supported_games(loadout, scanners)
        .map(|s| s.contains(&game))
        .unwrap_or(true)
}

// ---------- Views ----------

/// Package cartridges offer named gamemodes beside the game picker. Legacy
/// families retain their patch and version controls during migration.
pub fn game_row<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
    downloads: &'a crate::library::patch::Downloads,
) -> Element<'a, Message> {
    // Gaps are explicit children rather than row spacing, because a
    // fetch replaces the patch AND version pickers with one unbroken
    // download strip -- and a uniform spacing would leave a seam down
    // the middle of it. Laid out this way the fixed 8 + 8 + version
    // width comes off the row identically in both states, so the game
    // picker never moves.
    if loadout.package_rom.is_some() {
        if loadout.gamemodes.choices.is_empty() && loadout.gamemodes.selected.is_none() {
            return game_picker(loadout, lang, scanners).width(Length::Fill).into();
        }
        let options: Vec<_> = loadout
            .gamemodes
            .choices
            .iter()
            .map(|choice| widgets::Choice::new(choice.reference.clone(), choice.label(&lang.to_string())))
            .collect();
        let selected = options
            .iter()
            .find(|choice| Some(&choice.value) == loadout.gamemodes.selected.as_ref())
            .cloned();
        return row![
            game_picker(loadout, lang, scanners).width(Length::FillPortion(3)),
            widgets::picker(options, selected, |choice| Message::GameModeSelected(choice.value))
                .placeholder(t!(lang, "lobby-match-type"))
                .width(Length::FillPortion(2)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into();
    }
    let gap = || iced::widget::space::horizontal().width(Length::Fixed(8.0));
    let download = patch_download(loadout, lang, downloads);
    // The game the download belongs to can't be changed out from under
    // it, so its picker goes inert for the duration -- same footprint,
    // same name on it, just not something you can open. Cancelling the
    // fetch hands it back.
    let game: Element<'a, Message> = match (&download, game_label(loadout, lang, scanners)) {
        (Some(_), Some(label)) => widgets::disabled_pick_list(label).width(Length::FillPortion(3)).into(),
        _ => game_picker(loadout, lang, scanners)
            .width(Length::FillPortion(3))
            .into(),
    };
    let rest: Vec<Element<'a, Message>> = match download {
        Some((bar, controls)) => vec![bar, controls],
        None => vec![
            patch_picker(loadout, lang, scanners, config)
                .width(Length::FillPortion(2))
                .into(),
            gap().into(),
            version_picker(loadout, lang, scanners),
        ],
    };

    let mut strip = row![game, gap()].spacing(0).align_y(Alignment::Center);
    for element in rest {
        strip = strip.push(element);
    }
    strip.into()
}

/// Width of the version slot. Shared by the picker and the download
/// strip that replaces it, so a swap can't change the row's shape.
const VERSION_PICKER_WIDTH: f32 = 100.0;

/// The download strip that replaces the patch and version pickers
/// while a fetch is in flight or has failed: one continuous run of
/// bar, percent and a ✕ to call it off, returned as the two adjacent
/// pieces the row needs to keep its widths (they abut, so it reads as
/// one). `None` whenever there's nothing to report, which is the
/// normal case.
///
/// The pieces carry exactly the width and height the pickers they
/// stand in for lay out to, so nothing around them moves.
fn patch_download<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    downloads: &'a crate::library::patch::Downloads,
) -> Option<(Element<'a, Message>, Element<'a, Message>)> {
    let key = (loadout.patch.clone()?, loadout.patch_version.clone()?);
    let piece = |content: Element<'a, Message>, width| {
        Element::from(
            container(content)
                .width(width)
                .height(Length::Fixed(crate::ui::style::PICKER_HEIGHT))
                .align_y(Alignment::Center),
        )
    };
    // The trailing piece swallows the gap the pickers had between them,
    // so the strip has no seam.
    let trailing = Length::Fixed(VERSION_PICKER_WIDTH + 8.0);

    match downloads.get(&key) {
        Some(download) if download.is_running() => {
            let caption = match download.percent() {
                Some(percent) => t!(lang, "play-patch-downloading-progress", percent = percent as i64),
                None => t!(lang, "play-patch-downloading"),
            };
            Some((
                piece(
                    iced::widget::progress_bar(0.0..=1.0, download.fraction().unwrap_or(0.0))
                        .girth(Length::Fixed(4.0))
                        .length(Length::Fill)
                        .style(widgets::slim_progress_bar)
                        .into(),
                    Length::FillPortion(2),
                ),
                piece(
                    // Percent and ✕ sit as one group at the row's right
                    // edge, which puts the slack in a single place
                    // instead of splitting it either side of the
                    // readout. Right-aligning also pins the number's
                    // right edge, so 9% → 100% grows leftwards into
                    // that slack and moves nothing.
                    row![
                        iced::widget::space::horizontal(),
                        text(caption).size(TEXT_CAPTION).style(widgets::muted_text_style),
                        // Calling it off puts both pickers straight back.
                        widgets::icon_button(
                            lucide_icons::Icon::X,
                            t!(lang, "patches-cancel"),
                            Message::CancelPatchDownload(key),
                            [1.0, 1.0],
                        ),
                    ]
                    .spacing(4)
                    .align_y(Alignment::Center)
                    .into(),
                    trailing,
                ),
            ))
        }
        Some(crate::library::patch::Download::Failed) => Some((
            piece(
                text(t!(lang, "play-patch-download-failed"))
                    .size(TEXT_CAPTION)
                    .style(widgets::danger_text_style)
                    .into(),
                Length::FillPortion(2),
            ),
            piece(
                row![
                    iced::widget::space::horizontal(),
                    widgets::icon_button(
                        lucide_icons::Icon::RefreshCw,
                        t!(lang, "patches-retry"),
                        Message::RetryPatchDownload(key),
                        [1.0, 1.0],
                    ),
                ]
                .align_y(Alignment::Center)
                .into(),
                trailing,
            ),
        )),
        _ => None,
    }
}

/// What the family picker currently reads, for the inert stand-in that
/// replaces it while a download runs. `None` with nothing selected —
/// then the picker itself (with its placeholder) is the better thing
/// to show anyway.
fn game_label(loadout: &Loadout, lang: &LanguageIdentifier, scanners: &Scanners) -> Option<String> {
    let choice = loadout.choice()?;
    Some(
        game_options(lang, scanners)
            .into_iter()
            .find(|opt| opt.choice == choice)?
            .to_string(),
    )
}

fn game_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
) -> sweeten::widget::PickList<'a, GameOption, Vec<GameOption>, GameOption, Message> {
    let options = game_options(lang, scanners);
    let selected = loadout.choice().map(|choice| {
        options
            .iter()
            .find(|opt| opt.choice == choice)
            .cloned()
            .unwrap_or_else(|| GameOption {
                choice,
                display: t!(lang, "play-package-unavailable"),
                available: false,
            })
    });
    widgets::picker(options, selected, Message::GameSelected)
        .disabled(|opts: &[GameOption]| opts.iter().map(|o| !o.available).collect())
        .placeholder(t!(lang, "play-no-game"))
}

pub fn editor_picker<'a>(loadout: &'a Loadout, lang: &'a LanguageIdentifier) -> Option<Element<'a, Message>> {
    if loadout.package_rom.is_none() || (loadout.editors.choices.len() <= 1 && loadout.editors.error.is_none()) {
        return None;
    }
    let options: Vec<_> = loadout
        .editors
        .choices
        .iter()
        .map(|choice| widgets::Choice::new(choice.reference.clone(), choice.label(&lang.to_string())))
        .collect();
    let selected = loadout.editors.selected.as_ref().map(|reference| {
        options
            .iter()
            .find(|choice| &choice.value == reference)
            .cloned()
            .unwrap_or_else(|| {
                widgets::Choice::new(
                    reference.clone(),
                    format!(
                        "{} / {} (v{})",
                        reference.package.name, reference.name, reference.package.version
                    ),
                )
            })
    });
    Some(
        widgets::picker(options, selected, |choice| Message::EditorSelected(choice.value))
            .placeholder(t!(lang, "play-select-editor"))
            .width(Length::Fill)
            .into(),
    )
}

/// The save picker on its own — the Play tab embeds it in its
/// save-action row (next to the rename / delete / new buttons), which
/// is that tab's own furniture.
pub fn save_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
) -> sweeten::widget::PickList<'a, SaveOption, Vec<SaveOption>, SaveOption, Message> {
    let options = save_options(loadout, lang, scanners, config);
    let selected = loadout
        .save
        .as_ref()
        .and_then(|p| options.iter().find(|s| &s.path == p).cloned());
    // Grey out saves the active patch can't run (alongside saves whose
    // ROM isn't owned) so an incompatible save can't be picked under a
    // patch — switch/clear the patch first. `None` (no patch) disables
    // nothing on this axis.
    let patch_supported = patch_supported_games(loadout, scanners);
    widgets::picker(options, selected, Message::SaveSelected)
        .disabled(move |opts: &[SaveOption]| {
            opts.iter()
                .map(|o| {
                    !o.available
                        || patch_supported
                            .as_ref()
                            .map(|s| o.game.is_none_or(|game| !s.contains(&game)))
                            .unwrap_or(false)
                })
                .collect()
        })
        .placeholder(t!(lang, "play-no-save"))
}

fn patch_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
) -> sweeten::widget::PickList<
    'a,
    widgets::Choice<String>,
    Vec<widgets::Choice<String>>,
    widgets::Choice<String>,
    Message,
> {
    let (options, selected) = patch_options(loadout, lang, scanners, config);
    widgets::picker(options, selected, |c: widgets::Choice<String>| {
        Message::PatchSelected(c.value)
    })
}

/// No patch selected (or none with matching versions) → render the
/// shared disabled-dropdown placeholder so the version slot reads as
/// locked-off instead of an empty picker users can still click.
fn version_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
) -> Element<'a, Message> {
    let options = version_options(loadout, scanners);
    if options.is_empty() {
        return widgets::disabled_pick_list(t!(lang, "play-version-placeholder"))
            .width(Length::Fixed(VERSION_PICKER_WIDTH))
            .into();
    }

    // Plain: the patch slot beside it reports any fetch.
    let selected = loadout
        .patch_version
        .as_ref()
        .and_then(|version| options.iter().find(|o| &o.value == version).cloned());
    widgets::picker(options, selected, |c: widgets::Choice<semver::Version>| {
        Message::PatchVersionSelected(c.value)
    })
    .placeholder(t!(lang, "play-version-placeholder"))
    .width(Length::Fixed(VERSION_PICKER_WIDTH))
    .into()
}
