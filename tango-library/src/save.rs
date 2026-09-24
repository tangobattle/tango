use crate::storage::{Listing, Storage};
use crate::{rom::GameRef, scanner};
use std::path::{Path, PathBuf};

pub struct ScannedSave {
    pub path: std::path::PathBuf,
    pub save: tango_gamesupport::BoxedSave,
}

impl Clone for ScannedSave {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            save: self.save.clone_box(),
        }
    }
}

pub type Scanner = scanner::Scanner<std::collections::HashMap<GameRef, Vec<ScannedSave>>>;

pub fn scan_saves(storage: &dyn Storage, listing: &Listing) -> std::collections::HashMap<GameRef, Vec<ScannedSave>> {
    let mut by_game: std::collections::HashMap<GameRef, Vec<ScannedSave>> = std::collections::HashMap::new();

    for entry in listing.entries() {
        let buf = match storage.read(&entry.path) {
            Ok(b) => b,
            Err(e) => {
                log::warn!("{}: {e}", entry.path.display());
                continue;
            }
        };

        let mut matched = false;
        for game in crate::game::GAMES.iter().copied() {
            if let Ok(save) = game.parse_save(&buf) {
                by_game.entry(game).or_default().push(ScannedSave {
                    path: entry.path.clone(),
                    save,
                });
                matched = true;
            }
        }

        if !matched {
            log::warn!("save scan: {}: no matching game", entry.path.display());
        }
    }

    for (_, saves) in by_game.iter_mut() {
        // Order by extensionless name (full path as the tiebreak), the
        // same way the save picker displays rows — consumers take the
        // first entry as a default pick, and that should agree with
        // what the picker shows first.
        saves.sort_by(|a, b| {
            a.path
                .file_stem()
                .cmp(&b.path.file_stem())
                .then_with(|| a.path.cmp(&b.path))
        });
    }

    by_game
}

/// The order a save picker lists a family's saves in. Variant first, so
/// the list reads as one block per variant in the order the games
/// themselves are numbered. Within a variant, a folder-first recursive
/// sort: at the first differing path component under `saves_path`,
/// whichever side still has components after it (i.e. is "inside a
/// folder at this level") wins. Files at a given level sort below any
/// subfolders at that level, and order among themselves by their
/// extensionless name — so "Blue.sav" sits next to "Blue Moon.sav"
/// instead of wherever the '.' happens to fall against spaces and digits
/// — with the raw name breaking stem ties.
pub fn picker_order(saves_path: &Path, a: (GameRef, &Path), b: (GameRef, &Path)) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let variant = |game: GameRef| game.family_and_variant().1;
    variant(a.0).cmp(&variant(b.0)).then_with(|| {
        let av: Vec<&std::ffi::OsStr> = a.1.strip_prefix(saves_path).unwrap_or(a.1).iter().collect();
        let bv: Vec<&std::ffi::OsStr> = b.1.strip_prefix(saves_path).unwrap_or(b.1).iter().collect();
        for i in 0..av.len().min(bv.len()) {
            if av[i] != bv[i] {
                let a_is_dir = i + 1 < av.len();
                let b_is_dir = i + 1 < bv.len();
                return match (a_is_dir, b_is_dir) {
                    (true, false) => Ordering::Less,
                    (false, true) => Ordering::Greater,
                    (true, true) => av[i].cmp(bv[i]),
                    (false, false) => Path::new(av[i])
                        .file_stem()
                        .cmp(&Path::new(bv[i]).file_stem())
                        .then_with(|| av[i].cmp(bv[i])),
                };
            }
        }
        av.len().cmp(&bv.len())
    })
}

// ---------- Save-file operations ----------
//
// Everything a host does to the saves folder beyond scanning it: naming,
// creating from a template, duplicating, renaming, deleting. Every file
// access goes through [`Storage`], so the browser's store follows the
// same rules as the desktop's filesystem.

/// Why a save-file operation refused or failed.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("empty save name")]
    EmptyName,
    #[error("invalid save name")]
    InvalidName,
    #[error("save has no parent directory")]
    NoParent,
    #[error("destination already exists")]
    Exists,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// The first registered game whose parser accepts `bytes` — the same
/// rule [`scan_saves`] files a save under, for a host importing one.
pub fn detect_game(bytes: &[u8]) -> Option<GameRef> {
    crate::game::GAMES
        .iter()
        .copied()
        .find(|game| game.parse_save(bytes).is_ok())
}

