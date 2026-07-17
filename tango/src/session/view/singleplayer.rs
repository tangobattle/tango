//! Single-player session view: just the emulator pane and the shared
//! corner commands — no messages of its own.

use super::*;
use crate::session::singleplayer::SinglePlayerSession;
use crate::session::Message as SessionMessage;

/// Single-player: just the emulator and the corner commands.
pub(crate) fn view<'a>(s: &'a SinglePlayerSession, ctx: Ctx<'a>) -> Element<'a, SessionMessage> {
    let Ctx { lang, state, .. } = ctx;
    let now = iced::time::Instant::now();
    let frame = framebuffer_view(state, ctx.fractional_scaling, ctx.effect);
    let body = emulator_body(s.local_game(), frame, ctx.hide_emulator_border, [false, false]);
    let mut stacked = stack![body];
    if state.controls_anim.visible(now) {
        stacked = stacked.push(corner_commands_overlay(lang, state, SessionMessage::Close, false));
    }
    finish_session_stack(lang, state, stacked)
}
