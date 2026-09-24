//! Putting the emulator's frames on screen: the main pane, the opponent's
//! pane or picture-in-picture inset, and the multi-screen arrangement
//! all of them share.

use super::priming::{priming_copy, priming_notice};
use super::*;
// Explicit so these win over iced's prelude macros; see the parent module.
use sweeten::widget::{column, row};
use tango_match::screens::{Arrangement, Stacking};

/// How the active session's frames present in this view: the multi-screen
/// arrangement the settings ask for, the native size of a frame laid out
/// that way, and where its touch screen lands. Taken live from config, like
/// the effect, so flipping a setting re-lays out an active session
/// immediately. Every pane showing a perspective of the session — the main
/// one, the opponent's, the inset — presents through the same one, since
/// they are the same console shape.
struct Presentation {
    layout: Option<tango_match::ScreenLayout>,
    /// More than one screen: the arrangement applies at all.
    multi: bool,
    arrangement: Arrangement,
    /// Native size of a presented frame. A DS puts two 256-wide screens
    /// where a GBA has one, and the session knows its console's screens
    /// from boot, so the pane holds the right shape before the first
    /// frame lands.
    native: (u32, u32),
    /// Where the touch screen starts in a presented frame, and its size.
    touch_screen: Option<((f32, f32), tango_match::Screen)>,
}

impl Presentation {
    fn of(ctx: Ctx<'_>) -> Self {
        let active = ctx.state.active.as_ref();
        let layout = active.map(|s| s.screen_layout());
        // The session always composes multiple screens side by side, upper
        // screen first; the arrangement is this view's re-layout of that
        // frame (see `present_frame`).
        let multi = layout.as_ref().is_some_and(|layout| layout.screens.len() > 1);
        let touch_first = multi && ctx.ds_primary_screen == crate::config::DsPrimaryScreen::Touch;
        let arrangement = ds_arrangement(ctx.ds_screen_stacking, touch_first);
        let native = match &layout {
            Some(layout) if multi && arrangement.stacking != Stacking::Horizontal => arrangement.size(layout),
            // Horizontal keeps the session's own composition, so its size
            // is the frame's.
            _ => active
                .map(|s| s.frame_size())
                // Unreachable in practice — the app only renders this view
                // over an active session — and an absent one draws only
                // the black placeholder, where shape doesn't matter.
                .unwrap_or((1, 1)),
        };
        let touch_screen = layout
            .as_ref()
            .and_then(|layout| arrangement.touch_screen_placement(layout));
        Self {
            layout,
            multi,
            arrangement,
            native,
            touch_screen,
        }
    }

    /// `frame` laid out as this presentation shows it.
    fn present(&self, frame: crate::platform::video::framebuffer::Frame) -> crate::platform::video::framebuffer::Frame {
        match (&self.layout, self.multi) {
            (Some(layout), true) => present_frame(frame, layout, self.arrangement),
            _ => frame,
        }
    }

    /// A recorded touch as a fraction of the presented frame: through the
    /// same origin as the stylus mapping, so the spot lands on the touch
    /// screen wherever the arrangement puts it.
    fn spot(&self, touch: Option<(u16, u16)>) -> Option<(f32, f32)> {
        let (tx, ty) = touch?;
        let ((origin_x, origin_y), _) = self.touch_screen?;
        let (native_w, native_h) = self.native;
        Some((
            (origin_x + tx as f32 + 0.5) / native_w as f32,
            (origin_y + ty as f32 + 0.5) / native_h as f32,
        ))
    }
}

/// The frame's on-screen size in a pane of `size`: an exact integer
/// multiple of `img` (crisp, the default) or, with fractional scaling, a
/// smooth aspect-fit.
fn fit(size: iced::Size, (img_w, img_h): (f32, f32), fractional_scaling: bool) -> (f32, f32) {
    let raw = (size.width / img_w).min(size.height / img_h);
    let scale = if fractional_scaling {
        raw.max(0.0)
    } else {
        raw.floor().max(1.0)
    };
    (img_w * scale, img_h * scale)
}

/// `fb` (drawn at `w` × `h`) with a recorded touch over it at `spot`: an
/// event-less canvas, so the pointer passes straight through it.
fn with_touch_spot<'a>(
    fb: Element<'a, Message>,
    spot: Option<(f32, f32)>,
    native_w: u32,
    w: f32,
    h: f32,
) -> Element<'a, Message> {
    let Some((fx, fy)) = spot else { return fb };
    let overlay = Canvas::new(TouchSpot {
        fx,
        fy,
        rf: TOUCH_SPOT_R / native_w as f32,
    })
    .width(Length::Fixed(w))
    .height(Length::Fixed(h));
    stack![fb, overlay].into()
}

