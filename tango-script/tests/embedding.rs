use tango_script::{Action, EditCommand, EditorSession, Effect, Embedding, HostAction, Package, PackageRef, Profile};

fn session(embedding: Embedding) -> EditorSession {
    let package = Package::load(
        [
            (
                "package.toml".into(),
                b"api=1\nname='embed'\nversion='1.0.0'\n[[editor]]\nname='main'\npath='./init'".to_vec(),
            ),
            (
                "init.luau".into(),
                br#"--!strict
return {
    decode = function(bytes: buffer): buffer return bytes end,
    encode = function(bytes: buffer): buffer return bytes end,
    validate = function(_bytes: buffer): {string} return {} end,
    view = function(bytes: buffer, state: ViewState, context: EditorContext): Node
        assert(not state.fail or context.embedding.inline_actions, "cannot render external controls")
        local offered = context.embedding.actions.run
        local children: {Node} = {
            {kind = "text", text = tostring(buffer.readu8(bytes, 0)) .. ":" .. (state.count or "0") .. ":"
                .. tostring(offered and offered.enabled) .. ":" .. tostring(context.embedding.inline_actions)} :: Node,
        }
        table.insert(children, {kind = "input", id = "forge"} :: Node)
        for _, id in {"invoke", "begin", "save", "mutate", "fail"} do
            table.insert(children, {kind = "button", id = id, value = if id == "fail" then "" else "run"} :: Node)
        end
        return {kind = "column", children = children}
    end,
    update = function(bytes: buffer, state: ViewState, action: Action, context: EditorContext): {Effect}?
        if action.kind ~= "activate" and action.kind ~= "change" then return nil end
        if action.id == "fail" then state.fail = "yes"; return nil end
        state.count = tostring((tonumber(state.count or "0") :: number) + 1)
        if action.id == "mutate" then buffer.writeu8(bytes, 0, 8) end
        if action.id == "forge" then
            context.embedding.actions[action.value] = {enabled = true, while_editing = true}
            if context.session then context.session.editing = false end
        end
        local effects: {Effect} = {{kind = "invoke", id = action.value} :: Effect}
        if action.id == "begin" or action.id == "save" then
            table.insert(effects, {kind = "edit", command = action.id :: EditCommand} :: Effect)
        end
        return effects
    end,
}"#
                .to_vec(),
            ),
        ]
        .into(),
    )
    .unwrap();
    let profile = Profile::resolve(
        &[package],
        &PackageRef {
            name: "embed".into(),
            version: "1.0.0".parse().unwrap(),
        },
    )
    .unwrap();
    EditorSession::open_embedded(profile, &[7], true, embedding).unwrap()
}

fn embedding(enabled: bool, while_editing: bool) -> Embedding {
    Embedding {
        actions: [("run".into(), HostAction { enabled, while_editing })].into(),
        ..Default::default()
    }
}

fn label(session: &EditorSession) -> String {
    serde_json::to_value(session.document().view()).unwrap()["children"][0]["text"]
        .as_str()
        .unwrap()
        .to_string()
}

#[test]
fn panel_configuration_changes_locale_permissions_and_controls_atomically() {
    let mut editor = session(embedding(true, true));
    editor.edit(EditCommand::Begin).unwrap();
    editor.dispatch(Action::activate("mutate", "run")).unwrap();
    editor.dispatch(Action::activate("fail", "")).unwrap();
    let before = editor.document().view().clone();
    let external = Embedding {
        inline_actions: false,
        ..embedding(true, true)
    };
    assert!(editor.configure("ja-JP", false, external).is_err());
    assert_eq!(editor.document().locale().to_string(), "en-US");
    assert!(!editor.document().is_read_only());
    assert_eq!(editor.document().view(), &before);
    assert!(editor.document().can_undo());
    assert_eq!(editor.document().snapshot().unwrap(), [8]);
    editor.configure("ja-JP", false, embedding(false, false)).unwrap();
    assert_eq!(editor.document().locale().to_string(), "ja-JP");
    assert!(editor.document().is_read_only());
    assert!(editor.dispatch(Action::activate("invoke", "run")).is_err());
    assert!(editor.dispatch(Action::activate("mutate", "run")).is_err());
    assert_eq!(editor.document().snapshot().unwrap(), [8]);
}

