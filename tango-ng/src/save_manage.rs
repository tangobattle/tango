//! Save-file management: the duplicate / rename / delete /
//! create-from-template on-disk operations and the new-save template
//! resolution, ported from `tango/src/tabs/play/save_manage.rs` (the
//! form state lives in the Slint layer; main.rs drives these).

use std::collections::HashMap;

use crate::rom::GameRef;

/// One entry in the new-save template picker: a concrete owned-ROM
/// variant plus a raw template name, with a display label ("Blue –
/// Heat Guts") resolved via the family's `save-<name>` string.
pub struct TemplateOption {
    pub game: GameRef,
    pub raw: String,
    pub label: String,
}

/// Picker entries for the new-save dialog: every (owned-ROM variant ×
/// template) across `family`. When a patch (+version) is active,
/// variants it doesn't support are dropped and its templates override
/// bundled ones of the same name — creating a save under an active
/// patch is a patch-specific flow.
pub fn creation_options(
    lang: &unic_langid::LanguageIdentifier,
    family: &str,
    roms: &HashMap<GameRef, Vec<u8>>,
    patches: &crate::patch::PatchMap,
    patch: Option<&(String, semver::Version)>,
) -> Vec<TemplateOption> {
    let version = patch.and_then(|(name, ver)| patches.get(name).and_then(|p| p.versions.get(ver)));
    let mut out = Vec::new();
    for game in crate::game::games_in_family(family) {
        if !roms.contains_key(&game) {
            continue;
        }
        if let Some(v) = version {
            if !v.supported_games.contains(&game) {
                continue;
            }
        }
        for raw in template_names_for_game(game, version) {
            let label = format!(
                "{} \u{2013} {}",
                crate::game::variant_short_name(lang, game),
                template_label(lang, game.family_and_variant().0, &raw)
            );
            out.push(TemplateOption { game, raw, label });
        }
    }
    out
}

/// Template names for one game: patch-provided first (they override
/// bundled ones of the same name), then the game's bundled templates.
fn template_names_for_game(game: GameRef, version: Option<&std::sync::Arc<crate::patch::Version>>) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Some(m) = version.and_then(|v| v.save_templates.get(&game)) {
        names.extend(m.keys().cloned());
    }
    for (name, _) in game.save_templates.iter() {
        if !names.iter().any(|n| n == name) {
            names.push((*name).to_string());
        }
    }
    names
}

/// Resolve the actual template `Save` for a (game, template-name)
/// pick. Falls back to the default/first template if the exact name
/// vanished between pick and confirm.
pub fn creation_template(
    game: GameRef,
    template_name: &str,
    patches: &crate::patch::PatchMap,
    patch: Option<&(String, semver::Version)>,
) -> Option<Box<dyn tango_dataview::save::Save + Send + Sync>> {
    let version = patch.and_then(|(name, ver)| patches.get(name).and_then(|p| p.versions.get(ver)));
    if let Some(m) = version.and_then(|v| v.save_templates.get(&game)) {
        if let Some(save) = m.get(template_name).or_else(|| m.get("")) {
            return Some(save.clone_box());
        }
    }
    game.save_templates
        .iter()
        .find(|(name, _)| *name == template_name)
        .or_else(|| game.save_templates.first())
        .map(|(_, save)| save.clone_box())
}

/// Bare localized template label (e.g. "Heat Guts"). Empty `raw` is
/// the unnamed default template; the family's `save-megaman` string
/// usually carries the right label for it.
fn template_label(lang: &unic_langid::LanguageIdentifier, family: &str, raw: &str) -> String {
    let key_suffix = if raw.is_empty() { "megaman" } else { raw };
    crate::game::family_str(family, lang, &format!("save-{key_suffix}")).unwrap_or_else(|| {
        if raw.is_empty() {
            crate::t!(lang, "save-template-default")
        } else {
            raw.to_string()
        }
    })
}

