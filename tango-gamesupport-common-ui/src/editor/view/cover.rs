use super::*;
use sweeten::widget::column;

/// The game's logo composition, decoded once in loaded-save baking. Logos
/// vary in aspect ratio, so each is sized to a fixed height and Contain'd.
fn cover_art<M: 'static>(loaded: &OpenSave) -> Element<'static, M> {
    let inner: Element<'static, M> = match loaded.logos.as_slice() {
        // Two variant logos in the family (e.g. Gregar/Falzar) — stack
        // them vertically with a left/right stagger, the way the Legacy
        // Collection lays out twin-version covers: the family's first
        // variant sits up and to the left, the second down and to the
        // right, whichever one of them is loaded.
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
    inner
}

fn cover_frame<M: 'static>(content: Element<'static, M>) -> Element<'static, M> {
    container(column![content].width(Fill).align_x(Alignment::Center))
        .width(Fill)
        // Fill the viewer's height, with the cover centered vertically.
        .height(Fill)
        .align_y(iced::alignment::Vertical::Center)
        // Extra breathing room above/below the logo(s); standard
        // horizontal inset.
        .padding([crate::style::PANE_PADDING + 24.0, crate::style::PANE_PADDING + 24.0])
        .style(crate::widgets::pane)
        .into()
}

/// Legacy per-game render fallback. Cover is no longer included in the tab
/// list, but keeping this entry point makes older game-specific render arms
/// harmless while they are still exhaustive over [`Tab`].
pub fn render_cover<M: 'static>(_lang: &LanguageIdentifier, loaded: &OpenSave) -> Element<'static, M> {
    cover_frame(cover_art(loaded))
}

/// Streamer-mode gate: Cover replaces the entire save viewer until the user
/// explicitly asks to review the build.
pub fn render_cover_gate<M: Clone + 'static>(
    lang: &LanguageIdentifier,
    loaded: &OpenSave,
    review: M,
) -> Element<'static, M> {
    let cover = cover_frame(cover_art(loaded));
    let review = container(
        crate::widgets::labeled_icon_button(
            lucide_icons::Icon::Eye,
            t!(lang, "save-review"),
            review,
            [5.0, 12.0],
            crate::widgets::neutral,
        ),
    )
    .width(Fill)
    .height(Fill)
    .align_x(iced::alignment::Horizontal::Right)
    .align_y(iced::alignment::Vertical::Bottom)
    .padding(crate::style::PANE_PADDING + 12.0);
    iced::widget::Stack::new().push(cover).push(review).into()
}
