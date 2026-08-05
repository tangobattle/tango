//! BN3's save-editor UI: the style-bearing navicust (read-only) and the chip folder.

use tango_gamesupport_common::editor::loaded::OpenSave;
use tango_gamesupport_common::editor::view as sv;
use tango_gamesupport_common::editor::view::{Action, RenderOpts, State, Tab};
use tango_gamesupport_common::editor::{GameSaveEditor, SaveEditorShell};
use unic_langid::LanguageIdentifier;

pub struct Ui;

/// The instance tango's per-family registry hands out.
pub static SAVE_EDITOR: SaveEditorShell<Ui> = SaveEditorShell(Ui);

impl GameSaveEditor for Ui {
    fn tabs(&self, loaded: &OpenSave) -> Vec<Tab> {
        let _ = loaded;
        {
            let mut tabs = vec![];
            if loaded.save.view_navicust().is_some() {
                tabs.push(Tab::Navicust);
            }
            tabs.push(Tab::Folder);
            tabs
        }
    }

    fn render<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        opts: RenderOpts,
    ) -> iced::Element<'a, Action> {
        let _ = opts;
        match tab {
            Tab::Cover => sv::cover::render_cover(lang, loaded),
            Tab::Navicust => sv::navicust::render_navicust_tab(lang, loaded),
            Tab::Folder => sv::folder::render_folder(lang, loaded, opts.folder_grouped),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn render_edit<'a>(
        &self,
        lang: &'a LanguageIdentifier,
        tab: Tab,
        loaded: &'a OpenSave,
        state: &'a State,
    ) -> iced::Element<'a, Action> {
        match tab {
            Tab::Folder => sv::folder::render_folder_edit(lang, loaded, state),
            _ => sv::placeholder(tango_gamesupport_common::t!(lang, "save-empty")),
        }
    }

    fn tab_as_text(&self, _lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
        let _ = opts;
        match tab {
            Tab::Navicust => sv::navicust::navicust_as_text(loaded),
            Tab::Folder => sv::folder::as_text(loaded, opts),
            _ => None,
        }
    }

    fn tab_as_image(&self, tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
        match tab {
            Tab::Navicust => sv::navicust::as_image(loaded),
            _ => None,
        }
    }
}
