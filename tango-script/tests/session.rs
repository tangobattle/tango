use tango_script::{Action, EditCommand, EditorSession, Effect, Package, PackageRef, Profile};

fn profile(reset: &str, encode: &str) -> Profile {
    let source = format!(
        r#"--!strict
return {{
    decode = function(bytes: buffer): buffer
        assert(buffer.len(bytes) == 2 and buffer.readu8(bytes, 0) == buffer.readu8(bytes, 1), "bad checksum")
        return bytes
    end,
    encode = function(bytes: buffer): buffer {encode} end,
    validate = function(bytes: buffer): {{string}} return if buffer.readu8(bytes, 0) == 99 then {{"illegal value"}} else {{}} end,
    reset_view = function(state: ViewState): () {reset} end,
    view = function(bytes: buffer, state: ViewState, context: EditorContext): Node
        local session = context.session
        assert(session)
        assert(not state.fail_view or session.editing, "exit view failed")
        local children: {{Node}} = {{
            {{kind = "text", text = tostring(buffer.readu8(bytes, 0)) .. ":" .. tostring(buffer.readu8(bytes, 1))
                .. ":" .. tostring(session.editing) .. ":" .. tostring(session.saving) .. ":" .. tostring(session.can_save)
                .. ":" .. (state.pref or "") .. ":" .. (state.draft or "")}} :: Node,
            {{kind = "input", id = "value"}} :: Node,
            {{kind = "input", id = "pref"}} :: Node,
            {{kind = "input", id = "draft"}} :: Node,
        }}
        for _, id in {{"begin", "cancel", "save", "copy", "forge", "mixed", "mutate_begin", "fail_view"}} do
            table.insert(children, {{kind = "button", id = id}} :: Node)
        end
        return {{kind = "column", children = children}}
    end,
    update = function(bytes: buffer, state: ViewState, action: Action, context: EditorContext): {{Effect}}?
        if action.kind == "change" then
            if action.id == "value" then buffer.writeu8(bytes, 0, tonumber(action.value) :: number)
            else state[action.id] = action.value end
        elseif action.kind == "activate" then
            if action.id == "begin" then return {{{{kind = "edit", command = "begin"}} :: Effect}} end
            if action.id == "cancel" then return {{{{kind = "edit", command = "cancel"}} :: Effect}} end
            if action.id == "save" then return {{{{kind = "edit", command = "save"}} :: Effect}} end
            if action.id == "copy" then return {{{{kind = "copy_text", text = tostring(buffer.readu8(bytes, 0))}} :: Effect}} end
            if action.id == "forge" then
                local session = context.session :: EditContext; session.editable = true; context.read_only = false
                state.pref = "forged"
                return {{{{kind = "edit", command = "begin"}} :: Effect}}
            end
            if action.id == "mutate_begin" then
                buffer.writeu8(bytes, 0, 42)
                return {{{{kind = "edit", command = "begin"}} :: Effect}}
            end
            if action.id == "mixed" then
                state.pref = "mixed"
                return {{{{kind = "edit", command = "begin"}} :: Effect, {{kind = "edit", command = "save"}} :: Effect}}
            end
            if action.id == "fail_view" then state.fail_view = "yes" end
        end
        return nil
    end,
}}"#
    );
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"name='session'\nversion='1.0.0'\napi=1\n[[editor]]\nname='main'\npath='./init'\n".to_vec(),
            ),
            ("init.luau".into(), source.into_bytes()),
        ]
        .into(),
    )
    .unwrap();
    Profile::resolve(
        &[package],
        &PackageRef {
            name: "session".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap()
}

fn session(editable: bool) -> EditorSession {
    EditorSession::open(
        profile(
            "state.draft = nil",
            "buffer.writeu8(bytes, 1, buffer.readu8(bytes, 0)); return bytes",
        ),
        &[7, 7],
        editable,
    )
    .unwrap()
}
fn send(session: &mut EditorSession, command: &str) -> tango_script::SessionUpdate {
    session.dispatch(Action::activate(command, "")).unwrap()
}
fn text(session: &EditorSession) -> &str {
    let tango_script::ui::Kind::Column { children } = &session.document().view().kind else {
        panic!()
    };
    let tango_script::ui::Kind::Text { text, .. } = &children[0].kind else {
        panic!()
    };
    text
}

#[test]
fn embedding_configuration_revokes_edits_without_discarding_the_draft() {
    let mut s = session(true);
    send(&mut s, "begin");
    s.dispatch(Action::change("value", "9")).unwrap();
    s.dispatch(Action::change("draft", "unsaved input")).unwrap();
    s.configure("ja-JP", false, Default::default()).unwrap();
    assert_eq!(s.document().locale().to_string(), "ja-JP");
    assert!(s.is_editing() && s.document().is_read_only() && s.document().can_undo());
    assert_eq!(text(&s), "9:7:true:false:false::unsaved input");
    assert!(s.dispatch(Action::change("value", "10")).is_err());
    assert!(s.edit(EditCommand::Save).is_err());
    assert!(s.undo().is_err() && s.redo().is_err());
    assert_eq!(s.document().snapshot().unwrap(), [9, 9]);
    s.configure("ja-JP", true, Default::default()).unwrap();
    assert!(!s.document().is_read_only());
    s.undo().unwrap();
    assert_eq!(s.document().snapshot().unwrap(), [7, 7]);
    s.redo().unwrap();
    assert_eq!(s.document().snapshot().unwrap(), [9, 9]);
    s.configure("en-US", false, Default::default()).unwrap();
    send(&mut s, "cancel");
    assert!(!s.is_editing());
    assert_eq!(s.document().snapshot().unwrap(), [7, 7]);
    assert!(s.edit(EditCommand::Begin).is_err());
}

#[test]
fn identical_configuration_is_allowed_while_a_write_is_pending() {
    let mut s = session(true);
    send(&mut s, "begin");
    s.dispatch(Action::change("value", "9")).unwrap();
    let request = send(&mut s, "save").save.unwrap();
    let pending_view = s.document().view().clone();
    for _ in 0..3 {
        s.configure("en-US", true, Default::default()).unwrap();
    }
    assert!(s.configure("en-US", false, Default::default()).is_err());
    assert!(s.configure("ja-JP", true, Default::default()).is_err());
    assert_eq!(s.document().view(), &pending_view);
    s.finish_save(&request, false).unwrap();
    assert!(s.is_editing() && !s.document().is_read_only());
    assert_eq!(s.document().snapshot().unwrap(), [9, 9]);
}

#[test]
fn whole_document_cancel_restores_baseline_and_preserves_selected_preferences() {
    let mut s = session(true);
    s.dispatch(Action::change("pref", "tab two")).unwrap();
    s.dispatch(Action::change("draft", "old draft")).unwrap();
    send(&mut s, "begin");
    assert!(s.is_editing() && !s.document().is_read_only());
    assert_eq!(text(&s), "7:7:true:false:true:tab two:");
    s.dispatch(Action::change("value", "9")).unwrap();
    s.dispatch(Action::change("draft", "new draft")).unwrap();
    assert!(s.document().is_dirty() && s.document().can_undo());
    assert_eq!(s.document().snapshot().unwrap(), [9, 9]);
    assert!(text(&s).starts_with("9:7:"), "snapshot must repair a clone");
    send(&mut s, "cancel");
    assert_eq!(text(&s), "7:7:false:false:false:tab two:");
    assert!(!s.is_editing() && !s.document().is_dirty() && !s.document().can_undo() && !s.document().can_redo());
    assert_eq!(s.document().snapshot().unwrap(), [7, 7]);
    assert!(s.undo().is_err() && s.redo().is_err());
}

#[test]
fn failed_writes_keep_edits_and_success_adopts_exact_encoded_model() {
    let mut s = session(true);
    send(&mut s, "begin");
    s.dispatch(Action::change("value", "9")).unwrap();
    s.dispatch(Action::change("draft", "pending input")).unwrap();
    let before = s.document().view().clone();
    let request = send(&mut s, "save").save.unwrap();
    assert_eq!(request.bytes(), [9, 9]);
    assert!(s.is_editing() && s.is_saving() && s.document().is_dirty());
    assert!(text(&s).contains(":true:true:false:"));
    assert!(s.dispatch(Action::change("value", "10")).is_err());
    assert!(s.edit(EditCommand::Cancel).is_err() && s.undo().is_err() && s.set_locale("ja-JP").is_err());
    s.finish_save(&request, false).unwrap();
    assert_eq!(s.document().view(), &before);
    assert!(s.is_editing() && s.document().is_dirty() && s.document().can_undo());
    let retry = send(&mut s, "save").save.unwrap();
    assert!(
        s.finish_save(&request, true).is_err(),
        "a failed receipt cannot acknowledge a retry"
    );
    s.finish_save(&retry, true).unwrap();
    assert_eq!(text(&s), "9:9:false:false:false::");
    assert!(!s.is_editing() && !s.is_saving() && !s.document().is_dirty() && !s.document().can_undo());
    assert!(s.finish_save(&retry, true).is_err());
    send(&mut s, "begin");
    s.dispatch(Action::change("value", "12")).unwrap();
    send(&mut s, "cancel");
    assert_eq!(
        s.document().snapshot().unwrap(),
        [9, 9],
        "cancel now restores the last successful write"
    );
}

#[test]
fn edit_capabilities_and_transitions_are_enforced_before_any_state_commits() {
    for editable in [false, true] {
        let mut s = session(editable);
        for action in [
            Action::activate("forge", ""),
            Action::activate("mixed", ""),
            Action::activate("mutate_begin", ""),
            Action::activate("save", ""),
            Action::activate("cancel", ""),
            Action::change("value", "42"),
        ] {
            if editable && action.id == "forge" {
                continue;
            }
            let view = s.document().view().clone();
            assert!(s.dispatch(action).is_err());
            assert_eq!(s.document().view(), &view);
            assert_eq!(s.document().snapshot().unwrap(), [7, 7]);
        }
        let copied = send(&mut s, "copy");
        assert!(matches!(&copied.effects[..], [Effect::CopyText{text}] if text == "7"));
        if !editable {
            assert!(s.edit(EditCommand::Begin).is_err());
        }
    }
    let mut s = session(true);
    s.edit(EditCommand::Begin).unwrap();
    assert!(s.edit(EditCommand::Begin).is_err());
    s.dispatch(Action::change("value", "99")).unwrap();
    let before = s.document().view().clone();
    assert!(!text(&s).contains(":true:false:true:"));
    assert!(s.edit(EditCommand::Save).is_err());
    assert_eq!(s.document().view(), &before);
    assert_eq!(
        s.document().snapshot().unwrap(),
        [99, 99],
        "build warnings do not prevent a session snapshot"
    );
    s.edit(EditCommand::Cancel).unwrap();
}

#[test]
fn save_receipts_are_scoped_to_their_session() {
    let mut a = session(true);
    let mut b = session(true);
    send(&mut a, "begin");
    send(&mut b, "begin");
    let first = send(&mut a, "save").save.unwrap();
    let second = send(&mut b, "save").save.unwrap();
    assert!(a.finish_save(&second, true).is_err());
    assert!(b.finish_save(&first, true).is_err());
    a.finish_save(&first, true).unwrap();
    b.finish_save(&second, false).unwrap();
    assert!(!a.is_editing() && b.is_editing());
}

#[test]
fn transition_and_encoder_failures_happen_before_the_host_gets_a_write() {
    for (reset, encoder) in [
        (
            "error('reset failed')",
            "buffer.writeu8(bytes, 1, buffer.readu8(bytes, 0)); return bytes",
        ),
        ("state.draft = nil", "error('encoder failed'); return bytes"),
        ("state.draft = nil", "return buffer.create(1)"),
    ] {
        let mut s = EditorSession::open(profile(reset, encoder), &[7, 7], true).unwrap();
        if reset.starts_with("error") {
            assert!(s.edit(EditCommand::Begin).is_err());
            assert!(!s.is_editing());
        } else {
            send(&mut s, "begin");
            s.dispatch(Action::change("value", "9")).unwrap();
            let before = s.document().view().clone();
            assert!(s.document().snapshot().is_err());
            assert!(s.edit(EditCommand::Save).is_err());
            assert_eq!(s.document().view(), &before);
            assert!(s.is_editing() && !s.is_saving() && s.document().is_dirty());
        }
    }
    let mut s = session(true);
    send(&mut s, "begin");
    send(&mut s, "fail_view");
    let before = s.document().view().clone();
    assert!(
        s.edit(EditCommand::Save).is_err(),
        "prepare the exit view before asking the host to write"
    );
    assert_eq!(s.document().view(), &before);
    assert!(s.is_editing() && !s.is_saving());
}
