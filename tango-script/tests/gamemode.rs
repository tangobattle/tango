use tango_match::gamemode::{Context, Controller, Error, Event, Machine, Memory, Reader, Value, Write};
use tango_script::{GameModeProgram, Package, PackageRef, Profile};

fn profile(body: &str, setup: &str) -> Profile {
    let source = format!(
        r#"--!strict
local retained: Game? = nil
return {{
    setup = function(context: GameModeContext, game: Game)
        assert(retained == nil)
        retained = game
        local captured = 10
        {setup}
        game.hook(256, function()
            assert(captured == 10)
            captured += 1
            {body}
        end)
    end,
}}
"#
    );
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='hook-test'\nversion='1.0.0'\ndefault_gamemode='main'\n[[gamemode]]\nname='main'\npath='./init'\n".to_vec(),
            ),
            ("init.luau".into(), source.into_bytes()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "hook-test".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

const BODY: &str = r#"
    local bytes = game.read("main", 0, 1)
    assert(buffer.readu8(bytes, 0) == game.read_register("pc"))
    local value = buffer.readu8(bytes, 0) + 1
    buffer.writeu8(bytes, 0, value)
    game.write("main", 0, bytes)
    game.write_register("pc", value)
    assert(buffer.readu8(game.read("main", 0, 1), 0) == value)
    assert(game.read_register("pc") == value)
    game.round_outcome(context.player)
    if game.read_register("fail") == 2 then error("hook failed") end
"#;

fn context() -> Context {
    Context {
        disable_bgm: false,
        player: 1,
        seed: [7; 16],
        options: Default::default(),
    }
}
fn controller(profile: Profile, context: Context) -> Controller {
    Controller::new(Box::new(GameModeProgram::new(profile, context).unwrap().unwrap())).unwrap()
}
fn invoke(hooks: &mut Controller, core: &mut Core) -> Result<Vec<Event>, Error> {
    hooks.on_hook(&hooks.hooks()[0].name.clone(), core)
}

#[derive(Clone, Default)]
struct Core {
    bytes: [u8; 8],
    pc: u32,
    fail: u32,
    applied: usize,
}
impl Memory for Core {
    fn read(&mut self, space: &str, address: u32, output: &mut [u8]) -> Result<(), Error> {
        if space != "main" {
            return Err("invalid memory space".into());
        }
        let bytes = self
            .bytes
            .get(address as usize..address as usize + output.len())
            .ok_or("invalid memory read")?;
        output.copy_from_slice(bytes);
        Ok(())
    }
}
impl Reader for Core {
    fn read_register(&mut self, name: &str) -> Result<u32, Error> {
        match name {
            "pc" => Ok(self.pc),
            "fail" => Ok(self.fail),
            _ => Err("invalid register".into()),
        }
    }
}
impl Machine for Core {
    fn validate_update(&self, update: &tango_match::gamemode::Update) -> Result<(), Error> {
        for write in &update.writes {
            match write {
                Write::Memory { space, address, data }
                    if space == "main" && u64::from(*address) + data.len() as u64 <= self.bytes.len() as u64 => {}
                Write::Register { name, value } if name == "pc" && *value < 256 && self.fail == 0 => {}
                _ => return Err("rejected write".into()),
            }
        }
        Ok(())
    }
    fn apply_writes(&mut self, writes: &[Write]) {
        self.applied += 1;
        for write in writes {
            match write {
                Write::Memory { address, data, .. } => {
                    self.bytes[*address as usize..*address as usize + data.len()].copy_from_slice(data)
                }
                Write::Register { value, .. } => self.pc = *value,
            }
        }
    }
}

#[test]
fn callbacks_use_emulator_state_and_failed_writes_are_atomic() {
    let profile = profile(BODY, "");
    let mut hooks = controller(profile.clone(), context());
    let mut core = Core::default();
    assert_eq!(
        invoke(&mut hooks, &mut core).unwrap(),
        [Event::RoundOutcome { winner: Some(1) }]
    );
    assert_eq!((core.bytes[0], core.pc, core.applied), (1, 1, 1));
    let capture = hooks.snapshot();
    let core_capture = core.clone();
    for fail in [1, 2] {
        core.fail = fail;
        assert!(invoke(&mut hooks, &mut core).is_err());
        assert_eq!((core.bytes[0], core.pc, core.applied), (1, 1, 1));
    }
    core.fail = 0;
    assert!(hooks.on_hook("unknown", &mut core).is_err());
    invoke(&mut hooks, &mut core).unwrap();
    assert_eq!((core.bytes[0], core.pc), (2, 2));
    hooks.restore(&capture).unwrap();
    core = core_capture.clone();
    invoke(&mut hooks, &mut core).unwrap();
    assert_eq!((core.bytes[0], core.pc), (2, 2));

    // A fresh worker recreates captured constants; only emulator state rewinds.
    let mut other = controller(profile.clone(), context());
    other.restore(&capture).unwrap();
    core = core_capture;
    invoke(&mut other, &mut core).unwrap();
    assert_eq!(core.bytes[0], 2);
    for changed in [
        Context { player: 2, ..context() },
        Context {
            seed: [8; 16],
            ..context()
        },
        Context {
            options: [("option".into(), Value::Boolean(true))].into(),
            ..context()
        },
    ] {
        assert!(controller(profile.clone(), changed).restore(&capture).is_err());
    }
}

#[test]
fn direct_reads_observe_overlapping_writes_and_buffers_are_copied() {
    let body = r#"
        local bytes = buffer.fromstring("abcd")
        game.write("main", 1, bytes)
        buffer.writeu8(bytes, 0, 0)
        game.write("main", 2, buffer.fromstring("XY"))
        assert(buffer.tostring(game.read("main", 0, 6)) == "\0aXYd\0")
        game.write("main", 0, buffer.fromstring("01"))
        assert(buffer.tostring(game.read("main", 0, 6)) == "01XYd\0")
        game.write_register("pc", 1)
        game.write_register("pc", 2)
        assert(game.read_register("pc") == 2)
        game.ready()
        game.round_started()
        game.round_outcome(nil)
        game.match_ended()
        game.match_aborted()
    "#;
    let mut hooks = controller(profile(body, ""), context());
    let mut core = Core::default();
    assert_eq!(
        invoke(&mut hooks, &mut core).unwrap(),
        [
            Event::Ready,
            Event::RoundStarted,
            Event::RoundOutcome { winner: None },
            Event::MatchEnded,
            Event::MatchAborted
        ]
    );
    assert_eq!(&core.bytes[..6], b"01XYd\0");
    assert_eq!(core.pc, 2);
}

#[test]
fn direct_operations_are_scoped_and_bounded() {
    for body in [
        "game.write_register('pc', 1.5)",
        "(game :: any).write_register('pc', '1')",
        "game.round_outcome(0)",
        "game.write('main', 0xffffffff, buffer.create(2))",
        "game.write('main', 0, buffer.create(4 * 1024 * 1024 + 1))",
        "for _ = 1, 4097 do game.read_register('pc') end",
        "for _ = 1, 4097 do game.write_register('pc', 0) end",
        "for _ = 1, 129 do game.ready() end",
        "(game :: any).read = function() return buffer.create(0) end",
        "game.read('main', 0.5, 1)",
        "game.hook(257, function() end)",
        "game.write_register('pc', 1); return {} :: any",
    ] {
        let mut hooks = controller(profile(body, ""), context());
        let mut core = Core::default();
        assert!(invoke(&mut hooks, &mut core).is_err(), "accepted {body}");
        assert_eq!(core.applied, 0);
    }
    for setup in [
        "game.hook(256, function() end)",
        "game.hook(1.5, function() end)",
        "for address = 1, 257 do game.hook(address, function() end) end",
        "game.read('main', 0, 1)",
        "game.write('main', 0, buffer.create(1))",
        "game.read_register('pc')",
        "game.write_register('pc', 1)",
        "game.ready()",
        "game.round_outcome(nil)",
    ] {
        assert!(
            GameModeProgram::new(profile("", setup), context()).is_err(),
            "accepted {setup}"
        );
    }
    for invalid in [
        Context { player: 0, ..context() },
        Context {
            options: [("option".into(), Value::Number(f64::NAN))].into(),
            ..context()
        },
    ] {
        assert!(GameModeProgram::new(profile(BODY, ""), invalid).is_err());
    }
}
