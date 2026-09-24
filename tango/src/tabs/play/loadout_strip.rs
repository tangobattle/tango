//! The loadout strip: the family, save, patch, and version pickers
//! over the App-level [`Selection`], and the messages they emit.
//!
//! The selection policy — what each pick does to the rest of the
//! selection, and what's remembered per family and per save — is
//! [`tango_library::loadout::Selection`]'s, shared with the browser
//! host. What's here is only the iced side: option lists, pickers, and
//! the download strip that stands in for them.

use crate::config;
use crate::i18n::t;
use crate::library::Catalog;
use crate::library::{game, rom};
use crate::ui::style::TEXT_CAPTION;
use crate::ui::widgets;
use iced::widget::{container, row, text};
use iced::{Alignment, Element, Length};
use tango_library::loadout::Selection;
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone)]
pub enum Message {
    FamilySelected(FamilyOption),
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
/// pure state mutations happen inside [`update`]; anything
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

/// Apply a strip message to the selection.
pub fn update(selection: &mut Selection, msg: Message, scanners: &Catalog, config: &config::Config) -> Option<Effect> {
    match msg {
        Message::FamilySelected(f) => selection.pick_family(f.family, scanners, config),
        Message::SaveSelected(s) => selection.pick_save(s.game, s.path, scanners, config),
        // Empty string is the "no patch" sentinel.
        Message::PatchSelected(name) => selection.pick_patch((!name.is_empty()).then_some(name), scanners, config),
        Message::PatchVersionSelected(v) => selection.pick_patch_version(v),
        Message::RetryPatchDownload(_) => return Some(Effect::RetryDownload),
        Message::CancelPatchDownload(_) => return Some(Effect::CancelDownload),
    }
    Some(Effect::SelectionChanged)
}

/// Single source of truth for the local side's `protocol::Settings`.
/// App calls this when actually sending settings on the wire; the lobby
/// view calls it as the "You" slot fallback during
/// Connecting/Negotiating (before `lobby.local` has been populated by
/// the netplay loop).
pub fn local_settings(
    selection: &Selection,
    config: &config::Config,
    lobby: &crate::netplay::LobbyState,
) -> tango_net_protocol::control::Settings {
    tango_net_protocol::control::Settings {
        nickname: config.nickname.clone().unwrap_or_default(),
        match_type: lobby.match_type,
        game_info: selection.game_info(),
        blind_setup: lobby.blind_setup,
    }
}

// ---------- Family / Save pick_list options ----------

#[derive(Clone)]
pub struct FamilyOption {
    /// Region-specific gamedb family string (e.g. `"bn3"`).
    pub family: &'static str,
    pub display: String,
    /// `false` unless *every* game in this family has a ROM in the scan
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
    /// in the picker), behind the short variant tag when the family has
    /// more than one variant to tell apart. Built when the option list
    /// is constructed because `Display::fmt` gets neither the saves root
    /// nor the language as input.
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
    /// `variant` is the save's short variant tag (e.g. "Blue Moon"),
    /// `None` for families with only one variant — nothing to tell apart
    /// there, so the row stays bare.
    pub fn new(
        saves_path: &std::path::Path,
        path: std::path::PathBuf,
        game: rom::GameRef,
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
pub fn family_options(lang: &LanguageIdentifier, scanners: &Catalog) -> Vec<FamilyOption> {
    let roms = scanners.roms.read();
    let mut families: Vec<&'static str> = Vec::new();
    for g in crate::library::game::GAMES.iter() {
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
            available: game::games_in_family(fam).all(|g| roms.contains_key(&g)),
        })
        .collect();
    family_options.sort_by(|a, b| {
        (!a.available).cmp(&(!b.available)).then_with(|| {
            let ar = !game::family_matches_language(lang, a.family);
            let br = !game::family_matches_language(lang, b.family);
            ar.cmp(&br)
        })
    });
    family_options
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
    loadout: &Selection,
    lang: &LanguageIdentifier,
    scanners: &Catalog,
    config: &config::Config,
) -> Vec<SaveOption> {
    let saves_path = config.saves_path();
    let roms = scanners.roms.read();
    let saves = scanners.saves.read();
    let mut save_options: Vec<SaveOption> = Vec::new();
    if let Some(family) = loadout.family() {
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
                            g,
                            available,
                            variant.as_deref(),
                        ));
                    }
                }
            }
        }
    }
    // One block per variant, folders first — see `save::picker_order`.
    save_options.sort_by(|a, b| crate::library::save::picker_order(&saves_path, (a.game, &a.path), (b.game, &b.path)));
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
    loadout: &Selection,
    lang: &LanguageIdentifier,
    scanners: &Catalog,
    config: &config::Config,
) -> (Vec<widgets::Choice<String>>, Option<widgets::Choice<String>>) {
    let patches = scanners.patches.read();
    let family_games: Vec<rom::GameRef> = loadout
        .family()
        .map(|f| game::games_in_family(f).collect())
        .unwrap_or_default();
    let names = tango_library::loadout::patch_names_for(&patches, &family_games, &config.favorite_patches);
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
    let selected_patch = match loadout.patch_name() {
        Some(n) => patch_options.iter().find(|o| o.value == n).cloned(),
        None => Some(no_patch_option),
    };
    (patch_options, selected_patch)
}

