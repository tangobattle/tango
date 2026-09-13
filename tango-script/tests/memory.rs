use tango_script::{Package, PackageRef, Profile, TelemetryContext, TelemetryValue};

fn profile(body: &str) -> Profile {
    let source = format!(
        r#"--!strict
local retained: Memory? = nil
return {{poll = function(state: buffer, context: TelemetryContext, memory: Memory): TelemetryFrame
    assert(retained == nil)
    retained = memory;
    {body}
end}}
"#
    );
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='memory-test'\nversion='1.0.0'\n[[telemetry]]\nname='main'\npath='./init'\n".to_vec(),
            ),
            ("init.luau".into(), source.into_bytes()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "memory-test".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

const CONTEXT: TelemetryContext = TelemetryContext {
    tick: 7,
    round: 2,
    player: 1,
};
const READ: &str = r#"
    local bytes = memory.read("main", 0x02000000, 2)
    buffer.writeu8(state, 0, buffer.readu8(bytes, 0))
    buffer.writeu8(bytes, 0, 0) -- returned buffers are independent
    local again = memory.read("main", 0x02000000, 2)
    return {values = {value = buffer.readu8(again, 0), round = context.round}, events = {}}
"#;

#[test]
fn borrowed_memory_readers_are_scoped_isolated_and_read_only() {
    let profile = profile(READ);
    let state = [0];
    for first in [7, 23] {
        let source = [first, 99];
        let mut calls = 0;
        let sample = profile
            .poll_telemetry_with_memory(&state, CONTEXT, |space, address, output| {
                assert_eq!(space, "main");
                assert_eq!(address, 0x02000000);
                assert_eq!(output.len(), 2);
                output.copy_from_slice(&source);
                calls += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(sample.state, [first]);
        assert_eq!(source, [first, 99]);
        assert_eq!(sample.frame.values["value"], TelemetryValue::Number(first as f64));
        assert_eq!(sample.frame.values["round"], TelemetryValue::Number(2.0));
    }
    assert_eq!(state, [0]);
    assert!(profile.poll_telemetry(&state, CONTEXT).is_err());
    let error = profile
        .poll_telemetry_with_memory(&state, CONTEXT, |_, _, _| {
            Err(tango_script::Error::Invalid("unknown address space".into()))
        })
        .unwrap_err();
    assert!(error.to_string().contains("unknown address space"));
    // Failed calls also expire their borrowed reader and module captures.
    assert!(profile
        .poll_telemetry_with_memory(&state, CONTEXT, |_, _, output| {
            output.fill(5);
            Ok(())
        })
        .is_ok());
    assert_eq!(state, [0]);
}

#[test]
fn memory_arguments_are_checked_before_the_host_can_read() {
    for args in [
        "'main', -1, 1",
        "'main', 1.5, 1",
        "'main', 0/0, 1",
        "'main', math.huge, 1",
        "'main', 0x100000000, 0",
        "'main', 0xffffffff, 2",
        "'main', 0, -1",
        "'main', 0, 0.5",
        "'main', 0, 0/0",
        "'main', 0, math.huge",
        "'main', 0, 1048577",
        "'', 0, 1",
        "string.rep('x', 65), 0, 1",
        "nil, 0, 1",
        "1, 0, 1",
        "'main', '0', 1",
        "'main', 0, '1'",
    ] {
        let source = format!("(memory.read :: any)({args}); return {{values = {{}}, events = {{}}}}");
        let profile = profile(&source);
        let mut calls = 0;
        assert!(
            profile
                .poll_telemetry_with_memory(&[], CONTEXT, |_, _, _| {
                    calls += 1;
                    Ok(())
                })
                .is_err(),
            "{args}"
        );
        assert_eq!(calls, 0, "{args}");
    }
    let profile = profile(
        "memory.read('main', 0xffffffff, 1); memory.read('main', 0xffffffff, 0); return {values = {}, events = {}}",
    );
    let mut lengths = Vec::new();
    profile
        .poll_telemetry_with_memory(&[], CONTEXT, |_, address, output| {
            assert_eq!(address, u32::MAX);
            lengths.push(output.len());
            Ok(())
        })
        .unwrap();
    assert_eq!(lengths, [1, 0]);
}

#[test]
fn memory_reads_have_aggregate_byte_and_call_budgets() {
    for (count, size, expected, message) in [(5, 1048576, 4, "byte limit"), (4097, 0, 4096, "call limit")] {
        let source = format!(
            "for _ = 1, {count} do memory.read('main', 0, {size}) end; return {{values = {{}}, events = {{}}}}"
        );
        let profile = profile(&source);
        let mut calls = 0;
        let error = profile
            .poll_telemetry_with_memory(&[], CONTEXT, |_, _, _| {
                calls += 1;
                Ok(())
            })
            .unwrap_err();
        assert_eq!(calls, expected);
        assert!(error.to_string().contains(message), "{error}");
    }
    let profile =
        profile("(memory :: any).read = function() return buffer.create(0) end; return {values = {}, events = {}}");
    assert!(profile
        .poll_telemetry_with_memory(&[], CONTEXT, |_, _, _| Ok(()))
        .is_err());
}
