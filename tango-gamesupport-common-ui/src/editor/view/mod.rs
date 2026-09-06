//! The save editor's view layer: the state machine (tabs, [`State`],
//! [`Action`], [`Outcome`]), the shell ([`view`]) that hosts a game's
//! editor, and the shared components the per-game `ui` crates compose.
//! Nothing here is visible outside the gamesupport layer — the app
//! drives all of it through the shell in [`crate::editor::shell`].

use crate::editor::loaded::OpenSave;
use crate::i18n::t;
use crate::style::{self, BADGE_COLUMN_WIDTH, TEXT_BODY, TEXT_CAPTION};
use crate::widgets::muted_text_style;
use iced::widget::{button, container, image as iced_image, scrollable, stack, text, tooltip, Image, Space};
use iced::{Alignment, ContentFit, Element, Fill, Length};
use sweeten::widget::{column, row};
use unic_langid::LanguageIdentifier;

pub mod abd;
pub mod cover;
pub mod folder;
pub mod navi;
pub mod navicust;
pub mod patch_cards;

mod actions;
mod components;
mod state;

pub use actions::{Action, Outcome};
pub use components::{
    badge, card_wrap, card_wrap_maybe_danger_text, clear_all_button, colored_badge, colored_badge_sized,
    colored_badge_sized_with_danger, drag_handle, edit_row_wrap, edit_row_wrap_maybe_danger_text, edit_toggle_maybe,
    editor_header, editor_pane, editor_panes, library_header, limit_caption, placeholder, remove_button,
    reorder_drag_style, stat, tooltip_style,
};
pub use state::{
    AutoBattleDataSort, EditState, HeldPart, LibrarySort, NavicustSort, PatchCard56Sort, RenderOpts, State, Tab,
};

/// Left edge fade for the scrollable sub-tab strip: opaque pane plate at the
/// left edge dissolving to transparent inward, so a tab scrolled past the
/// start reads as "more to the left". Pure presentation — no event handlers,
/// so clicks and wheel-scroll fall through to the strip beneath.
fn tab_fade_left(theme: &iced::Theme) -> container::Style {
    edge_fade(theme, std::f32::consts::FRAC_PI_2)
}

/// Right edge fade for the scrollable sub-tab strip. It sits over the empty
/// plate when the tabs fit, so it stays invisible until tabs reach the edge.
fn tab_fade_right(theme: &iced::Theme) -> container::Style {
    edge_fade(theme, 3.0 * std::f32::consts::FRAC_PI_2)
}

/// A one-sided fade from the pane plate (at the `angle`-direction edge) to
/// transparent, for the tab strip's scroll-edge fades.
fn edge_fade(theme: &iced::Theme, angle: f32) -> container::Style {
    let plate = crate::widgets::plate_color(theme);
    let transparent = iced::Color { a: 0.0, ..plate };
    container::Style {
        background: Some(iced::Background::Gradient(iced::Gradient::Linear(
            iced::gradient::Linear::new(angle)
                .add_stop(0.0, plate)
                .add_stop(1.0, transparent),
        ))),
        ..Default::default()
    }
}

/// Scrollbar style for the sub-tab strip: invisible at rest (the edge fades are
/// the resting affordance) and revealing the standard chunky scrollbar only
/// while the strip is hovered or dragged.
fn tab_scrollbar(theme: &iced::Theme, status: iced::widget::scrollable::Status) -> iced::widget::scrollable::Style {
    let mut style = crate::widgets::chunky_scrollable(theme, status);
    if matches!(status, iced::widget::scrollable::Status::Active { .. }) {
        for rail in [&mut style.horizontal_rail, &mut style.vertical_rail] {
            rail.background = None;
            rail.scroller.background = iced::Background::Color(iced::Color::TRANSPARENT);
        }
    }
    style
}

/// Per-tab Lucide icon glyph used by the tab strip in [`view`].
fn tab_icon(tab: Tab) -> lucide_icons::Icon {
    use lucide_icons::Icon;
    match tab {
        Tab::Cover => Icon::Eye,
        Tab::Navicust => Icon::Puzzle,
        Tab::Folder => Icon::Files,
        Tab::PatchCards => Icon::CreditCard,
        Tab::AutoBattleData => Icon::Bot,
        Tab::ProgramDeck => Icon::Network,
        Tab::Party => Icon::Users,
    }
}

