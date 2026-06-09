use crate::hooks::{CompletionToken, MatchHandle, Trap};

use super::rng::pick_rng_state;
use super::INIT_RX;

pub(super) fn traps(
    hooks: &super::Hooks,
    joyflags: std::sync::Arc<std::sync::atomic::AtomicU32>,
    match_: MatchHandle,
    completion_token: CompletionToken,
) -> Vec<Trap> {
    let make_send_and_receive_call_hook = || {
        let match_ = match_.clone();
        let munger = hooks.munger();
        Box::new(move |mut core: mgba::core::CoreMutRef| {
            let pc = core.as_ref().gba().cpu().thumb_pc();
            core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);

            let Some(_) = match_.get() else {
                core.gba_mut().cpu_mut().set_gpr(0, 0);
                return;
            };
            core.gba_mut().cpu_mut().set_gpr(0, 3);

            munger.set_rx_packet(core, 0, &INIT_RX);
            munger.set_rx_packet(core, 1, &INIT_RX);
        })
    };
    vec![
        (hooks.offsets.rom.comm_menu_init_ret, {
            let munger = hooks.munger();
            Box::new(move |core| {
                munger.start_battle_from_comm_menu(core);
            })
        }),
        (
            hooks.offsets.rom.match_end_ret,
            Box::new(move |_core| {
                completion_token.complete();
            }),
        ),
        (hooks.offsets.rom.round_ending_entry1, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let Some(match_) = match_.get() else { return };
                if let Err(e) = match_.end_round() {
                    log::error!("end round failed: {e:#}");
                    match_.cancel();
                }
            })
        }),
        (hooks.offsets.rom.round_ending_entry2, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let Some(match_) = match_.get() else { return };
                if let Err(e) = match_.end_round() {
                    log::error!("end round failed: {e:#}");
                    match_.cancel();
                }
            })
        }),
        (hooks.offsets.rom.round_start_entry, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let Some(match_) = match_.get() else { return };
                let mut rng = match_.lock_rng();
                let rng_state = pick_rng_state(&mut *rng, match_.is_offerer());
                munger.set_rng_state(core, rng_state);
                munger.set_frame_counter(core, rng_state as u16);
            })
        }),
        (hooks.offsets.rom.round_start_ret, {
            let match_ = match_.clone();
            Box::new(move |_core| {
                let Some(match_) = match_.get() else { return };
                if let Err(e) = crate::sync::block_on(match_.start_round()) {
                    log::error!("start round failed: {e:#}");
                    match_.cancel();
                }
            })
        }),
        (hooks.offsets.rom.link_is_p2_ret, {
            let match_ = match_.clone();
            Box::new(move |mut core| {
                let Some(match_) = match_.get() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };
                core.gba_mut().cpu_mut().set_gpr(0, round.local_player_index() as i32);
            })
        }),
        (hooks.offsets.rom.main_read_joyflags, {
            let munger = hooks.munger();
            let match_ = match_.clone();
            Box::new(move |core| {
                let Some(match_) = match_.get() else { return };
                let mut round_state = match_.lock_round_state();
                let Some(round) = round_state.as_mut() else { return };

                if !round.has_settled_snapshot() {
                    let state = match core.save_state() {
                        Ok(state) => state,
                        Err(e) => {
                            log::error!("save state for first commit failed: {e:#}");
                            match_.cancel();
                            return;
                        }
                    };
                    if let Err(e) = match_.record_first_commit(round, state, &munger.tx_packet(core)) {
                        log::error!("record first commit failed: {e:#}");
                        match_.cancel();
                        return;
                    }
                    log::info!("primary rng state: {:08x}", munger.rng_state(core));
                }

                if let Err(e) =
                    crate::sync::block_on(round.add_local_input_and_fastforward(
                        core,
                        joyflags.load(std::sync::atomic::Ordering::Relaxed) as u16,
                    ))
                {
                    log::error!("failed to add local input: {}", e);
                    match_.cancel();
                }
            })
        }),
        (
            hooks.offsets.rom.handle_input_custom_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        (
            hooks.offsets.rom.handle_input_in_turn_send_and_receive_call,
            make_send_and_receive_call_hook(),
        ),
        {
            let munger = hooks.munger();
            (
                hooks.offsets.rom.comm_menu_send_and_receive_call,
                Box::new(move |mut core| {
                    let pc = core.as_ref().gba().cpu().thumb_pc();
                    core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
                    core.gba_mut().cpu_mut().set_gpr(0, 3);
                    munger.set_rx_packet(core, 0, &INIT_RX);
                    munger.set_rx_packet(core, 1, &INIT_RX);
                }),
            )
        },
        (
            hooks.offsets.rom.init_sio_call,
            Box::new(|mut core| {
                let pc = core.as_ref().gba().cpu().thumb_pc();
                core.gba_mut().cpu_mut().set_thumb_pc(pc + 4);
            }),
        ),
    ]
}
