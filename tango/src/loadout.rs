//! The local loadout — which game family, save, and (optionally)
//! patch the user is bringing to a match — hoisted to App level so
//! every tab renders from one source of truth and the netplay
//! settings-resend machinery doesn't have to reach into a tab's
//! private state.
//!
//! The *identity* of a loadout is `(family, game, save)`. The patch
//! is deliberately not part of that identity: it's an overlay,
//! dynamically selectable per loadout and remembered per save
//! ([`crate::config::Config::last_patch_per_save`]). Picking a save
//! restores the patch it was last used with; picking a patch sticks
//! to the current save. Saves whose patch association is intrinsic
//! (created from a patch's save template) keep it automatically;
//! vanilla-compatible saves just remember whatever they last ran
//! under.

use crate::app::Scanners;
use crate::i18n::t;
use crate::style::STANDARD_PADDING;
use crate::{anim, config, game, rom, widgets};
use iced::widget::row;
use iced::{Alignment, Element, Length};
use lucide_icons::Icon;
use sweeten::widget::pick_list;
use unic_langid::LanguageIdentifier;

pub struct Loadout {
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
    /// Whether the patch/version pickers in the selector rows are
    /// unfolded. Patching is a niche flow, so the pickers stay
    /// collapsed behind a small toggle until the user opts in;
    /// selecting "no patch" folds them away again. Not persisted —
    /// an *active* patch always forces the pickers visible, so a
    /// remembered patch selection survives without this flag. Shared
    /// by both rows ([`game_row`] / [`compact_row`]), so the fold
    /// state stays consistent across the Saves and Fight tabs.
    pub patch_picker_open: bool,
    /// Show/hide transition for the expanded patch controls (patch +
    /// version pickers). Mirrors `patch.is_some() ||
    /// patch_picker_open` (synced via [`Loadout::sync_patch_row`]);
    /// the selector rows fade-through morph between their folded and
    /// expanded shapes.
    pub patch_row: anim::Transition,
}