fn loaded_save_has_build_issue(loaded: &OpenSave) -> bool {
    loaded.save_editor.build_report(loaded).has_errors()
}

/// Wholesale save-view widget: tab strip with Lucide icons, optional
/// per-tab extras (folder group toggle, copy buttons), and the body.
/// Embedders just call this and `.map(Message::SaveViewAction)`.
///
/// `play_button`:
///   * `None`        — no Play button in the tab strip.
///   * `Some(true)`  — Play button rendered and enabled.
///   * `Some(false)` — Play button rendered but disabled (e.g.
///     while a netplay lobby is active and singleplayer would
///     conflict with the open session).
///
/// `editable`: when `true` (only the play tab passes this) and the
/// loaded save supports it, the Folder tab gains an Edit button that
/// flips its body into the in-place chip-deck editor. Replay /
/// opponent panels pass `false`, so they never show the affordance.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a OpenSave,
    state: &'a State,
    streamer_mode: bool,
    play_button: Option<bool>,
    inline_actions: bool,
    editable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use iced::{Alignment, Fill};

    let now = iced::time::Instant::now();
    if streamer_mode && !state.reviewing {
        let cover = cover::render_cover_gate(lang, loaded, Action::Review);
        return crate::anim::slide_in_opt(cover, state.enter.progress(now), state.enter_from);
    }

    let available = available_tabs(loaded, streamer_mode);
    let build_report = loaded.save_editor.build_report(loaded);
    let can_save = !build_report.has_errors();

    // Body entrance — restarted on sub-tab switches (sliding along the strip's
    // direction of travel), edit-mode toggles and game/save swaps (rising in
    // vertically).
    let enter = state.enter.progress(now);
    let enter_from = state.enter_from;
    let entered = move |el: Element<'a, Action>| crate::anim::slide_in_opt(el, enter, enter_from);
    // Tab-tail extras animate only when their content actually changed — a
    // sub-tab switch (horizontal enter). A game/save swap rises the body in, but
    // the extras are typically identical across saves, and re-animating them
    // there reads as a glitch.
    let tail_slide = enter.filter(|_| enter_from.x != 0.0);
    let extras_dx = if enter_from.x != 0.0 { enter_from.x } else { 24.0 };
    let extras_entered =
        move |el: Element<'a, Action>| crate::anim::slide_in_opt(el, tail_slide, iced::Vector::new(extras_dx, 0.0));
    // Edit-mode morph: the navi header's Edit / Play and the Save / Cancel pair
    // fade-through swap in both directions, so Edit visibly turns into Save /
    // Cancel and back.
    let (edit_side, edit_swap) = crate::anim::swap_phase(&state.edit_anim, now);
    let render_edit_buttons = editable && edit_side;
    // One global edit session covers the whole save (entered from the Edit button
    // in the navi header); `editing_session` is on whenever it's open and selects
    // each section's editable body below. It also suppresses the Play button
    // (single-player would fight the open session); one Save / Cancel commits /
    // discards every section at once.
    let editing_session = editable && state.editing.is_some();
    // The single Edit button covers the whole save: shown whenever *any* section
    // is editable, not per-tab. Once open, the user navigates tabs to edit each
    // section (and clicks the navi header to swap navi). A game whose editable
    // thing lives outside the shared sections — BN5DS's file and cross picks,
    // which are its own top-bar control — says so itself.
    let save_editable = editable && (loaded.editability.any() || loaded.save_editor.extra_editable(loaded));

    // The save-level actions live at the navi header's right edge (not the tab
    // strip): Edit + Play in read mode, Save / Cancel while editing, swapping
    // between the two as one unit.
    let mut actions = row![].spacing(6).align_y(Alignment::Center);
    if render_edit_buttons {
        if inline_actions {
            actions = actions.push(edit_buttons(lang, can_save));
        }
    } else {
        if inline_actions && save_editable {
            actions = actions.push(widgets::labeled_icon_button(
                lucide_icons::Icon::Pencil,
                t!(lang, "save-edit"),
                Action::EnterEdit,
                [4.0, 10.0],
                widgets::neutral,
            ));
        }
        if let Some(enabled) = play_button {
            // Training is hidden for now: the session, its action and
            // everything behind [`Action::TrainingClicked`] are intact,
            // so putting the button back is this block alone.
            let label = row![lucide_icons::Icon::Play.widget(), text(t!(lang, "play-play"))]
                .spacing(6)
                .align_y(Alignment::Center);
            let mut btn = button(label).padding([4, 10]);
            if enabled {
                btn = btn.style(widgets::primary_button).on_press(Action::PlayClicked);
            } else {
                btn = btn.style(widgets::neutral);
            }
            actions = actions.push(btn);
        }
    }
    let mut actions_tail: Element<'a, Action> = actions.into();
    if let Some(phase) = edit_swap {
        actions_tail = crate::anim::swap_transform(
            actions_tail,
            phase,
            iced::Vector::new(32.0, 0.0),
            crate::widgets::plate_color,
        );
    }
    // The game's own bar control (BN5DS's file + cross picks) rides at
    // the left of the cluster, so Save / Cancel keep the right edge. It
    // sits outside the edit-mode swap above: what it selects isn't an
    // action, and it must not fade with Save/Cancel turning back into
    // Edit. Shown only while an edit session is open — what it changes
    // is a staged edit like any other — so a read-only embed (a pvp
    // setup pane, the replay viewer) never carries it. See
    // `GameSaveEditor`.
    if editing_session {
        if let Some(control) = loaded.save_editor.top_bar_control(lang, loaded) {
            actions_tail = row![control, actions_tail].spacing(6).align_y(Alignment::Center).into();
        }
    }

    // The equipped navi (emblem / name / HP / buster) rides in a slim header
    // strip above the body on every tab — it used to be a tab of its own, but
    // it's a single row of stats, so a tab spent the whole body on it. The
    // save-level actions sit at its right edge. While editing (and the navi is
    // editable) the card itself becomes the change-navi affordance — clicking it
    // opens the picker as the body.
    // Always rendered, navi or not: the strip is where Play lives, and Play
    // is the only way to start the game.
    let navi_edit = (editing_session && loaded.editability.navi).then_some(Action::EnterEditNavi);
    // The game's own, which by default *is* the navi strip — a game whose
    // identity isn't a navi off the shared roster (BN5DS's cross) overrides it
    // and gets told whether the session is open so it can name what the save
    // brings while reading and offer the pick while editing.
    let navi_strip = loaded
        .save_editor
        .identity_strip(lang, loaded, navi_edit, editing_session, actions_tail);

    if available.is_empty() {
        // No section tabs (an unsupported / empty save) — the strip still
        // goes up, since it carries Play.
        return column![navi_strip, placeholder(t!(lang, "save-empty"))]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .into();
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    // The global edit session selects each section's editable body below.
    let navi_editing = editing_session && loaded.editability.navi;

    // Tab strip: tabs left, the active tab's contextual extras (copy, folder
    // group toggle, copy-as-image) right — the save-level actions live in the
    // navi header now. We split into two rows so the tab list can wrap/scroll
    // without dragging the extras tail with it. The tail is a separate row,
    // sized to its content and capped to the tab button height so the strip's
    // overall height doesn't grow when the extras change.
    // Height of a small `widgets::tab_button`: TEXT_BODY at iced's
    // default 1.3 line height plus the [6, 14] chip padding. The
    // tail is pinned to exactly this so both halves of the strip
    // share a centerline (Start-aligned row + equal heights =
    // aligned); a stale hand-tuned 31.0 here had the chips riding
    // ~2px high against the tail buttons.
    const TAB_STRIP_HEIGHT: f32 = style::TEXT_BODY * 1.3 + 12.0;
    let mut tabs_only = row![].spacing(2).align_y(Alignment::Center);
    for tab in &available {
        let label = match tab {
            Tab::Cover => t!(lang, "save-tab-cover"),
            Tab::Navicust => t!(lang, "save-tab-navicust"),
            Tab::Folder => t!(lang, "save-tab-folder"),
            Tab::PatchCards => t!(lang, "save-tab-patch-cards"),
            Tab::AutoBattleData => t!(lang, "save-tab-auto-battle-data"),
            Tab::ProgramDeck => t!(lang, "save-tab-program-deck"),
            Tab::Party => t!(lang, "save-tab-party"),
        };
        let legality_errors = build_report
            .error_tabs
            .contains(tab)
            .then(|| loaded.save_editor.tab_errors(lang, *tab, loaded));
        tabs_only = tabs_only.push(widgets::tab_button(
            tab_icon(*tab),
            label,
            Action::SelectTab(*tab),
            *tab == active,
            legality_errors.as_deref(),
        ));
    }
    // Horizontally scrollable strip with a hidden scrollbar, so a long /
    // localized tab list scrolls instead of wrapping to a second line — the
    // edge fades below are the only scroll affordance.
    let tabs_scroll = scrollable(tabs_only)
        .id(state.tab_scroll_id.clone())
        .direction(scrollable::Direction::Horizontal(scrollable::Scrollbar::new()))
        .on_scroll(|v| Action::TabScrolled(v.relative_offset().x))
        .style(tab_scrollbar)
        .width(Fill);
    const TAB_FADE_W: f32 = 24.0;
    // Left fade only once scrolled off the start; right fade until the end.
    let left_fade: Element<'a, Action> = if state.tab_scroll > 0.01 {
        container(Space::new())
            .width(Length::Fixed(TAB_FADE_W))
            .height(Fill)
            .style(tab_fade_left)
            .into()
    } else {
        Space::new().into()
    };
    let right_fade: Element<'a, Action> = if state.tab_scroll < 0.99 {
        container(Space::new())
            .width(Length::Fixed(TAB_FADE_W))
            .height(Fill)
            .style(tab_fade_right)
            .into()
    } else {
        Space::new().into()
    };
    let tabs_only = stack![
        tabs_scroll,
        row![left_fade, Space::new().width(Fill), right_fade].height(Fill),
    ];
    // The tail carries only the active tab's contextual extras now — the
    // save-level Edit / Play / Save-Cancel actions moved to the navi header.
    // Extras are read-mode only; entering edit fades them out under the same
    // swap phase the header's morph uses.
    let mut side = row![].spacing(6).align_y(Alignment::Center);
    if !render_edit_buttons && inline_actions {
        // Per-control entrances: a control carried over from the previous
        // sub-tab (the copy button lives on most tabs) stays anchored; only
        // controls that actually appeared slide in. Suppressed while the
        // edit-mode morph runs — the whole tail is moving then.
        let prev_kinds = state.prev_tab.map(|p| extra_kinds(p)).unwrap_or_default();
        for kind in extra_kinds(active) {
            let el = render_extra(lang, state, active, kind);
            let carried = enter_from.x != 0.0 && prev_kinds.contains(&kind);
            let el = if edit_swap.is_some() || carried {
                el
            } else {
                extras_entered(el)
            };
            side = side.push(el);
        }
    }
    let mut tail: Element<'a, Action> = side.into();
    if let Some(phase) = edit_swap {
        tail = crate::anim::swap_transform(tail, phase, iced::Vector::new(32.0, 0.0), crate::widgets::plate_color);
    }
    let tab_row = row![
        container(tabs_only).width(Fill),
        container(tail)
            .height(Length::Fixed(TAB_STRIP_HEIGHT))
            .align_y(Alignment::Center),
    ]
    .spacing(8)
    .align_y(Alignment::Start);

    let tab_pane = container(tab_row.padding([4, 8])).width(Fill).style(widgets::pane);

    // The navi picker shows over the [tab strip + body] region, reached via the
    // header's change-navi card. It isn't a tab — `navi_select` alone says
    // whether it's up, and `active_tab` stays on whatever it's covering; the
    // incoming side slides up into place on open/dismiss — a plain vertical
    // slide like every tab/screen transition. The equipped navi stays visible in
    // the header above throughout.
    let show_picker = navi_editing && state.navi_select.shown();
    let navi_sliding = navi_editing && state.navi_select.is_animating(now);

    // The region below the header. The navi picker, the in-place editors and the
    // Cover logo banner each claim the full available height; the read-only
    // section views hug their content inside a shrink-height scrollable so a
    // short tab doesn't stretch. `fill` carries that distinction to the column.
    let (region, mut fill): (Element<'a, Action>, bool) = if show_picker {
        (navi::render_navi_edit(lang, loaded), true)
    } else {
        let (body, body_fill): (Element<'a, Action>, bool) =
            if editing_session && loaded.save_editor.tab_editable(active, loaded) {
                // The editors lay out side-by-side panes, each with its own
                // scrollbar, and want the full height — so they bypass the
                // read-only views' shared shrink-height body scrollable.
                (loaded.save_editor.render_edit(lang, active, loaded, state), true)
            } else if active == Tab::Cover {
                // Legacy fallback for a game-specific tab list that still
                // returns Cover; the shared list never includes it.
                (cover::render_cover::<Action>(lang, loaded), true)
            } else {
                let opts = RenderOpts {
                    folder_grouped: state.folder_grouped,
                };
                let body = loaded.save_editor.render(lang, active, loaded, opts);
                // Each render_* returns one-or-more pane-styled containers stacked
                // into an Element. We wrap that whole group in a shrink-height
                // scrollable so when its panes don't fill the available space the
                // column hugs them, and when they do the user can scroll past the
                // visible window. The per-instance id is what [`State::apply`] snaps
                // to the top on tab changes.
                let body_scrollable = scrollable(body)
                    .id(state.body_scroll_id.clone())
                    .style(crate::widgets::chunky_scrollable)
                    .width(Fill);
                (body_scrollable.into(), false)
            };
        // The whole region (tab strip + body) slides as one while the picker
        // animates; the tab-switch slide is suppressed then (the navi slide owns
        // the motion), and the tab strip rides along instead of popping.
        let body = if navi_sliding { body } else { entered(body) };
        let mut tab_col = column![tab_pane, body].spacing(style::PANE_GAP).width(Fill);
        if body_fill {
            tab_col = tab_col.height(Fill);
        }
        (tab_col.into(), body_fill)
    };

    // Slide the incoming side up into place. While sliding, keep the region
    // full-height so it doesn't change the column's height mid-motion.
    let region = if navi_sliding {
        fill = true;
        let progress = state.navi_select.progress(now);
        // Entrance progress of the side actually on screen (the target): the
        // picker on open, the tab content on dismiss.
        let entrance = if state.navi_select.shown() {
            progress
        } else {
            1.0 - progress
        };
        crate::anim::slide_in(region, entrance, iced::Vector::new(0.0, 20.0))
    } else {
        region
    };

    // Assemble: the persistent strip, then the body region.
    let mut col = column![navi_strip, region].spacing(style::PANE_GAP).width(Fill);
    if fill {
        col = col.height(Fill);
    }
    col.into()
}

