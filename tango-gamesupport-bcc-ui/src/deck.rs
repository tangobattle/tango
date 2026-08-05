//! The program deck board, drawn the way BCC's PRG DECK screen draws it.
//!
//! The deck's eleven usable positions are wired as a circuit the battle
//! engine walks in position order: an entry column of two slots off the
//! navi socket, a middle column of three, a final column of four, then
//! the exit. Two more slots hang off the R and L buttons — the game
//! triggers those manually, not in sequence. Verified against the real
//! screen by scripted-emulation probes (equipping into every slot and
//! watching the deck array), so the arrangement here is the game's:
//!
//! ```text
//!     [R=10]   [3]   [6]
//! (◎)  [1]     [4]   [7]
//!      [2]     [5]   [8]
//!     [L=11]         [9]  → out
//! ```
//!
//! The traces show the deck's actual wiring, a binary-tree fan: the
//! navi feeds positions 1 and 2, and each chip feeds the two adjacent
//! chips of the next column (1 → 3,4; 2 → 4,5; 3 → 6,7; 4 → 7,8;
//! 5 → 8,9). R and L are wired to nothing — the buttons trigger them.
//!
//! The (◎) is the deck's navi chip, [`NAVI_SLOT`] save-side: the fan's
//! root, drawn as a card like the slots and replaceable in the editor
//! the same way (pick it, then pick a navi from the library — which
//! shows only navi chips while the navi is the target). The navi's MB
//! stat is the deck's MB capacity, shown as `used/capacity` in the
//! header.
//!
//! Save-side, `ChipsView` slot index `i` is deck-array position `i + 1`;
//! the columns below hold slot indexes.

use std::cmp::Ordering;

use iced::widget::canvas::{self, Canvas};
use iced::widget::{button, column, container, row, text, Space};
use iced::{mouse, Alignment, Element, Fill, Length, Point, Rectangle, Renderer, Size, Theme};
use tango_gamesupport_bcc_dataview::save::{DECK_SLOTS, NAVI_CHIP_IDS, NAVI_SLOT, PROGRAM_CHIP_IDS};
use tango_gamesupport_common::editor::loaded::OpenSave;
use tango_gamesupport_common::editor::view::{
    editor_header, editor_pane, editor_panes, folder, library_header, placeholder, Action, LibrarySort, State,
};
use tango_gamesupport_common::style::{self, TEXT_BODY, TEXT_CAPTION};
use tango_gamesupport_common::t;
use tango_gamesupport_common::widgets::{self, muted_color, muted_text_style};
use unic_langid::LanguageIdentifier;

/// The board's chip columns, entry to exit, as slot indexes top-to-bottom.
const COLUMNS: [&[usize]; 3] = [&[0, 1], &[2, 3, 4], &[5, 6, 7, 8]];
/// The manually-triggered side slots.
const R_SLOT: usize = 9;
const L_SLOT: usize = 10;

// Board geometry. Every element is fixed-size and placed at a computed
// position so the widget layer and the wire canvas share one layout.
// Widths scale with the pane (see `board`); heights and type don't.
const SLOT_W: f32 = 168.0;
const SLOT_H: f32 = 44.0;
/// Vertical gap between stacked cards.
const COL_VGAP: f32 = 10.0;
/// Horizontal gap between columns — where the wire fan-outs run.
const COL_HGAP: f32 = 28.0;
/// Margin around the board.
const PAD: f32 = 16.0;
/// How far apart two wires entering the same card sit.
const ENTRY_SPREAD: f32 = 12.0;
/// The inline remove button's edge, and the room every card reserves
/// for it so the editor's X never sits on top of a chip name — and the
/// read view lays out identically.
const REMOVE_SIZE: f32 = 20.0;
/// The gutter the R/L trigger letters sit in, left of their cards —
/// wide enough that the letter reads as a label on the slot rather
/// than something crowding the card's edge.
const TRIGGER_LABEL_W: f32 = 22.0;
/// How much of that gutter stays empty between the letter and the card.
const TRIGGER_LABEL_GAP: f32 = 6.0;
/// How far the board may shrink to fit a narrow pane before clipping.
const MIN_FIT_SCALE: f32 = 0.4;

