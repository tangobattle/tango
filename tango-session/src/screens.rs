//! Presenting a multi-screen composition in another arrangement.
//!
//! A session composes its screens side by side ([`composite_size`]),
//! the shape replays, exports and the wire see. A host that shows them
//! otherwise — stacked, reordered, or cut down to one screen — re-packs
//! that frame at draw time with these helpers, and maps the stylus
//! through the same geometry, so the picture and the touch target
//! can't disagree about where a screen ended up.
//!
//! [`composite_size`]: crate::composite_size

/// How the presented screens run.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stacking {
    /// One screen above the other.
    Vertical,
    /// Side by side, as the session composes them.
    Horizontal,
    /// Only the leading screen.
    PrimaryOnly,
}

/// A host's arrangement of a layout's screens: which way they run, and
/// whether the touch screen is pulled to the front.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Arrangement {
    pub stacking: Stacking,
    pub touch_first: bool,
}

impl Arrangement {
    /// The session's own composition: side by side, canonical order.
    pub const CANONICAL: Self = Self {
        stacking: Stacking::Horizontal,
        touch_first: false,
    };

    /// Whether presenting in this arrangement needs a re-pack at all:
    /// only a horizontal pair in canonical order passes through
    /// untouched.
    pub fn rearranges(self) -> bool {
        self != Self::CANONICAL
    }

    /// The layout's screens in the order this arrangement lays them
    /// out, as indices into it: canonical, or with the touch screen
    /// pulled to the front when it leads. Shared by the placement and
    /// the re-pack below so the two can't disagree about where a screen
    /// ended up.
    pub fn order(self, layout: &tango_match::ScreenLayout) -> Vec<usize> {
        let mut order: Vec<usize> = (0..layout.screens.len()).collect();
        if self.touch_first {
            if let Some(touch) = layout.touch {
                // `order` starts as the identity, so the touch screen sits
                // at its own index; the rest keep their relative order.
                order.remove(touch);
                order.insert(0, touch);
            }
        }
        order
    }

    /// The screens this arrangement actually shows, in order.
    fn shown(self, layout: &tango_match::ScreenLayout) -> Vec<usize> {
        let mut order = self.order(layout);
        // Primary-only shows the leading screen and drops the rest.
        if self.stacking == Stacking::PrimaryOnly {
            order.truncate(1);
        }
        order
    }

    /// Which of `layout`'s screens this arrangement actually puts in
    /// front of the player, as a bitmask over the layout's own order.
    ///
    /// Handed to the session so the console can stop composing a screen
    /// nobody is shown - on the DS that is a whole 2D engine, and
    /// primary-only is exactly the arrangement that drops one.
    pub fn presented_mask(self, layout: &tango_match::ScreenLayout) -> u8 {
        self.shown(layout).iter().fold(0u8, |m, &i| m | 1 << i)
    }

    /// The presented frame's size in native pixels.
    pub fn size(self, layout: &tango_match::ScreenLayout) -> (u32, u32) {
        let shown = self.shown(layout);
        let screens = shown.iter().map(|&i| layout.screens[i]);
        match self.stacking {
            Stacking::Vertical => (
                screens.clone().map(|s| s.width).max().unwrap_or(0),
                screens.map(|s| s.height).sum(),
            ),
            Stacking::Horizontal | Stacking::PrimaryOnly => (
                screens.clone().map(|s| s.width).sum(),
                screens.map(|s| s.height).max().unwrap_or(0),
            ),
        }
    }