/// Versions of the selected patch that support the current game, newest
/// first. Empty when no patch is selected. Includes versions that aren't
/// downloaded — the repo keeps every release forever, and an old one is
/// exactly what a replay or an opponent may need — marked with a ↓, same
/// as the patch list.
pub fn version_options(loadout: &Selection, scanners: &Catalog) -> Vec<widgets::Choice<semver::Version>> {
    let patches = scanners.patches.read();
    loadout
        .patch_name()
        .map(|name| {
            let game = loadout.game();
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

// ---------- Views ----------

/// The full game row for the Play tab's selector strip: family
/// picker, patch + version pickers. The patch controls are always
/// visible. No rescan button — scans re-run on their own (tab
/// entry, session close).
pub fn game_row<'a>(
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    scanners: &'a Catalog,
    config: &'a config::Config,
    downloads: &'a crate::library::patch::Downloads,
) -> Element<'a, Message> {
    // Gaps are explicit children rather than row spacing, because a
    // fetch replaces the patch AND version pickers with one unbroken
    // download strip -- and a uniform spacing would leave a seam down
    // the middle of it. Laid out this way the fixed 8 + 8 + version
    // width comes off the row identically in both states, so the game
    // picker never moves.
    let gap = || iced::widget::space::horizontal().width(Length::Fixed(8.0));
    let download = patch_download(loadout, lang, downloads);
    // The game the download belongs to can't be changed out from under
    // it, so its picker goes inert for the duration -- same footprint,
    // same name on it, just not something you can open. Cancelling the
    // fetch hands it back.
    let game: Element<'a, Message> = match (&download, family_label(loadout, lang, scanners)) {
        (Some(_), Some(label)) => widgets::disabled_pick_list(label).width(Length::FillPortion(3)).into(),
        _ => family_picker(loadout, lang, scanners)
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
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    downloads: &'a crate::library::patch::Downloads,
) -> Option<(Element<'a, Message>, Element<'a, Message>)> {
    let key = (loadout.patch_name()?.to_owned(), loadout.patch_version()?.clone());
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
fn family_label(loadout: &Selection, lang: &LanguageIdentifier, scanners: &Catalog) -> Option<String> {
    let family = loadout.family()?;
    Some(
        family_options(lang, scanners)
            .into_iter()
            .find(|opt| opt.family == family)?
            .to_string(),
    )
}

fn family_picker<'a>(
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    scanners: &'a Catalog,
) -> sweeten::widget::PickList<'a, FamilyOption, Vec<FamilyOption>, FamilyOption, Message> {
    let options = family_options(lang, scanners);
    let selected = loadout
        .family()
        .and_then(|fam| options.iter().find(|opt| opt.family == fam).cloned());
    widgets::picker(options, selected, Message::FamilySelected)
        .disabled(|opts: &[FamilyOption]| opts.iter().map(|o| !o.available).collect())
        .placeholder(t!(lang, "play-no-game"))
}

/// The save picker on its own — the Play tab embeds it in its
/// save-action row (next to the rename / delete / new buttons), which
/// is that tab's own furniture.
pub fn save_picker<'a>(
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    scanners: &'a Catalog,
    config: &'a config::Config,
) -> sweeten::widget::PickList<'a, SaveOption, Vec<SaveOption>, SaveOption, Message> {
    let options = save_options(loadout, lang, scanners, config);
    let selected = loadout
        .save()
        .and_then(|p| options.iter().find(|s| s.path == p).cloned());
    // Grey out saves the active patch can't run (alongside saves whose
    // ROM isn't owned) so an incompatible save can't be picked under a
    // patch — switch/clear the patch first. `None` (no patch) disables
    // nothing on this axis.
    let patch_supported = loadout.patch_supported_games(scanners);
    widgets::picker(options, selected, Message::SaveSelected)
        .disabled(move |opts: &[SaveOption]| {
            opts.iter()
                .map(|o| !o.available || patch_supported.as_ref().map(|s| !s.contains(&o.game)).unwrap_or(false))
                .collect()
        })
        .placeholder(t!(lang, "play-no-save"))
}

fn patch_picker<'a>(
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    scanners: &'a Catalog,
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
    loadout: &'a Selection,
    lang: &'a LanguageIdentifier,
    scanners: &'a Catalog,
) -> Element<'a, Message> {
    let options = version_options(loadout, scanners);
    if options.is_empty() {
        return widgets::disabled_pick_list(t!(lang, "play-version-placeholder"))
            .width(Length::Fixed(VERSION_PICKER_WIDTH))
            .into();
    }

    // Plain: the patch slot beside it reports any fetch.
    let selected = loadout
        .patch_version()
        .and_then(|version| options.iter().find(|o| &o.value == version).cloned());
    widgets::picker(options, selected, |c: widgets::Choice<semver::Version>| {
        Message::PatchVersionSelected(c.value)
    })
    .placeholder(t!(lang, "play-version-placeholder"))
    .width(Length::Fixed(VERSION_PICKER_WIDTH))
    .into()
}