/// Where everything on the board sits, in board-local coordinates.
/// `s` scales the horizontal dimensions only — cards get narrower to
/// fit the pane, never shorter.
struct Geometry {
    size: Size,
    slots: [Rectangle; DECK_SLOTS],
    navi: Rectangle,
}

impl Geometry {
    fn new(s: f32) -> Self {
        // The first column is inset by the trigger gutter so R/L can sit
        // beside their cards rather than inside them.
        let (slot_w, hgap) = (SLOT_W * s, COL_HGAP * s);
        let pad_x = PAD * s + TRIGGER_LABEL_W;
        let col_h = |n: usize| n as f32 * SLOT_H + (n - 1) as f32 * COL_VGAP;
        // The exit column is the tallest; it sets the board's height and
        // the R/L slots pin to its top and bottom.
        let board_h = col_h(COLUMNS[2].len());
        // Column 0 is the navi/R/L column; chip columns follow.
        let col_x = move |c: usize| pad_x + (c + 1) as f32 * (slot_w + hgap);

        let mut slots = [Rectangle::new(Point::ORIGIN, Size::ZERO); DECK_SLOTS];
        for (c, col) in COLUMNS.iter().enumerate() {
            let top = PAD + (board_h - col_h(col.len())) / 2.0;
            for (r, &i) in col.iter().enumerate() {
                slots[i] = Rectangle::new(
                    Point::new(col_x(c), top + r as f32 * (SLOT_H + COL_VGAP)),
                    Size::new(slot_w, SLOT_H),
                );
            }
        }
        let side = Size::new(slot_w, SLOT_H);
        slots[R_SLOT] = Rectangle::new(Point::new(pad_x, PAD), side);
        slots[L_SLOT] = Rectangle::new(Point::new(pad_x, PAD + board_h - SLOT_H), side);

        let navi = Rectangle::new(Point::new(pad_x, PAD + (board_h - SLOT_H) / 2.0), side);
        Geometry {
            size: Size::new(col_x(COLUMNS.len() - 1) + slot_w + pad_x, board_h + 2.0 * PAD),
            slots,
            navi,
        }
    }

    /// The wire polylines: the deck's binary-tree fan. The navi feeds
    /// the entry pair and chip `r` of a column feeds chips `r` and
    /// `r + 1` of the next. Each edge leaves its source's right edge,
    /// jogs at the gap's midpoint, and enters its destination's left
    /// edge — and a chip fed by two parents shows two distinct entry
    /// points, the upper parent's above center and the lower's below.
    fn wires(&self) -> Vec<Vec<Point>> {
        // Sources are pushed top-to-bottom per column, so each dest
        // collects its parents in upper-first order.
        let mut edges: Vec<(Rectangle, usize)> = COLUMNS[0].iter().map(|&i| (self.navi, i)).collect();
        for cols in COLUMNS.windows(2) {
            for (r, &a) in cols[0].iter().enumerate() {
                for &b in &cols[1][r..=r + 1] {
                    edges.push((self.slots[a], b));
                }
            }
        }

        let mut fan_in = [0usize; DECK_SLOTS];
        for &(_, to) in &edges {
            fan_in[to] += 1;
        }
        let mut routed = [0usize; DECK_SLOTS];
        edges
            .into_iter()
            .map(|(from, to)| {
                let dy = (routed[to] as f32 - (fan_in[to] as f32 - 1.0) / 2.0) * ENTRY_SPREAD;
                routed[to] += 1;
                let dst = self.slots[to];
                let start = Point::new(from.x + from.width, from.center_y());
                let end = Point::new(dst.x, dst.center_y() + dy);
                let mid = (start.x + end.x) / 2.0;
                vec![start, Point::new(mid, start.y), Point::new(mid, end.y), end]
            })
            .collect()
    }
}

/// The trace layer under the cards: one canvas stroking every wire in
/// the card-border color.
struct Wires {
    lines: Vec<Vec<Point>>,
}

