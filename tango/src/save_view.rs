use crate::i18n::t;
use crate::selection::Loaded;
use crate::style::{self, TEXT_BODY, TEXT_CAPTION, TEXT_DISPLAY};
use crate::widgets::{muted_color, muted_text_style};
use iced::widget::{button, container, image as iced_image, scrollable, stack, text, tooltip, Image, Space};
use sweeten::widget::{column, pick_list, row, text_input};

/// Save view is read-only — every interactive bit (NCP hover, chip
/// hover) is handled by tooltip/canvas widgets that manage their own
/// state internally, so render fns never emit caller-visible messages.
/// The Element is generic over the embedder's Message type.
use iced::{Alignment, ContentFit, Element, Fill, Length};
use tango_dataview::rom::NavicustPartColor;
use tango_dataview::save::Save;
use unic_langid::LanguageIdentifier;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tab {
    Cover,
    Navi,
    Folder,
    PatchCards,
    AutoBattleData,
}

#[derive(Default, Clone, Copy)]
pub struct RenderOpts {
    pub folder_grouped: bool,
}

/// Sort order for the editor's chip-library (right) pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibrarySort {
    Id,
    Name,
    Code,
    Attack,
    Element,
    Mb,
}

impl LibrarySort {
    pub const ALL: [LibrarySort; 6] = [
        LibrarySort::Id,
        LibrarySort::Name,
        LibrarySort::Code,
        LibrarySort::Attack,
        LibrarySort::Element,
        LibrarySort::Mb,
    ];
}

impl LibrarySort {
    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            LibrarySort::Id => t!(lang, "folder-sort-id"),
            LibrarySort::Name => t!(lang, "folder-sort-name"),
            LibrarySort::Code => t!(lang, "folder-sort-code"),
            LibrarySort::Attack => t!(lang, "folder-sort-attack"),
            LibrarySort::Element => t!(lang, "folder-sort-element"),
            LibrarySort::Mb => t!(lang, "folder-sort-mb"),
        }
    }
}

/// A sort mode paired with its localized label, for the editors' sort
/// pick_lists — the picker renders options via `Display`, which can't
/// reach the language, so the label is resolved up front. Equality is
/// by mode so the picker can match the current selection.
#[derive(Clone)]
struct SortChoice<S> {
    sort: S,
    label: String,
}

impl<S: PartialEq> PartialEq for SortChoice<S> {
    fn eq(&self, other: &Self) -> bool {
        self.sort == other.sort
    }
}

impl<S> std::fmt::Display for SortChoice<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// The filter box + sort picker strip shared by all four editor library
/// panes (folder, navicust palette, patch cards, auto battle data).
/// `search_placeholder` is the filter box's pre-resolved placeholder
/// text (`t!` only takes literal keys, so the lookup stays at the call
/// site); `sort_label` is the sort enum's `label` method.
fn library_header<'a, S: Copy + PartialEq + 'static>(
    lang: &LanguageIdentifier,
    search_placeholder: String,
    filter_value: &str,
    on_filter: fn(String) -> Action,
    sorts: &[S],
    current: S,
    sort_label: fn(S, &LanguageIdentifier) -> String,
    on_sort: fn(S) -> Action,
) -> Element<'a, Action> {
    let filter_input = text_input(&search_placeholder, filter_value)
        .on_input(on_filter)
        .padding(style::CONTROL_PADDING)
        .size(TEXT_BODY)
        .width(Fill)
        .style(crate::widgets::chunky_text_input);
    let sort_options: Vec<SortChoice<S>> = sorts
        .iter()
        .map(|&sort| SortChoice {
            sort,
            label: sort_label(sort, lang),
        })
        .collect();
    let sort_selected = sort_options.iter().find(|c| c.sort == current).cloned();
    let sort_pick = pick_list(sort_options, sort_selected, move |c: SortChoice<S>| on_sort(c.sort))
        .padding(style::CONTROL_PADDING)
        .text_size(TEXT_BODY)
        .style(crate::widgets::chunky_pick_list);
    container(
        row![
            filter_input,
            text(t!(lang, "save-edit-sort"))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            sort_pick,
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING)
    .into()
}

/// One editor pane: a header strip pinned above a scrollable body, on
/// the standard pane plate. Every pane in the four editors is this
/// shape.
fn editor_pane<'a>(
    header: impl Into<Element<'a, Action>>,
    body: impl Into<Element<'a, Action>>,
) -> Element<'a, Action> {
    container(column![
        header.into(),
        scrollable(body.into())
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(crate::widgets::pane)
    .into()
}

/// The editors' two-pane layout: working set on the left, library /
/// palette on the right.
fn editor_panes<'a>(left: Element<'a, Action>, right: Element<'a, Action>) -> Element<'a, Action> {
    row![left, right]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .height(Fill)
        .into()
}

/// Caption text that turns danger-red when an editor budget is blown
/// (folder class limits, patch-card MB, folder over-fill) and reads
/// muted otherwise.
fn limit_caption<'a>(label: String, over: bool) -> iced::widget::Text<'a> {
    text(label)
        .size(TEXT_CAPTION)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(if over {
                theme.palette().danger
            } else {
                muted_color(theme)
            }),
        })
}

/// The chips the player owns (their pack), as `(id, name, code)`, in
/// `sort` order — one row per owned chip+code. Only chips+codes with a
/// pack count > 0 are returned, so a folder can only be built from what
/// the save legitimately holds. Ties fall back to id for a stable order.
fn sorted_library_entries(loaded: &Loaded, sort: LibrarySort) -> Vec<(usize, String, tango_dataview::save::ChipCode)> {
    use tango_dataview::save::ChipCode;
    let assets = loaded.assets.as_ref();
    let chips_view = loaded.save.view_chips();
    struct E {
        id: usize,
        name: String,
        code: ChipCode,
        code_rank: u8,
        atk: u32,
        elem: usize,
        mb: u8,
    }
    let mut rows: Vec<E> = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        let Some(name) = info.name() else { continue };
        let (atk, elem, mb) = (info.attack_power(), info.element(), info.mb());
        // One row per valid code (e.g. Cannon A / Cannon B / Cannon *),
        // but only for codes the player owns (pack count > 0). `variant`
        // is the code's index within the chip's code list — the index the
        // pack table is keyed by. Ids past the pack table (Program
        // Advances, etc.) return `None` and are dropped. The editor only
        // renders for games with a pack, so a missing count means "not
        // owned", not "unsupported".
        for (variant, ch) in info.codes().into_iter().enumerate() {
            let Some(code) = ChipCode::from_char(ch) else { continue };
            let owned = chips_view
                .as_ref()
                .and_then(|v| v.pack_count(id, variant))
                .map_or(false, |c| c > 0);
            if !owned {
                continue;
            }
            rows.push(E {
                id,
                name: name.clone(),
                code,
                code_rank: code as u8,
                atk,
                elem,
                mb,
            });
        }
    }
    // All ties fall back to (id, code) so the order stays stable.
    match sort {
        LibrarySort::Id => {}
        LibrarySort::Name => rows.sort_by(|a, b| {
            a.name
                .cmp(&b.name)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Code => rows.sort_by(|a, b| a.code_rank.cmp(&b.code_rank).then(a.id.cmp(&b.id))),
        LibrarySort::Attack => rows.sort_by(|a, b| {
            a.atk
                .cmp(&b.atk)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Element => rows.sort_by(|a, b| {
            a.elem
                .cmp(&b.elem)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
        LibrarySort::Mb => rows.sort_by(|a, b| {
            a.mb.cmp(&b.mb)
                .then(a.id.cmp(&b.id))
                .then(a.code_rank.cmp(&b.code_rank))
        }),
    }
    rows.into_iter().map(|e| (e.id, e.name, e.code)).collect()
}

/// A part picked up from the palette: its id plus the orientation +
/// compression it'll be dropped with. Lives in the save-view state
/// because the palette (which sets it) and the editor canvas (which
/// draws its ghost) are separate widgets.
#[derive(Debug, Clone, Copy)]
pub struct HeldPart {
    pub id: usize,
    pub rot: u8,
    pub compressed: bool,
    /// Where on the part it was grabbed: the offset (in the *current*
    /// orientation) of the grabbed cell from the part's center anchor,
    /// as `(row, col)`. Keeps that cell under the cursor as it's dragged
    /// instead of snapping the center there. `(0, 0)` for palette
    /// pick-ups (no meaningful grab point).
    pub grab_row: i8,
    pub grab_col: i8,
}

impl HeldPart {
    /// Rotate the grab point 90° clockwise to track [`Self::rot`] being
    /// advanced — keeps the grabbed cell under the cursor through a
    /// rotate. Mirrors the clockwise cell map in
    /// [`crate::navicust_editor::rotated_offsets`]: `(dy, dx) -> (dx, -dy)`.
    fn rotate_grab_cw(&mut self) {
        let (r, c) = (self.grab_row, self.grab_col);
        self.grab_row = c;
        self.grab_col = -r;
    }
}

/// Sort order for the navicust editor's palette pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavicustSort {
    Id,
    Name,
    Color,
}

impl NavicustSort {
    pub const ALL: [NavicustSort; 3] = [NavicustSort::Id, NavicustSort::Name, NavicustSort::Color];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            NavicustSort::Id => t!(lang, "navicust-sort-id"),
            NavicustSort::Name => t!(lang, "navicust-sort-name"),
            NavicustSort::Color => t!(lang, "navicust-sort-color"),
        }
    }
}

/// Total MB an enabled patch-card set may use in BN5/BN6. Enabling a card
/// past this is blocked, and a freshly added card lands disabled if it
/// wouldn't fit — so a committed save never exceeds the in-game limit.
pub const MAX_PATCH_CARD56_MB: u32 = 80;
pub const MAX_FOLDER_CHIPS: usize = 30;

/// Sort order for the BN5/BN6 patch-card editor's library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatchCard56Sort {
    Id,
    Name,
    Mb,
}

impl PatchCard56Sort {
    pub const ALL: [PatchCard56Sort; 3] = [PatchCard56Sort::Id, PatchCard56Sort::Name, PatchCard56Sort::Mb];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            PatchCard56Sort::Id => t!(lang, "patch-card-sort-id"),
            PatchCard56Sort::Name => t!(lang, "patch-card-sort-name"),
            PatchCard56Sort::Mb => t!(lang, "patch-card-sort-mb"),
        }
    }
}

/// A choice in a BN4 slot's card dropdown: the card id (`None` clears the
/// slot) plus a pre-resolved label. The label folds the card's effect into
/// the name (`"Max HP Up — Max HP+100"`), since within one slot several
/// cards share a name and only the effect tells them apart. `Display`
/// renders the label; equality is by id so the picker can match the
/// currently-installed card.
#[derive(Clone)]
struct PatchCard4Choice {
    id: Option<usize>,
    label: String,
}

impl PatchCard4Choice {
    fn none(lang: &LanguageIdentifier) -> Self {
        Self {
            id: None,
            label: t!(lang, "patch-card4-none"),
        }
    }

    fn card(loaded: &Loaded, id: usize) -> Self {
        let info = loaded.assets.patch_card4(id);
        let name = info.as_ref().and_then(|c| c.name()).unwrap_or_else(|| format!("#{id}"));
        // 3-digit catalog number prefix (also disambiguates same-named
        // cards in the dropdown); then the effect to tell them apart.
        let label = format!(
            "{id:03} {name} — {}",
            patch_card4_effect_label(
                info.as_ref()
                    .map_or(tango_dataview::rom::PatchCard4Effect::None, |c| c.effect(),)
            )
        );
        Self { id: Some(id), label }
    }
}

impl PartialEq for PatchCard4Choice {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl std::fmt::Display for PatchCard4Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

/// Human-readable label for a BN4 patch-card effect, derived from the
/// machine-readable [`tango_dataview::rom::PatchCard4Effect`] decoded out of
/// the ROM. (B-shortcut chip params are shown raw for now — the shortcut →
/// chip-id table isn't mapped yet.)
fn patch_card4_effect_label(effect: tango_dataview::rom::PatchCard4Effect) -> String {
    use tango_dataview::rom::{
        PatchCard4Aura as A, PatchCard4Color as C, PatchCard4Effect as E, PatchCard4Panel as P,
        PatchCard4PetColor as PT, PatchCard4Soul as S,
    };
    match effect {
        E::None => "—".to_string(),
        E::PetMenu(c) => format!(
            "{} PET menu",
            match c {
                PT::Blue => "Blue",
                PT::Pink => "Pink",
                PT::Green => "Green",
                PT::Black => "Black",
            }
        ),
        E::MaxHP(n) => format!("Max HP +{n}"),
        E::BusterAttack(n) => format!("Buster Attack {}", n as u16 + 1),
        E::BButton(s) => format!("B Button {s:?}"),
        E::BCharge(s) => format!("B Charge {s:?}"),
        E::BLeft(s) => format!("B + ← {s:?}"),
        E::CustomSlots(n) => format!("Custom +{n}"),
        E::MegaFolder(n) => format!("Mega Chip +{n}"),
        E::GigaFolder(n) => format!("Giga Chip +{n}"),
        E::TripleSupporter => "Triple Supporter".to_string(),
        E::PanelStep(p) => format!(
            "{} Panel Step",
            match p {
                P::Broken => "Broken",
                P::Cracked => "Cracked",
                P::Metal => "Metal",
                P::Holy => "Holy",
            }
        ),
        E::FullSynchro => "Full Synchro".to_string(),
        E::Aura(a) => match a {
            A::Barrier100 => "Barrier 100",
            A::Barrier200 => "Barrier 200",
            A::LifeAura => "LifeAura",
        }
        .to_string(),
        E::Soul(s) => format!(
            "{} Soul",
            match s {
                S::Roll => "Roll",
                S::Guts => "Guts",
                S::Wind => "Wind",
                S::Search => "Search",
                S::Fire => "Fire",
                S::Thunder => "Thunder",
                S::Proto => "Proto",
                S::Number => "Number",
                S::Metal => "Metal",
                S::Junk => "Junk",
                S::Aqua => "Aqua",
                S::Wood => "Wood",
            }
        ),
        E::Color(c) => format!(
            "{} MegaMan",
            match c {
                C::Red => "Red",
                C::Yellow => "Yellow",
                C::White => "White",
                C::Green => "Green",
            }
        ),
        E::AllGuard => "All Guard".to_string(),
    }
}

/// Joined human-readable label for a card's bugs, or `None` if it has none.
fn patch_card4_bugs_label(bugs: &[tango_dataview::rom::PatchCard4Bug]) -> Option<String> {
    use tango_dataview::rom::PatchCard4Bug as B;
    if bugs.is_empty() {
        return None;
    }
    Some(
        bugs.iter()
            .map(|b| match b {
                B::Confused => "Start battle Confused",
                B::AutoMove => "Auto-move forward",
                B::HP(_) => "HP Bug",
                B::CustomHP => "Custom HP Bug",
                B::CustomMinus1 => "Custom −1",
                B::PoisonPanelStep => "Poison Panel Step",
            })
            .collect::<Vec<_>>()
            .join(" & "),
    )
}

/// Sort order for the auto-battle-data editor's chip library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBattleDataSort {
    Id,
    Name,
    Used,
}

impl AutoBattleDataSort {
    pub const ALL: [AutoBattleDataSort; 3] = [
        AutoBattleDataSort::Id,
        AutoBattleDataSort::Name,
        AutoBattleDataSort::Used,
    ];

    fn label(self, lang: &LanguageIdentifier) -> String {
        match self {
            AutoBattleDataSort::Id => t!(lang, "folder-sort-id"),
            AutoBattleDataSort::Name => t!(lang, "folder-sort-name"),
            AutoBattleDataSort::Used => t!(lang, "auto-battle-data-edit-used"),
        }
    }
}

/// Width of each use-count column (caption + numeric field) in the Auto
/// Battle Data editor's library, so the Used / Sec. fields line up as
/// columns across rows (and a non-standard chip's missing Sec. field can
/// reserve the same gap).
const ABD_COUNT_COL_W: f32 = 104.0;
/// Use counts are stored as `u16` in the save, so the numeric fields
/// clamp entries to this ceiling.
const MAX_ABD_USE_COUNT: usize = u16::MAX as usize;

/// Stable color ordering for the palette's Color sort.
fn ncp_color_rank(color: &Option<NavicustPartColor>) -> u8 {
    use NavicustPartColor as N;
    match color {
        Some(N::White) => 0,
        Some(N::Yellow) => 1,
        Some(N::Pink) => 2,
        Some(N::Red) => 3,
        Some(N::Blue) => 4,
        Some(N::Green) => 5,
        Some(N::Orange) => 6,
        Some(N::Purple) => 7,
        Some(N::Gray) => 8,
        None => 9,
    }
}

/// Every navicust part the ROM defines, as `(id, name, description)`,
/// filtered by `filter` (case-insensitive name match) and in `sort`
/// order. Color/solidity are used for the Color sort but the palette
/// reads the rest (shape, color) from the baked thumbnails. Ties fall
/// back to id for a stable order.
fn sorted_navicust_parts(loaded: &Loaded, sort: NavicustSort, filter: &str) -> Vec<(usize, String, Option<String>)> {
    let assets = loaded.assets.as_ref();
    let filter = filter.to_lowercase();
    struct E {
        id: usize,
        name: String,
        desc: Option<String>,
        color_rank: u8,
    }
    let mut rows: Vec<E> = Vec::new();
    // Cap how many variants of a given part type (by name) appear, so the
    // list stays tidy when a ROM carries many near-duplicate color/junk
    // variants of one part.
    let mut per_type: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for id in 0..assets.num_navicust_parts() {
        let Some(info) = assets.navicust_part(id) else { continue };
        // Skip unused/padding slots: a real part has a color and a
        // non-empty shape. Placeholder entries have an all-zero bitmap.
        let Some(color) = info.color() else { continue };
        if !info.uncompressed_bitmap().iter().any(|&set| set) {
            continue;
        }
        let Some(name) = info.name() else { continue };
        if name.trim().is_empty() {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        let count = per_type.entry(name.clone()).or_insert(0);
        if *count >= 9 {
            continue;
        }
        *count += 1;
        rows.push(E {
            id,
            name,
            desc: info.description(),
            color_rank: ncp_color_rank(&Some(color)),
        });
    }
    match sort {
        NavicustSort::Id => {}
        NavicustSort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        NavicustSort::Color => rows.sort_by(|a, b| a.color_rank.cmp(&b.color_rank).then(a.id.cmp(&b.id))),
    }
    rows.into_iter().map(|e| (e.id, e.name, e.desc)).collect()
}

pub fn available_tabs(save: &dyn Save, streamer_mode: bool) -> Vec<Tab> {
    let mut tabs = vec![];
    if streamer_mode {
        tabs.push(Tab::Cover);
    }
    if save.view_navi().is_some() {
        tabs.push(Tab::Navi);
    }
    if save.view_chips().is_some() {
        tabs.push(Tab::Folder);
    }
    if save.view_patch_cards().is_some() {
        tabs.push(Tab::PatchCards);
    }
    if save.view_auto_battle_data().is_some() {
        tabs.push(Tab::AutoBattleData);
    }
    tabs
}

pub fn render<M: 'static>(
    lang: &LanguageIdentifier,
    tab: Tab,
    loaded: &Loaded,
    opts: RenderOpts,
) -> Element<'static, M> {
    match tab {
        Tab::Cover => render_cover(lang, loaded),
        Tab::Navi => render_navi(lang, loaded),
        Tab::Folder => render_folder(lang, loaded, opts.folder_grouped),
        Tab::PatchCards => render_patch_cards(lang, loaded),
        Tab::AutoBattleData => render_auto_battle_data(lang, loaded),
    }
}

/// Per-tab Lucide icon glyph used by the tab strip in [`view`].
fn tab_icon(tab: Tab) -> lucide_icons::Icon {
    use lucide_icons::Icon;
    match tab {
        Tab::Cover => Icon::Eye,
        Tab::Navi => Icon::Bot,
        Tab::Folder => Icon::Files,
        Tab::PatchCards => Icon::CreditCard,
        Tab::AutoBattleData => Icon::Swords,
    }
}

