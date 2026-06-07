pub async fn eval(
    replay: &crate::replay::Replay,
    rom: &[u8],
    hooks: &'static (dyn crate::hooks::Hooks + Sync + Send),
    extra_traps: impl FnOnce() -> Vec<crate::hooks::Trap> + Send + Sync,
) -> Result<(crate::stepper::RoundResult, Box<mgba::state::State>), anyhow::Error> {
    let mut core = mgba::core::Core::new_gba("tango", &mgba::core::Options { ..Default::default() })?;

    let vf = mgba::vfile::VFile::from_vec(rom.to_vec());
    core.as_mut().load_rom(vf)?;
    core.as_mut()
        .load_save(mgba::vfile::VFile::from_vec(replay.local_sram_dump()))?;
    core.as_mut().reset();

    if replay.rounds.is_empty() {
        return Err(anyhow::anyhow!("replay has no rounds"));
    }

    let total_replay_ticks = replay.rounds.iter().map(|r| r.len() as u32).sum::<u32>();
    let match_type = (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8);

    let shadow = crate::shadow::Shadow::new_for_replay(rom, replay, hooks)?;
    let shadow = std::sync::Arc::new(std::sync::Mutex::new(shadow));

    let stepper_state = crate::stepper::State::new(
        match_type,
        replay.local_player_index,
        replay.rounds.clone(),
        0,
        replay.rng_seed,
        replay.is_offerer,
        total_replay_ticks,
        shadow,
        Box::new(|| {}),
    );

    hooks.patch(core.as_mut());
    {
        let stepper_state = stepper_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        traps.extend(extra_traps());
        core.set_traps(traps);
    }

    let replay_is_complete = replay.is_complete;
    loop {
        {
            let mut stepper_state = stepper_state.lock_inner();
            if let Some(err) = stepper_state.take_error() {
                return Err(err);
            }
            if stepper_state.is_round_ended() {
                break;
            }
            if !replay_is_complete && stepper_state.total_input_pairs_left() == 0 {
                // Incomplete replay: ran out of inputs before the final
                // round naturally ended. Take whatever round_result we have.
                break;
            }
        }

        core.as_mut().run_frame();
    }

    // The result is one frame past the last frame.
    core.as_mut().run_frame();

    let result = {
        let mut stepper_state = stepper_state.lock_inner();
        if let Some(err) = stepper_state.take_error() {
            return Err(err);
        }
        stepper_state.round_result()
    };

    let Some(result) = result else {
        return Err(anyhow::anyhow!("failed to read round result"));
    };

    Ok((result, core.as_mut().save_state()?))
}