/// A user-typed file stem is refused when empty or when it could escape
/// the save's own folder.
pub fn validate_stem(stem: &str) -> Result<(), Error> {
    if stem.is_empty() {
        return Err(Error::EmptyName);
    }
    if stem.contains('/') || stem.contains('\\') || stem.contains("..") {
        return Err(Error::InvalidName);
    }
    Ok(())
}

/// Replace characters a file name can't carry (and control characters)
/// with spaces, then collapse runs of whitespace.
pub fn sanitize_filename(s: &str) -> String {
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

/// The default name for a save created from a template:
/// "<game> - <template>" (or just "<game>" before a template is picked),
/// made safe to use as a file name. Both names arrive localized.
pub fn suggest_name(game_name: &str, template_label: Option<&str>) -> String {
    match template_label {
        Some(label) => sanitize_filename(&format!("{game_name} - {label}")),
        None => sanitize_filename(game_name),
    }
}

/// Appends ` 2`, ` 3`, ... to `base` until the resulting `<name>.sav`
/// doesn't already exist in `saves_dir`. Gives up at 99 to avoid an
/// unbounded scan if the directory is somehow saturated.
pub fn free_name(storage: &dyn Storage, saves_dir: &Path, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !storage.is_file(&saves_dir.join(format!("{draft}.sav"))) {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// Next free "<stem> (copy)" / "<stem> (copy N)" stem for `src` — the
/// prefill for a duplicate, so accepting it as-is never collides.
pub fn suggest_duplicate_stem(storage: &dyn Storage, src: &Path) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    for n in 1..1000 {
        let candidate = if n == 1 {
            format!("{stem} (copy)")
        } else {
            format!("{stem} (copy {n})")
        };
        let taken = src
            .parent()
            .is_some_and(|p| storage.is_file(&sibling(p, src, &candidate)));
        if !taken {
            return candidate;
        }
    }
    format!("{stem} (copy)")
}

/// `new_stem` in `parent`, carrying `src`'s extension over.
fn sibling(parent: &Path, src: &Path, new_stem: &str) -> PathBuf {
    match src.extension() {
        Some(ext) => parent.join(format!("{new_stem}.{}", ext.to_string_lossy())),
        None => parent.join(new_stem),
    }
}

/// The save templates `game` can be created from, by name (empty string
/// = the default template), in offer order: the patch's own first, then
/// the game's bundled ones. A patch template overrides a bundled one of
/// the same name. Empty when neither ships any.
pub fn templates(
    game: GameRef,
    patches: &crate::patch::Catalog,
    patch: Option<(&str, &semver::Version)>,
) -> Vec<(String, tango_gamesupport::BoxedSave)> {
    let mut out: Vec<(String, tango_gamesupport::BoxedSave)> = Vec::new();
    if let Some(version) = patch.and_then(|(name, version)| patches.version(name, version)) {
        if let Some(templates) = version.save_templates.get(&game) {
            out.extend(templates.iter().map(|(name, save)| (name.clone(), save.clone_box())));
        }
    }
    for (name, save) in game.save_templates.iter().flat_map(|t| t.iter()) {
        if !out.iter().any(|(existing, _)| existing == name) {
            out.push(((*name).to_string(), save.clone_box()));
        }
    }
    out
}

/// The template to create from for a `(game, name)` pick. Falls back to
/// the default, then the first template, if the exact name vanished.
pub fn template(
    game: GameRef,
    patches: &crate::patch::Catalog,
    patch: Option<(&str, &semver::Version)>,
    name: &str,
) -> Option<tango_gamesupport::BoxedSave> {
    let mut templates = templates(game, patches, patch);
    let index = templates
        .iter()
        .position(|(n, _)| n == name)
        .or_else(|| templates.iter().position(|(n, _)| n.is_empty()))
        .or((!templates.is_empty()).then_some(0))?;
    Some(templates.swap_remove(index).1)
}

/// A template's label in its family's own words ("Heat Guts"), if the
/// family names it. The unnamed default template is keyed `save-megaman`.
/// Hosts choose their own fallback.
pub fn template_label(lang: &unic_langid::LanguageIdentifier, family: &str, template: &str) -> Option<String> {
    let key = if template.is_empty() { "megaman" } else { template };
    crate::game::family_str(family, lang, &format!("save-{key}"))
}

/// Write a template's SRAM to `saves_dir/<name>.sav`. The file name is
/// taken verbatim from `name` (trimmed); an existing file is refused.
///
/// The checksum is rebuilt first: a template's is stale (computed before
/// this game-specific clone), and both the game and [`scan_saves`] reject
/// the resulting file without it.
pub fn create(
    storage: &dyn Storage,
    saves_dir: &Path,
    name: &str,
    template: &dyn tango_gamesupport::SaveData,
) -> Result<PathBuf, Error> {
    let name = name.trim();
    validate_stem(name)?;
    let file_name = if name.ends_with(".sav") {
        name.to_string()
    } else {
        format!("{name}.sav")
    };
    let dst = saves_dir.join(file_name);
    if storage.is_file(&dst) {
        return Err(Error::Exists);
    }
    storage.create_dir_all(saves_dir)?;
    let mut save = template.clone_box();
    save.rebuild_checksum();
    storage.write(&dst, &save.to_sram_dump())?;
    Ok(dst)
}

/// Copy `src` to a sibling file named `new_stem` (extension preserved).
/// Refuses invalid names and existing destinations.
pub fn duplicate(storage: &dyn Storage, src: &Path, new_stem: &str) -> Result<PathBuf, Error> {
    validate_stem(new_stem)?;
    let dst = sibling(src.parent().ok_or(Error::NoParent)?, src, new_stem);
    if dst == src || storage.is_file(&dst) {
        return Err(Error::Exists);
    }
    storage.write(&dst, &storage.read(src)?)?;
    Ok(dst)
}

/// Rename `src` to use `new_stem` (extension preserved). Renaming to the
/// same name is a no-op; any other existing destination is refused.
pub fn rename(storage: &dyn Storage, src: &Path, new_stem: &str) -> Result<PathBuf, Error> {
    validate_stem(new_stem)?;
    let dst = sibling(src.parent().ok_or(Error::NoParent)?, src, new_stem);
    if dst == src {
        return Ok(dst);
    }
    if storage.is_file(&dst) {
        return Err(Error::Exists);
    }
    storage.rename(src, &dst)?;
    Ok(dst)
}

/// Delete a save file.
pub fn delete(storage: &dyn Storage, path: &Path) -> Result<(), Error> {
    Ok(storage.remove_file(path)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::Memory;

    #[test]
    fn names_are_sanitized_validated_and_disambiguated() {
        assert_eq!(sanitize_filename(" a:b/c\u{1}  d "), "a b c d");
        assert_eq!(suggest_name("BN6: Gregar", Some("Heat/Guts")), "BN6 Gregar - Heat Guts");
        assert_eq!(suggest_name("BN6", None), "BN6");
        assert!(matches!(validate_stem(""), Err(Error::EmptyName)));
        for bad in ["a/b", "a\\b", ".."] {
            assert!(matches!(validate_stem(bad), Err(Error::InvalidName)));
        }

        let files = Memory::default();
        let dir = Path::new("/saves");
        assert_eq!(free_name(&files, dir, "x"), "x");
        files.write(&dir.join("x.sav"), b"1").unwrap();
        files.write(&dir.join("x 2.sav"), b"1").unwrap();
        assert_eq!(free_name(&files, dir, "x"), "x 3");
        assert_eq!(suggest_duplicate_stem(&files, &dir.join("x.sav")), "x (copy)");
        files.write(&dir.join("x (copy).sav"), b"1").unwrap();
        assert_eq!(suggest_duplicate_stem(&files, &dir.join("x.sav")), "x (copy 2)");
    }

    #[test]
    fn duplicate_rename_and_delete_go_through_storage() {
        let files = Memory::default();
        let src = PathBuf::from("/saves/a.sav");
        files.write(&src, b"save").unwrap();

        let copy = duplicate(&files, &src, "b").unwrap();
        assert_eq!(copy, Path::new("/saves/b.sav"));
        assert_eq!(files.read(&copy).unwrap(), b"save");
        assert!(matches!(duplicate(&files, &src, "b"), Err(Error::Exists)));
        assert!(matches!(duplicate(&files, &src, "a"), Err(Error::Exists)));

        assert_eq!(rename(&files, &src, "a").unwrap(), src);
        assert!(matches!(rename(&files, &src, "b"), Err(Error::Exists)));
        let renamed = rename(&files, &src, "c").unwrap();
        assert!(!files.is_file(&src));
        assert_eq!(files.read(&renamed).unwrap(), b"save");

        delete(&files, &renamed).unwrap();
        assert!(!files.is_file(&renamed));
    }
}