/// Seat a `w` × `h` frame in its pane at the given alignment. An
/// integer-scaled frame gets a tight container so its drop shadow traces
/// the frame's edges, not the surrounding pane; a smooth aspect-fit gets
/// none.
fn place<'a>(
    fb: Element<'a, Message>,
    (w, h): (f32, f32),
    fractional_scaling: bool,
    horizontal_alignment: iced::alignment::Horizontal,
    vertical_alignment: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let content: Element<'a, Message> = if fractional_scaling {
        fb
    } else {
        container(fb)
            .width(Length::Fixed(w))
            .height(Length::Fixed(h))
            .style(|_theme: &iced::Theme| iced::widget::container::Style {
                shadow: iced::Shadow {
                    color: iced::Color::from_rgba(0.0, 0.0, 0.0, 0.55),
                    offset: iced::Vector::new(0.0, 8.0),
                    blur_radius: 24.0,
                },
                ..Default::default()
            })
            .into()
    };
    container(content)
        .width(Fill)
        .height(Fill)
        .align_x(horizontal_alignment)
        .align_y(vertical_alignment)
        .into()
}

/// The live framebuffer, rendered through a custom wgpu shader widget
/// (one persistent GPU texture, written in place each vblank) instead
/// of a per-frame `image` handle. The shader fills the widget's
/// bounds, so the widget is sized to the framebuffer rect — an exact
/// integer multiple (crisp, the default) or a smooth aspect-fit —
/// using `responsive` for the pane size both need. Before the first
/// frame, a 1×1 black placeholder keeps the pane opaque.
pub(super) fn framebuffer_view<'a>(
    ctx: Ctx<'a>,
    // A recorded touch to draw at its spot on the touch screen (the
    // replay input display); `None` everywhere else.
    touch_spot: Option<(u16, u16)>,
    // Stacked layouts dock the two frames against their center seam; every
    // other presentation centers the main frame in its pane.
    horizontal_alignment: iced::alignment::Horizontal,
    vertical_alignment: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let state = ctx.state;
    let (fractional_scaling, effect) = (ctx.fractional_scaling, ctx.effect);
    // Resolved out here, where the language is still in hand; the
    // closure below only lays it out (see `priming_notice`).
    let priming = priming_copy(ctx.lang, state);
    let presentation = Presentation::of(ctx);
    // The widget is sized to native·scale — the same rectangle the old CPU
    // upscalers produced — and the effect's fragment shader magnifies the
    // native texture to fill it.
    let (native_w, native_h) = presentation.native;
    let img = ((native_w * effect.scale) as f32, (native_h * effect.scale) as f32);
    let base_frame = presentation.present(
        state
            .current_frame
            .clone()
            .unwrap_or_else(crate::platform::video::framebuffer::Frame::black),
    );

    // The arrangement is settled here, so this is where the session
    // learns which screens are worth composing. Cheap enough to repeat
    // every repaint (one atomic store), which is also what makes a
    // setting flipped mid-session take effect straight away.
    if let (Some(layout), Some(active)) = (&presentation.layout, state.active.as_ref()) {
        active.set_displayed_screens(presentation.arrangement.presented_mask(layout));
    }

    let spot = presentation.spot(touch_spot);
    let touch_screen = presentation.touch_screen;
    iced::widget::responsive(move |size| {
        let (w, h) = fit(size, img, fractional_scaling);
        let mut frame = base_frame.clone();
        // The uploaded texture is always the native frame; the effect is just
        // the draw-time pipeline pick. Take it live from config here (not from
        // whatever was current when the frame was produced) so switching the
        // video filter re-renders immediately — even on a paused replay that
        // isn't producing new frames.
        frame.effect = effect;
        let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::Program::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));
        // The recorded touch, over the frame and under the stylus area.
        let mut fb = with_touch_spot(fb.into(), spot, native_w, w, h);
        // The stylus: pointer events over the widget, handed to the
        // session mapped into the touch screen's own pixels. The whole
        // surface reports (so a drag can scrape along the screen's
        // edges); whether a *press* lands on the touch screen travels
        // with each move as `inside`.
        if let Some(((origin_x, origin_y), screen)) = touch_screen {
            fb = iced::widget::mouse_area(fb)
                .on_move(move |p| {
                    let nx = p.x / w * native_w as f32 - origin_x;
                    let ny = p.y / h * native_h as f32 - origin_y;
                    let inside = (0.0..screen.width as f32).contains(&nx) && (0.0..screen.height as f32).contains(&ny);
                    let pos = (
                        nx.clamp(0.0, (screen.width - 1) as f32) as u16,
                        ny.clamp(0.0, (screen.height - 1) as f32) as u16,
                    );
                    Message::Stylus(StylusEvent::Moved { pos, inside })
                })
                .on_press(Message::Stylus(StylusEvent::Pressed))
                .on_release(Message::Stylus(StylusEvent::Released))
                .on_exit(Message::Stylus(StylusEvent::Released))
                .into();
        }

        // The priming notice, bounded to the frame rect itself so it
        // reads as part of the screen rather than of the window. Last
        // in, so it sits over the stylus area: while it's up there is
        // no battle to point at, and its own dismissal (the failure
        // case) has to be the thing a press lands on.
        if let Some(copy) = priming.as_ref() {
            fb = stack![fb, priming_notice(copy, w, h)].into();
        }

        place(fb, (w, h), fractional_scaling, horizontal_alignment, vertical_alignment)
    })
    .into()
}