impl<M> canvas::Program<M> for Wires {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        let stroke = canvas::Stroke::default()
            .with_width(2.0)
            .with_color(theme.extended_palette().background.strong.color);
        for line in &self.lines {
            let path = canvas::Path::new(|b| {
                b.move_to(line[0]);
                for &p in &line[1..] {
                    b.line_to(p);
                }
            });
            frame.stroke(&path, stroke);
        }
        vec![frame.into_geometry()]
    }
}

/// Absolutely position `content` at `rect`'s top-left within the board
/// stack (the stack gives every layer the full board; the padding is the
/// offset).
fn at<'a>(rect: Rectangle, content: Element<'a, Action>) -> Element<'a, Action> {
    container(content)
        .padding(iced::Padding {
            top: rect.y,
            right: 0.0,
            bottom: 0.0,
            left: rect.x,
        })
        .into()
}

/// A slot's clipboard label: its execution position for the wired run,
/// the trigger button for the side slots.
fn slot_label(slot: usize) -> String {
    match slot {
        R_SLOT => "R".to_string(),
        L_SLOT => "L".to_string(),
        _ => format!("{}", slot + 1),
    }
}

/// The concrete BCC assets: this UI is BCC-only, so it reads the
/// game's own chip model (HP, deck-capacity MB) directly rather than
/// through the shared trait, which the dataview keeps implemented only
/// for the shared plumbing (icon/artwork baking, the popover).
fn bcc_assets(loaded: &OpenSave) -> Option<&tango_gamesupport_bcc_dataview::rom::Assets> {
    // `underlying_any`, not `as_any`: the patch-override layer wraps the
    // assets and forwards this hook to the game's own.
    loaded.assets.underlying_any().downcast_ref()
}

/// A chip resolved for display: `(chip id, name, mb)`.
type ChipEntry = (usize, String, u16);

fn chip_entry(loaded: &OpenSave, id: usize) -> ChipEntry {
    let info = bcc_assets(loaded).and_then(|a| a.chip_info(id));
    (
        id,
        info.as_ref().and_then(|i| i.name()).unwrap_or_else(|| format!("#{id}")),
        info.as_ref().map(|i| i.mb()).unwrap_or(0),
    )
}

/// What's in each deck slot, read once per render.
fn deck_chips(loaded: &OpenSave) -> Vec<Option<ChipEntry>> {
    let Some(v) = loaded.save.view_chips() else {
        return vec![None; DECK_SLOTS];
    };
    let deck = v.equipped_folder_index();
    (0..DECK_SLOTS)
        .map(|slot| v.chip(deck, slot).map(|c| chip_entry(loaded, c.id)))
        .collect()
}

/// The deck's navi chip — the fan's root, whose MB stat is the base of
/// the deck's MB capacity.
fn navi_chip(loaded: &OpenSave) -> Option<ChipEntry> {
    let v = loaded.save.view_chips()?;
    let deck = v.equipped_folder_index();
    Some(chip_entry(loaded, v.chip(deck, NAVI_SLOT)?.id))
}

/// The deck's two MB rules: the wired run's shared capacity (the navi's
/// MB stat plus the save's upgrade bonus) and the slot-in cap, which is
/// a per-slot ceiling — R and L each get the whole allowance, they
/// don't share one.
#[derive(Clone, Copy)]
struct Limits {
    /// `None` when the deck has no navi to price the capacity.
    capacity: Option<u32>,
    slot_in: u32,
}

fn limits(loaded: &OpenSave, navi: &Option<ChipEntry>) -> Option<Limits> {
    let save = loaded
        .save
        .as_any()
        .downcast_ref::<tango_gamesupport_bcc_dataview::save::Save>()?;
    let deck = loaded.save.view_chips()?.equipped_folder_index();
    Some(Limits {
        capacity: navi
            .as_ref()
            .map(|(_, _, mb)| *mb as u32 + save.mb_capacity_bonus(deck) as u32),
        slot_in: save.slot_in_max(deck),
    })
}