/// Persistent UI state for [`view`]. The active tab + folder
/// grouping live here so callers don't have to mirror the fields
/// themselves; apply incoming [`Action`]s via [`State::apply`].
/// The `body_scroll_id` is per-instance unique so multiple
/// save_views on screen at once (e.g. play tab + in-session
/// opponent panel) have distinct scrollable identities.
#[derive(Clone)]
pub struct State {
    pub active_tab: Option<Tab>,
    pub folder_grouped: bool,
    body_scroll_id: iced::widget::Id,
    /// The in-progress save edit, or `None` when not editing. It's one
    /// global toggle for the whole save: while `Some`, every editable tab
    /// shows its editor, and one Save / Cancel commits / discards them all.
    /// Bundling every editor's scratch state here means leaving edit mode
    /// (or swapping saves) is a single `editing = None`.
    pub editing: Option<EditState>,
    /// Sort order for the chip library pane. A persistent UI preference
    /// (kept across edit sessions), so it lives outside [`EditState`].
    pub library_sort: LibrarySort,
    /// Sort order for the navicust palette pane (persistent preference).
    pub navicust_sort: NavicustSort,
    /// Sort order for the BN5/BN6 patch-card library pane (persistent
    /// preference).
    pub patch_card56_sort: PatchCard56Sort,
    /// Sort order for the auto-battle-data chip library pane (persistent
    /// preference).
    pub auto_battle_data_sort: AutoBattleDataSort,
    /// Entrance restarted on each sub-tab switch — the tab body
    /// (and the per-tab extras in the strip's tail) slides in,
    /// direction following the strip's order like the app's
    /// top-level tabs.
    pub enter: crate::anim::Enter,
    /// Starting offset for `enter`. Horizontal (sign following
    /// the direction of travel along the strip) for sub-tab
    /// switches; vertical for whole-body swaps (edit mode toggles,
    /// a different game/save selected).
    pub enter_from: iced::Vector,
    /// The sub-tab that was active before the last [`Action::SelectTab`].
    /// Lets the view tell whether a control in the strip's tail (the
    /// Edit affordance) was already on screen on the previous tab and
    /// skip re-animating it.
    pub prev_tab: Option<Tab>,
    /// Show/hide transition for the edit-mode Save / Cancel pair
    /// in the strip's tail. They slide in horizontally when edit
    /// mode opens and back out when it closes — and because this
    /// is keyed on the mode (not the sub-tab), they stay planted
    /// while the user flips between editor tabs.
    pub edit_anim: crate::anim::Transition,
}

/// New index of an element originally at `i` after an ordered move that takes
/// the element at `from` and reinserts it at `to` (i.e. `vec.remove(from);
/// vec.insert(to, x)`). Elements between the two endpoints shift by one toward
/// the vacated side; everything outside the range is unchanged. Used to keep
/// slot-indexed references (REG/TAG, staged tags) aligned with a drag reorder.
pub fn reorder_index(i: usize, from: usize, to: usize) -> usize {
    if i == from {
        to
    } else if from < to && i > from && i <= to {
        i - 1
    } else if from > to && i >= to && i < from {
        i + 1
    } else {
        i
    }
}

/// Everything an in-progress save edit needs that's thrown away when the
/// edit ends. Held as [`State::editing`]'s `Option` payload so one
/// assignment clears it all.
#[derive(Clone, Default)]
pub struct EditState {
    /// Folder editor: in-progress tag-chip selection (≤2 raw slot
    /// indexes). Seeded from the equipped folder's tag pair on entering
    /// edit mode; a committed pair is written to the save only when
    /// exactly two are selected (see [`State::toggle_tag`]).
    pub tags: Vec<usize>,
    /// Folder editor: chip library filter text.
    pub library_filter: String,
    /// Navicust editor: the part currently picked up from the palette
    /// (id + orientation + compression), drawn as a ghost under the cursor.
    pub held_part: Option<HeldPart>,
    /// Navicust editor: per-part picker orientation (`id -> (rot,
    /// compressed)`). Each palette row's rotate / (de)compress buttons edit
    /// this; picking a part up keeps it in sync, so a part is always picked
    /// up in the orientation shown. Missing id = default (rot 0, compressed).
    pub part_orient: std::collections::HashMap<usize, (u8, bool)>,
    /// Navicust editor: palette filter text.
    pub navicust_filter: String,
    /// BN5/BN6 patch-card editor: library filter text.
    pub patch_card56_filter: String,
    /// Auto-battle-data editor: chip library filter text.
    pub auto_battle_data_filter: String,
}

impl EditState {
    /// The orientation a palette part is shown / picked up in: an explicit
    /// per-part override, else the default (rotation 0, compressed — the
    /// smaller shape parts are usually placed in).
    pub fn orient_of(&self, id: usize) -> (u8, bool) {
        self.part_orient.get(&id).copied().unwrap_or((0, true))
    }
}

impl Default for State {
    fn default() -> Self {
        Self::new()
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            active_tab: None,
            folder_grouped: true,
            body_scroll_id: iced::widget::Id::unique(),
            editing: None,
            library_sort: LibrarySort::Id,
            navicust_sort: NavicustSort::Id,
            patch_card56_sort: PatchCard56Sort::Id,
            auto_battle_data_sort: AutoBattleDataSort::Id,
            enter: crate::anim::Enter::default(),
            enter_from: iced::Vector::new(24.0, 0.0),
            prev_tab: None,
            edit_anim: crate::anim::Transition::swap(false),
        }
    }

    /// Enter the global save edit mode. It's a single toggle for the whole
    /// save: every editable tab (Folder, Navi, Patch Cards) shows its
    /// editor while set, and one Save / Cancel commits / discards them all.
    /// Seeds the tag toggles from the equipped folder's current tag pair so
    /// they start in the right state. Needs `loaded` (the read view), so
    /// the play tab calls this rather than routing through [`Self::apply`].
    pub fn enter_edit(&mut self, loaded: &Loaded) {
        // A fresh EditState — every editor opens with clean scratch state.
        let mut edit = EditState::default();
        // Seed the tag toggles from the equipped folder's tag pair, if
        // the game has tag chips and a pair is set.
        edit.tags = loaded
            .save
            .view_chips()
            .and_then(|v| {
                let folder = v.equipped_folder_index();
                v.tag_chip_indexes(folder)
            })
            .flatten()
            .map(|[a, b]| vec![a, b])
            .unwrap_or_default();
        self.editing = Some(edit);
        // Mode change, not navigation — the editor body rises in
        // while the Save / Cancel pair slides into the tail.
        let now = iced::time::Instant::now();
        self.enter_from = iced::Vector::new(0.0, 20.0);
        self.enter.start(now);
        self.edit_anim.set(true, now);
    }

    /// Drop any in-progress edit without animation bookkeeping
    /// beyond the exit transition — used by hosts that reset the
    /// edit state out-of-band (e.g. the App when the loaded save
    /// is swapped out from under the view).
    pub fn clear_editing(&mut self) {
        self.editing = None;
        self.edit_anim.set(false, iced::time::Instant::now());
    }

    /// Toggle `slot` in the in-progress tag selection (capped at two).
    /// Returns the pair to commit to the save: `Some([a, b])` once two
    /// slots are selected, else `None` (which clears the tag pairing —
    /// a lone tag chip isn't a valid state in-game).
    pub fn toggle_tag(&mut self, slot: usize) -> Option<[usize; 2]> {
        let Some(edit) = self.editing.as_mut() else { return None };
        if let Some(pos) = edit.tags.iter().position(|&s| s == slot) {
            edit.tags.remove(pos);
        } else if edit.tags.len() < 2 {
            edit.tags.push(slot);
        }
        match edit.tags.as_slice() {
            [a, b] => Some([*a, *b]),
            _ => None,
        }
    }

    /// Remap the in-progress tag selection when `removed_slot`'s chip is
    /// removed and the chips below it shift up one: drop that slot and
    /// shift any higher selected slots down, mirroring the save-side
    /// compaction.
    pub fn compact_tags(&mut self, removed_slot: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        edit.tags.retain(|&s| s != removed_slot);
        for s in edit.tags.iter_mut() {
            if *s > removed_slot {
                *s -= 1;
            }
        }
    }

    /// Remap the in-progress tag selection through a chip reorder (ordered
    /// move from `from` to `to`), so the staged TAG toggles keep pointing at
    /// the same chips after a drag — the mirror of [`compact_tags`] for moves.
    pub fn move_tags(&mut self, from: usize, to: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        for s in edit.tags.iter_mut() {
            *s = reorder_index(*s, from, to);
        }
    }

    /// Shift the staged tag selection when a chip is added at the top: the run
    /// of chips above the first empty slot (`gap`) slides down one, so any
    /// staged tag in that run moves down with it.
    pub fn shift_tags_for_top_insert(&mut self, gap: usize) {
        let Some(edit) = self.editing.as_mut() else { return };
        for s in edit.tags.iter_mut() {
            if *s < gap {
                *s += 1;
            }
        }
    }

    /// Apply an `Action` to the state. `CopyTab` is left for the
    /// caller to handle (clipboard side-effects can't happen inside
    /// `apply`); everything else is folded in. Returns a Task the
    /// caller should run — used for save-view-internal side
    /// effects (notably the scroll-to-top snap on a tab change)
    /// so hosts don't have to know about them.
    pub fn apply(&mut self, action: &Action) -> iced::Task<Action> {
        match action {
            Action::SelectTab(t) => {
                if self.active_tab != Some(*t) {
                    // The strip lays tabs out in declaration order,
                    // so the discriminants double as positions:
                    // moving right enters from the right, moving
                    // left from the left.
                    if let Some(prev) = self.active_tab {
                        let dx = if (*t as u8) > (prev as u8) { 24.0 } else { -24.0 };
                        self.enter_from = iced::Vector::new(dx, 0.0);
                    }
                    self.prev_tab = self.active_tab;
                    self.active_tab = Some(*t);
                    self.enter.start(iced::time::Instant::now());
                }
                iced::widget::operation::snap_to(
                    self.body_scroll_id.clone(),
                    iced::widget::scrollable::RelativeOffset::START,
                )
            }
            Action::ToggleFolderGrouped(g) => {
                self.folder_grouped = *g;
                iced::Task::none()
            }
            // Save and Cancel both leave the global edit mode; the host
            // runs the commit/discard side effect (covering every tab).
            // Dropping the whole EditState clears every editor's scratch.
            Action::SaveEdit | Action::CancelEdit => {
                self.editing = None;
                // Returning read-only body rises in (mirroring
                // `enter_edit`) while Save / Cancel slide back out.
                let now = iced::time::Instant::now();
                self.enter_from = iced::Vector::new(0.0, 20.0);
                self.enter.start(now);
                self.edit_anim.set(false, now);
                iced::Task::none()
            }
            Action::LibraryFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.library_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::LibrarySortChanged(s) => {
                self.library_sort = *s;
                iced::Task::none()
            }
            // ----- Navicust editor: state-local folds -----
            Action::PickUpPalettePart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    // Toggle: clicking the held part deselects it; otherwise
                    // pick it up in the orientation set in the picker.
                    if e.held_part.map_or(false, |h| h.id == *id) {
                        e.held_part = None;
                    } else {
                        let (rot, compressed) = e.orient_of(*id);
                        e.held_part = Some(HeldPart {
                            id: *id,
                            rot,
                            compressed,
                            grab_row: 0,
                            grab_col: 0,
                        });
                    }
                }
                iced::Task::none()
            }
            Action::RotateHeld => {
                // Scroll-wheel rotate over the grid: rotates the held part
                // and the picker entry together (so they stay in sync).
                if let Some(e) = self.editing.as_mut() {
                    if let Some(mut h) = e.held_part {
                        h.rot = (h.rot + 1) % 4;
                        h.rotate_grab_cw();
                        e.held_part = Some(h);
                        e.part_orient.insert(h.id, (h.rot, h.compressed));
                    }
                }
                iced::Task::none()
            }
            Action::RotatePart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    let (rot, compressed) = e.orient_of(*id);
                    let rot = (rot + 1) % 4;
                    e.part_orient.insert(*id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == *id {
                            h.rot = rot;
                            h.rotate_grab_cw();
                        }
                    }
                }
                iced::Task::none()
            }
            Action::ToggleCompressPart { id } => {
                if let Some(e) = self.editing.as_mut() {
                    let (rot, compressed) = e.orient_of(*id);
                    let compressed = !compressed;
                    e.part_orient.insert(*id, (rot, compressed));
                    if let Some(h) = e.held_part.as_mut() {
                        if h.id == *id {
                            h.compressed = compressed;
                            // The shape changes entirely, so the old grab
                            // point no longer maps to a cell — re-center.
                            h.grab_row = 0;
                            h.grab_col = 0;
                        }
                    }
                }
                iced::Task::none()
            }
            Action::ClearHeld => {
                if let Some(e) = self.editing.as_mut() {
                    e.held_part = None;
                }
                iced::Task::none()
            }
            Action::NavicustFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.navicust_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::NavicustSortChanged(s) => {
                self.navicust_sort = *s;
                iced::Task::none()
            }
            // ----- BN5/BN6 patch-card editor: state-local folds -----
            Action::PatchCard56FilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.patch_card56_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::PatchCard56SortChanged(s) => {
                self.patch_card56_sort = *s;
                iced::Task::none()
            }
            // ----- Auto-battle-data editor: state-local folds -----
            Action::AutoBattleDataFilterChanged(s) => {
                if let Some(e) = self.editing.as_mut() {
                    e.auto_battle_data_filter = s.clone();
                }
                iced::Task::none()
            }
            Action::AutoBattleDataSortChanged(s) => {
                self.auto_battle_data_sort = *s;
                iced::Task::none()
            }
            // EnterEdit needs `&Loaded` (to seed tag state), and the
            // mutation actions become host Effects — all are driven by
            // the embedder (play tab), so they're no-ops here.
            Action::EnterEdit
            | Action::AddChip { .. }
            | Action::RemoveChip { .. }
            | Action::ReorderChips(_)
            | Action::ClearFolder
            | Action::ToggleRegular { .. }
            | Action::ToggleTag { .. }
            | Action::PlaceHeld { .. }
            | Action::PickUpInstalledPart { .. }
            | Action::ClearNavicust
            | Action::AddPatchCard56 { .. }
            | Action::RemovePatchCard56 { .. }
            | Action::ReorderPatchCard56s(_)
            | Action::TogglePatchCard56 { .. }
            | Action::ClearPatchCard56s
            | Action::AddPatchCard4 { .. }
            | Action::RemovePatchCard4 { .. }
            | Action::TogglePatchCard4 { .. }
            | Action::ClearPatchCard4s
            | Action::SetChipUseCount { .. }
            | Action::SetSecondaryChipUseCount { .. }
            | Action::ClearAutoBattleData
            | Action::CopyTab(_)
            | Action::CopyTabImage(_)
            | Action::PlayClicked => iced::Task::none(),
        }
    }
}

/// User-driven changes the embedded save view wants to surface. The
/// caller `.map`s its top-level Message onto this and dispatches:
/// most variants just need `state.apply(&action)`; the Copy
/// variants need the caller's `tab_as_text` / `tab_as_image` plus
/// a clipboard write.
#[derive(Debug, Clone)]
pub enum Action {
    SelectTab(Tab),
    ToggleFolderGrouped(bool),
    CopyTab(Tab),
    CopyTabImage(Tab),
    /// Embedder-defined "start single-player here" action.
    /// Emitted by the Play button rendered in the save_view tab
    /// strip when [`view`] is called with `play_button = Some(_)`.
    /// The play tab routes this to `Effect::StartSinglePlayer`;
    /// other embedders (replay, opponent panel) pass `None` and
    /// the button isn't rendered.
    PlayClicked,
    // ----- Folder editor (only emitted when `view`'s `editable` is set) -----
    /// Enter folder edit mode. The play tab seeds tag state via
    /// [`State::enter_edit`]; the rest is handled in [`State::apply`].
    /// Edits are staged live in the loaded save but not written to disk
    /// until [`Action::SaveEdit`].
    EnterEdit,
    /// Finish editing: commit the staged folder to the save file on
    /// disk, then leave edit mode.
    SaveEdit,
    /// Discard all staged edits (reverts the loaded save to the
    /// on-disk original) and leave edit mode.
    CancelEdit,
    /// Library pane: add this chip+code to the first empty folder slot.
    AddChip {
        chip_id: usize,
        code: tango_dataview::save::ChipCode,
    },
    /// Folder pane: empty `slot`.
    RemoveChip {
        slot: usize,
    },
    /// Folder pane: a drag-reorder gesture from the draggable folder list
    /// (carries sweeten's raw [`DragEvent`]; only a completed drop between two
    /// filled slots actually moves a chip — see the play tab's handler).
    ReorderChips(sweeten::widget::drag::DragEvent),
    /// Folder pane: empty every slot (and clear REG/TAG).
    ClearFolder,
    /// Toggle `slot` as the folder's Regular chip — set it, or clear it
    /// if it's already the regular chip.
    ToggleRegular {
        slot: usize,
    },
    /// Toggle `slot`'s membership in the Tag chip pair.
    ToggleTag {
        slot: usize,
    },
    /// Library pane: the filter text changed.
    LibraryFilterChanged(String),
    /// Library pane: the sort order changed.
    LibrarySortChanged(LibrarySort),
    // ----- Navicust editor (only emitted when `editable` is set) -----
    /// Palette: pick up part `id` in the orientation shown in the picker.
    PickUpPalettePart {
        id: usize,
    },
    /// Rotate the held part 90° clockwise (grid scroll-wheel).
    RotateHeld,
    /// Palette: rotate this part's picker entry 90° clockwise.
    RotatePart {
        id: usize,
    },
    /// Palette: toggle this part's picker entry between its compressed
    /// and uncompressed shape.
    ToggleCompressPart {
        id: usize,
    },
    /// Drop the held part without placing it.
    ClearHeld,
    /// Place the held part with its center on grid cell `(col, row)`.
    PlaceHeld {
        col: u8,
        row: u8,
    },
    /// Pick an installed part back up — it's removed and becomes held.
    /// `(col, row)` is the cell that was clicked, so the part can be
    /// grabbed at that point rather than re-centered on the cursor.
    PickUpInstalledPart {
        slot: usize,
        col: u8,
        row: u8,
    },
    /// Remove every installed part.
    ClearNavicust,
    /// Palette: the filter text changed.
    NavicustFilterChanged(String),
    /// Palette: the sort order changed.
    NavicustSortChanged(NavicustSort),
    // ----- BN5/BN6 patch-card editor (only emitted when `editable` is set) -----
    /// Library pane: register patch card `id` (appended to the list,
    /// enabled).
    AddPatchCard56 {
        id: usize,
    },
    /// List pane: unregister the patch card in `slot`.
    RemovePatchCard56 {
        slot: usize,
    },
    /// List pane: a drag-reorder gesture (carries sweeten's raw [`DragEvent`];
    /// only a completed drop reorders — see the play tab's handler).
    ReorderPatchCard56s(sweeten::widget::drag::DragEvent),
    /// List pane: toggle the patch card in `slot` between enabled and
    /// disabled.
    TogglePatchCard56 {
        slot: usize,
    },
    /// List pane: unregister every patch card.
    ClearPatchCard56s,
    /// Library pane: the filter text changed.
    PatchCard56FilterChanged(String),
    /// Library pane: the sort order changed.
    PatchCard56SortChanged(PatchCard56Sort),
    // ----- BN4 patch-card editor (only emitted when `editable` is set) -----
    /// A slot's dropdown picked card `id` — install it into its own catalog
    /// slot, enabled (replacing whatever card occupied that slot).
    AddPatchCard4 {
        id: usize,
    },
    /// A slot's dropdown picked "None" — clear catalog slot `slot`.
    RemovePatchCard4 {
        slot: usize,
    },
    /// Toggle slot `slot`'s card between enabled and disabled.
    TogglePatchCard4 {
        slot: usize,
    },
    /// Clear every slot.
    ClearPatchCard4s,
    // ----- Auto Battle Data editor (only emitted when `editable` is set) -----
    /// Library pane: set chip `id`'s primary use count (the count that
    /// drives the materialized deck for every section).
    SetChipUseCount {
        id: usize,
        count: usize,
    },
    /// Library pane: set chip `id`'s secondary use count (drives the
    /// secondary-standard section — only meaningful for Standard chips).
    SetSecondaryChipUseCount {
        id: usize,
        count: usize,
    },
    /// Deck pane: zero every chip's use counts, emptying the deck.
    ClearAutoBattleData,
    /// Library pane: the filter text changed.
    AutoBattleDataFilterChanged(String),
    /// Library pane: the sort order changed.
    AutoBattleDataSortChanged(AutoBattleDataSort),
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
/// `editable`: when `true` (only the play tab passes this) and the
/// loaded save supports it, the Folder tab gains an Edit button that
/// flips its body into the in-place chip-deck editor. Replay /
/// opponent panels pass `false`, so they never show the affordance.
pub fn view<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
    streamer_mode: bool,
    play_button: Option<bool>,
    inline_actions: bool,
    editable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use iced::{Alignment, Fill};

