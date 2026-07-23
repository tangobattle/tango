//! Save-file management for the Play tab: the duplicate / rename /
//! delete / create-from-template flows — their inline-form state
//! ([`SaveAction`]), message handling, form views, and the on-disk
//! file operations the App runs for the resulting Effects. Pure
//! save-library concerns; nothing here touches netplay or the save
//! view.

use super::*;

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub enum SaveAction {
    #[default]
    None,
    Renaming {
        draft: String,
    },
    /// Duplicating the selected save. `draft` is the new file's name,
    /// prefilled with the next free "<stem> (copy)" suggestion so a
    /// plain Enter behaves like the old one-click duplicate.
    Duplicating {
        draft: String,
    },
    ConfirmDelete,
    /// Creating a new save. `template` is the template name (empty
    /// string is the default unnamed template); `draft` is the user's
    /// chosen filename. `game` is the concrete variant the save is
    /// created for — chosen together with the template, since within a
    /// family the same template name exists per color (White/Blue), and
    /// the new file must carry the right variant signature.
    /// `template`/`game` stay `None` until the user picks (auto-selected
    /// when only one option exists). The Confirm button is disabled in
    /// that state — there's no "default" template to fall back on.
    NewSave {
        draft: String,
        game: Option<rom::GameRef>,
        template: Option<String>,
        /// The auto-generated default we last wrote into `draft`. While
        /// the user hasn't typed over it, switching templates regenerates
        /// the suggestion; once they edit it, this is `None` and we leave
        /// their value alone.
        auto_default: Option<String>,
    },
}