/// MB used by the wired run — slots 0..[`R_SLOT`], the game's positions
/// 1–9. The navi and the R/L pair don't count against the capacity.
fn wired_mb(chips: &[Option<ChipEntry>]) -> u32 {
    chips[..R_SLOT].iter().flatten().map(|(_, _, mb)| *mb as u32).sum()
}

/// Whether either trigger slot holds a chip over the slot-in cap — the
/// editor won't install one, but a save edited elsewhere can arrive
/// that way.
fn slot_in_over(chips: &[Option<ChipEntry>], cap: u32) -> bool {
    chips[R_SLOT..].iter().flatten().any(|(_, _, mb)| *mb as u32 > cap)
}

/// One card on the board — the same shape everywhere, so a card reads
/// the same whether it's a wired slot, a trigger slot or the navi:
/// chip icon, name, MB. `on_press` makes it a button (the editor's
/// slot-select affordance); `selected` draws it in the accent; `width`
/// is the scaled card width from the board's geometry.
fn slot_card<'a>(
    loaded: &'a OpenSave,
    width: f32,
    chip: &Option<ChipEntry>,
    on_press: Option<Action>,
    selected: bool,
) -> Element<'a, Action> {
    let body = row![].spacing(8).align_y(Alignment::Center);
    let body: Element<'a, Action> = match chip {
        Some((id, name, mb)) => {
            let name_col = column![
                text(name.clone()).size(TEXT_BODY).wrapping(text::Wrapping::None),
                text(format!("{mb}MB")).size(10).style(muted_text_style),
            ]
            .spacing(1);
            body.push(folder::chip_icon(loaded, Some(*id))).push(name_col).into()
        }
        None => body
            .push(Space::new().width(Length::Fixed(28.0)).height(Length::Fixed(28.0)))
            .push(text("—").size(TEXT_BODY).style(muted_text_style))
            .into(),
    };

    let filled = chip.is_some();
    let plate = move |theme: &iced::Theme| {
        let ep = theme.extended_palette();
        container::Style {
            background: Some(iced::Background::Color(if filled {
                ep.background.weak.color
            } else {
                iced::Color::TRANSPARENT
            })),
            border: iced::Border {
                width: 1.0,
                color: if selected {
                    theme.palette().primary
                } else {
                    ep.background.strong.color
                },
                radius: 4.0.into(),
            },
            ..Default::default()
        }
    };

    // Fixed card size: the board's wire geometry depends on it. Content
    // clips at the reserved remove-button zone so the editor's inline X
    // never overlaps a name — and the read view lays out the same.
    let body = container(body)
        .height(Length::Fill)
        .align_y(iced::alignment::Vertical::Center)
        .clip(true);
    let padding = iced::Padding {
        top: 6.0,
        right: REMOVE_SIZE + 6.0,
        bottom: 6.0,
        left: 8.0,
    };
    let card: Element<'a, Action> = match on_press {
        Some(action) => button(body)
            .padding(padding)
            .width(Length::Fixed(width))
            .height(Length::Fixed(SLOT_H))
            .style(move |theme: &iced::Theme, status| {
                let mut s = iced::widget::button::Style {
                    background: plate(theme).background,
                    text_color: theme.palette().text,
                    border: plate(theme).border,
                    ..Default::default()
                };
                if matches!(status, iced::widget::button::Status::Hovered) && !selected {
                    s.border.color = theme.extended_palette().primary.weak.color;
                }
                s
            })
            .on_press(action)
            .into(),
        None => container(body)
            .padding(padding)
            .width(Length::Fixed(width))
            .height(Length::Fixed(SLOT_H))
            .style(plate)
            .into(),
    };

    // Hover detail: the chip's artwork + description, same popover the
    // folder rows use.
    match chip {
        Some((id, _, _)) => folder::with_chip_tooltip(loaded, Some(*id), None, card),
        None => card,
    }
}