    let available = available_tabs(loaded.save.as_ref(), streamer_mode);
    if available.is_empty() {
        return placeholder(t!(lang, "save-empty"));
    }
    let active = state
        .active_tab
        .filter(|t| available.contains(t))
        .unwrap_or(available[0]);

    // Body entrance — restarted on sub-tab switches (sliding
    // along the strip's direction of travel), edit-mode toggles
    // and game/save swaps (rising in vertically). The Play button
    // in the strip's tail is a fixture and never animates;
    // everything else in the tail slides horizontally only (these
    // are controls in a fixed-height row, not part of the body).
    let now = iced::time::Instant::now();
    let enter = state.enter.progress(now);
    let enter_from = state.enter_from;
    let entered = move |el: Element<'a, Action>| -> Element<'a, Action> {
        match enter {
            Some(p) => crate::anim::slide_in(el, p, enter_from),
            None => el,
        }
    };
    // Tail buttons animate only when their content actually
    // changed — a sub-tab switch (horizontal enter). A game/save
    // swap rises the body in, but the strip's buttons are
    // typically identical across saves, and re-animating them
    // there reads as a glitch. Edit-mode toggles run their own
    // two-phase swap below instead.
    let tail_slide = enter.filter(|_| enter_from.x != 0.0);
    let extras_dx = if enter_from.x != 0.0 { enter_from.x } else { 24.0 };
    let extras_entered = move |el: Element<'a, Action>| -> Element<'a, Action> {
        match tail_slide {
            Some(p) => crate::anim::slide_in(el, p, iced::Vector::new(extras_dx, 0.0)),
            None => el,
        }
    };
    // Edit-mode tail morph: the per-tab extras and the Save /
    // Cancel pair fade-through swap in both directions, so the
    // Edit affordance visibly turns into Save / Cancel and back.
    let (edit_side, edit_swap) = crate::anim::swap_phase(&state.edit_anim, now);
    let render_edit_buttons = editable && edit_side;
    // True while one of the in-place editors is open. Suppresses the
    // Play button (single-player would fight the open edit session) and
    // selects the editable body below.
    // One global edit toggle: while set, every editable tab shows its
    // editor (gated by that feature's editability), and one Save / Cancel
    // commits / discards them all.
    let editing_session = editable && state.editing.is_some();
    let folder_editing = editing_session && loaded.chips_editable;
    let navicust_editing = editing_session && loaded.navicust_editable;
    let patch_cards_editing = editing_session && loaded.patch_cards_editable;
    let auto_battle_data_editing = editing_session && loaded.auto_battle_data_editable;

    // Tab strip: tabs left, extras+Play right. We split into two
    // rows so the tab list can wrap onto a second line without
    // dragging the extras/Play tail with it. The tail is a
    // separate row, sized to its content and capped to the tab
    // button height so the strip's overall height doesn't grow
    // when active-tab extras (folder group toggle, copy buttons)
    // change.
    const TAB_STRIP_HEIGHT: f32 = 31.0;
    let mut tabs_only = row![].spacing(2).align_y(Alignment::Center);
    for tab in &available {
        let label = match tab {
            Tab::Cover => t!(lang, "save-tab-cover"),
            Tab::Navi => t!(lang, "save-tab-navi"),
            Tab::Folder => t!(lang, "save-tab-folder"),
            Tab::PatchCards => t!(lang, "save-tab-patch-cards"),
            Tab::AutoBattleData => t!(lang, "save-tab-auto-battle-data"),
        };
        tabs_only = tabs_only.push(widgets::tab_button(
            tab_icon(*tab),
            label,
            Action::SelectTab(*tab),
            *tab == active,
        ));
    }
    let tabs_only = tabs_only.wrap();
    // The whole tail morphs as ONE unit between its two sides —
    // (extras + Edit + Play) and (Save / Cancel) — so entering or
    // leaving edit mode dissolves everything together. Animating
    // only the extras left the Play button popping out instantly
    // and the exiting controls shifting into its freed space.
    let mut side = row![].spacing(6).align_y(Alignment::Center);
    if render_edit_buttons {
        // Save / Cancel are keyed on the mode, not the active
        // sub-tab — they stay planted while the user flips
        // between editor tabs.
        if inline_actions {
            side = side.push(edit_buttons(lang, loaded));
        }
    } else {
        if inline_actions {
            // Per-control entrances: a control carried over from
            // the previous sub-tab (the copy button lives on most
            // tabs) stays anchored in the strip; only controls
            // that actually appeared slide in. Suppressed while
            // the edit-mode morph runs — the whole side is moving
            // then.
            let prev_kinds = state.prev_tab.map(|p| extra_kinds(p, loaded)).unwrap_or_default();
            for kind in extra_kinds(active, loaded) {
                let el = render_extra(lang, state, active, kind);
                let carried = enter_from.x != 0.0 && prev_kinds.contains(&kind);
                let el = if edit_swap.is_some() || carried {
                    el
                } else {
                    extras_entered(el)
                };
                side = side.push(el);
            }
            if tab_has_edit(active, loaded, editable) {
                let edit_btn: Element<'a, Action> = widgets::labeled_icon_button(
                    lucide_icons::Icon::Pencil,
                    t!(lang, "save-edit"),
                    Action::EnterEdit,
                    [4.0, 10.0],
                    widgets::neutral,
                );
                // If the previous tab had the Edit affordance too, the
                // button never left the strip — re-animating it would
                // read as a glitch. Only sub-tab switches (horizontal
                // enters) can carry it over; vertical enters are
                // whole-body swaps where everything is new.
                let carried_over =
                    enter_from.x != 0.0 && state.prev_tab.map_or(false, |p| tab_has_edit(p, loaded, editable));
                let el = if edit_swap.is_some() || carried_over {
                    edit_btn
                } else {
                    extras_entered(edit_btn)
                };
                side = side.push(el);
            }
        }
        if let Some(enabled) = play_button {
            use lucide_icons::Icon;
            let label = row![Icon::Play.widget(), text(t!(lang, "play-play"))]
                .spacing(6)
                .align_y(Alignment::Center);
            let mut btn = button(label).padding([4, 10]);
            if enabled {
                btn = btn.style(widgets::primary_button).on_press(Action::PlayClicked);
            } else {
                btn = btn.style(widgets::neutral);
            }
            side = side.push(btn);
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

    // The folder editor lays out two side-by-side panes, each with its
    // own scrollbar, and wants the full available height — so it bypasses
    // the shared Shrink-height body scrollable the read-only views use.
    if folder_editing && active == Tab::Folder {
        let editor = render_folder_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if navicust_editing && active == Tab::Navi {
        let editor = render_navicust_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if patch_cards_editing && active == Tab::PatchCards {
        let editor = render_patch_cards_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }
    if auto_battle_data_editing && active == Tab::AutoBattleData {
        let editor = render_auto_battle_data_edit(lang, loaded, state);
        return column![tab_pane, entered(editor)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }

    // The Cover tab is a single full-height pane (logo banner), so it skips
    // the shrink-height body scrollable the other read-only views use.
    if active == Tab::Cover {
        let cover = render_cover::<Action>(lang, loaded);
        return column![tab_pane, entered(cover)]
            .spacing(style::PANE_GAP)
            .width(Fill)
            .height(Fill)
            .into();
    }

    let opts = RenderOpts {
        folder_grouped: state.folder_grouped,
    };
    let body = render::<Action>(lang, active, loaded, opts);
    // Body: each render_* returns one-or-more pane-styled
    // containers stacked into an Element. We wrap that whole
    // group in a Shrink-height scrollable so when its panes don't
    // fill the available space the column hugs them, and when
    // they do the user can scroll past the visible window. The
    // per-instance id is what [`State::apply`] snaps to the top
    // on tab changes.
    let body_scrollable = scrollable(body)
        .id(state.body_scroll_id.clone())
        .style(crate::widgets::chunky_scrollable)
        .width(Fill);
    column![tab_pane, entered(body_scrollable.into())]
        .spacing(style::PANE_GAP)
        .width(Fill)
        .into()
}

/// The global edit mode's Save / Cancel pair, shown in the tab
/// strip's tail while edit mode is on (or sliding out). One pair
/// for the whole save: they commit / discard the edits on *all*
/// tabs at once. Save is gated on a legal folder when chips are
/// editable — a full 30 chips with no folder-limit violations (an
/// incomplete or over-limit folder can't be written over the
/// save); navicust / patch-card layouts are always valid to write.
fn edit_buttons<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let can_save = !loaded.chips_editable || {
        let full = loaded.save.view_chips().map_or(true, |v| {
            let folder = v.equipped_folder_index();
            (0..MAX_FOLDER_CHIPS).all(|i| v.chip(folder, i).is_some())
        });
        full && folder_limits_satisfied(loaded)
    };
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

/// Whether `tab` offers the Edit affordance for this save. Split
/// from [`tab_extras`] so the view can keep the Edit button
/// unanimated when a sub-tab switch carries it over.
fn tab_has_edit(tab: Tab, loaded: &Loaded, editable: bool) -> bool {
    editable
        && match tab {
            // Only saves with a writable chip view (BN4/5/6);
            // `chips_editable` is the cached `view_chips_mut()`
            // probe.
            Tab::Folder => loaded.chips_editable,
            // Only BN4/5/6 (writable navicust) — and only saves
            // that actually have a navicust grid (LinkNavi BN4.5
            // navis have nothing to edit).
            Tab::Navi => {
                loaded.navicust_editable
                    && matches!(
                        loaded.save.view_navi(),
                        Some(tango_dataview::save::NaviView::Navicust(_))
                    )
            }
            // BN4 (PatchCard4s) and BN5/BN6 (PatchCard56s) are
            // both writable, each via its own editor.
            Tab::PatchCards => loaded.patch_cards_editable,
            // Only BN4/BN5 (writable auto-battle data).
            Tab::AutoBattleData => loaded.auto_battle_data_editable,
            _ => false,
        }
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
fn extra_kinds(tab: Tab, loaded: &Loaded) -> Vec<ExtraKind> {
    match tab {
        Tab::Folder => vec![ExtraKind::FolderGroup, ExtraKind::Copy],
        Tab::Navi => {
            let has_navicust = matches!(
                loaded.save.view_navi(),
                Some(tango_dataview::save::NaviView::Navicust(_))
            );
            if has_navicust {
                vec![ExtraKind::CopyImage, ExtraKind::Copy]
            } else {
                vec![ExtraKind::Copy]
            }
        }
        Tab::PatchCards | Tab::AutoBattleData => vec![ExtraKind::Copy],
        Tab::Cover => vec![],
    }
}

/// Stable copy-feedback key for a tab's copy buttons — shared between
/// the view (which renders the "Copied!" flash) and the host tabs'
/// update paths (which fire it once the copy actually lands on the
/// clipboard). See [`crate::copy_feedback`].
pub fn copy_flash_key(tab: Tab, image: bool) -> String {
    format!(
        "save-view-copy-{}-{}",
        if image { "image" } else { "text" },
        tab as u8
    )
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
            16.0,
            t!(lang, "save-copy-image"),
            t!(lang, "copied"),
            Some(Action::CopyTabImage(tab)),
            [4.0, 10.0],
        ),
        ExtraKind::Copy => widgets::copy_icon_button(
            &copy_flash_key(tab, false),
            Icon::ClipboardCopy,
            16.0,
            t!(lang, "save-copy"),
            t!(lang, "copied"),
            Some(Action::CopyTab(tab)),
            [4.0, 10.0],
        ),
    }
}

/// Plain-text representation of the active save-view tab, for the
/// clipboard. `None` = tab not exportable in this form. The Folder
/// branch honors `opts.folder_grouped`, mirroring [`render_folder`]'s
/// collapsed-by-identity layout so the clipboard matches what the
/// user sees.
pub fn tab_as_text(_lang: &LanguageIdentifier, tab: Tab, loaded: &Loaded, opts: RenderOpts) -> Option<String> {
    let assets = loaded.assets.as_ref();
    match tab {
        Tab::Folder => {
            let chips_view = loaded.save.view_chips()?;
            let folder_idx = chips_view.equipped_folder_index();
            // Read-only display treats "unsupported" and "unset" the
            // same — flatten the outer Option away.
            let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
            let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();

            let mut chips: Vec<Option<tango_dataview::save::Chip>> =
                (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();
            let regular_display_idx = if !assets.regular_chip_is_in_place() {
                if let Some(ri) = regular_idx {
                    let c = chips.remove(0);
                    chips.insert(ri, c);
                    Some(ri)
                } else {
                    None
                }
            } else {
                regular_idx
            };

            let mut out = String::new();
            if opts.folder_grouped {
                let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
                    indexmap::IndexMap::new();
                for (i, chip) in chips.iter().enumerate() {
                    let g = grouped_map.entry(chip.clone()).or_default();
                    g.count += 1;
                    if regular_display_idx == Some(i) {
                        g.is_regular = true;
                    }
                    if let Some(t) = tag_idxs {
                        if t[0] == i {
                            g.has_tag1 = true;
                        }
                        if t[1] == i {
                            g.has_tag2 = true;
                        }
                    }
                }
                for (chip, g) in &grouped_map {
                    let Some(c) = chip else {
                        out.push_str(&format!("{}\t---\n", g.count));
                        continue;
                    };
                    let name = assets
                        .chip(c.id)
                        .and_then(|info| info.name())
                        .unwrap_or_else(|| "???".to_string());
                    out.push_str(&format!("{}\t{name}\t{}", g.count, c.code));
                    if g.is_regular {
                        out.push_str("\t[REG]");
                    }
                    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
                        out.push_str("\t[TAG]");
                    }
                    out.push('\n');
                }
            } else {
                for (i, chip) in chips.iter().enumerate() {
                    let Some(c) = chip else {
                        out.push_str("---\n");
                        continue;
                    };
                    let name = assets
                        .chip(c.id)
                        .and_then(|info| info.name())
                        .unwrap_or_else(|| "???".to_string());
                    out.push_str(&format!("{name}\t{}", c.code));
                    if regular_display_idx == Some(i) {
                        out.push_str("\t[REG]");
                    }
                    if let Some(ti) = tag_idxs {
                        if ti.contains(&i) {
                            out.push_str("\t[TAG]");
                        }
                    }
                    out.push('\n');
                }
            }
            Some(out)
        }
        Tab::PatchCards => {
            let view = loaded.save.view_patch_cards()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
                    for i in 0..v.count() {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card56(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                        out.push_str(&format!(
                            "{name}\t{mb}MB\t{}\n",
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
                tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
                    for i in 0..6 {
                        let Some(card) = v.patch_card(i) else { continue };
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|c| c.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        out.push_str(&format!(
                            "0{}\t{name}\t{}\n",
                            ['A', 'B', 'C', 'D', 'E', 'F'][i],
                            if card.enabled { "ON" } else { "off" }
                        ));
                    }
                }
            }
            Some(out)
        }
        Tab::AutoBattleData => {
            let view = loaded.save.view_auto_battle_data()?;
            let mat = view.materialized();
            let chip_name = |id: Option<usize>| match id {
                Some(id) => assets
                    .chip(id)
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{id}")),
                None => "—".to_string(),
            };
            let mut out = String::new();
            let mut section = |title: &str, ids: &[Option<usize>]| {
                out.push_str(&format!("[{title}]\n"));
                for id in ids {
                    out.push_str(&chip_name(*id));
                    out.push('\n');
                }
                out.push('\n');
            };
            section("Secondary standard", mat.secondary_standard_chips());
            section("Standard", mat.standard_chips());
            section("Mega", mat.mega_chips());
            section("Giga", &[mat.giga_chip()]);
            section("Combos", mat.combos());
            section("Program advance", &[mat.program_advance()]);
            Some(out)
        }
        Tab::Navi => {
            let view = loaded.save.view_navi()?;
            let mut out = String::new();
            match view {
                tango_dataview::save::NaviView::LinkNavi(v) => {
                    let id = v.navi();
                    let name = assets
                        .navi(id)
                        .and_then(|n| n.name())
                        .unwrap_or_else(|| format!("#{id}"));
                    out.push_str(&format!("{name}\n"));
                }
                tango_dataview::save::NaviView::Navicust(v) => {
                    // Style name first (BN3 only), then two TSV
                    // columns: solid parts on the left, plus parts on
                    // the right, lined up row-by-row to match the
                    // side-by-side layout the UI renders. Shorter
                    // column gets blank cells; the trailing tab keeps
                    // a paste into Google Sheets / Excel parsing as
                    // two columns even when the last solid row has
                    // no plus partner.
                    if let Some(style_id) = v.style() {
                        if let Some(name) = assets.style(style_id).and_then(|s| s.name()) {
                            out.push_str(&name);
                            out.push('\n');
                        }
                    }
                    let mut solid = Vec::new();
                    let mut plus = Vec::new();
                    for i in 0..v.count() {
                        let Some(part) = v.navicust_part(i) else {
                            continue;
                        };
                        let Some(info) = assets.navicust_part(part.id) else {
                            continue;
                        };
                        let name = info.name().unwrap_or_else(|| format!("#{}", part.id));
                        if info.is_solid() {
                            solid.push(name);
                        } else {
                            plus.push(name);
                        }
                    }
                    for i in 0..solid.len().max(plus.len()) {
                        let s = solid.get(i).map(String::as_str).unwrap_or("");
                        let p = plus.get(i).map(String::as_str).unwrap_or("");
                        out.push_str(s);
                        out.push('\t');
                        out.push_str(p);
                        out.push('\n');
                    }
                }
            }
            Some(out)
        }
        Tab::Cover => None,
    }
}

/// Render a save-view tab to an RGBA image, for clipboard
/// "copy as image". Currently only Navi/Navicust supports this
/// (the rendered grid is already an image; we just hand back a
/// fresh copy). Returns `None` for tabs without a meaningful
/// image representation.
pub fn tab_as_image(tab: Tab, loaded: &Loaded) -> Option<image::RgbaImage> {
    let nv = loaded.save.view_navi()?;
    let v = match nv {
        tango_dataview::save::NaviView::Navicust(v) => v,
        _ => return None,
    };
    if !matches!(tab, Tab::Navi) {
        return None;
    }
    let layout = loaded.assets.navicust_layout()?;
    let materialized = v.materialized();
    let lang = crate::game::region_to_language(loaded.game.region());
    // Clipboard / export path: render at native (high) resolution.
    Some(crate::navicust::render(
        &materialized,
        &layout,
        v.as_ref(),
        loaded.assets.as_ref(),
        &lang,
        None,
    ))
}

fn render_cover<M: 'static>(_lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    // The cover tab carries no save data of its own — it just shows the
    // game's logo(s), decoded once in Loaded::build. Logos vary in
    // aspect ratio, so each is sized to a fixed height and Contain'd.
    let inner: Element<'static, M> = match loaded.logos.as_slice() {
        // Two variant logos in the family (e.g. Gregar/Falzar) — stack
        // them vertically with a left/right stagger, the way the Legacy
        // Collection lays out twin-version covers: the loaded variant
        // (logos[0]) sits up and to the left, its sibling down and to
        // the right.
        [top_logo, bottom_logo, ..] => {
            const H: f32 = 140.0;
            const STAGGER: f32 = 64.0;
            // Each logo sized to its own aspect ratio at height H (so
            // neither gets squished), returned alongside its width.
            let sized = |&(w, h, ref handle): &(u32, u32, iced_image::Handle)| -> (f32, Element<'static, M>) {
                let disp_w = H * (w as f32) / (h as f32);
                (
                    disp_w,
                    Image::new(handle.clone())
                        .content_fit(ContentFit::Contain)
                        .width(Length::Fixed(disp_w))
                        .height(Length::Fixed(H))
                        .into(),
                )
            };
            let (top_w, top_img) = sized(top_logo);
            let (bottom_w, bottom_img) = sized(bottom_logo);
            // Shared lane width so the pair centers as a unit: the top
            // logo hugs the lane's left edge, the bottom logo its right
            // edge, leaving STAGGER of diagonal offset between them.
            let lane = top_w.max(bottom_w) + STAGGER;
            let top = container(top_img)
                .width(Length::Fixed(lane))
                .align_x(iced::alignment::Horizontal::Left);
            let bottom = container(bottom_img)
                .width(Length::Fixed(lane))
                .align_x(iced::alignment::Horizontal::Right);
            column![top, bottom].spacing(20).into()
        }
        // Single logo — centered banner.
        [(_, _, handle), ..] => Image::new(handle.clone())
            .content_fit(ContentFit::Contain)
            .width(Fill)
            .height(Length::Fixed(220.0))
            .into(),
        // No registered logo — render an empty cover.
        [] => Space::new().into(),
    };
    container(column![inner].width(Fill).align_x(Alignment::Center))
        .width(Fill)
        // Fill the tab body's height, with the logo(s) centered vertically.
        .height(Fill)
        .align_y(iced::alignment::Vertical::Center)
        // Extra breathing room above/below the logo(s); standard
        // horizontal inset.
        .padding([crate::style::PANE_PADDING + 24.0, crate::style::PANE_PADDING + 24.0])
        .style(crate::widgets::pane)
        .into()
}

// ---------- Folder ----------

#[derive(Default)]
struct GroupedChip {
    count: usize,
    is_regular: bool,
    has_tag1: bool,
    has_tag2: bool,
}

fn render_folder<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded, grouped: bool) -> Element<'static, M> {
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let folder_idx = chips_view.equipped_folder_index();
    // Read-only display treats "unsupported" and "unset" the same —
    // flatten the outer Option away.
    let regular_idx = chips_view.regular_chip_index(folder_idx).flatten();
    let tag_idxs = chips_view.tag_chip_indexes(folder_idx).flatten();
    let chips_have_mb = assets.chips_have_mb();

    // Pull the 30-chip folder.
    let mut chips: Vec<Option<tango_dataview::save::Chip>> =
        (0..MAX_FOLDER_CHIPS).map(|i| chips_view.chip(folder_idx, i)).collect();
    let regular_display_idx = if !assets.regular_chip_is_in_place() {
        if let Some(ri) = regular_idx {
            let c = chips.remove(0);
            chips.insert(ri, c);
            Some(ri)
        } else {
            None
        }
    } else {
        regular_idx
    };

    // Build display items: either grouped (collapsed by chip identity)
    // or per-slot (one row per slot, possibly empty).
    type Item = (Option<tango_dataview::save::Chip>, GroupedChip);
    let items: Vec<Item> = if grouped {
        let mut grouped_map: indexmap::IndexMap<Option<tango_dataview::save::Chip>, GroupedChip> =
            indexmap::IndexMap::new();
        for (i, chip) in chips.iter().enumerate() {
            let g = grouped_map.entry(chip.clone()).or_default();
            g.count += 1;
            if regular_display_idx == Some(i) {
                g.is_regular = true;
            }
            if let Some(t) = tag_idxs {
                if t[0] == i {
                    g.has_tag1 = true;
                }
                if t[1] == i {
                    g.has_tag2 = true;
                }
            }
        }
        grouped_map.into_iter().collect()
    } else {
        chips
            .into_iter()
            .enumerate()
            .map(|(i, c)| {
                let [t1, t2] = tag_idxs.map(|t| [t[0] == i, t[1] == i]).unwrap_or([false, false]);
                (
                    c,
                    GroupedChip {
                        count: 1,
                        is_regular: regular_display_idx == Some(i),
                        has_tag1: t1,
                        has_tag2: t2,
                    },
                )
            })
            .collect()
    };

    // No column header. The rows themselves carry enough visual info
    // (icon, name+code, element icon, ATK value, MB value) that a label
    // strip would be redundant — and labels are what make it read as a
    // spreadsheet. When ungrouped, skip empty slots so we don't waste
    // a full-height row on each "—".
    // Tight stack — rows already have their own padding + accent
    // stripe; extra column spacing here adds dead gaps that read
    // as "spreadsheet" rather than "chip list".
    let mut body = column![].spacing(1).padding(0);
    let total_visible = if grouped {
        items.len()
    } else {
        items.iter().filter(|(c, _)| c.is_some()).count()
    };
    let mut visible_idx = 0usize;
    for (chip, g) in &items {
        if !grouped && chip.is_none() {
            continue;
        }
        let chip_id = chip.as_ref().map(|c| c.id);
        let code = chip.as_ref().map(|c| c.code.to_string());
        let is_first = visible_idx == 0;
        let is_last = visible_idx + 1 == total_visible;
        body = body.push(chip_row(
            loaded,
            chip_id,
            code,
            g,
            grouped,
            chips_have_mb,
            visible_idx,
            is_first,
            is_last,
        ));
        visible_idx += 1;
    }

    let _ = grouped;
    // Rows are flush to the pane edges; the outer scrollable in
    // `view` handles vertical overflow once total content exceeds
    // the available height.
    container(body).width(Fill).style(crate::widgets::pane).into()
}

/// Mega/Giga class usage and per-chip copies in one folder, used to honor
/// the equipped navi's [`tango_dataview::save::FolderLimits`] in both the
/// editor UI (greying out un-addable library chips) and the apply path
/// ([`crate::app`]'s `apply_chip_edit`). Built by scanning the folder's 30
/// slots; cheap enough to rebuild per edit / per frame.
pub struct FolderUsage {
    pub navi: usize,
    pub mega: usize,
    pub giga: usize,
    pub dark: usize,
    /// Copies installed per chip id (codes collapsed — the copy cap is
    /// per chip, not per code).
    pub copies: std::collections::HashMap<usize, usize>,
}

impl FolderUsage {
    /// Tally the equipped folder's 30 slots.
    pub fn scan(loaded: &Loaded, folder_idx: usize) -> Self {
        use tango_dataview::rom::ChipClass;
        let assets = loaded.assets.as_ref();
        let mut navi = 0;
        let mut mega = 0;
        let mut giga = 0;
        let mut dark = 0;
        let mut copies: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
        if let Some(view) = loaded.save.view_chips() {
            for slot in 0..MAX_FOLDER_CHIPS {
                let Some(c) = view.chip(folder_idx, slot) else { continue };
                *copies.entry(c.id).or_insert(0) += 1;
                let Some(chip) = assets.chip(c.id) else {
                    continue;
                };
                if chip.dark() {
                    dark += 1;
                    continue;
                }
                match chip.class() {
                    ChipClass::Navi => navi += 1,
                    ChipClass::Mega => mega += 1,
                    ChipClass::Giga => giga += 1,
                    _ => {}
                }
            }
        }
        Self {
            navi,
            mega,
            giga,
            dark,
            copies,
        }
    }