impl Default for Loadout {
    fn default() -> Self {
        Self {
            family: None,
            game: None,
            save: None,
            patch: None,
            patch_version: None,
            patch_picker_open: false,
            patch_row: anim::Transition::swap(false),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    FamilySelected(FamilyOption),
    SaveSelected(SaveOption),
    /// Real patch name; empty string is the "no patch" sentinel.
    PatchSelected(String),
    PatchVersionSelected(semver::Version),
    /// Unfold the collapsed patch/version pickers in the selector
    /// rows. There's no explicit close message — picking "no patch"
    /// folds them back up (see PatchSelected).
    PatchPickerOpened,
    Rescan,
}

/// Side-effects bubble-up, mirroring the tab modules' convention:
/// pure state mutations happen inside [`Loadout::update`]; anything
/// that needs App-level collaborators comes back as an `Effect`.
#[derive(Debug, Clone, Copy)]
pub enum Effect {
    /// Selection (family / game / save / patch / version) changed.
    /// App should rebuild its `Loaded` cache, persist config, and
    /// resend lobby settings if one is live.
    SelectionChanged,
    /// User clicked Rescan; App should scanner-rescan + refresh.
    Rescan,
}

impl Loadout {
    pub fn update(&mut self, msg: Message, scanners: &Scanners, config: &config::Config) -> Option<Effect> {
        let effect = self.update_inner(msg, scanners, config);
        self.sync_patch_row(iced::time::Instant::now());
        effect
    }

    fn update_inner(&mut self, msg: Message, scanners: &Scanners, config: &config::Config) -> Option<Effect> {
        match msg {
            Message::FamilySelected(f) => {
                self.family = Some(f.family);
                // Auto-land on the family's remembered (or first
                // available) save, which also fixes the concrete game.
                match resolve_family_save(config, scanners, f.family) {
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
                self.game = Some(s.game);
                self.family = Some(s.game.family_and_variant().0);
                self.save = Some(s.path);
                self.restore_patch_memory(config, scanners);
                Some(Effect::SelectionChanged)
            }
            Message::PatchPickerOpened => {
                self.patch_picker_open = true;
                None
            }
            Message::PatchSelected(name) => {
                if name.is_empty() {
                    self.patch = None;
                    self.patch_version = None;
                    // Picking the sentinel doubles as "put the patch
                    // controls away".
                    self.patch_picker_open = false;
                } else {
                    self.patch_version = newest_supporting_version(scanners, &name, self.game);
                    self.patch = Some(name);
                }
                // The picker only offers patches that support the
                // selected save's variant, so the save always keeps.
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
            Message::PatchVersionSelected(v) => {
                // The version list is filtered to versions supporting
                // the current variant, so nothing else needs fixing up.
                self.patch_version = Some(v);
                Some(Effect::SelectionChanged)
            }
            Message::Rescan => Some(Effect::Rescan),
        }
    }

    /// Drive [`Loadout::patch_row`] toward the current expanded
    /// state. Called after every [`Loadout::update`], and by the App
    /// after effect handlers that mutate `patch` outside this module.
    pub fn sync_patch_row(&mut self, now: iced::time::Instant) {
        self.patch_row.set(self.patch.is_some() || self.patch_picker_open, now);
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
                    patches
                        .get(&name)
                        .and_then(|p| p.versions.get(&version))
                        .map(|v| self.game.map(|g| v.supported_games.contains(&g)).unwrap_or(false))
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
                .and_then(|v| patches.get(&name).and_then(|p| p.versions.get(v)))
                .map(|v| v.supported_games.contains(&g))
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
        scanners: &Scanners,
    ) -> crate::net::protocol::Settings {
        use crate::net::protocol::{GameInfo, PatchInfo, Settings};
        let roms = scanners.roms.read();
        let patches = scanners.patches.read();
        Settings {
            nickname: config.nickname.clone().unwrap_or_default(),
            match_type: lobby.match_type,
            game_info: self.game.map(|game| {
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
                }
            }),
            available_games: roms
                .keys()
                .map(|g| {
                    let (family, variant) = g.family_and_variant();
                    (family.to_string(), variant)
                })
                .collect(),
            available_patches: patches
                .iter()
                .map(|(name, info)| (name.clone(), info.versions.keys().cloned().collect()))
                .collect(),
            reveal_setup: lobby.reveal_setup,
        }
    }
}

// ---------- Family / Save pick_list options ----------

#[derive(Clone)]
pub struct FamilyOption {
    /// Region-specific gamedb family string (e.g. `"bn3"`).
    pub family: &'static str,
    pub display: String,
    /// `false` when no game in this family has a ROM in the scan
    /// results. Drives sweeten's `.disabled()` closure on the picker so
    /// the row renders greyed out and refuses clicks.
    pub available: bool,
}

impl PartialEq for FamilyOption {
    fn eq(&self, o: &Self) -> bool {
        self.family == o.family
    }
}
impl Eq for FamilyOption {}
impl std::hash::Hash for FamilyOption {
    fn hash<H: std::hash::Hasher>(&self, s: &mut H) {
        self.family.hash(s);
    }
}
impl std::fmt::Display for FamilyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}
impl std::fmt::Debug for FamilyOption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.display)
    }
}