/// The wired board. `interactive` adds slot-select buttons and an
/// inline remove X on every filled board slot (the editor); `selected`
/// highlights the library's target — a board slot, or [`NAVI_SLOT`] for
/// the navi card. The board narrows to the width it's given (down to
/// [`MIN_FIT_SCALE`]) instead of scrolling.
fn board<'a>(
    loaded: &'a OpenSave,
    chips: Vec<Option<ChipEntry>>,
    navi: Option<ChipEntry>,
    interactive: bool,
    selected: Option<usize>,
) -> Element<'a, Action> {
    let natural = Geometry::new(1.0);
    let height = natural.size.height;

    let board =
        iced::widget::responsive(move |size: Size| {
            let geo = Geometry::new((size.width / natural.size.width).clamp(MIN_FIT_SCALE, 1.0));
            let card_w = geo.slots[0].width;
            let card = |i: usize, chip: &Option<ChipEntry>| -> Element<'a, Action> {
                let on_press = interactive.then(|| {
                    // Reselecting the picked card clears the selection.
                    Action::SelectDeckSlot(if selected == Some(i) { None } else { Some(i) })
                });
                slot_card(loaded, card_w, chip, on_press, selected == Some(i))
            };

            // The traces underneath, every card on top.
            let mut layers: Vec<Element<'a, Action>> = vec![Canvas::new(Wires { lines: geo.wires() })
                .width(Length::Fixed(geo.size.width))
                .height(Length::Fixed(geo.size.height))
                .into()];
            for i in 0..DECK_SLOTS {
                layers.push(at(geo.slots[i], card(i, &chips[i])));
            }
            layers.push(at(geo.navi, card(NAVI_SLOT, &navi)));

            // R and L ride *outside* their cards, in the margin the board
            // reserves for them, so every card on the board is the same
            // shape — the trigger letter is a label on the slot, not part
            // of its contents.
            for (slot, letter) in [(R_SLOT, "R"), (L_SLOT, "L")] {
                let r = geo.slots[slot];
                layers.push(at(
                    Rectangle::new(
                        Point::new(r.x - TRIGGER_LABEL_W, r.y + (r.height - TEXT_BODY) / 2.0 - 2.0),
                        Size::new(TRIGGER_LABEL_W - TRIGGER_LABEL_GAP, TEXT_BODY),
                    ),
                    container(text(letter).size(TEXT_CAPTION).font(style::MONOSPACE_FONT).style(
                        |theme: &iced::Theme| iced::widget::text::Style {
                            color: Some(theme.palette().primary),
                        },
                    ))
                    .width(Length::Fixed(TRIGGER_LABEL_W))
                    .align_x(iced::alignment::Horizontal::Center)
                    .into(),
                ));
            }

            // The inline remove X, in the zone every card reserves for it.
            if interactive {
                for (i, r) in geo.slots.iter().enumerate() {
                    if chips[i].is_none() {
                        continue;
                    }
                    let zone = Rectangle::new(
                        Point::new(r.x + r.width - REMOVE_SIZE - 4.0, r.y + (r.height - REMOVE_SIZE) / 2.0),
                        Size::new(REMOVE_SIZE, REMOVE_SIZE),
                    );
                    layers.push(at(
                        zone,
                        button(lucide_icons::Icon::X.widget().size(12.0))
                            .padding(3)
                            .style(|theme: &iced::Theme, status| iced::widget::button::Style {
                                background: None,
                                text_color: if matches!(status, iced::widget::button::Status::Hovered) {
                                    theme.palette().danger
                                } else {
                                    muted_color(theme)
                                },
                                ..Default::default()
                            })
                            .on_press(Action::ClearDeckChip { slot: i })
                            .into(),
                    ));
                }
            }

            iced::widget::stack(layers)
                .width(Length::Fixed(geo.size.width))
                .height(Length::Fixed(geo.size.height))
                .into()
        });

    container(board)
        .padding(style::PANE_PADDING)
        .width(Fill)
        .height(Length::Fixed(height + 2.0 * style::PANE_PADDING))
        .into()
}