    /// Whether one more copy of `chip_id` fits under `limits` — the
    /// per-chip copy cap plus the mega/giga class cap. The folder-full
    /// (30-slot) check is separate. Unknown chips aren't blocked.
    pub fn can_add(&self, loaded: &Loaded, chip_id: usize, limits: &tango_dataview::save::FolderLimits) -> bool {
        use tango_dataview::rom::ChipClass;
        let Some(info) = loaded.assets.chip(chip_id) else {
            return true;
        };
        if self.copies.get(&chip_id).copied().unwrap_or(0) >= (limits.max_copies)(info.as_ref()) {
            return false;
        }
        if info.dark() {
            return limits.dark_limit.map(|limit| self.dark < limit).unwrap_or(true);
        }
        match info.class() {
            ChipClass::Navi => limits.navi_limit.map(|limit| self.navi < limit).unwrap_or(true),
            ChipClass::Mega => limits.mega_limit.map(|limit| self.mega < limit).unwrap_or(true),
            ChipClass::Giga => limits.giga_limit.map(|limit| self.giga < limit).unwrap_or(true),
            _ => true,
        }
    }
}

/// Whether the equipped folder satisfies the navi's
/// [`tango_dataview::save::FolderLimits`] — the mega/giga class caps, the
/// per-chip copy cap, and Regular/Tag memory. `true` when the game defines
/// no limits. Gates Save: the folder pane blocks *adding* a violation, but
/// cross-tab edits can still leave an already-built folder illegal (e.g.
/// pulling a MegFldr part on the Navi tab lowers the mega cap under the
/// chips already in the folder), and a save edited elsewhere may arrive
/// over a limit.
pub fn folder_limits_satisfied(loaded: &Loaded) -> bool {
    let Some(view) = loaded.save.view_chips() else {
        return true;
    };
    let folder_idx = view.equipped_folder_index();
    let limits = loaded.save.folder_limits(&*loaded.assets);
    let usage = FolderUsage::scan(loaded, folder_idx);
    if limits.navi_limit.map(|limit| usage.navi > limit).unwrap_or(false)
        || limits.mega_limit.map(|limit| usage.mega > limit).unwrap_or(false)
        || limits.giga_limit.map(|limit| usage.giga > limit).unwrap_or(false)
        || limits.dark_limit.map(|limit| usage.dark > limit).unwrap_or(false)
    {
        return false;
    }
    // Per-chip copy cap.
    for (&id, &count) in &usage.copies {
        if let Some(chip) = loaded.assets.chip(id) {
            if count > (limits.max_copies)(chip.as_ref()) {
                return false;
            }
        }
    }
    let mb_of = |slot: usize| {
        view.chip(folder_idx, slot)
            .and_then(|c| loaded.assets.chip(c.id))
            .map_or(0u32, |c| c.mb() as u32)
    };
    // The Regular chip must fit Regular memory.
    if let Some(cap) = limits.reg_memory {
        if let Some(Some(reg)) = view.regular_chip_index(folder_idx) {
            if mb_of(reg) > cap as u32 {
                return false;
            }
        }
    }
    // The Tag pair's combined MB must fit Tag memory.
    if let Some(budget) = limits.tag_memory {
        if let Some(Some([a, b])) = view.tag_chip_indexes(folder_idx) {
            if mb_of(a) + mb_of(b) > budget {
                return false;
            }
        }
    }
    true
}

/// Editable folder view: the folder (left) beside the chip library
/// (right). The left pane lists the 30 raw slots — each filled slot can
/// be removed or marked REG/TAG; the right pane lists every selectable
/// chip with a button per valid code that adds it to the first empty
/// slot. Each pane scrolls independently. The equipped navi's
/// [`tango_dataview::save::FolderLimits`] (mega/giga caps, per-chip copy
/// cap, Regular/Tag memory) are surfaced in the folder header and enforced
/// by greying out library chips / REG / TAG toggles that would break them.
fn render_folder_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded, state: &'a State) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(chips_view) = loaded.save.view_chips() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let folder_idx = chips_view.equipped_folder_index();
    // Outer Some = the game has the feature, so show its toggle.
    let reg = chips_view.regular_chip_index(folder_idx);
    let regular_supported = reg.is_some();
    let regular_idx = reg.flatten();
    let tag_supported = chips_view.tag_chip_indexes(folder_idx).is_some();

    // Folder-construction limits for the equipped navi (mega/giga class
    // caps, per-chip copy cap, Regular/Tag memory budgets). `None` for
    // games that don't define them — those stay unrestricted.
    let assets = loaded.assets.as_ref();
    let limits = loaded.save.folder_limits(assets);
    let usage = FolderUsage::scan(loaded, folder_idx);
    // If exactly one Tag chip is picked, a second can only join if the
    // pair's combined MB fits Tag memory; capture the partner's MB so each
    // slot can test its own addition.
    let tag_partner_mb: Option<u32> = match edit.tags.as_slice() {
        [only] => chips_view
            .chip(folder_idx, *only)
            .and_then(|c| assets.chip(c.id))
            .map(|c| c.mb() as u32),
        _ => None,
    };

    // ----- Left pane: the folder -----
    let filled = (0..MAX_FOLDER_CHIPS)
        .filter(|&i| chips_view.chip(folder_idx, i).is_some())
        .count();
    let mut folder_rows: Vec<Element<'a, Action>> = Vec::with_capacity(MAX_FOLDER_CHIPS);
    for slot in 0..MAX_FOLDER_CHIPS {
        let chip = chips_view.chip(folder_idx, slot);
        let is_regular = regular_idx == Some(slot);
        let is_tag = edit.tags.contains(&slot);
        // This slot's chip MB, for the Regular / Tag memory gates.
        let this_mb = chip.as_ref().and_then(|c| assets.chip(c.id)).map(|c| c.mb());
        // A chip can be made Regular only if its MB fits Regular memory;
        // clearing the current Regular is always allowed.
        let reg_allowed = match limits.reg_memory {
            Some(cap) => is_regular || this_mb.map_or(true, |mb| mb <= cap),
            None => true,
        };
        // It can join the Tag pair only if it fits Tag memory on its own
        // (a chip bigger than the whole budget can never be tagged) and,
        // once a partner is picked, the pair's combined MB still fits.
        // Deselecting is always allowed.
        let tag_allowed = match limits.tag_memory {
            Some(budget) => {
                is_tag || {
                    let this = this_mb.map(|m| m as u32).unwrap_or(0);
                    this <= budget && tag_partner_mb.map_or(true, |partner| partner + this <= budget)
                }
            }
            None => true,
        };
        folder_rows.push(folder_slot_row(
            loaded,
            slot,
            chip,
            is_regular,
            regular_supported,
            tag_supported,
            is_tag,
            reg_allowed,
            tag_allowed,
        ));
    }
    // Draggable list: grab a chip row and drop it to reorder. The handler
    // ignores drops involving an empty slot, so only chips move (no dragging a
    // gap, no dropping into one).
    // `width(Fill)` is required because the rows contain `Fill` cells — unlike
    // iced's `column!`, sweeten's `from_vec` defaults to `Shrink` and won't
    // adapt to Fill children (they'd collapse to zero width, hiding the rows).
    let folder_list = sweeten::widget::Column::from_vec(folder_rows)
        .width(Fill)
        .spacing(1)
        .style(reorder_drag_style)
        .on_drag(Action::ReorderChips);
    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        Action::ClearFolder,
        [5.0, 10.0],
        widgets::danger_button,
    );
    // "Folder" label, then a smaller count that turns red while the
    // folder is short of the 30 chips a legal folder needs.
    let count = limit_caption(
        t!(
            lang,
            "folder-edit-count",
            count = filled as i64,
            limit = MAX_FOLDER_CHIPS
        ),
        filled < MAX_FOLDER_CHIPS,
    );
    let header_row = row![
        text(t!(lang, "folder-edit-folder")).size(TEXT_BODY),
        count,
        Space::new().width(Fill),
        clear_all,
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    // Second line (only for navis with folder limits): mega/dark/giga usage vs
    // their caps (red when over) plus the Regular/Tag memory budgets.
    let stats_row = {
        let mut r = row![].spacing(12).align_y(Alignment::Center);
        // Per-class usage vs cap, red when over. Labels are resolved
        // up front so every `t!` key stays a literal.
        let class_stats = [
            limits.navi_limit.map(|l| {
                (
                    t!(lang, "folder-edit-navi", used = usage.navi as i64, limit = l as i64),
                    usage.navi > l,
                )
            }),
            limits.mega_limit.map(|l| {
                (
                    t!(lang, "folder-edit-mega", used = usage.mega as i64, limit = l as i64),
                    usage.mega > l,
                )
            }),
            limits.giga_limit.map(|l| {
                (
                    t!(lang, "folder-edit-giga", used = usage.giga as i64, limit = l as i64),
                    usage.giga > l,
                )
            }),
            limits.dark_limit.map(|l| {
                (
                    t!(lang, "folder-edit-dark", used = usage.dark as i64, limit = l as i64),
                    usage.dark > l,
                )
            }),
        ];
        for (label, over) in class_stats.into_iter().flatten() {
            r = r.push(limit_caption(label, over));
        }
        if let Some(reg) = limits.reg_memory {
            r = r.push(
                text(t!(lang, "folder-edit-reg-memory", mb = reg as i64))
                    .size(TEXT_CAPTION)
                    .style(muted_text_style),
            );
        }
        if let Some(tag) = limits.tag_memory {
            r = r.push(
                text(t!(lang, "folder-edit-tag-memory", mb = tag as i64))
                    .size(TEXT_CAPTION)
                    .style(muted_text_style),
            );
        }
        r
    };
    let header_col = column![header_row, stats_row].spacing(4);
    let folder_header = container(header_col).width(Fill).padding(style::HEADER_PADDING);
    let folder_pane = editor_pane(folder_header, folder_list);

    // ----- Right pane: the chip library -----
    let chips_have_mb = loaded.assets.chips_have_mb();
    let filter = edit.library_filter.to_lowercase();
    let mut lib_list = column![].spacing(1).padding(0);
    let mut shown = 0usize;
    for (id, name, code) in sorted_library_entries(loaded, state.library_sort) {
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        // Disabled when the folder is full or adding this chip would break
        // the navi's mega/giga/copy limits.
        let addable = filled < MAX_FOLDER_CHIPS && usage.can_add(loaded, id, &limits);
        lib_list = lib_list.push(library_entry_row(loaded, id, name, code, shown, chips_have_mb, addable));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "folder-edit-search"),
        &edit.library_filter,
        Action::LibraryFilterChanged,
        &LibrarySort::ALL,
        state.library_sort,
        LibrarySort::label,
        Action::LibrarySortChanged,
    );
    editor_panes(folder_pane, editor_pane(lib_header, lib_list))
}

/// The palette thumbnail for part `id` at orientation `(rot, compressed)`.
/// The default orientation reuses the icon baked once at load; a rotated /
/// uncompressed shape is drawn live by a small canvas ([`PartThumb`]) so
/// we never re-bake an image (which would mint a fresh texture id every
/// frame). `dim` fades it for at-cap rows. `None` for an empty shape.
fn part_thumb<'a>(loaded: &'a Loaded, id: usize, rot: u8, compressed: bool, dim: bool) -> Option<Element<'a, Action>> {
    if rot == 0 && compressed {
        let (w, h, handle) = loaded.navicust_part_icons.get(id)?.as_ref()?;
        return Some(
            Image::new(handle.clone())
                .width(Length::Fixed(*w as f32))
                .height(Length::Fixed(*h as f32))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::None)
                .opacity(if dim { 0.35 } else { 1.0 })
                .into(),
        );
    }
    let info = loaded.assets.navicust_part(id)?;
    let color = info.color()?;
    let bitmap = info
        .compressed_bitmap()
        .filter(|_| compressed)
        .unwrap_or_else(|| info.uncompressed_bitmap());
    let rotated = crate::navicust::rotate_bitmap(&bitmap, rot);
    crate::navicust_editor::PartThumb::new(&rotated, color, info.is_solid(), dim).map(|t| t.view())
}