/// Localized "<game-variant> - <template>" suggestion for the new-save
/// filename field, filesystem-sanitized.
pub fn suggest_save_name(lang: &unic_langid::LanguageIdentifier, game: GameRef, template: Option<&str>) -> String {
    let game_name = crate::game::display_name(lang, game);
    let family = game.family_and_variant().0;
    let name = match template {
        Some(raw) => {
            let label = template_label(lang, family, raw);
            format!("{game_name} - {label}")
        }
        None => game_name,
    };
    sanitize_filename(&name)
}

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

/// Appends ` 2`, ` 3`, ... to `base` until the resulting `<name>.sav`
/// doesn't already exist in `saves_dir`. Gives up at 99.
pub fn disambiguate_save_name(saves_dir: &std::path::Path, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !saves_dir.join(format!("{draft}.sav")).exists() {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// Next free "<stem> (copy)" / "<stem> (copy N)" stem for `src` — the
/// prefill for the duplicate form, so a plain Enter behaves like a
/// one-click duplicate.
pub fn suggest_duplicate_stem(src: &std::path::Path) -> String {
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    for n in 1..1000 {
        let suffix = if n == 1 {
            " (copy)".to_string()
        } else {
            format!(" (copy {n})")
        };
        let candidate_stem = format!("{stem}{suffix}");
        let filename = match &ext {
            Some(ext) => format!("{candidate_stem}.{ext}"),
            None => candidate_stem.clone(),
        };
        let taken = src.parent().map(|p| p.join(filename).exists()).unwrap_or(false);
        if !taken {
            return candidate_stem;
        }
    }
    format!("{stem} (copy)")
}

/// Copy `src` to a sibling file named `new_stem` (extension
/// preserved). Refuses path-traversal, empty names, and existing
/// destinations — same rules as [`rename_save`].
pub fn duplicate_save(src: &std::path::Path, new_stem: &str) -> anyhow::Result<std::path::PathBuf> {
    if new_stem.is_empty() {
        anyhow::bail!("empty save name");
    }
    if new_stem.contains('/') || new_stem.contains('\\') || new_stem.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    let new_name = if let Some(ext) = ext {
        format!("{new_stem}.{ext}")
    } else {
        new_stem.to_string()
    };
    let dst = parent.join(new_name);
    if dst == src || dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

/// Rename `src` to use `new_stem` (extension preserved). Refuses
/// path-traversal or empty names.
pub fn rename_save(src: &std::path::Path, new_stem: &str) -> anyhow::Result<std::path::PathBuf> {
    if new_stem.is_empty() {
        anyhow::bail!("empty save name");
    }
    if new_stem.contains('/') || new_stem.contains('\\') || new_stem.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let parent = src.parent().ok_or_else(|| anyhow::anyhow!("save has no parent dir"))?;
    let ext = src.extension().map(|e| e.to_string_lossy().into_owned());
    let new_name = if let Some(ext) = ext {
        format!("{new_stem}.{ext}")
    } else {
        new_stem.to_string()
    };
    let dst = parent.join(new_name);
    if dst == src {
        return Ok(dst);
    }
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::rename(src, &dst)?;
    Ok(dst)
}

/// Write a template's SRAM to `saves_dir/<name>.sav`.
///
/// `rebuild_checksum()` is required before `to_sram_dump()` — without
/// it the SRAM checksum is stale (computed at template-construction
/// time) and both the GBA game and `parse_save` reject the file.
pub fn create_new_save(
    saves_dir: &std::path::Path,
    name: &str,
    template: &dyn tango_dataview::save::Save,
) -> anyhow::Result<std::path::PathBuf> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("empty save name");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        anyhow::bail!("invalid save name");
    }
    let filename = if name.ends_with(".sav") {
        name.to_string()
    } else {
        format!("{name}.sav")
    };
    let dst = saves_dir.join(filename);
    if dst.exists() {
        anyhow::bail!("destination already exists");
    }
    std::fs::create_dir_all(saves_dir)?;
    let mut save = template.clone_box();
    save.rebuild_checksum();
    let sram = save.to_sram_dump();
    std::fs::write(&dst, sram)?;
    Ok(dst)
}