/// Board header captions: the wired run's MB against the deck's
/// capacity, and the R/L pair's MB against the slot-in budget — each in
/// the danger color when over. (The navi's HP rides the save's own
/// strip beside Play, like every other game's.)
fn header_captions<'a>(
    lang: &LanguageIdentifier,
    chips: &[Option<ChipEntry>],
    limits: Option<Limits>,
) -> Vec<Element<'a, Action>> {
    let caption = |label: String, over: bool| -> Element<'a, Action> {
        text(label)
            .size(TEXT_CAPTION)
            .style(move |theme: &iced::Theme| iced::widget::text::Style {
                color: Some(if over {
                    theme.palette().danger
                } else {
                    muted_color(theme)
                }),
            })
            .into()
    };
    let wired = wired_mb(chips);
    let mb = match limits.and_then(|l| l.capacity) {
        Some(cap) => caption(
            t!(lang, "deck-mb", used = wired as i64, capacity = cap as i64),
            wired > cap,
        ),
        None => caption(t!(lang, "deck-mb-uncapped", used = wired as i64), false),
    };
    let mut captions = vec![mb];
    if let Some(l) = limits {
        captions.push(caption(
            t!(lang, "deck-slot-in", max = l.slot_in as i64),
            slot_in_over(chips, l.slot_in),
        ));
    }
    captions
}

/// The read-only Program Deck tab: the wired board, headerless like
/// the read-only folder list — the budget captions live in the
/// editor's header, where they can be acted on.
pub fn render<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave) -> Element<'a, Action> {
    if loaded.save.view_chips().is_none() {
        return placeholder(t!(lang, "save-empty"));
    }
    let chips = deck_chips(loaded);
    let navi = navi_chip(loaded);
    container(board(loaded, chips, navi, false, None))
        .width(Fill)
        .style(widgets::pane)
        .into()
}

/// The deck editor: the board on the left (click a card to aim the
/// library at it), the chip library on the right. Clicking a library
/// chip installs it into the picked slot — or the first empty slot when
/// none is picked. Picking the navi card re-aims the library at the
/// navi and narrows it to navi chips; picking a filled slot offers
/// remove under the board.
pub fn render_edit<'a>(lang: &'a LanguageIdentifier, loaded: &'a OpenSave, state: &'a State) -> Element<'a, Action> {
    let Some(edit) = state.editing.as_ref() else {
        return placeholder(t!(lang, "save-empty"));
    };
    if loaded.save.view_chips().is_none() {
        return placeholder(t!(lang, "save-empty"));
    }
    let chips = deck_chips(loaded);
    let navi = navi_chip(loaded);
    let limits = limits(loaded, &navi);
    let selected = edit.selected_deck_slot.filter(|&s| s <= NAVI_SLOT);
    let navi_targeted = selected == Some(NAVI_SLOT);
    let first_empty = chips.iter().position(|c| c.is_none());

    // What a chip aimed at `slot` may cost: the remaining capacity for
    // a wired slot, the remaining slot-in budget for R/L (each net of
    // whatever the slot already holds), no MB constraint for the navi.
    let mb_budget = |slot: usize| -> Option<u32> {
        let current = |s: usize| chips[s].as_ref().map(|(_, _, mb)| *mb as u32).unwrap_or(0);
        if slot == NAVI_SLOT {
            None
        } else if slot == R_SLOT || slot == L_SLOT {
            // A per-slot ceiling, not a pool: what the other trigger
            // slot holds doesn't matter.
            limits.map(|l| l.slot_in)
        } else {
            limits
                .and_then(|l| l.capacity)
                .map(|cap| cap.saturating_sub(wired_mb(&chips) - current(slot)))
        }
    };

    // ----- Left pane: the board -----
    let header = editor_header(
        lang,
        t!(lang, "save-tab-program-deck"),
        header_captions(lang, &chips, limits),
        Action::ClearFolder,
    );
    let left = editor_pane(
        header,
        column![board(loaded, chips.clone(), navi, true, selected)].spacing(8),
    );

    // ----- Right pane: the chip library -----
    let filter = edit.library_filter.to_lowercase();
    let mut lib_list = column![].spacing(3).padding(0);
    let mut shown = 0usize;
    for (id, name, mb) in sorted_library_entries(loaded, navi_targeted, state.library_sort) {
        if !filter.is_empty() && !name.to_lowercase().contains(filter.as_str()) {
            continue;
        }
        // The slot this row would fill: the picked one, else the first
        // empty one. A full deck with nothing picked disables the
        // library — as does a chip too big for the target's MB budget.
        let target = selected.or(first_empty);
        let fits = target.is_some_and(|slot| mb_budget(slot).is_none_or(|budget| mb as u32 <= budget));
        let on_add = target.filter(|_| fits).map(|slot| Action::SetDeckChip {
            slot,
            chip_id: id,
            code: tango_gamesupport_common::dataview::save::ChipCode::Star,
        });
        lib_list = lib_list.push(library_row(loaded, id, name, shown, on_add));
        shown += 1;
    }
    let lib_header = library_header(
        lang,
        t!(lang, "folder-edit-search"),
        &edit.library_filter,
        Action::LibraryFilterChanged,
        &[
            LibrarySort::Id,
            LibrarySort::Name,
            LibrarySort::Element,
            LibrarySort::Attack,
            LibrarySort::Hp,
            LibrarySort::Mb,
        ],
        state.library_sort,
        library_sort_label,
        Action::LibrarySortChanged,
    );
    editor_panes(left, editor_pane(lib_header, lib_list))
}

