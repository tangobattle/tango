pub async fn eval(
    replay: &crate::replay::Replay,
    rom: &[u8],
    hooks: &(dyn crate::hooks::Hooks + Sync + Send),
) -> Result<(crate::stepper::RoundResult, mgba::state::State), anyhow::Error> {
    let mut core = mgba::core::Core::new_gba("tango")?;

    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;
    core.as_mut().reset();

    let input_pairs = replay.input_pairs.clone();

    let stepper_state = crate::stepper::State::new(
        (replay.metadata.match_type as u8, replay.metadata.match_subtype as u8),
        replay.local_player_index,
        input_pairs,
        0,
        Box::new(|| {}),
    );

    hooks.patch(core.as_mut());
    {
        let stepper_state = stepper_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.stepper_traps(stepper_state.clone()));
        core.set_traps(traps);
    }
    core.as_mut().load_state(&replay.local_state)?;

    loop {
        {
            let mut stepper_state = stepper_state.lock_inner();
            if let Some(err) = stepper_state.take_error() {
                return Err(err);
            }
            if stepper_state.input_pairs_left() == 0 {
                break;
            }

            // For old-style replays, we don't have a precise ending, so we have to just take the result.
            if let Some(result) = stepper_state.round_result() {
                return Ok((result, core.as_mut().save_state()?));
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

    let result = if let Some(result) = result {
        result
    } else {
        return Err(anyhow::anyhow!("failed to read round result"));
    };

    Ok((result, core.as_mut().save_state()?))
}