impl State {
    /// Apply one of the save-management messages (duplicate / rename /
    /// delete / create + the folder-opening conveniences). Routed here
    /// from [`State::update`]'s dispatch so the tab shell stays about
    /// netplay + the save view.
    pub(super) fn update_save_manage(
        &mut self,
        msg: Message,
        scanners: &Scanners,
        config: &config::Config,
        loadout: &Loadout,
    ) -> Option<Effect> {
        match msg {
            Message::SaveOpenFolder => loadout.save.as_ref().map(|p| Effect::RevealPath(p.to_path_buf())),
            Message::OpenSavesFolder(path) => Some(Effect::OpenPath(path)),
            Message::SaveDuplicateStart => {
                // Prefill with the next free "<stem> (copy)" name so a
                // plain Enter behaves like the old one-click duplicate.
                let draft = loadout.save.as_deref().map(suggest_duplicate_stem).unwrap_or_default();
                self.save_action = SaveAction::Duplicating { draft };
                None
            }
            Message::SaveDuplicateDraftChanged(s) => {
                if let SaveAction::Duplicating { draft } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveDuplicateConfirm => {
                let new_stem = if let SaveAction::Duplicating { draft } = &self.save_action {
                    draft.trim().to_string()
                } else {
                    String::new()
                };
                self.save_action = SaveAction::None;
                if new_stem.is_empty() {
                    None
                } else {
                    Some(Effect::SaveDuplicate { new_stem })
                }
            }
            Message::SaveRenameStart => {
                let draft = loadout
                    .save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                self.save_action = SaveAction::Renaming { draft };
                None
            }
            Message::SaveRenameDraftChanged(s) => {
                if let SaveAction::Renaming { draft } = &mut self.save_action {
                    *draft = s;
                }
                None
            }
            Message::SaveRenameConfirm => {
                let new_stem = if let SaveAction::Renaming { draft } = &self.save_action {
                    draft.trim().to_string()
                } else {
                    String::new()
                };
                self.save_action = SaveAction::None;
                if new_stem.is_empty() {
                    None
                } else {
                    Some(Effect::SaveRename { new_stem })
                }
            }
            Message::SaveDeleteStart => {
                self.save_action = SaveAction::ConfirmDelete;
                None
            }
            Message::SaveDeleteConfirm => {
                self.save_action = SaveAction::None;
                Some(Effect::SaveDelete)
            }
            Message::SaveActionCancel => {
                self.save_action = SaveAction::None;
                None
            }
            Message::SaveNewStart => {
                let saves_dir = config.saves_path();
                // Candidate (variant, template) options span every
                // owned-ROM variant in the family — so you can bootstrap
                // the first save of an empty family, and a dual-ROM owner
                // can pick which color to create. Auto-select only when
                // there's exactly one option; otherwise force an explicit
                // pick (Confirm stays disabled until they do).
                let options = creation_template_options(&config.language, loadout, scanners);
                let (game, template) = if options.len() == 1 {
                    let (game, raw) = options[0].value.clone();
                    (Some(game), Some(raw))
                } else {
                    (None, None)
                };
                let draft = match game {
                    Some(g) => {
                        disambiguate_save_name(&saves_dir, &suggest_save_name(&config.language, g, template.as_deref()))
                    }
                    // No single default yet — seed the field with the
                    // variant-neutral family name so it isn't empty (and
                    // doesn't presume a color) while the user picks a
                    // template.
                    None => loadout
                        .family
                        .map(|f| {
                            disambiguate_save_name(
                                &saves_dir,
                                &sanitize_filename(&game::family_display_name(&config.language, f)),
                            )
                        })
                        .unwrap_or_else(|| "new save".to_string()),
                };
                self.save_action = SaveAction::NewSave {
                    auto_default: Some(draft.clone()),
                    draft,
                    game,
                    template,
                };
                None
            }
            Message::SaveNewDraftChanged(s) => {
                if let SaveAction::NewSave {
                    draft, auto_default, ..
                } = &mut self.save_action
                {
                    if auto_default.as_deref() != Some(s.as_str()) {
                        *auto_default = None;
                    }
                    *draft = s;
                }
                None
            }
            Message::SaveNewTemplateSelected(sel_game, name) => {
                if let SaveAction::NewSave {
                    draft,
                    game,
                    template,
                    auto_default,
                } = &mut self.save_action
                {
                    *game = Some(sel_game);
                    *template = Some(name);
                    if auto_default.as_deref() == Some(draft.as_str()) {
                        let new_draft = disambiguate_save_name(
                            &config.saves_path(),
                            &suggest_save_name(&config.language, sel_game, template.as_deref()),
                        );
                        *draft = new_draft.clone();
                        *auto_default = Some(new_draft);
                    }
                }
                None
            }
            Message::SaveNewConfirm => {
                let SaveAction::NewSave {
                    draft,
                    game: Some(game),
                    template: Some(template),
                    ..
                } = &self.save_action
                else {
                    return None;
                };
                let game = *game;
                let name = draft.trim().to_string();
                let template = template.clone();
                self.save_action = SaveAction::None;
                if name.is_empty() {
                    None
                } else {
                    Some(Effect::SaveNew { name, template, game })
                }
            }
            // Only the Save* family is routed here.
            _ => None,
        }
    }
}

impl State {
    /// The strip's second row: the save picker + action buttons at
    /// rest, or whichever rename / delete / create-from-template form
    /// is in flight ([`SaveAction`]).
    pub(super) fn save_action_row<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        save_picker: Element<'a, Message>,
    ) -> Element<'a, Message> {
        // The picker row fade-through morphs into whichever form
        // opens and back — the swap hides the row's reflow at its
        // fully-dissolved midpoint. While the form is on its way
        // out it has already been reset to `None`, so the exit
        // half renders the frozen copy.
        let now = iced::time::Instant::now();
        let (render_form, form_swap) = crate::ui::anim::swap_phase(&self.save_form, now);
        let action = if render_form && self.save_action == SaveAction::None {
            &self.save_action_exit
        } else {
            &self.save_action
        };
        let mut row_el: Element<'a, Message> =
            self.save_action_row_inner(lang, scanners, loadout, save_picker, render_form, action);
        if let Some(phase) = form_swap {
            row_el = crate::ui::anim::swap_transform(row_el, phase, iced::Vector::new(24.0, 0.0), widgets::plate_color);
        }
        row_el
    }

    fn save_action_row_inner<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
        save_picker: Element<'a, Message>,
        render_form: bool,
        action: &'a SaveAction,
    ) -> Element<'a, Message> {
        if !render_form {
            return row![
                self.new_save_button(lang, scanners, loadout),
                save_picker,
                save_actions_menu(lang, loadout),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into();
        }
        match action {
            SaveAction::None => {
                // Form side with nothing recorded (shouldn't happen
                // — the exit snapshot is always set before the swap
                // starts) — degrade to the picker row.
                row![
                    self.new_save_button(lang, scanners, loadout),
                    save_picker,
                    save_actions_menu(lang, loadout),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            // Every form ends [× cancel][confirm] — cancel before
            // confirm, same order as the edit-mode Save / Cancel pair
            // and the modal dialogs, so the primary action always
            // sits at the row's end. Confirm buttons repeat the icon
            // of the toolbar action that opened the form, so the form
            // visibly answers the button that started it.
            SaveAction::Renaming { draft } => row![
                save_name_input(lang, draft, Message::SaveRenameDraftChanged, Message::SaveRenameConfirm),
                save_action_cancel_button(lang),
                widgets::labeled_icon_button(
                    Icon::PencilLine,
                    t!(lang, "save-rename-confirm"),
                    Message::SaveRenameConfirm,
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::Duplicating { draft } => row![
                save_name_input(
                    lang,
                    draft,
                    Message::SaveDuplicateDraftChanged,
                    Message::SaveDuplicateConfirm
                ),
                save_action_cancel_button(lang),
                widgets::labeled_icon_button(
                    Icon::Files,
                    t!(lang, "save-duplicate"),
                    Message::SaveDuplicateConfirm,
                    STANDARD_PADDING,
                    widgets::primary_button,
                ),
            ]
            .spacing(8)
            .align_y(Alignment::Center)
            .into(),
            SaveAction::ConfirmDelete => {
                // Name the target — "Delete BN3 White?" reads as a
                // decision; "Delete this save?" reads as a riddle
                // about what's currently selected.
                let name = loadout
                    .save
                    .as_ref()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
                    .unwrap_or_default();
                row![
                    text(t!(lang, "save-delete-prompt", name = name))
                        .style(widgets::muted_text_style)
                        .width(Length::Fill),
                    save_action_cancel_button(lang),
                    widgets::labeled_icon_button(
                        Icon::Trash,
                        t!(lang, "save-delete-confirm"),
                        Message::SaveDeleteConfirm,
                        STANDARD_PADDING,
                        widgets::danger_button,
                    ),
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
            SaveAction::NewSave {
                draft, game, template, ..
            } => {
                // One option per (owned-ROM variant × template). Each
                // carries the raw name plus a locale-aware label; when the
                // family has more than one owned variant the label is
                // prefixed with the game name ("White – Heat Guts") so the
                // user picks color + template in one go.
                let options = creation_template_options(lang, loadout, scanners);
                let selected = match (game, template) {
                    (Some(g), Some(t)) => options.iter().find(|o| o.value.0 == *g && &o.value.1 == t).cloned(),
                    _ => None,
                };
                let can_confirm = game.is_some() && template.is_some() && !draft.trim().is_empty();
                let confirm_btn = if can_confirm {
                    widgets::labeled_icon_button(
                        Icon::FilePlus,
                        t!(lang, "save-new-confirm"),
                        Message::SaveNewConfirm,
                        STANDARD_PADDING,
                        widgets::primary_button,
                    )
                } else {
                    widgets::labeled_icon_button_maybe(
                        Icon::FilePlus,
                        t!(lang, "save-new-confirm"),
                        None,
                        STANDARD_PADDING,
                        widgets::neutral,
                    )
                };
                row![
                    widgets::picker(options, selected, |o: widgets::Choice<(rom::GameRef, String)>| {
                        Message::SaveNewTemplateSelected(o.value.0, o.value.1)
                    })
                    .placeholder(t!(lang, "save-template-pick"))
                    .width(Length::Fixed(180.0)),
                    save_name_input(lang, draft, Message::SaveNewDraftChanged, Message::SaveNewConfirm),
                    save_action_cancel_button(lang),
                    confirm_btn,
                ]
                .spacing(8)
                .align_y(Alignment::Center)
                .into()
            }
        }
    }

    /// The "new save" button, leading the row from the picker's left —
    /// creation stands apart from the manage-what's-there actions on
    /// the picker's right. Enabled whenever the selected family has an
    /// owned-ROM variant that ships (bundled or patch) save templates
    /// — independent of whether a save is currently selected, so the
    /// first save of an empty family can still be created.
    fn new_save_button<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &'a Scanners,
        loadout: &'a Loadout,
    ) -> Element<'a, Message> {
        let can_new = creation_games(loadout, scanners).iter().any(|g| {
            templates_for_game(g, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners).is_some()
        });
        widgets::icon_button_maybe(
            Icon::FilePlus,
            t!(lang, "save-new"),
            can_new.then_some(Message::SaveNewStart),
            STANDARD_PADDING,
        )
    }
}

/// The ⋮ menu of manage-what's-there actions (open folder /
/// duplicate / rename / delete), collapsed behind one trigger so
/// the picker row stays [New save][picker][⋮]. Every action needs
/// a selected save to act on, so the whole trigger disables when
/// there isn't one. Each row wears the icon its standalone button
/// used to, Delete in danger red — and its inline confirm still
/// stands between the click and the file.
fn save_actions_menu<'a>(lang: &LanguageIdentifier, loadout: &Loadout) -> Element<'a, Message> {
    let items = vec![
        widgets::MenuItem::new(Icon::FolderOpen, t!(lang, "save-open-folder"), Message::SaveOpenFolder),
        widgets::MenuItem::new(Icon::Files, t!(lang, "save-duplicate"), Message::SaveDuplicateStart),
        widgets::MenuItem::new(Icon::PencilLine, t!(lang, "save-rename"), Message::SaveRenameStart),
        widgets::MenuItem::danger(Icon::Trash, t!(lang, "save-delete"), Message::SaveDeleteStart),
    ];
    widgets::menu_button(
        Icon::EllipsisVertical,
        t!(lang, "save-actions"),
        items,
        loadout.save.is_some(),
        STANDARD_PADDING,
    )
}

/// The "× Cancel" button that ends every save-action form (rename / duplicate
/// / delete / new) — identical across all four.
fn save_action_cancel_button<'a>(lang: &LanguageIdentifier) -> Element<'a, Message> {
    widgets::icon_button(
        Icon::X,
        t!(lang, "save-action-cancel"),
        Message::SaveActionCancel,
        STANDARD_PADDING,
    )
}

/// The save-name text field shared by the rename / duplicate / new-save forms,
/// differing only in the draft-changed and submit messages it emits.
fn save_name_input<'a>(
    lang: &LanguageIdentifier,
    draft: &'a str,
    on_input: impl Fn(String) -> Message + 'a,
    on_submit: Message,
) -> Element<'a, Message> {
    text_input(&t!(lang, "save-name-placeholder"), draft)
        .on_input(on_input)
        .on_submit(on_submit)
        .style(widgets::chunky_text_input)
        .padding(STANDARD_PADDING)
        .width(Length::Fill)
        .into()
}

// ---------- New-save template helpers ----------

/// Localized "<game-variant> <template-display>" (or just "<game-variant>"
/// when no template is chosen yet), with filesystem-unsafe characters
/// stripped so it can be dropped straight into the new-save text field.
/// Uses the full variant-aware display name so multi-version games like
/// BN6 Gregar/Falzar get disambiguated.
fn suggest_save_name(lang: &unic_langid::LanguageIdentifier, game: rom::GameRef, template: Option<&str>) -> String {
    let game_name = crate::library::game::display_name(lang, game);
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

/// Appends ` 2`, ` 3`, ... to `base` until the resulting `<name>.sav`
/// doesn't already exist in `saves_dir`. Gives up at 99 to avoid an
/// unbounded scan if the directory is somehow saturated.
fn disambiguate_save_name(saves_dir: &std::path::Path, base: &str) -> String {
    let mut draft = base.to_string();
    for n in 2..100 {
        if !saves_dir.join(format!("{draft}.sav")).exists() {
            break;
        }
        draft = format!("{base} {n}");
    }
    draft
}

/// Owned-ROM games in the selected family, ascending variant order —
/// the candidate targets for creating a new save. Empty when no family
/// is selected or no ROM is owned. Independent of the resolved game, so
/// the new-save flow works even before any save exists in the family.
/// When a patch is selected, variants it doesn't support are dropped
/// (so their templates don't show) — creating a save under an active
/// patch is a patch-specific flow.
fn creation_games(loadout: &Loadout, scanners: &Scanners) -> Vec<rom::GameRef> {
    let Some(family) = loadout.family else {
        return Vec::new();
    };
    let roms = scanners.roms.read();
    let patch_supported = loadout::patch_supported_games(loadout, scanners);
    game::games_in_family(family)
        .filter(|g| roms.contains_key(g))
        .filter(|g| patch_supported.as_ref().map(|s| s.contains(g)).unwrap_or(true))
        .collect()
}

/// Save templates for one specific game (patch-provided override the
/// bundled ones), keyed by template name (empty string = default).
/// None when that game ships no templates.
fn templates_for_game(
    game: rom::GameRef,
    patch_name: Option<&str>,
    patch_version: Option<&semver::Version>,
    scanners: &Scanners,
) -> Option<indexmap::IndexMap<String, Box<dyn tango_dataview::save::Save + Send + Sync>>> {
    // IndexMap (not BTreeMap) so templates iterate in declaration order
    // — patch-provided first, then the game's bundled order — instead
    // of alphabetically by raw key.
    let mut out = indexmap::IndexMap::new();
    if let (Some(patch_name), Some(version)) = (patch_name, patch_version) {
        let patches = scanners.patches.read();
        if let Some(v) = patches.version(patch_name, version) {
            if let Some(m) = v.save_templates.get(&game) {
                for (name, save) in m.iter() {
                    out.insert(name.clone(), save.clone_box());
                }
            }
        }
    }
    // Fall back to bundled per-game templates registered via the Game
    // trait. Patch templates take precedence: if a patch ships a
    // "heat-guts" template, it overrides the built-in of the same name.
    if let Some(game_impl) = game::from_gamedb_entry(game) {
        for (name, save) in game_impl.save_templates.iter() {
            out.entry((*name).to_string()).or_insert_with(|| save.clone_box());
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Picker entries for the new-save dialog: every (owned-ROM variant ×
/// template) across the selected family. Each label is prefixed with the
/// short variant tag (e.g. "Blue – Heat Guts") in all cases.
fn creation_template_options(
    lang: &unic_langid::LanguageIdentifier,
    loadout: &Loadout,
    scanners: &Scanners,
) -> Vec<widgets::Choice<(rom::GameRef, String)>> {
    let games = creation_games(loadout, scanners);
    let mut out = Vec::new();
    for g in games {
        if let Some(tmpls) = templates_for_game(g, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners) {
            for name in tmpls.keys() {
                out.push(save_template_choice(lang, g, name));
            }
        }
    }
    out
}

/// Resolve the actual template `Save` for a (game, template-name) pick —
/// used by the App's SaveNew handler to materialize the file. Falls back
/// to the default/first template if the exact name vanished.
pub fn creation_template(
    game: rom::GameRef,
    template_name: &str,
    loadout: &Loadout,
    scanners: &Scanners,
) -> Option<Box<dyn tango_dataview::save::Save + Send + Sync>> {
    let tmpls = templates_for_game(game, loadout.patch.as_deref(), loadout.patch_version.as_ref(), scanners)?;
    tmpls
        .get(template_name)
        .or_else(|| tmpls.get(""))
        .or_else(|| tmpls.values().next())
        .map(|s| s.clone_box())
}

/// Bare localized template label (e.g. "Heat Guts"), without any
/// variant prefix. Empty `raw` is the unnamed default-template file that
/// patches ship as `<rom>_<rev>.sav`; the `.save-megaman` attr usually
/// carries the right label for it.
fn template_label(lang: &unic_langid::LanguageIdentifier, family: &str, raw: &str) -> String {
    let key_suffix = if raw.is_empty() { "megaman" } else { raw };
    game::family_str(family, lang, &format!("save-{key_suffix}")).unwrap_or_else(|| {
        if raw.is_empty() {
            t!(lang, "save-template-default")
        } else {
            raw.to_string()
        }
    })
}

/// One entry in the "new save" template pick_list: a concrete variant
/// plus a raw template name, with a display label resolved via
/// `game-<family>.save-<name>` (prefixed with the game name when the
/// family has more than one owned variant). The value is `(variant,
/// raw template name)` — the two together pick a unique creation
/// target.
fn save_template_choice(
    lang: &unic_langid::LanguageIdentifier,
    game: rom::GameRef,
    raw: &str,
) -> widgets::Choice<(rom::GameRef, String)> {
    let label = template_label(lang, game.family_and_variant().0, raw);
    // Always prefix with the short variant tag (e.g. "Blue – Heat
    // Guts"), even for single-owned-variant or single-variant
    // families, so the picker reads consistently.
    let display = format!(
        "{} \u{2013} {}",
        crate::library::game::variant_short_name(lang, game),
        label
    );
    widgets::Choice::new((game, raw.to_string()), display)
}

/// Next free "<stem> (copy)" / "<stem> (copy N)" stem for `src` —
/// the prefill for the duplicate form, so a plain Enter behaves like
/// the old one-click duplicate.
fn suggest_duplicate_stem(src: &std::path::Path) -> String {
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

/// Write a template's SRAM to `saves_dir/<name>.sav`. The filename is
/// taken verbatim from `name` (trimmed); on collisions returns Err.
///
/// `rebuild_checksum()` is required before `to_sram_dump()` — without
/// it the SRAM checksum is stale (computed at template-construction
/// time, before this game-specific clone) and both the GBA game and
/// Tango's `parse_save` reject the resulting file. The legacy app
/// does the same in `gui/save_select_view.rs::create_new_save`.
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

// ---------- "Commit to a match" CTA chrome ----------
//
// Shared between the bottom strip's Fight button and the lobby's
// Ready toggle — both are the same "slam this to fight" affordance,
// so they wear the same chrome.
