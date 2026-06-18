use super::*;
use sweeten::widget::{column, row, text_input};

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

pub(super) fn render_auto_battle_data<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
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
pub(super) fn render_auto_battle_data_edit<'a>(
    lang: &'a LanguageIdentifier,
    loaded: &'a Loaded,
    state: &'a State,
) -> Element<'a, Action> {
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
    let count = text(t!(lang, "auto-battle-data-edit-count", count = distinct as i64))
        .size(TEXT_CAPTION)
        .style(muted_text_style);
    let deck_header = editor_header(
        lang,
        t!(lang, "save-tab-auto-battle-data"),
        vec![count.into()],
        Action::ClearAutoBattleData,
    );
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

/// Sort order for the auto-battle-data editor's chip library pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AutoBattleDataSort {
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