/// The navicust editor: an interactive grid (left) + a part palette
/// (right), mirroring [`render_folder_edit`]'s two-pane layout — the grid
/// pane shrinks to the grid so the palette gets the rest of the width. The
/// grid is drawn live by [`crate::navicust_editor::EditorGrid`], which
/// shares the decoration-drawing routine ([`crate::navicust::paint`]) with
/// the read-only viewer and the clipboard image, and ghosts the held part.
/// Each palette row carries its own rotate / (de)compress buttons that set
/// the orientation the part is picked up in.
fn render_navicust_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a Loaded, state: &'a State) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(tango_dataview::save::NaviView::Navicust(v)) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let size = v.size();
    let (cols, rows) = (size[0], size[1]);
    // BN4/5/6 (the only editable navicust games) always publish a layout.
    let Some(layout) = assets.navicust_layout() else {
        return placeholder(t!(lang, "save-empty"));
    };

    // Live grid recomputed from the part slots (NOT the WRAM cache), so
    // staged edits show immediately. `materialize` takes `[rows, cols]`.
    let materialized = tango_dataview::navicust::materialize(v.as_ref(), [rows, cols], assets);
    let model = crate::navicust::build_model(&materialized, &layout, v.as_ref(), assets);
    let installed = (0..v.count()).filter(|&i| v.navicust_part(i).is_some()).count();
    // Cell → installed-part slot, captured before `model` is moved into the
    // grid, to drive the per-cell hover popover overlay below.
    let occupancy = model.occupancy.clone();

    // Held-part ghost data, resolved from the ROM.
    let held = edit.held_part.and_then(|hp| {
        let info = assets.navicust_part(hp.id)?;
        let color = info.color()?;
        let bitmap = info
            .compressed_bitmap()
            .filter(|_| hp.compressed)
            .unwrap_or_else(|| info.uncompressed_bitmap());
        let (solid, plus) = crate::navicust::part_colors(color);
        Some(crate::navicust_editor::Held {
            cells: crate::navicust_editor::rotated_offsets(&bitmap, hp.rot),
            grab: (hp.grab_row as isize, hp.grab_col as isize),
            solid,
            plus,
            is_solid: info.is_solid(),
        })
    });

    // Editor grid geometry (must match `EditorGrid::new`) so the hover
    // popover overlay's cells line up with the painted squares.
    let g = crate::navicust::geometry(cols, rows);
    let scale = crate::navicust::display_scale(crate::navicust_editor::DISPLAY_W);
    let cell = crate::navicust::SQUARE_SIZE * scale;
    let origin_x = (g.body_origin_x + crate::navicust::BORDER_WIDTH / 2.0) * scale;
    let origin_y = (g.body_origin_y + crate::navicust::BORDER_WIDTH / 2.0) * scale;
    let grid_w = g.total_w * scale;
    let grid_h = g.total_h * scale;

    let canvas_el: Element<'a, Action> = crate::navicust_editor::EditorGrid::new(model, held).view();

    // Per-cell hover popover (part name + description), mirroring the
    // read-only viewer: a fixed grid of cell-sized spaces with each covered
    // cell tooltip-wrapped. Stacked *over* the canvas: the cells report
    // `Interaction::None`, so iced's Stack doesn't levitate the cursor away
    // from the canvas beneath (its clicks / scroll / ghost still work, and
    // its Pointer/Crosshair cursor still wins), while the tooltips get the
    // real cursor and fire. Beneath the canvas they wouldn't — the canvas's
    // non-None interaction levitates the cursor off any lower layer.
    let mut overlay_col = column![Space::new().height(Length::Fixed(origin_y))];
    for r in 0..rows {
        let mut cell_row = row![Space::new().width(Length::Fixed(origin_x))];
        for c in 0..cols {
            let info = occupancy
                .get(r * cols + c)
                .copied()
                .flatten()
                .and_then(|slot| v.navicust_part(slot))
                .and_then(|p| assets.navicust_part(p.id));
            let cell_el: Element<'a, Action> = if let Some(info) = info {
                let name = info.name().unwrap_or_else(|| "?".to_string());
                let mut tip_col = column![text(name).size(TEXT_BODY)].spacing(2);
                if let Some(desc) = info.description() {
                    tip_col = tip_col.push(text(desc).size(TEXT_CAPTION));
                }
                let tip = container(tip_col).padding(8).style(tooltip_style);
                let space = Space::new().width(Length::Fixed(cell)).height(Length::Fixed(cell));
                tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
            } else {
                Space::new()
                    .width(Length::Fixed(cell))
                    .height(Length::Fixed(cell))
                    .into()
            };
            cell_row = cell_row.push(cell_el);
        }
        overlay_col = overlay_col.push(cell_row);
    }
    let canvas_el: Element<'a, Action> = stack![canvas_el, overlay_col]
        .width(Length::Fixed(grid_w))
        .height(Length::Fixed(grid_h))
        .into();

    // Installed copies per part id — palette entries for parts already at
    // the per-part cap are shown disabled (not selectable).
    let mut installed_counts: std::collections::HashMap<usize, usize> = std::collections::HashMap::new();
    for i in 0..v.count() {
        if let Some(p) = v.navicust_part(i) {
            *installed_counts.entry(p.id).or_insert(0) += 1;
        }
    }

    // ----- Left pane: grid + rotate/compress controls -----
    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        Action::ClearNavicust,
        [5.0, 10.0],
        widgets::danger_button,
    );
    let count = text(t!(lang, "navicust-edit-count", count = installed as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let grid_header = container(
        row![
            text(t!(lang, "navicust-edit-grid")).size(TEXT_BODY),
            count,
            Space::new().width(Fill),
            clear_all,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING);

    let held_opt = edit.held_part;

    // ----- Part palette (shown below the grid, like the read-only view) -----
    // Rows run flush to the pane sides (no side inset); shares only its
    // row spacing with the patches / replays lists.
    let mut palette = column![].spacing(2).padding(0).width(Fill);
    for (row_idx, (id, name, description)) in sorted_navicust_parts(loaded, state.navicust_sort, &edit.navicust_filter)
        .into_iter()
        .enumerate()
    {
        // Parts already at the per-part copy cap are greyed out + not
        // selectable.
        let at_cap = installed_counts.get(&id).copied().unwrap_or(0) >= crate::navicust_editor::MAX_COPIES_PER_PART;
        // Orientation shown in (and picked up from) the picker.
        let (rot, compressed) = edit.orient_of(id);
        // Shape thumbnail at the part's current picker orientation, shown
        // at the baked pixel size (1:1) so the 1px lines stay crisp; every
        // part shares the same n×n grid so rows align. Dimmed when at cap.
        let icon_el: Element<'a, Action> = part_thumb(loaded, id, rot, compressed, at_cap).unwrap_or_else(|| {
            Space::new()
                .width(Length::Fixed(40.0))
                .height(Length::Fixed(40.0))
                .into()
        });
        let name_text = if at_cap {
            text(name).size(TEXT_BODY).style(muted_text_style)
        } else {
            text(name).size(TEXT_BODY)
        };
        let mut info_col = column![name_text].spacing(1);
        if let Some(desc) = description.filter(|d| !d.trim().is_empty()) {
            info_col = info_col.push(text(desc).size(TEXT_CAPTION).style(muted_text_style));
        }
        // Per-part orientation controls: rotate, and (de)compress. They
        // edit this part's picker entry — including the thumbnail beside
        // them and the orientation it's picked up in. The compress button
        // names the action it performs (Uncompress when compressed, else
        // Compress). They're nested inside the row's pick-up button (iced
        // forwards clicks to these inner buttons first), so they live on the
        // menu item itself rather than floating beside it.
        let rotate_btn = widgets::icon_button(
            lucide_icons::Icon::RotateCw,
            t!(lang, "navicust-edit-rotate"),
            Action::RotatePart { id },
            [6.0, 8.0],
        );
        let (compress_icon, compress_label) = if compressed {
            (lucide_icons::Icon::Expand, t!(lang, "navicust-edit-uncompress"))
        } else {
            (lucide_icons::Icon::Shrink, t!(lang, "navicust-edit-compress"))
        };
        // A part whose compressed and uncompressed shapes are identical can't
        // be (de)compressed — render the button disabled rather than letting
        // it toggle a flag with no visible effect.
        let compressible = loaded
            .assets
            .navicust_part(id)
            .and_then(|info| info.compressed_bitmap().map(|bmp| bmp != info.uncompressed_bitmap()))
            .unwrap_or(false);
        let compress_btn = widgets::icon_button_maybe(
            compress_icon,
            compress_label,
            compressible.then_some(Action::ToggleCompressPart { id }),
            [6.0, 8.0],
        );
        let controls = column![rotate_btn, compress_btn].spacing(4).align_x(Alignment::Center);
        let content = row![icon_el, info_col, Space::new().width(Fill), controls]
            .spacing(8)
            .align_y(Alignment::Center);
        let selected = held_opt.map_or(false, |h| h.id == id);
        let mut pick = button(content)
            .padding(style::ROW_PADDING)
            .width(Fill)
            .style(widgets::list_item(selected, row_idx));
        if !at_cap {
            pick = pick.on_press(Action::PickUpPalettePart { id });
        }
        palette = palette.push(pick);
    }
    let parts_header = library_header(
        lang,
        t!(lang, "navicust-edit-search"),
        &edit.navicust_filter,
        Action::NavicustFilterChanged,
        &NavicustSort::ALL,
        state.navicust_sort,
        NavicustSort::label,
        Action::NavicustSortChanged,
    );

    // Left pane: mirrors the read-only Navi view — the grid with the
    // installed ("picked") parts listed below it — and fills/expands to
    // its half of the tab, with the grid + parts centered inside.
    let mut grid_inner = column![container(canvas_el).center_x(Fill)]
        .spacing(8)
        .align_x(Alignment::Center)
        .padding([4, 8]);
    if let Some(parts) = navicust_installed_parts::<Action>(loaded, v.as_ref()) {
        grid_inner = grid_inner.push(parts);
    }

    // Grid on the left, the editing palette filling the remaining width.
    editor_panes(editor_pane(grid_header, grid_inner), editor_pane(parts_header, palette))
}

/// 28×28 chip icon. Empty (`None`) renders a same-sized spacer so empty
/// rows keep the same height as filled ones.
fn chip_icon<'a>(loaded: &'a Loaded, chip_id: Option<usize>) -> Element<'a, Action> {
    match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new()
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into(),
    }
}

/// Wrap `inner` so hovering anywhere over it shows the chip's full image
/// + description (the read-only list's chip popover). No-op when the
/// chip has neither, or for an empty slot.
fn with_chip_tooltip<'a>(
    loaded: &'a Loaded,
    chip_id: Option<usize>,
    accent: Option<iced::Color>,
    inner: Element<'a, Action>,
) -> Element<'a, Action> {
    let Some(id) = chip_id else { return inner };
    let info = loaded.assets.chip(id);
    let description = info.as_ref().and_then(|i| i.description());
    // Program advances have no meaningful standalone artwork, so their
    // popover is description-only.
    let is_pa = info
        .as_ref()
        .map_or(false, |i| i.class() == tango_dataview::rom::ChipClass::ProgramAdvance);
    let image_handle = if is_pa {
        None
    } else {
        loaded.chip_images.get(id).cloned().flatten()
    };
    if description.is_none() && image_handle.is_none() {
        return inner;
    }
    let mut tip = column![].spacing(6);
    if let Some((w, h, h_handle)) = image_handle {
        tip = tip.push(
            Image::new(h_handle)
                .width(Length::Fixed(w as f32 * 2.0))
                .height(Length::Fixed(h as f32 * 2.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain),
        );
    }
    if let Some(desc) = description {
        tip = tip.push(text(desc).size(TEXT_CAPTION));
    }
    tooltip(
        inner,
        container(tip).padding(8).style(chip_tooltip_style(accent)),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
}

/// Wrap an editor row's content with the class-accent stripe + zebra
/// background, matching the read-only chip list.
fn edit_row_wrap<'a>(
    inner: Element<'a, Action>,
    accent: Option<iced::Color>,
    row_idx: usize,
    leading: Option<Element<'a, Action>>,
) -> Element<'a, Action> {
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_t: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    // `leading` (e.g. the library's add arrow) sits in the gutter to the
    // left of the accent stripe.
    let mut r = row![].height(Length::Shrink).align_y(Alignment::Center);
    if let Some(lead) = leading {
        r = r.push(container(lead).padding([0, 6]));
    }
    r = r.push(stripe).push(container(inner).width(Fill));
    container(r)
        .width(Fill)
        .style(crate::widgets::zebra_row(row_idx))
        .into()
}

/// Element-icon / ATK / MB stat cells shared by both editor panes,
/// matching the read-only chip list's columns. The MB cell collapses to
/// nothing when the game doesn't use MB.
fn chip_stat_cells<'a>(loaded: &'a Loaded, chip_id: usize, chips_have_mb: bool) -> [Element<'a, Action>; 3] {
    let info = loaded.assets.chip(chip_id);
    let element: Element<'a, Action> = info
        .as_ref()
        .map(|i| i.element())
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);
    let atk: Element<'a, Action> =
        container(text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(46.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into();
    let mb_cell: Element<'a, Action> = if chips_have_mb {
        container(text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION))
            .width(Length::Fixed(42.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    } else {
        Space::new().into()
    };
    [element, atk, mb_cell]
}

/// One folder slot in the editor's left pane. Filled slots show the
/// A muted grip glyph marking a row as drag-to-reorder. The whole row is the
/// drag surface (sweeten's `Column` owns the gesture); this is just the visual
/// affordance, in a fixed-width cell so rows line up. Wrapped in a `mouse_area`
/// only to show the grab-hand cursor on hover — it sets no handlers, so it
/// doesn't capture the press (the drag gesture still reaches the column).
fn drag_handle<'a>() -> Element<'a, Action> {
    use lucide_icons::Icon;
    let grip = container(Icon::GripVertical.widget().size(TEXT_BODY).style(muted_text_style))
        .width(Length::Fixed(16.0))
        .align_x(iced::alignment::Horizontal::Center);
    iced::widget::mouse_area(grip)
        .interaction(iced::mouse::Interaction::Grab)
        .into()
}

/// Drag styling shared by the reorderable folder / patch-card columns. The
/// default sweeten style tints the rows that shift aside with the theme's
/// *primary* color (green in this app); we don't want any overlay, so it's
/// turned off. The floating ghost is softened to a plain panel.
fn reorder_drag_style(theme: &iced::Theme) -> sweeten::widget::column::Style {
    let ep = theme.extended_palette();
    let ghost = {
        let mut c = ep.background.weak.color;
        c.a = 0.92;
        c
    };
    sweeten::widget::column::Style {
        scale: 1.02,
        // No tint on the rows that move to open a gap.
        moved_item_overlay: iced::Color::TRANSPARENT,
        ghost_border: iced::Border {
            width: 1.0,
            color: ep.background.strong.color,
            radius: 4.0.into(),
        },
        ghost_background: iced::Background::Color(ghost),
    }
}

/// chip's full stats (element / code / ATK / MB, like the read-only
/// list) plus Remove / REG / TAG controls (REG/TAG only where the game
/// supports them); empty slots show a muted placeholder.
fn folder_slot_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    chip: Option<tango_dataview::save::Chip>,
    is_regular: bool,
    regular_supported: bool,
    tag_supported: bool,
    is_tag: bool,
    reg_allowed: bool,
    tag_allowed: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();
    let chip_id = chip.as_ref().map(|c| c.id);
    let info = chip_id.and_then(|id| assets.chip(id));
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );

    let mut inner = row![chip_icon(loaded, chip_id)].spacing(8).align_y(Alignment::Center);
    match chip.as_ref() {
        Some(c) => {
            let name = info
                .as_ref()
                .and_then(|i| i.name())
                .unwrap_or_else(|| "???".to_string());
            let [element, atk, mb] = chip_stat_cells(loaded, c.id, chips_have_mb);
            let code = container(text(c.code.to_string()).size(TEXT_BODY).font(iced::Font::MONOSPACE))
                .width(Length::Fixed(22.0))
                .align_x(iced::alignment::Horizontal::Right);
            inner = inner
                .push(text(name).size(TEXT_BODY).width(Fill))
                .push(element)
                .push(code)
                .push(atk)
                .push(mb);
            if regular_supported {
                // Greyed out (no message) when the chip's MB won't fit
                // Regular memory; see render_folder_edit.
                inner = inner.push(edit_toggle_maybe(
                    "REG",
                    is_regular,
                    iced::Color::from_rgb8(0xff, 0x42, 0xa5),
                    reg_allowed.then_some(Action::ToggleRegular { slot }),
                ));
            }
            if tag_supported {
                // Greyed out when joining the Tag pair would bust Tag memory.
                inner = inner.push(edit_toggle_maybe(
                    "TAG",
                    is_tag,
                    iced::Color::from_rgb8(0x29, 0xa1, 0x21),
                    tag_allowed.then_some(Action::ToggleTag { slot }),
                ));
            }
            // ✕ → remove this chip (back out to the library).
            inner = inner.push(
                button(Icon::X.widget().size(TEXT_BODY))
                    .padding([3, 8])
                    .style(widgets::neutral)
                    .on_press(Action::RemoveChip { slot }),
            );
        }
        None => {
            inner = inner.push(text("—").size(TEXT_BODY).style(muted_text_style).width(Fill));
        }
    }
    // Drag handle in the far-left gutter (left of the accent stripe) on filled
    // rows; empty slots get a same-width spacer so the stripes stay aligned and
    // aren't draggable anyway.
    let leading: Option<Element<'a, Action>> = Some(if chip.is_some() {
        drag_handle()
    } else {
        Space::new().width(Length::Fixed(16.0)).into()
    });
    // Tooltip wraps only the chip content — not the leading grip gutter — so
    // hovering the drag handle doesn't pop the chip card.
    let tipped = with_chip_tooltip(loaded, chip_id, accent, inner.padding([3, 12]).into());
    edit_row_wrap(tipped, accent, slot, leading)
}

/// One chip+code in the editor's right pane (the library / palette).
/// Shows the chip's stats (element / code / ATK / MB, like the read-only
/// list). The whole row is a click-to-add button that drops this
/// chip+code into the folder; it's disabled (`addable == false`) when the
/// folder is full or adding the chip would break the navi's folder limits.
fn library_entry_row<'a>(
    loaded: &'a Loaded,
    chip_id: usize,
    name: String,
    code: tango_dataview::save::ChipCode,
    row_idx: usize,
    chips_have_mb: bool,
    addable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    let info = loaded.assets.chip(chip_id);
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );
    let [element, atk, mb] = chip_stat_cells(loaded, chip_id, chips_have_mb);

    let code_cell = container(text(code.to_string()).size(TEXT_BODY).font(iced::Font::MONOSPACE))
        .width(Length::Fixed(22.0))
        .align_x(iced::alignment::Horizontal::Right);

    let inner = row![
        chip_icon(loaded, Some(chip_id)),
        text(name).size(TEXT_BODY).width(Fill),
        element,
        code_cell,
        atk,
        mb,
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // The whole row is the add control: clicking anywhere drops this
    // chip+code into the folder. The class-accent stripe rides inside the
    // button as its leading column (flush left — the button carries no
    // padding of its own), so `list_item`'s zebra base paints the full
    // width behind it and the gutter stays tinted even when a chip has no
    // accent. Same composition as `edit_row_wrap` / `card_wrap`, so the
    // library row isn't a bespoke wrapper. Disabled when not addable (no
    // empty slot, or it would break the navi's folder limits). ChipCode is Copy.
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_t: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            ..Default::default()
        })
        .into();
    let content = row![stripe, container(inner).width(Fill).padding([3, 12])]
        .height(Length::Shrink)
        .align_y(Alignment::Center);
    let mut body = button(content)
        .width(Fill)
        .padding(0)
        .style(widgets::list_item(false, row_idx));
    if addable {
        body = body.on_press(Action::AddChip { chip_id, code });
    }
    // Un-addable chips (folder full, or adding would break a folder limit)
    // read as disabled: a translucent wash in the pane's background colour
    // over the whole non-pressable row. The Stack takes the button's size,
    // so the wash covers it exactly.
    let row_el: Element<'a, Action> = if addable {
        body.into()
    } else {
        stack![
            body,
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color {
                        a: 0.6,
                        ..theme.palette().background
                    })),
                    ..Default::default()
                }),
        ]
        .into()
    };
    with_chip_tooltip(loaded, Some(chip_id), accent, row_el)
}