#[derive(Clone, Debug)]
pub struct SaveOption {
    pub path: std::path::PathBuf,
    /// Pre-computed display label: the save's path relative to the
    /// saves dir, forward-slash separated (so nested folders show up
    /// in the picker). Built when the option list is constructed
    /// because `Display::fmt` doesn't get the saves root as input.
    pub display: String,
    /// The concrete game this save resolves to *within its family*
    /// (White/Blue picked from the save's own contents). Selecting the
    /// save sets `game` to this.
    pub game: rom::GameRef,
    /// `false` when `game`'s ROM isn't owned — the row greys out and
    /// can't be selected.
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
    pub fn new(saves_path: &std::path::Path, path: std::path::PathBuf, game: rom::GameRef, available: bool) -> Self {
        let display = path
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

/// Every supported family — not just the ones we have a ROM for, so
/// users can see what tango knows about. sweeten's `.disabled()` greys
/// out families with no owned ROM; available families stable-sort to
/// the top (then own-region first, then by family string) so the live
/// ones lead.
pub fn family_options(lang: &LanguageIdentifier, scanners: &Scanners) -> Vec<FamilyOption> {
    let roms = scanners.roms.read();
    let mut families: Vec<&'static str> = Vec::new();
    for g in tango_gamedb::GAMES.iter() {
        let fam = g.family_and_variant().0;
        if !families.contains(&fam) {
            families.push(fam);
        }
    }
    let mut family_options: Vec<FamilyOption> = families
        .iter()
        .map(|fam| FamilyOption {
            family: fam,
            display: game::family_display_name(lang, fam),
            available: game::games_in_family(fam).any(|g| roms.contains_key(&g)),
        })
        .collect();
    family_options.sort_by(|a, b| {
        (!a.available)
            .cmp(&(!b.available))
            .then_with(|| {
                let ar = !game::family_matches_language(lang, a.family);
                let br = !game::family_matches_language(lang, b.family);
                ar.cmp(&br)
            })
            .then_with(|| a.family.cmp(b.family))
    });
    family_options
}

/// Every save across the selected family's color variants,
/// intermingled. Each save is tagged with the concrete game it
/// resolves to and whether that game's ROM is owned (so the row can
/// grey out). A path appears under exactly one variant within a
/// family, but de-dup defensively. The list is NOT filtered by the
/// active patch — the save is the loadout's identity and the patch
/// follows it, so every save is always reachable; selecting one
/// re-resolves the overlay (see [`Loadout::restore_patch_memory`]).
pub fn save_options(loadout: &Loadout, scanners: &Scanners, config: &config::Config) -> Vec<SaveOption> {
    let saves_path = config.saves_path();
    let roms = scanners.roms.read();
    let saves = scanners.saves.read();
    let mut save_options: Vec<SaveOption> = Vec::new();
    if let Some(family) = loadout.family {
        let mut seen: std::collections::HashSet<std::path::PathBuf> = std::collections::HashSet::new();
        for g in game::games_in_family(family) {
            let available = roms.contains_key(&g);
            if let Some(saves_for_game) = saves.get(&g) {
                for s in saves_for_game {
                    if seen.insert(s.path.clone()) {
                        save_options.push(SaveOption::new(&saves_path, s.path.clone(), g, available));
                    }
                }
            }
        }
    }
    // Folder-first recursive sort: at the first differing path
    // component, whichever side still has components after it
    // (i.e. is "inside a folder at this level") wins. Files at
    // a given level sort below any subfolders at that level.
    save_options.sort_by(|a, b| {
        let av: Vec<&std::ffi::OsStr> = a.path.strip_prefix(&saves_path).unwrap_or(&a.path).iter().collect();
        let bv: Vec<&std::ffi::OsStr> = b.path.strip_prefix(&saves_path).unwrap_or(&b.path).iter().collect();
        for i in 0..av.len().min(bv.len()) {
            if av[i] != bv[i] {
                let a_is_dir = i + 1 < av.len();
                let b_is_dir = i + 1 < bv.len();
                return match (a_is_dir, b_is_dir) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => av[i].cmp(bv[i]),
                };
            }
        }
        av.len().cmp(&bv.len())
    });
    save_options
}

/// Patch picker options (with the "no patch" sentinel first) and the
/// currently-selected entry. Filtered to patches that support the
/// selected save's variant — the save is picked first and the patch
/// list adapts to it, never the other way around. With no concrete
/// game resolved yet, falls back to "supports any variant in the
/// family". Favorites sort first (and get a "★ " label prefix),
/// alphabetical within each group.
pub fn patch_options(
    loadout: &Loadout,
    lang: &LanguageIdentifier,
    scanners: &Scanners,
    config: &config::Config,
) -> (Vec<widgets::Choice<String>>, Option<widgets::Choice<String>>) {
    let patches = scanners.patches.read();
    let candidate_games: Vec<rom::GameRef> = match loadout.game {
        Some(g) => vec![g],
        None => loadout
            .family
            .map(|f| game::games_in_family(f).collect())
            .unwrap_or_default(),
    };
    let mut compatible_names: Vec<String> = patches
        .iter()
        .filter(|(_, p)| {
            p.versions
                .values()
                .any(|v| candidate_games.iter().any(|g| v.supported_games.contains(g)))
        })
        .map(|(n, _)| n.clone())
        .collect();
    compatible_names.sort_by(|a, b| {
        let fa = config.favorite_patches.contains(a);
        let fb = config.favorite_patches.contains(b);
        fb.cmp(&fa).then_with(|| a.cmp(b))
    });
    let no_patch_option = widgets::Choice::new(String::new(), t!(lang, "play-no-patch"));
    let patch_options: Vec<widgets::Choice<String>> = std::iter::once(no_patch_option.clone())
        .chain(compatible_names.into_iter().map(|n| {
            let display = if config.favorite_patches.contains(&n) {
                format!("\u{2605} {n}")
            } else {
                n.clone()
            };
            widgets::Choice::new(n, display)
        }))
        .collect();
    let selected_patch = match loadout.patch.as_ref() {
        Some(n) => patch_options.iter().find(|o| &o.value == n).cloned(),
        None => Some(no_patch_option),
    };
    (patch_options, selected_patch)
}

/// Versions of the selected patch that support the current game,
/// newest first. Empty when no patch is selected.
pub fn version_options(loadout: &Loadout, scanners: &Scanners) -> Vec<semver::Version> {
    let patches = scanners.patches.read();
    loadout
        .patch
        .as_ref()
        .and_then(|n| patches.get(n))
        .map(|p| {
            let game = loadout.game;
            let mut vs: Vec<semver::Version> = p
                .versions
                .iter()
                .filter(|(_, v)| game.map(|g| v.supported_games.contains(&g)).unwrap_or(true))
                .map(|(k, _)| k.clone())
                .collect();
            vs.sort_by(|a, b| b.cmp(a));
            vs
        })
        .unwrap_or_default()
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
    patches.get(patch_name).and_then(|p| {
        p.versions
            .iter()
            .filter(|(_, v)| game.map(|g| v.supported_games.contains(&g)).unwrap_or(true))
            .map(|(k, _)| k.clone())
            .max()
    })
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
        .last_save_per_game
        .get(&config::game_key(game))
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
    {
        let roms = scanners.roms.read();
        let saves = scanners.saves.read();
        for g in game::games_in_family(family) {
            if !roms.contains_key(&g) {
                continue;
            }
            if let Some(rel) = config.last_save_per_game.get(&config::game_key(g)) {
                let abs = config.data_relative_to_absolute(rel);
                if saves.get(&g).map(|v| v.iter().any(|s| s.path == abs)).unwrap_or(false) {
                    return Some((g, abs));
                }
            }
        }
    }
    first_available_family_save(scanners, family)
}

/// First owned-ROM save across every game in `family`, path-sorted.
/// Used as the family auto-pick fallback (and by the App's post-delete
/// auto-pick). Returns the concrete game alongside the path so callers
/// can set `game` without re-sniffing the save.
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
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
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
    scanners
        .patches
        .read()
        .get(name)
        .and_then(|p| p.versions.get(version))
        .map(|v| v.supported_games.clone())
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

/// The full game row for the saves tab: family picker + foldable
/// patch/version pickers + rescan button. The patch controls stay
/// folded behind a small icon toggle until the user opts in (or a
/// patch is already active) — in the saves tab, patching is a niche
/// flow that shouldn't share top billing with the game picker. With
/// the pickers folded, the game picker's FillPortion is the only fill
/// in the row, so it soaks up the freed width.
pub fn game_row<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
    rescanning: bool,
) -> Element<'a, Message> {
    let game = family_picker(loadout, lang, scanners).width(Length::FillPortion(3));

    let refresh = widgets::icon_button_maybe(
        Icon::RefreshCw,
        t!(lang, "rescan"),
        (!rescanning).then_some(Message::Rescan),
        STANDARD_PADDING,
    );

    let mut row_el = row![game];
    for el in patch_fold_segment(loadout, lang, scanners, config, Length::FillPortion(2)) {
        row_el = row_el.push(el);
    }
    row_el.push(refresh).spacing(8).align_y(Alignment::Center).into()
}