/// The global edit mode's Save / Cancel pair, shown at the navi
/// header's right edge while edit mode is on (or sliding out). One
/// pair for the whole save: they commit / discard the edits on *all*
/// tabs at once. Any save-editor error disables Save until it is resolved.
fn edit_buttons(lang: &LanguageIdentifier, can_save: bool) -> Element<'_, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    row![
        widgets::labeled_icon_button(
            Icon::X,
            t!(lang, "save-edit-cancel"),
            Action::CancelEdit,
            [4.0, 10.0],
            widgets::neutral,
        ),
        widgets::labeled_icon_button_maybe(
            Icon::Check,
            t!(lang, "save-edit-save"),
            can_save.then_some(Action::SaveEdit),
            [4.0, 10.0],
            widgets::primary_button,
        ),
    ]
    .spacing(6)
    .align_y(iced::Alignment::Center)
    .into()
}

/// One control in the tab strip's tail. Identified per-kind (not
/// per-row) so the view can keep a control that exists on both
/// the previous and current sub-tab anchored in place instead of
/// re-animating it — the copy button lives on most tabs and only
/// its target changes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtraKind {
    /// Folder-only: the group-by-identity toggle.
    FolderGroup,
    /// Navi-only, and only for saves with an actual navicust grid
    /// (LinkNavi BN4.5 navis have nothing to render): copy the
    /// grid as an image.
    CopyImage,
    /// Copy the tab as text — present on every tab except Cover.
    Copy,
}

