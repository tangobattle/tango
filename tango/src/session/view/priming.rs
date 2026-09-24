//! The notice a session lays over its own screen while a priming walk
//! holds it up.

use super::*;
// Explicit so these win over iced's prelude macros; see the parent module.
use sweeten::widget::column;

/// What a session says over its own screen while a priming walk — the
/// run from power-on to the games' link battle — is what it's doing.
/// Every case shows a black screen and no sound for as long as the walk
/// takes, which on a DS-class game is seconds rather than the instant a
/// GBA one takes; without this the session simply looks hung.
///
/// Resolved once per frame by [`framebuffer_view`], which lays it over
/// the rendered frame itself rather than the pane around it — see
/// [`priming_notice`].
pub(super) struct PrimingCopy {
    title: String,
    detail: String,
    /// How long the wait has run, or `None` once it's over — a failure
    /// has no clock to run.
    clock: Option<String>,
    /// The failure's dismissal, which is the session's own teardown:
    /// nothing is coming, so the only move left is leaving.
    dismiss: Option<String>,
}

pub(super) fn priming_copy(lang: &LanguageIdentifier, state: &State) -> Option<PrimingCopy> {
    let (title, detail) = match state.prime_wait()? {
        Priming::Match => (
            t!(lang, "playback-priming-match"),
            t!(lang, "playback-priming-match-detail"),
        ),
        Priming::Peer => (
            t!(lang, "playback-priming-peer"),
            t!(lang, "playback-priming-peer-detail"),
        ),
        Priming::Playback => (
            t!(lang, "playback-priming-replay"),
            t!(lang, "playback-priming-replay-detail"),
        ),
        // The engine's own reason, verbatim under a plain title: the
        // walk fails on things the user can act on (a save with no
        // NetBattle unlocked, a game that never reached its battle),
        // and paraphrasing them into one generic line would throw away
        // the part that says which.
        Priming::Failed(error) => {
            return Some(PrimingCopy {
                title: t!(lang, "playback-priming-failed"),
                detail: error,
                clock: None,
                dismiss: Some(t!(lang, "playback-close")),
            })
        }
    };
    // Counts from the frame the wait was first seen (see
    // [`State::prime_wait_since`]) — shown from zero rather than
    // appearing once the wait gets long, so nothing moves under the
    // user partway through.
    let elapsed = state
        .prime_wait_since
        .as_ref()
        .map_or(0, |(_, at)| at.elapsed().as_secs());
    Some(PrimingCopy {
        title,
        detail,
        clock: Some(t!(lang, "playback-priming-elapsed", secs = elapsed as i64)),
        dismiss: None,
    })
}

/// The priming notice as it's drawn: centered copy laid straight over
/// the frame, sized to exactly the rendered frame rect (`w` × `h`) so
/// it can't spill into the letterbox or the pane border around it. No
/// panel and no dim wash — the thing underneath is black anyway, and a
/// modal over it would be chrome around nothing.
///
/// While the walk runs, the title breathes on the shared
/// [`pulse`](crate::ui::anim::pulse) the lobby's in-flight status lines
/// use — the app's existing "still working" cue — over a plain seconds
/// counter, which is the only honest readout available: a walk ends
/// when the games' own traps say it has, not at a knowable fraction. A
/// failure stops breathing and takes a dismissal instead.
pub(super) fn priming_notice<'a>(copy: &PrimingCopy, w: f32, h: f32) -> Element<'a, Message> {
    let failed = copy.dismiss.is_some();
    let pulse = crate::ui::anim::pulse();
    let title = text(copy.title.clone())
        .size(TEXT_BODY)
        .align_x(iced::alignment::Horizontal::Center)
        .style(move |theme: &iced::Theme| iced::widget::text::Style {
            color: Some(if failed {
                theme.palette().danger
            } else {
                widgets::mix(
                    widgets::muted_color(theme),
                    theme.palette().primary,
                    0.45 + 0.55 * pulse,
                )
            }),
        });
    let mut copy_col = column![title].spacing(6).align_x(Alignment::Center);
    copy_col = copy_col.push(
        text(copy.detail.clone())
            .size(TEXT_CAPTION)
            .align_x(iced::alignment::Horizontal::Center)
            .style(widgets::muted_text_style),
    );
    if let Some(clock) = copy.clock.as_ref() {
        copy_col = copy_col.push(
            text(clock.clone())
                .size(TEXT_CAPTION)
                .align_x(iced::alignment::Horizontal::Center)
                .style(widgets::muted_text_style),
        );
    }
    if let Some(dismiss) = copy.dismiss.as_ref() {
        copy_col = copy_col.push(widgets::labeled_icon_button(
            Icon::X,
            dismiss.clone(),
            Message::Close,
            [6.0, 12.0],
            widgets::neutral,
        ));
    }
    // Padded so the copy wraps inside the frame instead of running to
    // its edges — a GBA frame at 1× is only 240 px wide, and the text
    // has to live within whatever the pane gave us.
    container(copy_col)
        .width(Length::Fixed(w))
        .height(Length::Fixed(h))
        .padding(12)
        .align_x(iced::alignment::Horizontal::Center)
        .align_y(iced::alignment::Vertical::Center)
        .into()
}