/// The compact one-row loadout strip for the fight tab: family + save
/// + the foldable patch segment (no rescan, no save management).
/// Embedded above the lobby so switching what you bring — including
/// unfolding the patch picker to match the opponent's patch — never
/// requires leaving the lobby.
pub fn compact_row<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
) -> Element<'a, Message> {
    let family = family_picker(loadout, lang, scanners).width(Length::FillPortion(3));
    let save = save_picker(loadout, lang, scanners, config).width(Length::FillPortion(4));
    let mut row_el = row![family, save];
    for el in patch_fold_segment(loadout, lang, scanners, config, Length::FillPortion(3)) {
        row_el = row_el.push(el);
    }
    row_el.spacing(8).align_y(Alignment::Center).into()
}

/// The foldable patch segment shared by [`game_row`] and
/// [`compact_row`]: a small puzzle toggle at rest, the patch +
/// version pickers once the user opts in (or a patch is already
/// active). Folding either way fade-through morphs ONLY this segment
/// — the toggle and the pickers swap through the pane plate,
/// horizontally, while the row's other controls stay untouched.
/// (They still reflow at the swap's midpoint, but the segment is
/// fully dissolved there.) The fold state lives on the shared
/// [`Loadout`], so opening it in one tab opens it in the other.
fn patch_fold_segment<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
    patch_width: Length,
) -> Vec<Element<'a, Message>> {
    let now = iced::time::Instant::now();
    let (render_expanded, patch_swap) = anim::swap_phase(&loadout.patch_row, now);
    let swapped = |el: Element<'a, Message>| -> Element<'a, Message> {
        match patch_swap {
            Some(phase) => anim::swap_transform(el, phase, iced::Vector::new(24.0, 0.0), widgets::plate_color),
            None => el,
        }
    };
    if render_expanded {
        let patch = patch_picker(loadout, lang, scanners, config).width(patch_width);
        let version = version_picker(loadout, lang, scanners);
        vec![swapped(patch.into()), swapped(version)]
    } else {
        let patch_toggle = widgets::icon_button(
            Icon::Puzzle,
            t!(lang, "play-patch-toggle"),
            Message::PatchPickerOpened,
            STANDARD_PADDING,
        );
        vec![swapped(patch_toggle)]
    }
}