/// Small toggle button used for the REG / TAG columns in the folder editor
/// and the patch-card ON column: tinted in `on_color` when active, neutral
/// (greyed) when not. A `None` message renders it disabled (greyed,
/// unclickable) — for the folder REG/TAG toggles when the chip's MB won't
/// fit Regular/Tag memory, or the patch-card toggle when enabling would
/// blow the MB budget.
fn edit_toggle_maybe<'a>(
    label: &'static str,
    on: bool,
    on_color: iced::Color,
    msg: Option<Action>,
) -> Element<'a, Action> {
    let mut b = button(text(label).size(TEXT_CAPTION)).padding([4, 8]);
    if let Some(msg) = msg {
        b = b.on_press(msg);
    }
    if on {
        b.style(move |theme: &iced::Theme, status| crate::widgets::tinted_button(theme, status, on_color))
            .into()
    } else {
        b.style(crate::widgets::neutral).into()
    }
}

// `code = None` skips the code badge (Auto Battle Data slots
// have a chip id but no code). `show_count_cell` toggles the
// leading "N×" column — on for the folder's grouped mode, off
// for ABD.
fn chip_row<M: 'static>(
    loaded: &Loaded,
    chip_id: Option<usize>,
    code: Option<String>,
    g: &GroupedChip,
    show_count_cell: bool,
    chips_have_mb: bool,
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    let info = chip_id.and_then(|id| loaded.assets.chip(id));
    let chip_class = info.as_ref().map(|i| i.class());
    let dark = info.as_ref().map(|i| i.dark()).unwrap_or(false);
    let accent = class_accent(chip_class, dark);
    let is_empty_slot = chip_id.is_none();

    // Chip icon — in-game sprite at 28 px so it reads as a chip
    // graphic rather than a row decoration. Empty slots reserve the
    // same 28 px square so their rows match filled rows' height.
    let icon: Element<'static, M> = match chip_id.and_then(|id| loaded.chip_icons.get(id).cloned().flatten()) {
        Some(h) => Image::new(h)
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .filter_method(iced_image::FilterMethod::Nearest)
            .content_fit(ContentFit::Contain)
            .into(),
        None => Space::new()
            .width(Length::Fixed(28.0))
            .height(Length::Fixed(28.0))
            .into(),
    };

    // Element icon. Same 14→28 (2× native) scaling as the chip
    // icon so both sprites read at the same size and stay on an
    // integer multiple of their source — anything else makes
    // cosmic-text's resampler eat the pixel grid.
    let element_id = info.as_ref().map(|i| i.element());
    let element_icon: Element<'static, M> = element_id
        .and_then(|id| loaded.element_icons.get(&id).cloned())
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());

    let name_text = info
        .as_ref()
        .and_then(|i| i.name())
        .unwrap_or_else(|| "???".to_string());
    let power = info.as_ref().map(|i| i.attack_power()).unwrap_or(0);
    let mb = info.as_ref().map(|i| i.mb()).unwrap_or(0);

    // Name only — chip code lives in its own right-aligned
    // column below so every row's letters line up cleanly with
    // the element / power / MB stats.
    let title: Element<'static, M> = if is_empty_slot {
        text("—").size(TEXT_BODY).style(muted_text_style).into()
    } else {
        text(name_text).size(TEXT_BODY).into()
    };

    // REG / TAG indicators sit inline with the title so the
    // row stays single-line and the card height stops growing
    // with metadata.
    let mut indicator_row = row![].spacing(4).align_y(Alignment::Center);
    if g.is_regular {
        indicator_row = indicator_row.push(badge("REG", iced::Color::from_rgb8(0xff, 0x42, 0xa5)));
    }
    // Tag chips come in pairs (tag1 + tag2). For the chip list
    // it's the chip-IS-a-tag-chip status the user cares about,
    // not which slot — collapse both flags into a single "TAG".
    for _ in 0..(g.has_tag1 as usize + g.has_tag2 as usize) {
        indicator_row = indicator_row.push(badge("TAG", iced::Color::from_rgb8(0x29, 0xa1, 0x21)));
    }

    // Right-side stats: fixed-width right-aligned columns so the
    // numbers line up vertically across rows. Both inherit the theme's
    // text color — no hard-coded white/yellow that breaks on light.
    // `code.filter().map(...)` consumes the String into the
    // Text widget so the resulting Element is `'static` (a
    // `&String` borrow would tie the Element to this stack
    // frame). The filter drops empty codes so we don't render a
    // blank fixed-width slot.
    let code_text: Option<Element<'static, M>> = code.filter(|s| !s.is_empty()).map(|code| {
        container(text(code).size(TEXT_BODY).font(iced::Font::MONOSPACE))
            .width(Length::Fixed(22.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    });
    let power_text: Element<'static, M> =
        container(text(if power > 0 { format!("{power}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into();
    let mb_text: Element<'static, M> = if chips_have_mb {
        container(text(if mb > 0 { format!("{mb}MB") } else { String::new() }).size(TEXT_CAPTION))
            .width(Length::Fixed(50.0))
            .align_x(iced::alignment::Horizontal::Right)
            .into()
    } else {
        Space::new().width(Length::Fixed(0.0)).into()
    };

    // Count column on the left for grouped mode. Theme-aware text:
    // full strength for count > 1, muted for count == 1 (since "1×" is
    // visual noise) — both readable on light + dark.
    let mut r = row![].spacing(10).align_y(Alignment::Center);
    if show_count_cell {
        let count_is_one = g.count == 1;
        r = r.push(
            text(format!("{}×", g.count))
                .size(TEXT_BODY)
                .width(Length::Fixed(22.0))
                .style(move |theme: &iced::Theme| iced::widget::text::Style {
                    color: Some(if count_is_one {
                        muted_color(theme)
                    } else {
                        theme.palette().text
                    }),
                }),
        );
    }
    r = r
        .push(icon)
        .push(container(row![title, indicator_row].spacing(8).align_y(Alignment::Center)).width(Length::Fill))
        .push(element_icon);
    if let Some(code_text) = code_text {
        r = r.push(code_text);
    }
    r = r.push(power_text).push(mb_text);

    let card = card_wrap(r.padding([3, 12]).into(), accent, row_idx, is_first, is_last);
    // Hover tooltip with chip image preview + description.
    // Always rendered when the chip has either; the folder list
    // and Auto Battle Data both want this affordance, so it
    // lives at the bottom of chip_row instead of as a wrapper
    // the callers have to remember to use.
    let Some(id) = chip_id else {
        return card;
    };
    let description = loaded.assets.chip(id).and_then(|info| info.description());
    // Program advances show description only — no standalone chip image.
    let image_handle = if chip_class == Some(tango_dataview::rom::ChipClass::ProgramAdvance) {
        None
    } else {
        loaded.chip_images.get(id).cloned().flatten()
    };
    if description.is_none() && image_handle.is_none() {
        return card;
    }
    let mut tip = column![].spacing(6);
    if let Some((w, h, h_handle)) = image_handle {
        tip = tip.push(
            Image::new(h_handle)
                .width(Length::Fixed(w as f32 * 2.0))
                .height(Length::Fixed(h as f32 * 2.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain),
        );
    }
    if let Some(desc) = description {
        tip = tip.push(text(desc).size(TEXT_CAPTION));
    }
    tooltip(
        card,
        container(tip).padding(8).style(chip_tooltip_style(accent)),
        tooltip::Position::FollowCursor,
    )
    .gap(8)
    .into()
}

/// Tooltip chrome for chip hovers — same shape as
/// [`tooltip_style`] but takes the chip's class accent so
/// mega / giga / dark chips get a background that matches the
/// row's left-edge stripe. Standard chips (accent = None) fall
/// back to the default near-black tooltip.
fn chip_tooltip_style(accent: Option<iced::Color>) -> impl Fn(&iced::Theme) -> container::Style {
    move |_theme: &iced::Theme| {
        let bg = accent.unwrap_or_else(|| iced::Color::from_rgba8(0, 0, 0, 0.85));
        container::Style {
            background: Some(iced::Background::Color(bg)),
            text_color: Some(iced::Color::WHITE),
            border: iced::Border {
                radius: 4.0.into(),
                width: 1.0,
                color: iced::Color::from_rgba8(255, 255, 255, 0.2),
            },
            ..Default::default()
        }
    }
}

/// Wraps the inner row content with a 4 px colored stripe on the
/// left for mega/giga/dark chip class accents. The outer container
/// carries the standard zebra-row style so every chip row matches
/// the patch-card / ABD / settings-bindings tables visually; the
/// accent strip sits as a sibling element on the left and paints
/// over the zebra wash where present. Rows without an accent
/// reserve the same 6 px gutter so columns line up across rows.
fn card_wrap<M: 'static>(
    inner: Element<'static, M>,
    accent: Option<iced::Color>,
    row_idx: usize,
    is_first: bool,
    is_last: bool,
) -> Element<'static, M> {
    // Match the pane's `radius: 4.0` on edge rows so the strip's solid
    // accent and the zebra wash don't paint into the pane's rounded
    // corners. The strip only ever touches the left edge, so just the
    // top-left / bottom-left corners need rounding there.
    let r = 4.0_f32;
    let mut strip_radius = iced::border::Radius::new(0.0);
    if is_first {
        strip_radius = strip_radius.top_left(r);
    }
    if is_last {
        strip_radius = strip_radius.bottom_left(r);
    }
    let mut outer_radius = iced::border::Radius::new(0.0);
    if is_first {
        outer_radius = outer_radius.top(r);
    }
    if is_last {
        outer_radius = outer_radius.bottom(r);
    }
    let strip: Element<'static, M> = container(iced::widget::Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .style(move |_theme: &iced::Theme| container::Style {
            background: accent.map(iced::Background::Color),
            border: iced::Border {
                radius: strip_radius,
                ..Default::default()
            },
            ..Default::default()
        })
        .into();
    let body: Element<'static, M> = container(inner).width(Fill).into();
    container(row![strip, body].height(Length::Shrink))
        .width(Fill)
        .style(move |theme: &iced::Theme| {
            let mut s = crate::widgets::zebra_row(row_idx)(theme);
            s.border.radius = outer_radius;
            s
        })
        .into()
}

/// Accent color for the left edge of a chip row. None = no accent (the
/// row reads as a default chip with no class adornment).
fn class_accent(class: Option<tango_dataview::rom::ChipClass>, dark: bool) -> Option<iced::Color> {
    if dark {
        return Some(iced::Color::from_rgb8(0x4a, 0x55, 0x82));
    }
    match class {
        Some(tango_dataview::rom::ChipClass::Mega) => Some(iced::Color::from_rgb8(0x52, 0x84, 0x9c)),
        Some(tango_dataview::rom::ChipClass::Giga) => Some(iced::Color::from_rgb8(0xc4, 0x52, 0x84)),
        _ => None,
    }
}

fn badge<M: 'static>(label: &'static str, color: iced::Color) -> Element<'static, M> {
    container(text(label).size(10).color(iced::Color::WHITE))
        .padding([1, 4])
        .style(move |_| container::Style {
            background: Some(iced::Background::Color(color)),
            border: iced::Border {
                radius: 3.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

fn colored_badge<M: 'static>(label: String, bg: iced::Color, text_color: iced::Color) -> Element<'static, M> {
    // Same dimensions as the NaviCust parts badges so the
    // patch-card effect chips and the NCP parts read as
    // family — chunkier than a chrome chip but smaller than a
    // CTA button.
    colored_badge_sized(label, bg, text_color, TEXT_BODY, [3.0, 8.0])
}

/// Variant that lets callers (NCP parts list) pick a larger text size
/// when the badge is being used as primary content rather than chrome.
fn colored_badge_sized<M: 'static>(
    label: String,
    bg: iced::Color,
    text_color: iced::Color,
    size: f32,
    padding: [f32; 2],
) -> Element<'static, M> {
    container(text(label).size(size).color(text_color))
        .padding(padding)
        .style(move |_theme: &iced::Theme| container::Style {
            background: Some(iced::Background::Color(bg)),
            border: iced::Border {
                radius: 6.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into()
}

/// Solid + plus colors for an NCP color, matching the navicust render.
fn ncp_colors(color: NavicustPartColor) -> (iced::Color, iced::Color) {
    use NavicustPartColor as N;
    match color {
        N::Red => (
            iced::Color::from_rgb8(0xde, 0x10, 0x00),
            iced::Color::from_rgb8(0xbd, 0x00, 0x00),
        ),
        N::Pink => (
            iced::Color::from_rgb8(0xde, 0x8c, 0xc6),
            iced::Color::from_rgb8(0xbd, 0x6b, 0xa5),
        ),
        N::Yellow => (
            iced::Color::from_rgb8(0xde, 0xde, 0x00),
            iced::Color::from_rgb8(0xbd, 0xbd, 0x00),
        ),
        N::Green => (
            iced::Color::from_rgb8(0x18, 0xc6, 0x00),
            iced::Color::from_rgb8(0x00, 0xa5, 0x00),
        ),
        N::Blue => (
            iced::Color::from_rgb8(0x29, 0x84, 0xde),
            iced::Color::from_rgb8(0x08, 0x60, 0xb8),
        ),
        N::White => (
            iced::Color::from_rgb8(0xde, 0xde, 0xde),
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
        ),
        N::Orange => (
            iced::Color::from_rgb8(0xde, 0x7b, 0x00),
            iced::Color::from_rgb8(0xbd, 0x5a, 0x00),
        ),
        N::Purple => (
            iced::Color::from_rgb8(0x94, 0x00, 0xce),
            iced::Color::from_rgb8(0x73, 0x00, 0xad),
        ),
        N::Gray => (
            iced::Color::from_rgb8(0x84, 0x84, 0x84),
            iced::Color::from_rgb8(0x63, 0x63, 0x63),
        ),
    }
}

fn effect_badge<M: 'static>(e: &tango_dataview::rom::PatchCard56Effect, enabled: bool) -> Element<'static, M> {
    let name = e.name.clone().unwrap_or_else(|| "???".to_string());
    let bg = if enabled {
        if e.is_debuff {
            iced::Color::from_rgb8(0xb5, 0x5a, 0xde)
        } else {
            iced::Color::from_rgb8(0xff, 0xbd, 0x18)
        }
    } else {
        iced::Color::from_rgb8(0xbd, 0xbd, 0xbd)
    };
    colored_badge(name, bg, iced::Color::BLACK)
}

// muted_color / muted_text_style / success_text_style /
// danger_text_style now live in `crate::widgets`. Kept here as
// nothing — every call site outside this module reaches the
// widgets module directly.

fn tooltip_style(_theme: &iced::Theme) -> container::Style {
    container::Style {
        background: Some(iced::Background::Color(iced::Color::from_rgba8(0, 0, 0, 0.85))),
        text_color: Some(iced::Color::WHITE),
        border: iced::Border {
            radius: 4.0.into(),
            width: 1.0,
            color: iced::Color::from_rgba8(255, 255, 255, 0.2),
        },
        ..Default::default()
    }
}

// ---------- Navi ----------

fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(navi_view) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    match navi_view {
        tango_dataview::save::NaviView::LinkNavi(v) => {
            let navi_id = v.navi();
            let name = assets
                .navi(navi_id)
                .and_then(|n| n.name())
                .unwrap_or_else(|| format!("Navi #{navi_id}"));
            // Plate/glow tint: the emblem's own signature color, with a
            // neutral slate fallback for monochrome emblems.
            let accent = loaded
                .navi_accents
                .get(&navi_id)
                .copied()
                .unwrap_or(iced::Color::from_rgb8(0x6b, 0x7a, 0x99));

            // Emblem at an integer multiple of its 15px crop so the
            // nearest-neighbor upscale lands on even pixels.
            let emblem: Element<'static, M> = loaded
                .navi_emblems
                .get(&navi_id)
                .cloned()
                .map(|h| {
                    Image::new(h)
                        .width(Length::Fixed(90.0))
                        .height(Length::Fixed(90.0))
                        .filter_method(iced_image::FilterMethod::Nearest)
                        .content_fit(ContentFit::Contain)
                        .into()
                })
                .unwrap_or_else(|| {
                    Space::new()
                        .width(Length::Fixed(90.0))
                        .height(Length::Fixed(90.0))
                        .into()
                });

            // Circular plate behind the emblem: accent-tinted fill, a
            // ring a shade brighter, and an accent glow lifting it off
            // the pane.
            let plate: Element<'static, M> = container(emblem)
                .width(Length::Fixed(140.0))
                .height(Length::Fixed(140.0))
                .align_x(Alignment::Center)
                .align_y(Alignment::Center)
                .style(move |theme: &iced::Theme| {
                    let bg = theme.palette().background;
                    container::Style {
                        background: Some(iced::Background::Color(crate::widgets::mix(bg, accent, 0.22))),
                        border: iced::Border {
                            radius: 70.0.into(),
                            width: 2.0,
                            color: iced::Color { a: 0.8, ..accent },
                        },
                        shadow: iced::Shadow {
                            color: iced::Color { a: 0.45, ..accent },
                            offset: iced::Vector::new(0.0, 0.0),
                            blur_radius: 26.0,
                        },
                        ..Default::default()
                    }
                })
                .into();

            let card = column![
                plate,
                column![
                    text(name).size(TEXT_DISPLAY),
                    text(t!(lang, "navi-link-navi"))
                        .size(TEXT_CAPTION)
                        .style(muted_text_style),
                ]
                .spacing(2)
                .align_x(Alignment::Center),
            ]
            .spacing(16)
            .align_x(Alignment::Center);

            // The pane itself picks up a whisper of the accent, fading
            // back to the standard plate color toward the bottom.
            container(card)
                .width(Fill)
                .align_x(Alignment::Center)
                .padding([28.0, crate::style::PANE_PADDING])
                .style(move |theme: &iced::Theme| {
                    let mut s = crate::widgets::pane(theme);
                    if let Some(iced::Background::Color(plate_color)) = s.background {
                        // Stop 0 sits at the bottom for a 0-radian linear
                        // gradient, so the accent goes on stop 1 — the
                        // tint halos the plate at the top of the card.
                        s.background = Some(iced::Background::Gradient(iced::Gradient::Linear(
                            iced::gradient::Linear::new(0.0)
                                .add_stop(0.0, plate_color)
                                .add_stop(1.0, crate::widgets::mix(plate_color, accent, 0.10)),
                        )));
                    }
                    s
                })
                .into()
        }
        tango_dataview::save::NaviView::Navicust(v) => render_navicust(lang, loaded, v.as_ref()),
    }
}

/// The installed-parts badge strip shown under the grid: two columns
/// (solid parts | plus parts), each badge colored by its NCP color with a
/// description tooltip. Reads the live view, so it reflects staged edits.
/// `None` when nothing is installed. Shared by the read-only Navi view and
/// the editor's grid pane.
fn navicust_installed_parts<M: 'static>(
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Option<Element<'static, M>> {
    let assets = loaded.assets.as_ref();
    let mut solid_col = column![].spacing(4);
    let mut plus_col = column![].spacing(4);
    let mut any = false;
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let description = info.description();
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };
        let badge_el = colored_badge_sized(part_name, bg, iced::Color::BLACK, TEXT_BODY, [3.0, 8.0]);
        let badge_el: Element<'static, M> = if let Some(desc) = description {
            tooltip(
                badge_el,
                container(text(desc).size(TEXT_CAPTION)).padding(8).style(tooltip_style),
                tooltip::Position::FollowCursor,
            )
            .gap(8)
            .into()
        } else {
            badge_el
        };
        any = true;
        if is_solid {
            solid_col = solid_col.push(badge_el);
        } else {
            plus_col = plus_col.push(badge_el);
        }
    }
    any.then(|| row![solid_col, plus_col].spacing(12).into())
}

