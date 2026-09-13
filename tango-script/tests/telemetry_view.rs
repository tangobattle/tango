use std::collections::BTreeMap;
use tango_match::telemetry::stream::{Batch, Context, Event, Frame, Record, Timeline, Value};
use tango_script::{Action, Package, PackageRef, Profile, TelemetryViewContext};

fn profile(view: &str, update: &str) -> Profile {
    let source = format!("--!strict\nreturn {{poll=function(): TelemetryFrame return {{values={{}},events={{}}}} end, view=function(history: TelemetryHistory, context: TelemetryViewContext, state: ViewState): Node {view} end, update_view=function(state: ViewState, action: Action): ViewState {update} end}}");
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='telemetry-view'\nversion='1.0.0'\n[[telemetry]]\nname='main'\npath='./view'".to_vec(),
            ),
            ("view.luau".into(), source.into_bytes()),
            ("init.luau".into(), b"--!strict\nerror('unrelated module')".to_vec()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "telemetry-view".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}
fn history() -> Timeline {
    let mut history = Timeline::new([None, None]);
    history.apply(Batch {
        rewind_to: None,
        records: vec![Record {
            context: Context {
                tick: 1,
                round: 0,
                player: 1,
            },
            frame: Ok(Frame {
                values: [
                    ("meter".into(), Value::Number(7.0)),
                    ("label".into(), Value::String("text".into())),
                    ("active".into(), Value::Boolean(true)),
                ]
                .into(),
                events: vec![Event {
                    name: "event".into(),
                    fields: BTreeMap::new(),
                }],
            }),
        }],
    });
    history
}
fn context() -> TelemetryViewContext {
    TelemetryViewContext {
        player: 1,
        from_tick: 0,
        to_tick: 3,
        ticks_per_second: 60.0,
        incomplete: false,
    }
}
#[test]
fn history_queries_return_typed_values_and_nil_gaps_and_ui_state_is_separate() {
    let profile = profile(
        r#"
local range = {player=1,name="meter",from_tick=0,to_tick=3}
local points = history.series(range,4)
assert(#points == 4 and points[1].first == nil and points[1].gap)
assert(points[2].first == 7 and not points[2].gap)
assert(history.value({player=1,name="label",tick=1}) == "text")
assert(history.value({player=1,name="active",tick=1}) == true)
assert(history.value({player=1,name="label",tick=0}) == nil)
assert(history.count_events({player=1,name="event",from_tick=0,to_tick=3}) == 1)
assert(history.count_events({player=2,name="event",from_tick=0,to_tick=3}) == 0)
return {kind="text",text=state.caption or "initial"}
"#,
        "state.caption = action.id; return state",
    );
    let state = BTreeMap::new();
    let before = profile.view_telemetry(&history(), context(), &state).unwrap().unwrap();
    let after = profile
        .update_telemetry_view(&state, &Action::activate("changed", ""))
        .unwrap();
    assert!(state.is_empty());
    let node = profile.view_telemetry(&history(), context(), &after).unwrap().unwrap();
    assert_ne!(before, node);
}
#[test]
fn queries_validate_ranges_resolutions_and_shared_work_limits() {
    for query in [
        "history.series({player=0,name='n',from_tick=0,to_tick=2},1)",
        "history.series({player=1,name='n',from_tick=2,to_tick=1},1)",
        "history.series({player=1,name='n',from_tick=0,to_tick=1},2049)",
        "history.series({player=1,name='n',from_tick=0,to_tick=1},1.5)",
        "history.series({player=1,name='n',from_tick=0,to_tick=1},'2' :: any)",
        "history.value({player=1,name='n',tick=-1})",
        "history.count_events({player=1,name='n',from_tick=0,to_tick=0/0})",
        "for i=1,65 do history.count_events({player=1,name='n',from_tick=0,to_tick=1}) end",
        "for i=1,5 do history.series({player=1,name='n',from_tick=0,to_tick=10000},2048) end",
        "(history :: any).value = nil",
    ] {
        let p = profile(
            &format!("{query}; return {{kind='text',text='unexpected'}}"),
            "return state",
        );
        assert!(
            p.view_telemetry(&history(), context(), &BTreeMap::new()).is_err(),
            "accepted: {query}"
        );
    }
}
#[test]
fn failed_view_updates_leave_the_original_state_intact_and_incomplete_is_authoritative() {
    let p = profile(
        "assert(context.incomplete); return {kind='text',text='partial'}",
        "state.changed='yes'; state.bad=string.rep('x',4097); return state",
    );
    let mut history = history();
    history.apply(Batch {
        rewind_to: None,
        records: vec![Record {
            context: Context {
                tick: 2,
                round: 0,
                player: 1,
            },
            frame: Err("failed".into()),
        }],
    });
    p.view_telemetry(&history, context(), &BTreeMap::new()).unwrap();
    let state = BTreeMap::from([("unchanged".into(), "yes".into())]);
    assert!(p.update_telemetry_view(&state, &Action::activate("x", "")).is_err());
    assert_eq!(state.len(), 1);
}
