#![windows_subsystem = "windows"]

use bitvec::view::BitView;
use byteorder::{ByteOrder, LittleEndian};
use clap::Parser;
use sha3::Digest;
use std::io::Write;
#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[derive(clap::Parser)]
struct Cli {
    #[clap(parse(from_os_str))]
    path: std::path::PathBuf,

    #[clap(long)]
    remote: bool,

    #[clap(subcommand)]
    action: Action,
}

#[derive(clap::Parser)]
struct VideoCli {
    #[clap(parse(from_os_str))]
    rom_path: std::path::PathBuf,

    #[clap(parse(from_os_str))]
    output_path: std::path::PathBuf,

    #[clap(long)]
    assume_incomplete: bool,

    #[clap(long, parse(from_os_str), default_value = "ffmpeg")]
    ffmpeg: std::path::PathBuf,

    #[clap(short('a'), long, default_value = "-c:a aac -ar 48000 -b:a 384k -ac 2")]
    ffmpeg_audio_flags: String,

    #[clap(
        short('v'),
        long,
        default_value = "-c:v libx264 -vf scale=iw*5:ih*5:flags=neighbor,format=yuv420p -force_key_frames expr:gte(t,n_forced/2) -crf 18 -bf 2"
    )]
    ffmpeg_video_flags: String,

    #[clap(short('m'), long, default_value = "-movflags +faststart")]
    ffmpeg_mux_flags: String,

    #[clap(long)]
    filter: String,
}

#[derive(clap::Parser)]
struct WRAMCli {}

#[derive(clap::Parser)]
struct TextCli {}

#[derive(clap::Parser)]
struct InputInfoCli {}

#[derive(clap::Parser)]
struct EvalCli {
    #[clap(parse(from_os_str))]
    rom_path: std::path::PathBuf,
}

#[derive(clap::Parser)]
struct StepCli {
    #[clap(parse(from_os_str))]
    rom_path: std::path::PathBuf,

    #[clap(required = true)]
    steps: u32,
}

#[derive(clap::Subcommand)]
enum Action {
    Video(VideoCli),
    WRAM(WRAMCli),
    Text(TextCli),
    InputInfo(InputInfoCli),
    Eval(EvalCli),
    Step(StepCli),
}

fn main() -> Result<(), anyhow::Error> {
    env_logger::Builder::from_default_env()
        .filter(Some("tango_core"), log::LevelFilter::Info)
        .filter(Some("replaydump"), log::LevelFilter::Info)
        .init();
    mgba::log::init();

    let args = Cli::parse();

    let mut f = std::fs::File::open(&args.path)?;

    let mut replay = tango_core::replay::Replay::decode(&mut f)?;

    if args.remote {
        replay = replay.into_remote().unwrap();
    }

    match args.action {
        Action::Video(args) => dump_video(args, replay),
        Action::WRAM(args) => dump_wram(args, replay),
        Action::Text(args) => dump_text(args, replay),
        Action::InputInfo(args) => dump_input_info(args, replay),
        Action::Eval(args) => dump_eval(args, replay),
        Action::Step(args) => dump_step(args, replay),
    }
}