/// The tail controls `tab` shows, in display order. The Edit
/// affordance is tracked separately — see [`tab_has_edit`].
fn extra_kinds(tab: Tab) -> Vec<ExtraKind> {
    match tab {
        Tab::Folder => vec![ExtraKind::FolderGroup, ExtraKind::Copy],
        // The navi card copies as text only; the navicust grid also
        // copies as an image.
        Tab::Navicust => vec![ExtraKind::CopyImage, ExtraKind::Copy],
        Tab::PatchCards | Tab::AutoBattleData | Tab::ProgramDeck | Tab::Party => vec![ExtraKind::Copy],
        Tab::Cover => vec![],
    }
}

/// Build one tail control. `tab` parameterizes the copy actions'
/// target.
fn render_extra<'a>(lang: &'a LanguageIdentifier, state: &'a State, tab: Tab, kind: ExtraKind) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    match kind {
        ExtraKind::FolderGroup => iced::widget::checkbox(state.folder_grouped)
            .label(t!(lang, "folder-group"))
            .on_toggle(Action::ToggleFolderGrouped)
            .size(TEXT_BODY)
            .text_size(12)
            .style(crate::widgets::chunky_checkbox)
            .into(),
        ExtraKind::CopyImage => widgets::copy_icon_button(
            &copy_flash_key(tab, true),
            Icon::ImageDown,
            TEXT_BODY,
            t!(lang, "save-copy-image"),
            t!(lang, "copied"),
            Some(Action::CopyTabImage(tab)),
            [4.0, 10.0],
        ),
        ExtraKind::Copy => widgets::copy_icon_button(
            &copy_flash_key(tab, false),
            Icon::ClipboardCopy,
            TEXT_BODY,
            t!(lang, "save-copy"),
            t!(lang, "copied"),
            Some(Action::CopyTab(tab)),
            [4.0, 10.0],
        ),
    }
}