/// The auxiliary opponent framebuffer as a full pane. This uses the PiP
/// primitive type so iced gives it an independent resident GPU texture, but
/// otherwise mirrors the main framebuffer's arrangement, scaling, filter,
/// touch marker, and shadow treatment.
fn opponent_framebuffer_view<'a>(
    ctx: Ctx<'a>,
    touch_spot: Option<(u16, u16)>,
    horizontal_alignment: iced::alignment::Horizontal,
    vertical_alignment: iced::alignment::Vertical,
) -> Element<'a, Message> {
    let (fractional_scaling, effect) = (ctx.fractional_scaling, ctx.effect);
    let presentation = Presentation::of(ctx);
    let (native_w, native_h) = presentation.native;
    let img = ((native_w * effect.scale) as f32, (native_h * effect.scale) as f32);
    // Keep the equal pane mounted while its first frame is being captured.
    // The black pixel stretches into the same aspect-sized widget, avoiding a
    // one-frame layout jump when the session publishes the auxiliary surface.
    let base_frame = presentation.present(
        ctx.state
            .pip_frame
            .clone()
            .unwrap_or_else(crate::platform::video::framebuffer::Frame::black),
    );
    let spot = presentation.spot(touch_spot);

    iced::widget::responsive(move |size| {
        let (w, h) = fit(size, img, fractional_scaling);
        let mut frame = base_frame.clone();
        frame.effect = effect;
        let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::PipProgram::new(frame))
            .width(Length::Fixed(w))
            .height(Length::Fixed(h));
        let fb = with_touch_spot(fb.into(), spot, native_w, w, h);
        place(fb, (w, h), fractional_scaling, horizontal_alignment, vertical_alignment)
    })
    .into()
}

/// Split the emulator body into two equal perspective panes along the
/// selected axis. Both arrangements have a zero-width center seam.
pub(super) fn stacked_framebuffers<'a>(
    ctx: Ctx<'a>,
    main: Element<'a, Message>,
    opponent_touch: Option<(u16, u16)>,
    view: crate::config::OpponentView,
) -> Element<'a, Message> {
    match view {
        crate::config::OpponentView::StackHorizontally => {
            let opponent = opponent_framebuffer_view(
                ctx,
                opponent_touch,
                iced::alignment::Horizontal::Left,
                iced::alignment::Vertical::Center,
            );
            row![
                container(main).width(Length::FillPortion(1)).height(Fill),
                container(opponent).width(Length::FillPortion(1)).height(Fill),
            ]
            .spacing(0)
            .padding([0, 12])
            .width(Fill)
            .height(Fill)
            .into()
        }
        crate::config::OpponentView::StackVertically => {
            let opponent = opponent_framebuffer_view(
                ctx,
                opponent_touch,
                iced::alignment::Horizontal::Center,
                iced::alignment::Vertical::Top,
            );
            column![
                container(main).width(Fill).height(Length::FillPortion(1)),
                container(opponent).width(Fill).height(Length::FillPortion(1)),
            ]
            .spacing(0)
            .padding([12, 0])
            .width(Fill)
            .height(Fill)
            .into()
        }
        _ => main,
    }
}

