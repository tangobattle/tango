use clap::Parser;

#[derive(clap::Parser)]
struct Cli {
    #[clap(parse(from_os_str))]
    path: std::path::PathBuf,
}

fn main() -> Result<(), anyhow::Error> {
    let args = Cli::parse();
    let raw = std::fs::read(&args.path)?;
    let state = mgba::state::State::from_slice(&raw);

    let mut wram_path = args.path.as_os_str().to_owned();
    wram_path.push(".wram");
    let wram_path = std::path::PathBuf::from(wram_path);

    println!(
        r#"GPRs:

 r0 = {:08x},  r1 = {:08x},  r2 = {:08x},  r3 = {:08x},
 r4 = {:08x},  r5 = {:08x},  r6 = {:08x},  r7 = {:08x},
 r8 = {:08x},  r9 = {:08x}, r10 = {:08x}, r11 = {:08x},
r12 = {:08x}, r13 = {:08x}, r14 = {:08x}, r15 = {:08x},
cpsr = {:08x}

WRAM will be dumped to: {}"#,
        state.gpr(0),
        state.gpr(1),
        state.gpr(2),
        state.gpr(3),
        state.gpr(4),
        state.gpr(5),
        state.gpr(6),
        state.gpr(7),
        state.gpr(8),
        state.gpr(9),
        state.gpr(10),
        state.gpr(11),
        state.gpr(12),
        state.gpr(13),
        state.gpr(14),
        state.gpr(15),
        state.cpsr(),
        wram_path.display(),
    );

    std::fs::write(&wram_path, state.wram())?;

    Ok(())
}