    /// Where the stylus target sits in the presented frame: (where it
    /// starts, its size), all in native pixels. `None` when this
    /// arrangement puts no touch screen on the pane — neither the stylus
    /// area nor a touch spot goes up without a screen to touch.
    ///
    /// The layout names which of its screens the stylus points at rather
    /// than the host inferring it from a count: a session composes
    /// whichever screens its mode uses, so the touch screen may lead the
    /// frame, trail it, or not be in it at all — as in a primary-only
    /// arrangement led by the upper screen, or a game whose link battle
    /// never leaves that screen.
    pub fn touch_screen_placement(
        self,
        layout: &tango_match::ScreenLayout,
    ) -> Option<((f32, f32), tango_match::Screen)> {
        let touch = layout.touch?;
        let shown = self.shown(layout);
        let at = shown.iter().position(|&i| i == touch)?;
        // Everything ahead of it along the axis the stacking runs; the
        // other axis stays at the frame's edge.
        let vertical = self.stacking == Stacking::Vertical;
        let ahead: u32 = shown[..at]
            .iter()
            .map(|&i| {
                if vertical {
                    layout.screens[i].height
                } else {
                    layout.screens[i].width
                }
            })
            .sum();
        let origin = if vertical {
            (0.0, ahead as f32)
        } else {
            (ahead as f32, 0.0)
        };
        Some((origin, layout.screens[touch]))
    }

    /// Re-pack a canonical RGBA8 composition (`layout`'s screens side by
    /// side, [`composite_size`](crate::composite_size)) into this
    /// arrangement: rows re-sliced into the presented order and axis, or
    /// down to the primary screen alone. Returns the presented size and
    /// pixels; [`size`](Self::size) gives the same size up front.
    pub fn rearrange(self, pixels: &[u8], layout: &tango_match::ScreenLayout) -> (u32, u32, Vec<u8>) {
        const BPP: usize = 4;
        let (width, height) = crate::composite_size(layout);
        // Each screen's column offset in the canonical composition.
        let mut x0 = vec![0usize; layout.screens.len()];
        for i in 1..x0.len() {
            x0[i] = x0[i - 1] + layout.screens[i - 1].width as usize;
        }
        let order = self.order(layout);
        let src_stride = width as usize * BPP;
        // A screen's row slice in the canonical frame, or `None` past its
        // height (screens shorter than the composite pad with opaque
        // black — never hit on a DS, whose screens match).
        let row_of = |i: usize, row: usize| -> Option<&[u8]> {
            let screen = &layout.screens[i];
            (row < screen.height as usize).then(|| {
                let start = row * src_stride + x0[i] * BPP;
                &pixels[start..start + screen.width as usize * BPP]
            })
        };
        let pad = |pixels: &mut Vec<u8>, px: usize| {
            for _ in 0..px {
                pixels.extend_from_slice(&[0, 0, 0, 0xff]);
            }
        };
        let (out_w, out_h) = self.size(layout);
        let mut out = Vec::with_capacity(out_w as usize * out_h as usize * BPP);
        match self.stacking {
            Stacking::Vertical => {
                for &i in &order {
                    for row in 0..layout.screens[i].height as usize {
                        out.extend_from_slice(row_of(i, row).unwrap());
                        pad(&mut out, out_w as usize - layout.screens[i].width as usize);
                    }
                }
            }
            Stacking::PrimaryOnly => {
                let i = order[0];
                for row in 0..out_h as usize {
                    out.extend_from_slice(row_of(i, row).unwrap());
                }
            }
            Stacking::Horizontal => {
                // Same dimensions as the canonical frame, columns
                // reordered within each row.
                for row in 0..height as usize {
                    for &i in &order {
                        match row_of(i, row) {
                            Some(slice) => out.extend_from_slice(slice),
                            None => pad(&mut out, layout.screens[i].width as usize),
                        }
                    }
                }
            }
        }
        (out_w, out_h, out)
    }
}

#[cfg(test)]
mod tests {
    use super::Stacking::{Horizontal, PrimaryOnly, Vertical};
    use super::{Arrangement, Stacking};

    fn screen() -> tango_match::Screen {
        tango_match::Screen {
            width: 256,
            height: 192,
        }
    }

