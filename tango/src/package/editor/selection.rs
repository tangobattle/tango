//! Independent editor choices, shared by the picker and read-only setup panels.
use crate::library::rom;
use crate::package::Choice;
use tango_library::package::{Catalog, ExportRef};
use tango_script::{ExportKind, Profile};

/// Offer current versions and retain a compatible, explicitly pinned older one.
pub(crate) fn choices(catalog: &Catalog, rom: &[u8], selected: Option<&ExportRef>) -> Vec<Choice> {
    let mut exports = catalog.latest_exports(ExportKind::Editor);
    if let Some(export) = selected
        .filter(|reference| reference.kind == ExportKind::Editor)
        .and_then(|reference| catalog.export(reference))
    {
        if !exports.iter().any(|candidate| candidate.reference == export.reference) {
            exports.push(export);
        }
    }
    exports
        .into_iter()
        .filter_map(|export| match export.profile.supports_editor(rom) {
            Ok(true) => Some(Choice {
                reference: export.reference.clone(),
                profile: export.profile.clone(),
            }),
            Ok(false) => None,
            Err(error) => {
                log::warn!(
                    "editor {} / {} detection failed: {error}",
                    export.reference.package.name,
                    export.reference.name
                );
                None
            }
        })
        .collect()
}

pub(crate) fn resolve(catalog: &Catalog, rom: &[u8], selected: Option<&ExportRef>) -> Result<Option<Profile>, String> {
    if let Some(selected) = selected {
        if selected.kind != ExportKind::Editor {
            return Err("selected export is not an editor".into());
        }
        let export = catalog
            .export(selected)
            .ok_or("selected editor is no longer installed")?;
        if !export.profile.supports_editor(rom).map_err(|error| error.to_string())? {
            return Err("selected editor does not recognize this ROM".into());
        }
        return Ok(Some(export.profile.clone()));
    }
    let mut choices = choices(catalog, rom, None).into_iter();
    let first = choices.next();
    if choices.next().is_some() {
        return Err("select an editor for this ROM".into());
    }
    Ok(first.map(|choice| choice.profile))
}

#[derive(Default)]
pub struct Selection {
    key: Option<(rom::Id, u64, String)>,
    pub choices: Vec<Choice>,
    pub selected: Option<ExportRef>,
    pub profile: Option<Profile>,
    pub error: Option<String>,
    pub templates: Vec<tango_script::SaveTemplate>,
}

impl Selection {
    pub fn restore(&mut self, reference: ExportRef) {
        self.selected = Some(reference);
        self.key = None;
    }

    pub fn select(&mut self, reference: ExportRef) {
        if self.choices.iter().any(|choice| choice.reference == reference) {
            self.restore(reference);
        }
    }

    pub fn refresh(&mut self, catalog: &Catalog, revision: u64, rom: &[u8], locale: &str) {
        let key = (rom::Id::of(rom), revision, locale.to_owned());
        if self.key.as_ref() == Some(&key) {
            return;
        }
        self.key = Some(key);
        self.choices = choices(catalog, rom, self.selected.as_ref());
        if self.selected.is_none() && self.choices.len() == 1 {
            self.selected = Some(self.choices[0].reference.clone());
        }
        let result = resolve(catalog, rom, self.selected.as_ref()).and_then(|profile| {
            profile
                .map(|profile| {
                    let inputs = tango_script::Inputs::new([("rom".into(), rom.to_vec())].into())
                        .map_err(|error| error.to_string())?;
                    profile
                        .with_inputs(inputs)
                        .with_locale(locale)
                        .map_err(|error| error.to_string())
                })
                .transpose()
        });
        self.profile = result.as_ref().ok().cloned().flatten();
        self.error = result.err();
        self.templates = self
            .profile
            .as_ref()
            .and_then(|profile| match profile.save_templates() {
                Ok(templates) => Some(templates),
                Err(error) => {
                    log::warn!("save templates failed: {error}");
                    None
                }
            })
            .unwrap_or_default();
    }

    /// Keep an explicit choice when files disappear, without retaining stale UI.
    pub fn unavailable(&mut self) {
        self.key = None;
        self.choices.clear();
        self.profile = None;
        self.templates.clear();
    }
}

pub(crate) fn remembered(config: &crate::config::Config, rom: rom::Id) -> Option<ExportRef> {
    config
        .package_loadouts
        .iter()
        .find(|entry| entry.rom == rom)
        .and_then(|entry| entry.editor.as_ref())
        .map(|editor| editor.reference(ExportKind::Editor))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(versions: &[&str]) -> Catalog {
        let packages: Vec<_> = versions.iter().map(|version| {
            tango_script::Package::load([
                ("package.toml".into(), format!("api=1\nname='editors'\nversion='{version}'\n[[editor]]\nname='first'\npath='./init'\n[[editor]]\nname='second'\npath='./init'\n").into_bytes()),
                ("init.luau".into(), br#"--!strict
return {
    detect_rom = function(rom) return buffer.len(rom) == 1 end,
    decode = function(bytes) return bytes end,
    encode = function(bytes) return bytes end,
    validate = function() return {} end,
    update = function() end,
    view = function() return {kind='text', text='Editor'} end,
} :: Editor
"#.to_vec()),
            ].into()).unwrap()
        }).collect();
        futures::executor::block_on(Catalog::scan(
            crate::library::storage(),
            std::path::Path::new("unused"),
            &Default::default(),
            &packages,
        ))
    }

    #[test]
    fn ambiguity_requires_an_explicit_editor_choice() {
        let catalog = catalog(&["1.0.0"]);
        let mut selected = Selection::default();
        selected.refresh(&catalog, 1, &[42], "en-US");
        assert_eq!(selected.choices.len(), 2);
        assert!(selected.error.as_deref().unwrap().contains("select an editor"));
        assert!(selected.profile.is_none());
        let second = selected
            .choices
            .iter()
            .find(|choice| choice.reference.name == "second")
            .unwrap()
            .reference
            .clone();
        selected.select(second.clone());
        selected.refresh(&catalog, 1, &[42], "en-US");
        assert!(selected.error.is_none());
        assert!(selected.profile.is_some());
        assert_eq!(selected.selected, Some(second));
    }

    #[test]
    fn a_pinned_editor_survives_updates_and_never_falls_back_when_removed() {
        let original = catalog(&["1.0.0"]);
        let pinned = choices(&original, &[42], None)[0].reference.clone();
        let mut selected = Selection::default();
        selected.restore(pinned.clone());
        let updated = catalog(&["1.0.0", "2.0.0"]);
        selected.refresh(&updated, 2, &[42], "en-US");
        assert_eq!(selected.selected, Some(pinned.clone()));
        assert_eq!(selected.choices.len(), 3);
        assert!(selected.profile.is_some());
        selected.refresh(&catalog(&["2.0.0"]), 3, &[42], "en-US");
        assert_eq!(selected.selected, Some(pinned));
        assert!(selected.profile.is_none());
        assert!(selected.error.as_deref().unwrap().contains("no longer installed"));
    }
}