/// BCC's sort-picker labels: the attack stat is this game's AP — the
/// card screen's own word for it — with everything else spelled the way
/// the shared folder picker spells it.
fn library_sort_label(sort: LibrarySort, lang: &LanguageIdentifier) -> String {
    match sort {
        LibrarySort::Attack => t!(lang, "folder-sort-ap"),
        other => folder::library_sort_label(other, lang),
    }
}

/// Every chip the deck editor's library offers, as `(id, name, mb)`, in
/// `sort` order: the navi roster while the navi card is targeted,
/// otherwise the equippable program chips — the placeholder ids (NO
/// DATA, DataChp1 and up) sit outside both ranges and are never listed.
///
/// Zero MB is a real price, not a marker for "not a program": Recov10,
/// Recov30 and PanlGrab are free to wire in. They list like any other
/// chip, and the row prints their cost as an explicit `0MB`.
fn sorted_library_entries(loaded: &OpenSave, navi_targeted: bool, sort: LibrarySort) -> Vec<(usize, String, u16)> {
    // The chip record computes each stat on demand, so the row is a
    // snapshot the comparators can read without re-decoding — the same
    // `E` the folder and ABD libraries sort.
    struct E {
        id: usize,
        name: String,
        elem: usize,
        hp: u16,
        ap: u16,
        mb: u16,
    }
    let range = if navi_targeted { NAVI_CHIP_IDS } else { PROGRAM_CHIP_IDS };
    let mut rows: Vec<E> = range
        .filter_map(|id| {
            let info = bcc_assets(loaded)?.chip_info(id)?;
            Some(E {
                id,
                name: info.name()?,
                elem: info.element(),
                hp: info.hp(),
                ap: info.attack_power(),
                mb: info.mb(),
            })
        })
        .collect();
    // All ties fall back to id so the order stays stable. BCC chips
    // carry no code letters, so Code never sorts; the folder library's
    // codeless sorts are Id and Hp instead.
    let cmp: fn(&E, &E) -> Ordering = match sort {
        LibrarySort::Id | LibrarySort::Code => |_a, _b| Ordering::Equal,
        LibrarySort::Name => |a, b| a.name.cmp(&b.name),
        LibrarySort::Attack => |a, b| a.ap.cmp(&b.ap),
        LibrarySort::Element => |a, b| a.elem.cmp(&b.elem),
        LibrarySort::Mb => |a, b| a.mb.cmp(&b.mb),
        LibrarySort::Hp => |a, b| a.hp.cmp(&b.hp),
    };
    rows.sort_by(|a, b| cmp(a, b).then(a.id.cmp(&b.id)));
    rows.into_iter().map(|e| (e.id, e.name, e.mb)).collect()
}