/// The viewer's installed-parts panel, shown beside the grid: one row
/// per part with its shape thumbnail (bounding-box crop, native pixel
/// scale), its name badge, and its description inline. Solid parts
/// first, then plus parts, keeping slot order within each group — the
/// same ordering the badge strip used. `None` when nothing is
/// installed.
fn navicust_parts_panel<M: 'static>(
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Option<Element<'static, M>> {
    let assets = loaded.assets.as_ref();
    let mut solid_rows: Vec<Element<'static, M>> = vec![];
    let mut plus_rows: Vec<Element<'static, M>> = vec![];
    for i in 0..v.count() {
        let Some(part) = v.navicust_part(i) else { continue };
        let Some(info) = assets.navicust_part(part.id) else {
            continue;
        };
        let part_name = info.name().unwrap_or_else(|| format!("#{}", part.id));
        let is_solid = info.is_solid();
        let (solid_color, plus_color) = info.color().map(ncp_colors).unwrap_or((
            iced::Color::from_rgb8(0xbd, 0xbd, 0xbd),
            iced::Color::from_rgb8(0x88, 0x88, 0x88),
        ));
        let bg = if is_solid { solid_color } else { plus_color };

        // Shape thumb at its native baked scale (8 px per cell), centered
        // in a fixed box so the name column lines up across rows. The
        // largest shapes (5+ cells on a side) scale down to fit.
        const THUMB_BOX: f32 = 40.0;
        let thumb: Element<'static, M> = loaded
            .navicust_part_icons_cropped
            .get(part.id)
            .and_then(|o| o.clone())
            .map(|(w, h, handle)| {
                Image::new(handle)
                    .width(Length::Fixed((w as f32).min(THUMB_BOX)))
                    .height(Length::Fixed((h as f32).min(THUMB_BOX)))
                    .filter_method(iced_image::FilterMethod::Nearest)
                    .content_fit(ContentFit::Contain)
                    .into()
            })
            .unwrap_or_else(|| Space::new().into());
        let thumb_box: Element<'static, M> = container(thumb)
            .width(Length::Fixed(THUMB_BOX))
            .height(Length::Fixed(THUMB_BOX))
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
            .into();

        let mut name_col = column![colored_badge_sized::<M>(
            part_name,
            bg,
            iced::Color::BLACK,
            TEXT_BODY,
            [3.0, 8.0]
        )]
        .spacing(3)
        .align_x(Alignment::Start);
        if let Some(desc) = info.description() {
            // ROM descriptions keep the game's own textbox line breaks —
            // they're authored to wrap there, so the text shrink-wraps to
            // its natural width.
            name_col = name_col.push(text(desc).size(TEXT_CAPTION).style(muted_text_style));
        }

        let row_el: Element<'static, M> = row![thumb_box, name_col].spacing(10).align_y(Alignment::Center).into();
        if is_solid {
            solid_rows.push(row_el);
        } else {
            plus_rows.push(row_el);
        }
    }
    if solid_rows.is_empty() && plus_rows.is_empty() {
        return None;
    }
    // Two top-aligned columns, like the badge strip this replaces:
    // solid parts on the left, plus parts on the right.
    let mut solid_col = column![].spacing(6);
    for r in solid_rows {
        solid_col = solid_col.push(r);
    }
    let mut plus_col = column![].spacing(6);
    for r in plus_rows {
        plus_col = plus_col.push(r);
    }
    Some(row![solid_col, plus_col].spacing(20).into())
}

fn render_navicust<M: 'static>(
    lang: &LanguageIdentifier,
    loaded: &Loaded,
    v: &dyn tango_dataview::save::NavicustView,
) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();
    let [cols, rows_n] = v.size();

    // Big rendered grid (tiny-skia, cached at load time). Scale down to
    // ~440 px wide if larger (5×5 grids render around 360 wide native;
    // bigger grids get scaled). Wrapped in mouse_area so hovering over
    // Per-cell tooltip overlay: render the image as one layer of a
    // Stack and a column-of-rows-of-cell-sized empty widgets as the
    // second layer. Each cell that's covered by an installed part gets
    // its own tooltip wrapper, so iced's tooltip widget manages the
    // hover state internally — no NavicustHover message round-trip
    // needed.
    let grid_el: Element<'static, M> = match loaded.navicust_render.as_ref() {
        Some(nc) => {
            // `source_w/h` are now in DISPLAY coords (see selection.rs);
            // the underlying Handle is 2× that, and iced linear-
            // downsamples it for HiDPI crispness.
            let dw = nc.source_w as f32;
            let dh = nc.source_h as f32;
            let body_x = nc.body_origin_x;
            let body_y = nc.body_origin_y;
            let cell_size = nc.cell_size;
            let g_cols = nc.cols;
            let g_rows = nc.rows;

            let image: Element<'static, M> = Image::new(nc.handle.clone())
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh))
                // Handle is 2× source for HiDPI (see selection.rs
                // build_navicust_render). On a 2× display iced
                // presents at native device pixels = perfect; on
                // a 1× display iced linear-downsamples 2:1.
                .filter_method(iced_image::FilterMethod::Linear)
                .content_fit(ContentFit::Contain)
                .into();

            // Build the overlay: a fixed-size column of fixed-size rows
            // matching the grid. Each cell is either a no-op Space or
            // a tooltip-wrapped Space carrying the part's name + desc.
            let mut overlay_col = column![Space::new().height(Length::Fixed(body_y))];
            for row_idx in 0..g_rows {
                let mut cell_row = row![Space::new().width(Length::Fixed(body_x))];
                for col_idx in 0..g_cols {
                    let cell_idx = nc.cell_part_idx.get(row_idx * g_cols + col_idx).copied().flatten();
                    let info = cell_idx
                        .and_then(|pi| v.navicust_part(pi))
                        .and_then(|p| assets.navicust_part(p.id));
                    let cell: Element<'static, M> = if let Some(info) = info {
                        let name = info.name().unwrap_or_else(|| "?".to_string());
                        let mut tip_col = column![text(name).size(TEXT_BODY)].spacing(2);
                        if let Some(desc) = info.description() {
                            tip_col = tip_col.push(text(desc).size(TEXT_CAPTION));
                        }
                        let tip = container(tip_col).padding(8).style(tooltip_style);
                        let space = Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size));
                        tooltip(space, tip, tooltip::Position::FollowCursor).gap(12).into()
                    } else {
                        Space::new()
                            .width(Length::Fixed(cell_size))
                            .height(Length::Fixed(cell_size))
                            .into()
                    };
                    cell_row = cell_row.push(cell);
                }
                overlay_col = overlay_col.push(cell_row);
            }

            // Top layer: outline the block under the cursor. It never
            // captures events, so the tooltip layer beneath still fires.
            let hover: Element<'static, M> = crate::navicust_editor::HoverOutline {
                cols: g_cols,
                rows: g_rows,
                origin_x: body_x,
                origin_y: body_y,
                cell: cell_size,
                width: dw,
                height: dh,
                occupancy: nc.cell_part_idx.clone(),
            }
            .view::<M>();

            let stacked = stack![image, overlay_col, hover]
                .width(Length::Fixed(dw))
                .height(Length::Fixed(dh));
            // Flush against the pane — no shadow, no extra padding.
            // The image's corners are pre-masked in selection.rs to
            // match the pane's rounded corners. No Fill / centering
            // here either: that would propagate up and stretch the
            // whole pane across the tab.
            stacked.into()
        }
        None => text(t!(lang, "navicust-grid-size", cols = cols as i64, rows = rows_n as i64))
            .size(TEXT_CAPTION)
            .into(),
    };

    // Single pane sized to its contents — no "(none installed)"
    // fallback; an empty navicust shows just the rounded image with
    // pane padding around it. The installed-parts panel sits beside
    // the grid (the tab is much wider than the grid), top-aligned
    // with the grid body (the small padding eats the gap the image's
    // baked-in margin leaves above the color bar). No Fill anywhere:
    // that would stretch the pane across the tab.
    let mut content = row![grid_el].spacing(20).align_y(Alignment::Start);
    if let Some(parts) = navicust_parts_panel::<M>(loaded, v) {
        content = content.push(container(parts).padding([14.0, 0.0]));
    }

    let _ = (cols, rows_n);
    container(content)
        .padding(crate::style::PANE_PADDING)
        .style(crate::widgets::pane)
        .into()
}

// ---------- Patch cards ----------

fn render_patch_cards<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    let mut list = column![].spacing(3).padding(0);
    match view {
        tango_dataview::save::PatchCardsView::PatchCard56s(v) => {
            for i in 0..v.count() {
                let Some(card) = v.patch_card(i) else { continue };
                let info = assets.patch_card56(card.id);
                let name = info
                    .as_ref()
                    .and_then(|c| c.name())
                    .unwrap_or_else(|| format!("#{}", card.id));
                let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
                let [name_cell, ability_cell, bug_cell] =
                    patch_card56_cells::<M>(loaded, &name, mb, card.enabled, card.id);

                let row = row![
                    text(format!("{:>2}", i + 1))
                        .size(TEXT_CAPTION)
                        .width(Length::Fixed(24.0)),
                    name_cell,
                    ability_cell,
                    bug_cell,
                ]
                .spacing(8)
                .align_y(Alignment::Start);
                list = list.push(
                    container(row)
                        .padding(style::ROW_PADDING)
                        .style(crate::widgets::zebra_row(i)),
                );
            }
        }
        tango_dataview::save::PatchCardsView::PatchCard4s(v) => {
            // Mirror the BN4 editor's slot form: a slot badge + the card's
            // "name — effect" line, with the bug (if any) in purple beneath.
            for (slot, slot_label) in PATCH_CARD4_SLOT_LABELS.iter().enumerate() {
                let badge: Element<'static, M> =
                    container(text(*slot_label).size(TEXT_BODY).font(iced::Font::MONOSPACE))
                        .width(Length::Fixed(34.0))
                        .align_x(iced::alignment::Horizontal::Center)
                        .into();
                let cell: Element<'static, M> = match v.patch_card(slot) {
                    Some(card) => {
                        let info = assets.patch_card4(card.id);
                        let name = info
                            .as_ref()
                            .and_then(|i| i.name())
                            .unwrap_or_else(|| format!("#{}", card.id));
                        let effect = info.as_ref().map(|i| i.effect());
                        // 3-digit catalog number, then the "name — effect"
                        // line (name struck + everything muted when off).
                        let number = text(format!("{:03}", card.id))
                            .size(TEXT_BODY)
                            .font(iced::Font::MONOSPACE)
                            .style(muted_text_style);
                        let label = patch_card_name(
                            match effect {
                                Some(effect) => format!("{name} — {}", patch_card4_effect_label(effect)),
                                None => name,
                            },
                            card.enabled,
                        );
                        let mut col = column![row![badge, number, container(label).width(Length::Fill)]
                            .spacing(8)
                            .align_y(Alignment::Center)]
                        .spacing(2);
                        if let Some(bug) = info.as_ref().and_then(|i| patch_card4_bugs_label(i.bugs())) {
                            col = col.push(
                                row![
                                    Space::new().width(Length::Fixed(44.0)),
                                    text(bug)
                                        .size(TEXT_BODY)
                                        .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
                                ]
                                .spacing(0),
                            );
                        }
                        col.into()
                    }
                    None => row![
                        badge,
                        text(t!(lang, "patch-card4-none"))
                            .size(TEXT_BODY)
                            .style(muted_text_style)
                            .width(Length::Fill),
                    ]
                    .spacing(10)
                    .align_y(Alignment::Center)
                    .into(),
                };
                list = list.push(
                    container(cell)
                        .width(Fill)
                        .padding([8, 10])
                        .style(crate::widgets::zebra_row(slot)),
                );
            }
        }
    }

    container(list).width(Fill).style(crate::widgets::pane).into()
}

/// Every PatchCard56 the ROM defines, as `(id, name, mb)`, in `sort`
/// order. The caller applies the name filter and excludes ids already in
/// the registered list. Ties fall back to id for a stable order.
fn sorted_patch_card56_library(loaded: &Loaded, sort: PatchCard56Sort) -> Vec<(usize, String, u8)> {
    let assets = loaded.assets.as_ref();
    let mut rows: Vec<(usize, String, u8)> = Vec::new();
    for id in 0..assets.num_patch_card56s() {
        let Some(info) = assets.patch_card56(id) else { continue };
        let name = info.name().unwrap_or_else(|| format!("#{id}"));
        rows.push((id, name, info.mb()));
    }
    match sort {
        PatchCard56Sort::Id => {}
        PatchCard56Sort::Name => rows.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0))),
        PatchCard56Sort::Mb => rows.sort_by(|a, b| a.2.cmp(&b.2).then(a.0.cmp(&b.0))),
    }
    rows
}

/// A patch-card name as an Element. Built as rich text so a disabled card's
/// name can be struck through (and muted) to read as inactive at a glance —
/// iced's strikethrough lives on rich-text spans. `on_link_click(never)`
/// pins the span's link type; these spans are never links.
fn patch_card_name<'a, M: 'a>(name: String, enabled: bool) -> Element<'a, M> {
    let mut el = iced::widget::rich_text([iced::widget::text::Span::new(name).strikethrough(!enabled)])
        .on_link_click(iced::never)
        .size(TEXT_BODY);
    if !enabled {
        el = el.style(muted_text_style);
    }
    el.into()
}

/// The viewer-style cells for a patch card: `[name+MB, abilities, bugs]`,
/// matching [`render_patch_cards`]'s column layout exactly (name with MB
/// stacked beneath, then a fixed-width ability column and bug column, each
/// a vertical stack of [`effect_badge`]s). Greyed when `enabled` is false.
/// Callers wrap these with a leading cell (index / add button) and, for the
/// registered list, trailing edit controls.
fn patch_card56_cells<'a, M: 'static>(
    loaded: &Loaded,
    name: &str,
    mb: u8,
    enabled: bool,
    id: usize,
) -> [Element<'a, M>; 3] {
    let effects = loaded.assets.patch_card56(id).map(|c| c.effects()).unwrap_or_default();
    let name_text = patch_card_name(name.to_string(), enabled);
    let name_col = column![name_text, text(format!("{mb}MB")).size(10).style(muted_text_style)].spacing(2);
    let mut ability_col = column![].spacing(2);
    for e in effects.iter().filter(|e| e.is_ability) {
        ability_col = ability_col.push(effect_badge::<M>(e, enabled));
    }
    let mut bug_col = column![].spacing(2);
    for e in effects.iter().filter(|e| !e.is_ability) {
        bug_col = bug_col.push(effect_badge::<M>(e, enabled));
    }
    [
        container(name_col).width(Length::Fill).into(),
        // Fixed-width ability / bug columns, matching the read-only viewer.
        container(ability_col).width(Length::Fixed(180.0)).into(),
        container(bug_col).width(Length::Fixed(180.0)).into(),
    ]
}

/// One registered patch card, laid out like a [`render_patch_cards`] row
/// (index · name+MB · abilities · bugs) with an enable toggle and an ✕
/// remove button appended. The name dims while the card is disabled.
/// `can_enable` is whether enabling this (currently disabled) card would
/// still fit the MB budget; it gates the ON toggle.
fn patch_card56_list_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    card: tango_dataview::save::PatchCard,
    can_enable: bool,
) -> Element<'a, Action> {
    use crate::widgets;
    use lucide_icons::Icon;
    let info = loaded.assets.patch_card56(card.id);
    let name = info
        .as_ref()
        .and_then(|c| c.name())
        .unwrap_or_else(|| format!("#{}", card.id));
    let mb = info.as_ref().map(|c| c.mb()).unwrap_or(0);
    let [name_cell, ability_cell, bug_cell] = patch_card56_cells(loaded, &name, mb, card.enabled, card.id);

    // Green "ON" toggle (matches the folder editor's TAG tint), then the
    // ✕ that backs the card out to the library. The toggle is disabled
    // when the card is off and enabling it would exceed the MB budget (an
    // already-on card can always be turned off).
    let toggle_msg = (card.enabled || can_enable).then_some(Action::TogglePatchCard56 { slot });
    let toggle = edit_toggle_maybe("ON", card.enabled, iced::Color::from_rgb8(0x29, 0xa1, 0x21), toggle_msg);
    let remove = button(Icon::X.widget().size(TEXT_BODY))
        .padding([3, 8])
        .style(widgets::neutral)
        .on_press(Action::RemovePatchCard56 { slot });

    let row = row![
        drag_handle(),
        text(format!("{:>2}", slot + 1))
            .size(TEXT_CAPTION)
            .width(Length::Fixed(24.0)),
        name_cell,
        ability_cell,
        bug_cell,
        toggle,
        remove,
    ]
    .spacing(8)
    .align_y(Alignment::Start);
    // Left padding trimmed (vs the usual 10) so the drag handle sits flush in
    // the gutter, matching the folder editor's grip.
    container(row)
        .padding(iced::Padding {
            top: 6.0,
            right: 10.0,
            bottom: 6.0,
            left: 6.0,
        })
        .style(crate::widgets::zebra_row(slot))
        .into()
}

/// One library card, laid out like a [`render_patch_cards`] row (index ·
/// name+MB · abilities · bugs). The whole row is a click-to-add button
/// (the palette affordance) that registers the card; effects show
/// enabled, since adding a card enables it when it fits. Disabled
/// (unclickable) when the list is full.
fn patch_card56_library_row<'a>(
    loaded: &'a Loaded,
    id: usize,
    name: String,
    mb: u8,
    row_idx: usize,
    list_full: bool,
) -> Element<'a, Action> {
    let [name_cell, ability_cell, bug_cell] = patch_card56_cells(loaded, &name, mb, true, id);

    let row = row![name_cell, ability_cell, bug_cell]
        .spacing(8)
        .align_y(Alignment::Start);
    // The entire row is the add control: clicking anywhere registers the
    // card. `list_item` paints the zebra base + hover highlight, so it
    // doubles as the palette's "click me" affordance.
    let mut b = button(row)
        .width(Fill)
        .padding(style::ROW_PADDING)
        .style(crate::widgets::list_item(false, row_idx));
    if !list_full {
        b = b.on_press(Action::AddPatchCard56 { id });
    }
    b.into()
}

/// Dispatch the Patch Cards tab's editor to the right implementation:
/// BN5/BN6 (PatchCard56s) is a variable MB-budgeted list; BN4
/// (PatchCard4s) is six fixed catalog slots. They're wholly separate
/// editors — the only thing they share is the tab.
fn render_patch_cards_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    match loaded.save.view_patch_cards() {
        Some(tango_dataview::save::PatchCardsView::PatchCard56s(_)) => render_patch_card56s_edit(lang, loaded, state),
        Some(tango_dataview::save::PatchCardsView::PatchCard4s(_)) => render_patch_card4s_edit(lang, loaded, state),
        None => placeholder(t!(lang, "save-empty")),
    }
}