fn dump_video(args: VideoCli, replay: tango_core::replay::Replay) -> Result<(), anyhow::Error> {
    let mut core = mgba::core::Core::new_gba("tango_core")?;
    core.enable_video_buffer();

    let rom = std::fs::read(&args.rom_path)?;
    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;

    core.as_mut().reset();

    let input_pairs = replay.input_pairs.clone();

    let replayer_state = tango_core::replayer::State::new(
        replay.local_player_index,
        input_pairs,
        0,
        Box::new(|| {}),
    );
    let hooks = tango_core::hooks::get(core.as_mut()).unwrap();
    hooks.patch(core.as_mut());
    {
        let replayer_state = replayer_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
        core.set_traps(traps);
    }
    core.as_mut().load_state(&replay.local_state.unwrap())?;

    #[cfg(windows)]
    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let filter = tango_core::video::filter_by_name(&args.filter).expect("unknown filter");
    let (vbuf_width, vbuf_height) = filter.output_size((
        mgba::gba::SCREEN_WIDTH as usize,
        mgba::gba::SCREEN_HEIGHT as usize,
    ));
    let mut emu_vbuf = vec![0u8; (mgba::gba::SCREEN_WIDTH * mgba::gba::SCREEN_HEIGHT * 4) as usize];
    let mut vbuf = vec![0u8; (vbuf_width * vbuf_height * 4) as usize];

    let video_output = tempfile::NamedTempFile::new()?;
    let mut video_child = std::process::Command::new(&args.ffmpeg);
    video_child
        .stdin(std::process::Stdio::piped())
        .args(&["-y"])
        // Input args.
        .args(&[
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgba",
            "-video_size",
            &format!("{}x{}", vbuf_width, vbuf_height),
            "-framerate",
            "16777216/280896",
            "-i",
            "pipe:",
        ])
        // Output args.
        .args(shell_words::split(&args.ffmpeg_video_flags)?)
        .args(&["-f", "mp4"])
        .arg(&video_output.path());
    #[cfg(windows)]
    video_child.creation_flags(CREATE_NO_WINDOW);
    let mut video_child = video_child.spawn()?;

    let audio_output = tempfile::NamedTempFile::new()?;
    let mut audio_child = std::process::Command::new(&args.ffmpeg);
    audio_child
        .stdin(std::process::Stdio::piped())
        .args(&["-y"])
        // Input args.
        .args(&["-f", "s16le", "-ar", "48k", "-ac", "2", "-i", "pipe:"])
        // Output args.
        .args(shell_words::split(&args.ffmpeg_audio_flags)?)
        .args(&["-f", "mp4"])
        .arg(&audio_output.path());
    #[cfg(windows)]
    audio_child.creation_flags(CREATE_NO_WINDOW);
    let mut audio_child = audio_child.spawn()?;

    const SAMPLE_RATE: f64 = 48000.0;
    let mut samples = vec![0i16; SAMPLE_RATE as usize];
    writeln!(
        std::io::stdout(),
        "{}",
        replayer_state.lock_inner().input_pairs_left()
    )?;
    loop {
        {
            let replayer_state = replayer_state.lock_inner();
            if (!replay.is_complete && replayer_state.input_pairs_left() == 0)
                || replayer_state.is_round_ended()
            {
                break;
            }
        }

        core.as_mut().run_frame();

        if let Some(err) = replayer_state.lock_inner().take_error() {
            Err(err)?;
        }

        let clock_rate = core.as_ref().frequency();
        let n = {
            let mut core = core.as_mut();
            let mut left = core.audio_channel(0);
            left.set_rates(clock_rate as f64, SAMPLE_RATE);
            let n = left.samples_avail();
            left.read_samples(&mut samples[..(n * 2) as usize], left.samples_avail(), true);
            n
        };
        {
            let mut core = core.as_mut();
            let mut right = core.audio_channel(1);
            right.set_rates(clock_rate as f64, SAMPLE_RATE);
            right.read_samples(&mut samples[1..(n * 2) as usize], n, true);
        }
        let samples = &samples[..(n * 2) as usize];

        emu_vbuf.copy_from_slice(core.video_buffer().unwrap());
        for i in (0..emu_vbuf.len()).step_by(4) {
            emu_vbuf[i + 3] = 0xff;
        }
        filter.apply(
            &emu_vbuf,
            &mut vbuf,
            (
                mgba::gba::SCREEN_WIDTH as usize,
                mgba::gba::SCREEN_HEIGHT as usize,
            ),
        );

        video_child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(vbuf.as_slice())?;

        let mut audio_bytes = vec![0u8; samples.len() * 2];
        LittleEndian::write_i16_into(samples, &mut audio_bytes[..]);
        audio_child
            .stdin
            .as_mut()
            .unwrap()
            .write_all(&audio_bytes)?;
        writeln!(
            std::io::stdout(),
            "{}",
            replayer_state.lock_inner().input_pairs_left()
        )?;
    }

    video_child.stdin = None;
    video_child.wait()?;
    audio_child.stdin = None;
    audio_child.wait()?;

    let mut mux_child = std::process::Command::new(&args.ffmpeg);
    mux_child
        .args(&["-y"])
        .args(&["-i"])
        .arg(video_output.path())
        .args(&["-i"])
        .arg(audio_output.path())
        .args(&["-c:v", "copy", "-c:a", "copy"])
        .args(shell_words::split(&args.ffmpeg_mux_flags)?)
        .arg(&args.output_path);
    #[cfg(windows)]
    mux_child.creation_flags(CREATE_NO_WINDOW);
    let mut mux_child = mux_child.spawn()?;
    mux_child.wait()?;

    Ok(())
}

fn dump_wram(_args: WRAMCli, replay: tango_core::replay::Replay) -> Result<(), anyhow::Error> {
    std::io::stdout().write_all(replay.local_state.unwrap().wram())?;
    std::io::stdout().flush()?;
    Ok(())
}