fn family_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
) -> sweeten::widget::PickList<'a, FamilyOption, Vec<FamilyOption>, FamilyOption, Message> {
    let options = family_options(lang, scanners);
    let selected = loadout
        .family
        .and_then(|fam| options.iter().find(|opt| opt.family == fam).cloned());
    pick_list(options, selected, Message::FamilySelected)
        .disabled(|opts: &[FamilyOption]| opts.iter().map(|o| !o.available).collect())
        .placeholder(t!(lang, "play-no-game"))
        .padding(STANDARD_PADDING)
        .style(widgets::chunky_pick_list)
}

/// The save picker on its own — the saves tab embeds it in its
/// save-action row (next to the rename / delete / new buttons), which
/// is that tab's own furniture.
pub fn save_picker<'a>(
    loadout: &'a Loadout,
    lang: &'a LanguageIdentifier,
    scanners: &'a Scanners,
    config: &'a config::Config,
) -> sweeten::widget::PickList<'a, SaveOption, Vec<SaveOption>, SaveOption, Message> {
    let options = save_options(loadout, scanners, config);
    let selected = loadout
        .save
        .as_ref()
        .and_then(|p| options.iter().find(|s| &s.path == p).cloned());
    pick_list(options, selected, Message::SaveSelected)
        .disabled(|opts: &[SaveOption]| opts.iter().map(|o| !o.available).collect())
        .placeholder(t!(lang, "play-no-save"))
        .padding(STANDARD_PADDING)
        .style(widgets::chunky_pick_list)
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
    pick_list(options, selected, |c: widgets::Choice<String>| {
        Message::PatchSelected(c.value)
    })
    .padding(STANDARD_PADDING)
    .style(widgets::chunky_pick_list)
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
        widgets::disabled_pick_list(t!(lang, "play-version-placeholder"))
            .width(Length::Fixed(100.0))
            .into()
    } else {
        pick_list(options, loadout.patch_version.clone(), Message::PatchVersionSelected)
            .placeholder(t!(lang, "play-version-placeholder"))
            .padding(STANDARD_PADDING)
            .width(Length::Fixed(100.0))
            .style(widgets::chunky_pick_list)
            .into()
    }
}