/// The BN5/BN6 patch-card editor: a two-pane layout (registered list left,
/// card library right) whose rows match the read-only viewer (index ·
/// name+MB stacked · ability column · bug column), with edit controls
/// appended — an enable toggle + remove on the list, an add button on the
/// library. Edits stage live in the loaded save and are written to disk
/// only on Save.
fn render_patch_card56s_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(tango_dataview::save::PatchCardsView::PatchCard56s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let count = v.count();
    let max = loaded.assets.num_patch_card56s();

    // ----- Left pane: the registered list -----
    // MB of each card (0 for the "no card" id / unknown), so the budget
    // and per-row gating are computed from one source.
    let card_mb = |id: usize| loaded.assets.patch_card56(id).map(|c| c.mb() as u32).unwrap_or(0);
    let cards: Vec<(usize, tango_dataview::save::PatchCard)> = (0..count)
        .filter_map(|slot| v.patch_card(slot).map(|c| (slot, c)))
        .collect();
    let in_list: std::collections::HashSet<usize> = cards.iter().map(|(_, c)| c.id).collect();
    let enabled_mb: u32 = cards
        .iter()
        .filter(|(_, c)| c.enabled)
        .map(|(_, c)| card_mb(c.id))
        .sum();

    let mut list_rows: Vec<Element<'a, Action>> = Vec::with_capacity(cards.len());
    for (slot, card) in &cards {
        // A disabled card can be enabled only if it still fits the budget;
        // an enabled card can always be turned off.
        let can_enable = enabled_mb + card_mb(card.id) <= MAX_PATCH_CARD56_MB;
        list_rows.push(patch_card56_list_row(loaded, *slot, card.clone(), can_enable));
    }
    // Draggable list: grab a card row and drop it to reorder the registered
    // order (dense list, so any drop is a valid ordered move).
    let list_col = sweeten::widget::Column::from_vec(list_rows)
        .width(Fill)
        .spacing(3)
        .style(reorder_drag_style)
        .on_drag(Action::ReorderPatchCard56s);
    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        Action::ClearPatchCard56s,
        [5.0, 10.0],
        widgets::danger_button,
    );
    // MB total turns red if it somehow exceeds the limit (e.g. a save
    // imported over-budget); the editor itself never lets it go over.
    let mb_text = limit_caption(
        t!(
            lang,
            "patch-card-edit-mb",
            mb = enabled_mb as i64,
            limit = MAX_PATCH_CARD56_MB
        ),
        enabled_mb > MAX_PATCH_CARD56_MB,
    );
    let list_header = container(
        row![
            text(t!(lang, "save-tab-patch-cards")).size(TEXT_BODY),
            text(t!(lang, "patch-card-edit-count", count = count as i64))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            mb_text,
            Space::new().width(Fill),
            clear_all,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING);
    let list_pane = editor_pane(list_header, list_col);

    // ----- Right pane: the card library -----
    let filter = edit.patch_card56_filter.to_lowercase();
    let list_full = count >= max;
    let mut lib_col = column![].spacing(3).padding(0);
    let mut shown = 0usize;
    for (id, name, mb) in sorted_patch_card56_library(loaded, state.patch_card56_sort) {
        if in_list.contains(&id) {
            continue;
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        lib_col = lib_col.push(patch_card56_library_row(loaded, id, name, mb, shown, list_full));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "patch-card-edit-search"),
        &edit.patch_card56_filter,
        Action::PatchCard56FilterChanged,
        &PatchCard56Sort::ALL,
        state.patch_card56_sort,
        PatchCard56Sort::label,
        Action::PatchCard56SortChanged,
    );
    editor_panes(list_pane, editor_pane(lib_header, lib_col))
}

// ---------- BN4 patch cards ----------

/// BN4 catalog-slot labels (the "0A"–"0F" the game shows). A BN4 patch
/// card belongs to exactly one of these six slots, and a slot holds at most
/// one card — so the editor is a per-slot picker, not the BN5/BN6 list.
const PATCH_CARD4_SLOT_LABELS: [&str; 6] = ["0A", "0B", "0C", "0D", "0E", "0F"];

/// One slot's row in the BN4 editor: the slot label, a dropdown of every
/// card that belongs to this slot (plus "None" to empty it), an ON/off
/// toggle for the installed card, and — since a card's downside isn't in
/// the dropdown label — its bug line in purple underneath.
fn patch_card4_slot_row<'a>(
    loaded: &'a Loaded,
    slot: usize,
    installed: Option<tango_dataview::save::PatchCard>,
    choices: Vec<PatchCard4Choice>,
) -> Element<'a, Action> {
    use crate::widgets;
    let badge = container(
        text(PATCH_CARD4_SLOT_LABELS[slot])
            .size(TEXT_BODY)
            .font(iced::Font::MONOSPACE),
    )
    .width(Length::Fixed(34.0))
    .align_x(iced::alignment::Horizontal::Center);

    let selected_id = installed.as_ref().map(|c| c.id);
    let selected = choices.iter().find(|c| c.id == selected_id).cloned();
    let picker = pick_list(choices, selected, move |c: PatchCard4Choice| match c.id {
        Some(id) => Action::AddPatchCard4 { id },
        None => Action::RemovePatchCard4 { slot },
    })
    .width(Fill)
    .padding(style::CONTROL_PADDING)
    .text_size(TEXT_BODY)
    .style(widgets::chunky_pick_list);

    // The ON toggle shows on every row (so the column stays aligned); an
    // empty slot has nothing to enable, so it renders disabled (greyed,
    // unclickable). Green matches the other editors' "on" tint.
    let toggle = edit_toggle_maybe(
        "ON",
        installed.as_ref().is_some_and(|c| c.enabled),
        iced::Color::from_rgb8(0x29, 0xa1, 0x21),
        installed.as_ref().map(|_| Action::TogglePatchCard4 { slot }),
    );
    let top = row![badge, picker, toggle].spacing(10).align_y(Alignment::Center);

    let mut cell = column![top].spacing(2);
    // Bug line for the installed card, aligned under the dropdown (past the
    // slot badge). The effect is already in the dropdown label; the bug is
    // the downside the user should still see at a glance.
    if let Some(bug) = installed
        .as_ref()
        .and_then(|c| loaded.assets.patch_card4(c.id))
        .and_then(|i| patch_card4_bugs_label(i.bugs()))
    {
        cell = cell.push(
            row![
                Space::new().width(Length::Fixed(44.0)),
                text(bug)
                    .size(TEXT_BODY)
                    .color(iced::Color::from_rgb8(0xb5, 0x5a, 0xde)),
            ]
            .spacing(0),
        );
    }
    container(cell)
        .width(Fill)
        .padding([8, 10])
        .style(crate::widgets::zebra_row(slot))
        .into()
}

/// The BN4 patch-card editor: the six catalog slots (0A–0F) as a single
/// form. Each slot has a dropdown of the cards that belong to it (plus
/// "None"), so the model is "pick one card per slot" — matching the in-game
/// Mod Card screen — rather than the BN5/BN6 collection-from-a-library.
/// There's no MB budget. Edits stage live in the loaded save and are
/// written to disk only on Save.
fn render_patch_card4s_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    if state.editing.is_none() {
        return placeholder(t!(lang, "save-empty"));
    }
    let Some(tango_dataview::save::PatchCardsView::PatchCard4s(v)) = loaded.save.view_patch_cards() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();

    // Bucket every card id by the slot it belongs to (one pass), so each
    // slot's dropdown lists only its own cards.
    let mut by_slot: [Vec<usize>; PATCH_CARD4_SLOT_LABELS.len()] = std::array::from_fn(|_| Vec::new());
    for id in 0..assets.num_patch_card4s() {
        if let Some(info) = assets.patch_card4(id) {
            let s = info.slot() as usize;
            if let Some(bucket) = by_slot.get_mut(s) {
                bucket.push(id);
            }
        }
    }

    let mut rows = column![].spacing(3).padding(0);
    let mut filled = 0usize;
    for (slot, ids) in by_slot.iter().enumerate() {
        let installed = v.patch_card(slot);
        if installed.is_some() {
            filled += 1;
        }
        let mut choices = vec![PatchCard4Choice::none(lang)];
        choices.extend(ids.iter().map(|&id| PatchCard4Choice::card(loaded, id)));
        rows = rows.push(patch_card4_slot_row(loaded, slot, installed, choices));
    }

    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        Action::ClearPatchCard4s,
        [5.0, 10.0],
        widgets::danger_button,
    );
    let header = container(
        row![
            text(t!(lang, "save-tab-patch-cards")).size(TEXT_BODY),
            text(t!(lang, "patch-card-edit-count", count = filled as i64))
                .size(TEXT_CAPTION)
                .style(muted_text_style),
            Space::new().width(Fill),
            clear_all,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING);

    container(column![
        header,
        scrollable(rows)
            .style(crate::widgets::chunky_scrollable)
            .height(Fill)
            .width(Fill)
    ])
    .width(Fill)
    .height(Fill)
    .style(widgets::pane)
    .into()
}

// ---------- Auto Battle Data ----------

/// The six deck sections in display order, as `(title, runs)` where each run
/// is a `(chip, slots)` pair (see [`GroupedAutoBattleData`]). Shared read model
/// for the read-only viewer and the editor's live preview; combos are always
/// unfilled (the game reserves those slots).
///
/// [`GroupedAutoBattleData`]: tango_dataview::auto_battle_data::GroupedAutoBattleData
fn abd_grouped_sections(
    lang: &LanguageIdentifier,
    grouped: &tango_dataview::auto_battle_data::GroupedAutoBattleData,
) -> Vec<(String, Vec<(Option<usize>, usize)>)> {
    vec![
        (
            t!(lang, "auto-battle-data-secondary-standard-chips"),
            grouped.secondary_standard_chips.clone(),
        ),
        (
            t!(lang, "auto-battle-data-standard-chips"),
            grouped.standard_chips.clone(),
        ),
        (t!(lang, "auto-battle-data-mega-chips"), grouped.mega_chips.clone()),
        (t!(lang, "auto-battle-data-giga-chip"), grouped.giga_chip.clone()),
        (t!(lang, "auto-battle-data-combos"), grouped.combos.clone()),
        (
            t!(lang, "auto-battle-data-program-advance"),
            grouped.program_advance.clone(),
        ),
    ]
}

/// One deck section's rows (title row + a `chip_row` per run), shared by the
/// read-only viewer and the editor's live preview. Each run carries the folder
/// view's leading "N× " count column, so a chip that fills four slots reads as
/// one row instead of four; unfilled runs still render as empty "—" rows so the
/// section keeps its full shape. ABD rows have no chip code and no REG/TAG
/// indicators, so `code=None` and a default badge struct (overridden only by
/// the count); hover preview comes for free from `chip_row`.
fn abd_grouped_section_rows<M: 'static>(
    loaded: &Loaded,
    title: String,
    runs: &[(Option<usize>, usize)],
    chips_have_mb: bool,
) -> Element<'static, M> {
    let title_el = container(text(title).size(TEXT_BODY)).padding(style::HEADER_PADDING);
    let mut col = column![title_el, Space::new().height(4)].spacing(1);
    let last_idx = runs.len().saturating_sub(1);
    for (idx, (id, count)) in runs.iter().enumerate() {
        let g = GroupedChip {
            count: *count,
            ..GroupedChip::default()
        };
        col = col.push(chip_row(
            loaded,
            *id,
            None,
            &g,
            true,
            chips_have_mb,
            idx,
            false,
            idx == last_idx,
        ));
    }
    col.into()
}

fn render_auto_battle_data<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();

    // Grouped form of the deck, computed from the per-chip use counts rather
    // than the flat materialized slots: a chip that fills several deck slots
    // becomes one "N× chip" row (the same count column the folder's grouped
    // view uses) instead of N identical rows, while unfilled slots still show
    // as empty rows so each section keeps its full shape.
    let grouped = tango_dataview::auto_battle_data::GroupedAutoBattleData::materialize(view.as_ref(), assets);

    // Each section becomes its own pane so the outer scrollable in `view`
    // shows them as distinct demarcated regions.
    let mut col = column![].spacing(crate::style::PANE_GAP).width(Fill);
    for (title, runs) in abd_grouped_sections(lang, &grouped) {
        let rows = abd_grouped_section_rows::<M>(loaded, title, &runs, chips_have_mb);
        col = col.push(container(rows).width(Fill).style(crate::widgets::pane));
    }
    col.into()
}

/// The chips offered by the auto-battle-data editor's library, as chip
/// ids: program advances (always available to the deck) plus every other
/// chip the player actually holds in their pack. Filtered by `filter`
/// (case-insensitive name match) and in `sort` order. Ties fall back to
/// id for a stable order. Stable sorts (Id / Name) keep a row in place
/// while its count fields are edited; Used reorders as counts change.
fn sorted_auto_battle_data_chips(loaded: &Loaded, sort: AutoBattleDataSort, filter: &str) -> Vec<usize> {
    use tango_dataview::rom::ChipClass as CC;
    let assets = loaded.assets.as_ref();
    let view = loaded.save.view_auto_battle_data();
    let chips_view = loaded.save.view_chips();
    let filter = filter.to_lowercase();
    struct E {
        id: usize,
        name: String,
        used: usize,
    }
    let mut rows: Vec<E> = Vec::new();
    for id in 0..assets.num_chips() {
        let Some(info) = assets.chip(id) else { continue };
        let class = info.class();
        let is_pa = class == CC::ProgramAdvance;
        if !is_pa && !matches!(class, CC::Standard | CC::Mega | CC::Giga) {
            continue;
        }
        let Some(name) = info.name() else { continue };
        if name.trim().is_empty() {
            continue;
        }
        // Program advances are always offered; every other chip must be
        // in the player's pack (some code variant owned), matching the
        // library editor's notion of "owned".
        if !is_pa {
            let in_pack = (0..info.codes().len()).any(|variant| {
                chips_view
                    .as_ref()
                    .and_then(|v| v.pack_count(id, variant))
                    .map_or(false, |c| c > 0)
            });
            if !in_pack {
                continue;
            }
        }
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        let used = view.as_ref().and_then(|v| v.chip_use_count(id)).unwrap_or(0);
        rows.push(E { id, name, used });
    }
    match sort {
        AutoBattleDataSort::Id => {}
        AutoBattleDataSort::Name => rows.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id))),
        AutoBattleDataSort::Used => rows.sort_by(|a, b| b.used.cmp(&a.used).then(a.id.cmp(&b.id))),
    }
    rows.into_iter().map(|e| e.id).collect()
}

/// A fixed-width numeric field for a use count: shows `value`, and emits
/// `make(parsed)` on every edit (digits only, clamped to the u16 the save
/// stores). The field copies its value string, so it can be a temporary —
/// no draft state needed; the source of truth is the save.
fn abd_count_input<'a>(value: usize, make: impl Fn(usize) -> Action + 'a) -> Element<'a, Action> {
    let s = value.to_string();
    text_input("0", &s)
        .on_input(move |t| {
            let digits: String = t.chars().filter(|c| c.is_ascii_digit()).take(5).collect();
            make(digits.parse::<usize>().unwrap_or(0).min(MAX_ABD_USE_COUNT))
        })
        .width(Length::Fixed(54.0))
        .padding([4, 8])
        .size(TEXT_BODY)
        .style(crate::widgets::chunky_text_input)
        .into()
}

/// A use-count column: a muted caption + [`abd_count_input`], boxed to
/// `ABD_COUNT_COL_W` so the Used / Sec. fields line up across rows.
fn abd_count_cell<'a>(label: String, value: usize, make: impl Fn(usize) -> Action + 'a) -> Element<'a, Action> {
    container(
        row![
            text(label).size(TEXT_CAPTION).style(muted_text_style),
            abd_count_input(value, make),
        ]
        .spacing(4)
        .align_y(Alignment::Center),
    )
    .width(Length::Fixed(ABD_COUNT_COL_W))
    .align_x(iced::alignment::Horizontal::Right)
    .into()
}

/// One chip in the auto-battle-data editor's library, laid out like the
/// read-only chip list (icon · name · element · ATK · MB) with editable
/// Used (and, for Standard chips, Sec.) use-count fields appended. A
/// non-standard chip reserves the Sec. column's width so the Used column
/// stays aligned.
fn abd_library_row<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    id: usize,
    used: usize,
    secondary: Option<usize>,
    chips_have_mb: bool,
    row_idx: usize,
) -> Element<'a, Action> {
    let info = loaded.assets.chip(id);
    let name = info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| format!("#{id}"));
    let accent = class_accent(
        info.as_ref().map(|i| i.class()),
        info.as_ref().map(|i| i.dark()).unwrap_or(false),
    );
    let [element, atk, mb] = chip_stat_cells(loaded, id, chips_have_mb);

    let used_cell = abd_count_cell(t!(lang, "auto-battle-data-edit-used"), used, move |n| {
        Action::SetChipUseCount { id, count: n }
    });
    let sec_cell: Element<'a, Action> = match secondary {
        Some(sec) => abd_count_cell(t!(lang, "auto-battle-data-edit-secondary"), sec, move |n| {
            Action::SetSecondaryChipUseCount { id, count: n }
        }),
        None => Space::new().width(Length::Fixed(ABD_COUNT_COL_W)).into(),
    };

    let inner = row![
        chip_icon(loaded, Some(id)),
        text(name).size(TEXT_BODY).width(Fill),
        element,
        atk,
        mb,
        used_cell,
        sec_cell,
    ]
    .spacing(8)
    .align_y(Alignment::Center)
    .padding([3, 12]);
    with_chip_tooltip(
        loaded,
        Some(id),
        accent,
        edit_row_wrap(inner.into(), accent, row_idx, None),
    )
}

/// The auto-battle-data editor: a two-pane layout (live deck preview left,
/// chip library right). The deck is derived from per-chip use counts, so
/// the library's Used / Sec. fields are what you actually edit; each edit
/// restages the counts and rebuilds the materialized deck, so the left
/// preview updates live. Edits stage in the loaded save and are written to
/// disk only on Save.
fn render_auto_battle_data_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
    use crate::widgets;
    // Only reached while editing, so the EditState is present.
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let Some(view) = loaded.save.view_auto_battle_data() else {
        return placeholder(t!(lang, "save-empty"));
    };
    let assets = loaded.assets.as_ref();
    let chips_have_mb = assets.chips_have_mb();
    // ----- Left pane: the live deck, grouped like the read-only viewer -----
    // Built from the staged use counts (not the WRAM-materialized deck), so
    // each edit's restaged counts show immediately and a chip that fills
    // several slots reads as one "N× chip" row.
    let grouped = tango_dataview::auto_battle_data::GroupedAutoBattleData::materialize(view.as_ref(), assets);
    let sections = abd_grouped_sections(lang, &grouped);
    let mut deck = column![].spacing(1).padding(0);
    for (title, runs) in &sections {
        deck = deck.push(abd_grouped_section_rows::<Action>(
            loaded,
            title.clone(),
            runs,
            chips_have_mb,
        ));
    }
    // Distinct chips currently contributing to the deck (runs repeat the top
    // chips, so a raw slot count would overstate it).
    let distinct = sections
        .iter()
        .flat_map(|(_, runs)| runs.iter())
        .filter_map(|(id, _)| *id)
        .collect::<std::collections::HashSet<_>>()
        .len();
    let clear_all = widgets::labeled_icon_button(
        lucide_icons::Icon::Trash2,
        t!(lang, "save-edit-clear"),
        Action::ClearAutoBattleData,
        [5.0, 10.0],
        widgets::danger_button,
    );
    let count = text(t!(lang, "auto-battle-data-edit-count", count = distinct as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let deck_header = container(
        row![
            text(t!(lang, "save-tab-auto-battle-data")).size(TEXT_BODY),
            count,
            Space::new().width(Fill),
            clear_all,
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding(style::HEADER_PADDING);
    let deck_pane = editor_pane(deck_header, deck);

    // ----- Right pane: the chip library with editable use counts -----
    let mut lib = column![].spacing(1).padding(0);
    for (row_idx, id) in
        sorted_auto_battle_data_chips(loaded, state.auto_battle_data_sort, &edit.auto_battle_data_filter)
            .into_iter()
            .enumerate()
    {
        // Secondary use count only feeds the secondary-standard section, so
        // only Standard chips get a Sec. field.
        let is_standard = assets
            .chip(id)
            .map(|i| matches!(i.class(), tango_dataview::rom::ChipClass::Standard))
            .unwrap_or(false);
        let used = view.chip_use_count(id).unwrap_or(0);
        let secondary = is_standard.then(|| view.secondary_chip_use_count(id).unwrap_or(0));
        lib = lib.push(abd_library_row(
            lang,
            loaded,
            id,
            used,
            secondary,
            chips_have_mb,
            row_idx,
        ));
    }
    let lib_header = library_header(
        lang,
        t!(lang, "folder-edit-search"),
        &edit.auto_battle_data_filter,
        Action::AutoBattleDataFilterChanged,
        &AutoBattleDataSort::ALL,
        state.auto_battle_data_sort,
        AutoBattleDataSort::label,
        Action::AutoBattleDataSortChanged,
    );
    editor_panes(deck_pane, editor_pane(lib_header, lib))
}

fn placeholder<M: 'static>(msg: String) -> Element<'static, M> {
    // Centered icon-over-message card rather than a bare line of
    // text in the pane corner — the empty state is a whole-pane
    // situation, so let it own the pane like one.
    container(
        column![
            lucide_icons::Icon::FileQuestion
                .widget()
                .size(36.0)
                .style(muted_text_style),
            text(msg).size(crate::style::TEXT_HEADING).style(muted_text_style),
        ]
        .spacing(8)
        .align_x(Alignment::Center),
    )
    .width(Fill)
    .align_x(Alignment::Center)
    .padding(32)
    .style(crate::widgets::pane)
    .into()
}