/// One library row, built on the concrete chip record: BCC's columns
/// are the element indicator then AP / HP / MB (the card screen's
/// stats — the chips carry no code letters). Same composition as the
/// shared folder library row — flush stripe gutter over `list_item`'s
/// zebra base, translucent wash when not installable — so the two
/// editors read identically.
fn library_row<'a>(
    loaded: &'a OpenSave,
    id: usize,
    name: String,
    row_idx: usize,
    on_add: Option<Action>,
) -> Element<'a, Action> {
    let info = bcc_assets(loaded).and_then(|a| a.chip_info(id));
    let (elem, hp, ap, mb) = info
        .map(|i| (i.element(), i.hp(), i.attack_power(), i.mb()))
        .unwrap_or_default();
    let element_icon: Element<'a, Action> = loaded
        .element_icons
        .get(&elem)
        .cloned()
        .map(|h| {
            iced::widget::image(h)
                .width(Length::Fixed(28.0))
                .height(Length::Fixed(28.0))
                .filter_method(iced::widget::image::FilterMethod::Nearest)
                .content_fit(iced::ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| Space::new().width(Length::Fixed(28.0)).into());
    let inner = row![
        folder::chip_icon(loaded, Some(id)),
        text(name).size(TEXT_BODY).width(Fill),
        element_icon,
        container(text(if ap > 0 { format!("{ap}") } else { String::new() }).size(TEXT_BODY))
            .width(Length::Fixed(46.0))
            .align_x(iced::alignment::Horizontal::Right),
        container(text(format!("{hp}HP")).size(TEXT_CAPTION))
            .width(Length::Fixed(46.0))
            .align_x(iced::alignment::Horizontal::Right),
        // Always printed, unlike the BN library's cell, which blanks a
        // zero: every BCC program carries an MB price and a free one is
        // worth saying out loud, so a 0MB chip reads `0MB`.
        container(text(format!("{mb}MB")).size(TEXT_CAPTION))
            .width(Length::Fixed(42.0))
            .align_x(iced::alignment::Horizontal::Right),
    ]
    .spacing(8)
    .align_y(Alignment::Center);

    // BCC chips carry no class accent; the stripe still reserves the
    // gutter so rows line up with every other library.
    let stripe: Element<'a, Action> = container(Space::new())
        .width(Length::Fixed(6.0))
        .height(Length::Fill)
        .into();
    let content = row![stripe, container(inner).width(Fill).padding([3, 12])]
        .height(Length::Shrink)
        .align_y(Alignment::Center);
    let addable = on_add.is_some();
    let mut body = button(content)
        .width(Fill)
        .padding(0)
        .style(widgets::list_item(false, row_idx));
    if let Some(action) = on_add {
        body = body.on_press(action);
    }
    let row_el: Element<'a, Action> = if addable {
        body.into()
    } else {
        iced::widget::stack([
            body.into(),
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(|theme: &iced::Theme| container::Style {
                    background: Some(iced::Background::Color(iced::Color {
                        a: 0.6,
                        ..theme.palette().background
                    })),
                    ..Default::default()
                })
                .into(),
        ])
        .into()
    };
    folder::with_chip_tooltip(loaded, Some(id), None, row_el)
}

/// The deck as clipboard text: the navi, one line per slot (position,
/// chip, MB), then the wired run against the capacity and the R/L pair
/// against the slot-in budget.
pub fn as_text(loaded: &OpenSave) -> Option<String> {
    loaded.save.view_chips()?;
    let chips = deck_chips(loaded);
    let navi = navi_chip(loaded);
    let limits = limits(loaded, &navi);
    let mut out = String::new();
    if let Some((_, name, _)) = &navi {
        out.push_str(&format!("Navi\t{name}\n"));
    }
    for (i, chip) in chips.iter().enumerate() {
        let Some((_, name, mb)) = chip else { continue };
        out.push_str(&format!("{}\t{name}\t{mb}MB\n", slot_label(i)));
    }
    out.push_str(&match limits.and_then(|l| l.capacity) {
        Some(cap) => format!("\t\t{}/{}MB\n", wired_mb(&chips), cap),
        None => format!("\t\t{}MB\n", wired_mb(&chips)),
    });
    if let Some(l) = limits {
        out.push_str(&format!("Slot-in\t\t{}MB\n", l.slot_in));
    }
    Some(out)
}