#[test]
fn host_actions_are_explicit_revocable_and_cannot_be_forged_by_scripts() {
    let mut editor = session(Embedding::default());
    for id in ["run", "", "missing", "\n", &"x".repeat(257)] {
        let before = editor.document().view().clone();
        assert!(editor.dispatch(Action::change("forge", id)).is_err());
        assert_eq!(editor.document().view(), &before);
    }
    editor.set_embedding(embedding(false, false)).unwrap();
    assert!(label(&editor).ends_with(":false:true"));
    assert!(editor.dispatch(Action::change("forge", "run")).is_err());
    editor.set_embedding(embedding(true, false)).unwrap();
    let update = editor.dispatch(Action::activate("invoke", "run")).unwrap();
    assert_eq!(update.effects, [Effect::Invoke { id: "run".into() }]);
    assert_eq!(label(&editor), "7:1:true:true");
    assert!(!editor.document().is_dirty());
    editor.set_embedding(Embedding::default()).unwrap();
    assert!(editor.dispatch(Action::activate("invoke", "run")).is_err());
    assert_eq!(editor.document().snapshot().unwrap(), [7]);
}

#[test]
fn actions_follow_edit_and_save_modes_before_effects_or_state_escape() {
    let mut editor = session(embedding(true, false));
    let before = editor.document().view().clone();
    assert!(editor.dispatch(Action::activate("begin", "run")).is_err());
    assert_eq!(editor.document().view(), &before);
    assert!(!editor.is_editing());
    editor.edit(EditCommand::Begin).unwrap();
    assert!(label(&editor).ends_with(":false:true"));
    assert!(editor.dispatch(Action::change("forge", "run")).is_err());
    assert!(editor.dispatch(Action::activate("mutate", "run")).is_err());
    assert_eq!(editor.document().snapshot().unwrap(), [7]);
    assert!(!editor.document().can_undo());
    editor.set_embedding(embedding(true, true)).unwrap();
    assert_eq!(
        editor
            .dispatch(Action::activate("mutate", "run"))
            .unwrap()
            .effects
            .len(),
        1
    );
    assert_eq!(editor.document().snapshot().unwrap(), [8]);
    let before = editor.document().view().clone();
    assert!(editor.dispatch(Action::activate("save", "run")).is_err());
    assert!(!editor.is_saving());
    assert_eq!(editor.document().view(), &before);
    let request = editor.edit(EditCommand::Save).unwrap().save.unwrap();
    assert!(label(&editor).ends_with(":false:true"));
    assert!(editor.set_embedding(Embedding::default()).is_err());
    assert!(editor.dispatch(Action::activate("invoke", "run")).is_err());
    editor.finish_save(&request, false).unwrap();
    assert_eq!(editor.document().view(), &before);
    editor.undo().unwrap();
    assert_eq!(editor.document().snapshot().unwrap(), [7]);
    editor.redo().unwrap();
    assert_eq!(editor.document().snapshot().unwrap(), [8]);
}

#[test]
fn embedding_refresh_is_atomic_and_bounded_without_resetting_drafts_or_history() {
    let mut editor = session(embedding(true, true));
    editor.edit(EditCommand::Begin).unwrap();
    editor.dispatch(Action::activate("mutate", "run")).unwrap();
    let mut external = embedding(true, true);
    external.inline_actions = false;
    editor.set_embedding(external.clone()).unwrap();
    assert_eq!(label(&editor), "8:1:true:false");
    assert!(editor.document().is_dirty() && editor.document().can_undo());
    editor.set_embedding(embedding(true, true)).unwrap();
    editor.dispatch(Action::activate("fail", "")).unwrap();
    let before = editor.document().view().clone();
    assert!(editor.set_embedding(external).is_err());
    for count in [65, 66] {
        let too_many = Embedding {
            actions: (0..count).map(|i| (i.to_string(), HostAction::default())).collect(),
            ..Default::default()
        };
        assert!(editor.set_embedding(too_many).is_err());
    }
    for id in ["", "bad\n", &"x".repeat(257)] {
        let bad = Embedding {
            actions: [(id.into(), HostAction::default())].into(),
            ..Default::default()
        };
        assert!(editor.set_embedding(bad).is_err());
    }
    assert_eq!(editor.document().view(), &before);
    assert!(editor.document().can_undo());
}