/// The tab list for this save: only the game's data sections (via its
/// [`crate::editor::GameSaveEditor`]). Streamer mode's Cover is a full-view
/// privacy gate rendered before this list, never a tab inside it.
/// The equipped navi (emblem / name / HP / buster) is not a tab — it
/// lives in the persistent strip above the body (see [`view`]), so it's
/// always on screen regardless of the active section.
pub fn available_tabs(loaded: &OpenSave, _streamer_mode: bool) -> Vec<Tab> {
    loaded.save_editor.tabs(loaded)
}

/// Stable copy-feedback key for a tab's copy buttons — shared between
/// the view (which renders the "Copied!" flash) and the host tabs'
/// update paths (which fire it once the copy actually lands on the
/// clipboard). See [`crate::copy_feedback`].
pub fn copy_flash_key(tab: Tab, image: bool) -> String {
    format!("save-view-copy-{}-{}", if image { "image" } else { "text" }, tab as u8)
}

/// A save-view tab as TSV text for clipboard "copy as text", or `None` for
/// tabs without a text form — the game's own [`crate::editor::GameSaveEditor`]
/// decides.
pub fn tab_as_text(lang: &LanguageIdentifier, tab: Tab, loaded: &OpenSave, opts: RenderOpts) -> Option<String> {
    loaded.save_editor.tab_as_text(lang, tab, loaded, opts)
}

/// Render a save-view tab to an RGBA image for clipboard "copy as image",
/// or `None` for tabs without an image form.
pub fn tab_as_image(tab: Tab, loaded: &OpenSave) -> Option<image::RgbaImage> {
    loaded.save_editor.tab_as_image(tab, loaded)
}