/// Picture-in-picture inset, top-right below the corner commands: the
/// other side's screen. Both cores render anyway (replay re-simulates the
/// opponent, training runs a live pair); this just insets the extra one.
/// Drawn through its own shader surface ([`PipProgram`]) because the main
/// framebuffer's pipeline owns a single resident texture. Reads the
/// host's captured [`State::pip_frame`], so it's pure presentation with no
/// message of its own — every session kind can push it directly.
///
/// [`PipProgram`]: crate::platform::video::framebuffer::PipProgram
pub(crate) fn pip_overlay<'a>(
    ctx: Ctx<'a>,
    // The PiP side's recorded touch to draw on its touch screen (the
    // replay input display); `None` everywhere else.
    touch_spot: Option<(u16, u16)>,
) -> Option<Element<'a, Message>> {
    // The PiP mirrors the main pane's arrangement — it's the same
    // console shape, just the other side's screens.
    let presentation = Presentation::of(ctx);
    let frame = presentation.present(ctx.state.pip_frame.clone()?);
    // 1.5x native: readable without dominating the main view.
    let (w, h) = (frame.width as f32 * 1.5, frame.height as f32 * 1.5);
    let native_w = frame.width;
    let fb = iced::widget::shader::Shader::new(crate::platform::video::framebuffer::PipProgram::new(frame))
        .width(Length::Fixed(w))
        .height(Length::Fixed(h));
    // The touch over the inset, same treatment as the main pane's.
    let fb = with_touch_spot(fb.into(), presentation.spot(touch_spot), native_w, w, h);
    let plate = container(fb).padding(3).style(hud_chip_plate);
    Some(
        container(plate)
            .width(Fill)
            .height(Fill)
            .align_x(iced::alignment::Horizontal::Right)
            .align_y(iced::alignment::Vertical::Top)
            .padding(iced::Padding {
                // Clear the corner commands' resting spot.
                top: 56.0,
                right: 12.0,
                bottom: 0.0,
                left: 0.0,
            })
            .into(),
    )
}

/// The arrangement settings in the shared geometry's terms.
fn ds_arrangement(stacking: crate::config::DsScreenStacking, touch_first: bool) -> Arrangement {
    let stacking = match stacking {
        crate::config::DsScreenStacking::Vertical => Stacking::Vertical,
        crate::config::DsScreenStacking::Horizontal => Stacking::Horizontal,
        crate::config::DsScreenStacking::PrimaryOnly => Stacking::PrimaryOnly,
    };
    Arrangement { stacking, touch_first }
}

/// A multi-screen frame as the arrangement settings present it:
/// screens reordered so the primary one leads, then packed side by
/// side, as a vertical stack, or cut down to the primary screen
/// alone. Pure presentation: the session's composition stays
/// canonical, so replays, exports and the wire never see this.
///
/// The revision is always remapped into a per-arrangement space, even
/// when the pixels pass through untouched: dimensions don't
/// distinguish every pair of arrangements (a horizontal swap keeps
/// them), and the GPU pipeline skips uploads on revision equality
/// alone — so two arrangements of one source revision must never
/// share a presented revision.
fn present_frame(
    frame: crate::platform::video::framebuffer::Frame,
    layout: &tango_match::ScreenLayout,
    arrangement: Arrangement,
) -> crate::platform::video::framebuffer::Frame {
    // Also guards the 1×1 black placeholder before the first frame,
    // which has no screens to rearrange.
    let canonical = (frame.width, frame.height) == layout.composite_size();
    let mut frame = if canonical && arrangement.rearranges() {
        let (width, height, pixels) = arrangement.rearrange(&frame.pixels, layout);
        crate::platform::video::framebuffer::Frame {
            pixels: std::sync::Arc::new(pixels),
            width,
            height,
            revision: frame.revision,
            effect: frame.effect,
        }
    } else {
        frame
    };
    let stacking = match arrangement.stacking {
        Stacking::Vertical => 0u64,
        Stacking::Horizontal => 1,
        Stacking::PrimaryOnly => 2,
    };
    frame.revision = frame
        .revision
        .wrapping_mul(8)
        .wrapping_add(stacking << 1 | arrangement.touch_first as u64);
    frame
}

/// Radius of a displayed touch, in native touch-screen pixels — about
/// a stylus tip. Scales with the pane like everything else drawn in it.
const TOUCH_SPOT_R: f32 = 6.0;

/// A recorded stylus touch, drawn over the framebuffer at the spot the
/// touch landed: a translucent accent fill under a solid ring, so it
/// reads over any game art without hiding what it points at. Position
/// and radius are fractions of the pane so the canvas needs no resize
/// handling of its own.
struct TouchSpot {
    fx: f32,
    fy: f32,
    rf: f32,
}

impl<M> canvas::Program<M> for TouchSpot {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        let center = Point::new(self.fx * bounds.width, self.fy * bounds.height);
        // Floor of 4px so the spot stays visible however small the
        // pane gets.
        let radius = (self.rf * bounds.width).max(4.0);
        let accent = theme.palette().primary;
        frame.fill(&Path::circle(center, radius), iced::Color { a: 0.35, ..accent });
        frame.stroke(
            &Path::circle(center, radius),
            Stroke::default().with_width(2.0).with_color(accent),
        );
        vec![frame.into_geometry()]
    }
}