    /// A DS composing its whole console: upper screen, then the touch
    /// screen it points at.
    fn both() -> tango_match::ScreenLayout {
        tango_match::ScreenLayout::new([screen(), screen()]).with_touch(1)
    }

    fn place(
        layout: &tango_match::ScreenLayout,
        stacking: Stacking,
        touch_first: bool,
    ) -> Option<((f32, f32), tango_match::Screen)> {
        Arrangement { stacking, touch_first }.touch_screen_placement(layout)
    }

    /// The stylus area follows the touch screen through every
    /// arrangement. Pinned because a wrong origin doesn't look wrong —
    /// the pane draws fine and every press just lands somewhere else.
    #[test]
    fn the_stylus_area_follows_the_touch_screen() {
        let place = |stacking, touch_first| place(&both(), stacking, touch_first);
        // Trailing the upper screen, along whichever axis stacks.
        assert_eq!(place(Horizontal, false).unwrap().0, (256.0, 0.0));
        assert_eq!(place(Vertical, false).unwrap().0, (0.0, 192.0));
        // Leading, when it's the primary screen.
        assert_eq!(place(Horizontal, true).unwrap().0, (0.0, 0.0));
        assert_eq!(place(Vertical, true).unwrap().0, (0.0, 0.0));
        assert_eq!(place(PrimaryOnly, true).unwrap().0, (0.0, 0.0));
        // Primary-only led by the upper screen leaves it off the pane.
        assert!(place(PrimaryOnly, false).is_none());
    }

    /// A session composing without its touch screen — a game whose
    /// netbattle never leaves the upper one — has nothing to point at,
    /// so no arrangement produces a stylus area.
    #[test]
    fn a_composition_without_the_touch_screen_has_no_stylus_area() {
        let upper = tango_match::ScreenLayout::new([screen()]);
        for stacking in [Horizontal, Vertical, PrimaryOnly] {
            for touch_first in [false, true] {
                assert!(place(&upper, stacking, touch_first).is_none());
            }
        }
    }

    /// The mirror case, which is the whole reason the layout names its
    /// touch screen rather than the pane assuming the second of two:
    /// a composition of the touch screen alone is all stylus.
    #[test]
    fn a_touch_only_composition_is_all_stylus() {
        let touch = tango_match::ScreenLayout::new([screen()]).with_touch(0);
        for stacking in [Horizontal, Vertical, PrimaryOnly] {
            for touch_first in [false, true] {
                let (origin, size) = place(&touch, stacking, touch_first).unwrap();
                assert_eq!(origin, (0.0, 0.0));
                assert_eq!(size, screen());
            }
        }
    }

    /// The re-pack puts each screen's rows where the placement says the
    /// screen is, and reports the size it promised.
    #[test]
    fn the_repack_moves_whole_screens() {
        let small = tango_match::Screen { width: 2, height: 2 };
        let layout = tango_match::ScreenLayout::new([small, small]).with_touch(1);
        // Upper screen all 1s, touch screen all 2s.
        let canonical: Vec<u8> = (0..2).flat_map(|_| [[1u8; 8], [2u8; 8]].concat()).collect();
        for stacking in [Horizontal, Vertical, PrimaryOnly] {
            for touch_first in [false, true] {
                let arrangement = Arrangement { stacking, touch_first };
                let (w, h, pixels) = arrangement.rearrange(&canonical, &layout);
                assert_eq!((w, h), arrangement.size(&layout));
                assert_eq!(pixels.len(), (w * h * 4) as usize);
                if let Some(((x, y), _)) = arrangement.touch_screen_placement(&layout) {
                    let at = (y as usize * w as usize + x as usize) * 4;
                    assert_eq!(pixels[at], 2);
                }
            }
        }
        let vertical = Arrangement {
            stacking: Vertical,
            touch_first: false,
        };
        assert_eq!(
            vertical.rearrange(&canonical, &layout).2,
            [[1u8; 16], [2u8; 16]].concat()
        );
    }
}