fn dump_step(args: StepCli, replay: tango_core::replay::Replay) -> Result<(), anyhow::Error> {
    let mut core = mgba::core::Core::new_gba("tango_core")?;
    let rom = std::fs::read(&args.rom_path)?;
    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;
    core.as_mut().reset();

    let input_pairs = replay.input_pairs.clone();

    let replayer_state = tango_core::replayer::State::new(
        replay.local_player_index,
        input_pairs,
        args.steps,
        Box::new(|| {}),
    );

    let hooks = tango_core::hooks::get(core.as_mut()).unwrap();
    hooks.patch(core.as_mut());
    {
        let replayer_state = replayer_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
        core.set_traps(traps);
    }
    core.as_mut().load_state(&replay.local_state.unwrap())?;

    loop {
        {
            let mut replayer_state = replayer_state.lock_inner();
            if replayer_state.input_pairs_left() == 0 || replayer_state.is_round_ended() {
                anyhow::bail!("overstepped");
            }

            if let Some(state) = replayer_state.take_committed_state() {
                std::io::stdout().write_all(state.state.wram())?;
                std::io::stdout().flush()?;
                break;
            }

            if let Some(err) = replayer_state.take_error() {
                Err(err)?;
            }
        }

        core.as_mut().run_frame();
    }

    Ok(())
}

fn dump_text(_args: TextCli, replay: tango_core::replay::Replay) -> Result<(), anyhow::Error> {
    for ip in &replay.input_pairs {
        println!(
            "tick = {:08x?}, l = {:02x} {:02x?}, r = {:02x} {:02x?}",
            ip.local.local_tick,
            ip.local.joyflags,
            ip.local.packet,
            ip.remote.joyflags,
            ip.remote.packet,
        );
    }
    Ok(())
}

#[derive(serde::Serialize)]
struct InputInfo {
    num_actual_input_pairs: usize,
    local_player_index: u8,
    local_input_histogram: [usize; 16],
    remote_input_histogram: [usize; 16],
    side_dependent_hash: String,
    side_independent_hash: String,
}

fn dump_input_info(
    _args: InputInfoCli,
    replay: tango_core::replay::Replay,
) -> Result<(), anyhow::Error> {
    let mut local_input_histogram = [0; 16];
    let mut remote_input_histogram = [0; 16];

    let mut side_dependent_sha3 = sha3::Sha3_256::new();
    let mut side_independent_sha3 = sha3::Sha3_256::new();
    for ip in &replay.input_pairs {
        side_dependent_sha3.update(
            &ip.local
                .packet
                .iter()
                .zip(ip.remote.packet.iter())
                .flat_map(|(x, y)| [*x, *y])
                .collect::<Vec<_>>(),
        );

        side_independent_sha3.update(
            &ip.local
                .packet
                .iter()
                .zip(ip.remote.packet.iter())
                .map(|(x, y)| *x ^ *y)
                .collect::<Vec<_>>(),
        );

        for i in ip
            .local
            .joyflags
            .view_bits::<bitvec::order::Lsb0>()
            .iter_ones()
        {
            local_input_histogram[i] += 1;
        }

        for i in ip
            .remote
            .joyflags
            .view_bits::<bitvec::order::Lsb0>()
            .iter_ones()
        {
            remote_input_histogram[i] += 1;
        }
    }

    serde_json::to_writer(
        std::io::stdout(),
        &InputInfo {
            num_actual_input_pairs: replay.input_pairs.len(),
            local_input_histogram,
            remote_input_histogram,
            local_player_index: replay.local_player_index,
            side_dependent_hash: hex::encode(side_dependent_sha3.finalize()),
            side_independent_hash: hex::encode(side_independent_sha3.finalize()),
        },
    )?;
    Ok(())
}

fn dump_eval(args: EvalCli, replay: tango_core::replay::Replay) -> Result<(), anyhow::Error> {
    let mut core = mgba::core::Core::new_gba("tango_core")?;
    let rom = std::fs::read(&args.rom_path)?;
    let vf = mgba::vfile::VFile::open_memory(&rom);
    core.as_mut().load_rom(vf)?;
    core.as_mut().reset();

    let input_pairs = replay.input_pairs.clone();

    let replayer_state = tango_core::replayer::State::new(
        replay.local_player_index,
        input_pairs,
        0,
        Box::new(|| {}),
    );
    let hooks = tango_core::hooks::get(core.as_mut()).unwrap();
    hooks.patch(core.as_mut());
    {
        let replayer_state = replayer_state.clone();
        let mut traps = hooks.common_traps();
        traps.extend(hooks.replayer_traps(replayer_state.clone()));
        core.set_traps(traps);
    }
    core.as_mut().load_state(&replay.local_state.unwrap())?;

    loop {
        {
            let replayer_state = replayer_state.lock_inner();
            if replayer_state.input_pairs_left() == 0 || replayer_state.is_round_ended() {
                break;
            }
        }

        core.as_mut().run_frame();

        {
            let mut replayer_state = replayer_state.lock_inner();
            if let Some(err) = replayer_state.take_error() {
                Err(err)?;
            }
        }
    }

    if let Some(result) = replayer_state.lock_inner().round_result() {
        println!("{}", result.result as u8);
    }

    Ok(())
}
