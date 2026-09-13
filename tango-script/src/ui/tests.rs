use mlua::{Lua, LuaSerdeExt};

use super::*;

fn read(lua: &Lua, source: &str) -> Result<Node> {
    Node::read(lua, lua.load(source).eval()?)
}

#[test]
fn motion_descriptors_are_bounded_and_passive_content_shares_their_namespace() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {kind = "button", id = "choose",
        motion = {key = "arrival", revision = "1", from = {-24, 0}}}"#,
    )
    .unwrap();
    let motion = node.motion.as_ref().unwrap();
    assert_eq!((motion.duration_ms, motion.delay_ms), (160, 0));
    assert!(node.accepts(&Action::activate("choose", "")));
    for descriptor in [
        "{key = '', revision = '1', from = {0, 20}}",
        "{key = string.rep('x', 1025), revision = '1', from = {0, 20}}",
        "{key = 'x', revision = string.rep('x', 1025), from = {0, 20}}",
        "{key = 'x', revision = '1', from = {0/0, 20}}",
        "{key = 'x', revision = '1', from = {0, 16385}}",
        "{key = 'x', revision = '1', from = {0}}",
        "{key = 'x', revision = '1', from = {0, 20, 40}}",
        "{key = 'x', revision = '1', from = {0, 20}, easing = 'unknown'}",
        "{key = 'x', revision = '1', from = {0, 20}, duration_ms = 60001}",
        "{key = 'x', revision = '1', from = {0, 20}, duration_ms = -1}",
        "{key = 'x', revision = '1', from = {0, 20}, duration_ms = 1.5}",
        "{key = 'x', revision = '1', from = {0, 20}, delay_ms = 60001}",
        "{key = 'x', revision = '1', from = {0, 20}, repeat_forever = true}",
    ] {
        assert!(
            read(&lua, &format!("return {{kind = 'space', motion = {descriptor}}}")).is_err(),
            "{descriptor}"
        );
    }
    for accessory in [
        "tooltip = {content = other}",
        "feedback = {child = other}",
        "feedback = {child = {kind = 'space'}, tooltip = {content = other}}",
    ] {
        assert!(
            read(
                &lua,
                &format!(
                    r#"
            local motion = {{key = "same", revision = "0", from = {{0, 20}}}}
            local other = {{kind = "space", motion = motion}}
            return {{kind = "button", id = "choose", motion = motion, {accessory}}}
        "#
                )
            )
            .is_err(),
            "{accessory}"
        );
    }
    assert!(read(&lua, r#"
        local children = {}
        for i = 1, 1025 do children[i] = {kind = "space", motion = {key = tostring(i), revision = "0", from = {0, 20}}} end
        return {kind = "column", children = children}
    "#).is_err());
}

#[test]
fn scroll_events_require_an_enabled_registered_listener_and_finite_offsets() {
    let lua = crate::runtime::new_lua();
    let mut node = read(
        &lua,
        r#"return {kind = "scroll", id = "tabs", on_scroll = true,
        child = {kind = "button", id = "child"}}"#,
    )
    .unwrap();
    let event = |id: &str, x, y| Action {
        id: id.into(),
        event: Event::Scroll { x, y },
    };
    assert!(node.accepts(&event("tabs", 0.5, 0.0)));
    assert!(node.accepts(&Action::activate("child", "")));
    assert!(!node.accepts(&event("other", 0.5, 0.0)));
    assert!(!node.accepts(&Action::activate("tabs", "")));
    for n in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        assert!(!node.accepts(&event("tabs", n, 0.0)));
        assert!(!node.accepts(&event("tabs", 0.0, n)));
    }
    node.enabled = false;
    assert!(!node.accepts(&event("tabs", 0.5, 0.0)));
    assert!(!node.accepts(&Action::activate("child", "")));
    let silent = read(
        &lua,
        r#"return {kind = "scroll", id = "tabs", child = {kind = "space"}}"#,
    )
    .unwrap();
    assert!(!silent.accepts(&event("tabs", 0.5, 0.0)));
    for source in [
        r#"return {kind = "scroll", on_scroll = true, child = {kind = "space"}}"#,
        r#"return {kind = "scroll", id = "tabs", child = {kind = "button", id = "tabs"}}"#,
        r#"return {kind = "scroll", scrollbar = "unknown", child = {kind = "space"}}"#,
    ] {
        assert!(read(&lua, source).is_err(), "{source}");
    }
}

#[test]
fn gradients_validate_their_stops_and_roundtrip_without_changing_colors() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {kind = "space", appearance = {background = {angle = 1.5, stops = {
        {offset = 0, color = "plate"}, {offset = 0.5, color = {1, 0, 0, 0.5}},
        {offset = 1, color = {theme = "plate", alpha = 0}},
    }}}}"#,
    )
    .unwrap();
    let encoded = lua
        .to_value_with(
            &node,
            mlua::serde::SerializeOptions::new()
                .set_array_metatable(false)
                .serialize_none_to_null(false),
        )
        .unwrap();
    let restored = Node::read(&lua, encoded.as_table().unwrap().clone()).unwrap();
    assert_eq!(node, restored);
    for gradient in [
        "{angle = 0/0, stops = {{offset = 0, color = 'plate'}}}",
        "{angle = 0, stops = {}}",
        "{angle = 0, stops = table.create(9, {offset = 0, color = 'plate'})}",
        "{angle = 0, stops = {{offset = -0.1, color = 'plate'}}}",
        "{angle = 0, stops = {{offset = 0/0, color = 'plate'}}}",
        "{angle = 0, stops = {{offset = 0.5, color = 'plate'}, {offset = 0.4, color = 'plate'}}}",
        "{angle = 0, stops = {{offset = 0, color = 'plate'}, {offset = 0, color = 'plate'}}}",
        "{angle = 0, stops = {{offset = 0, color = {1, 0, 0, 2}}}}",
        "{angle = 0, stops = {{offset = 0, color = 'plate', unknown = true}}}",
        "{angle = 0, stops = {{offset = 0, color = 'plate'}}, unknown = true}",
    ] {
        assert!(
            read(
                &lua,
                &format!("return {{kind = 'space', appearance = {{background = {gradient}}}}}")
            )
            .is_err(),
            "{gradient}"
        );
    }
}

#[test]
fn serde_decodes_tagged_nodes_defaults_and_empty_children() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {
        kind = "row", width = "fill", padding = {1, 2, 3, 4}, align_y = "center",
        children = {
            {kind = "text", text = "Label", size = "caption", font = "mono", color = {1, 0, 0, 1}},
            {kind = "button", id = "go", text = "Go"},
            {kind = "column", children = {}},
            {kind = "space", width = {portion = 2}},
        },
    }"#,
    )
    .unwrap();
    assert!(node.enabled);
    assert_eq!(node.layout.width, Size::Named(style::SizeName::Fill));
    assert_eq!(node.layout.padding.sides(), [1.0, 2.0, 3.0, 4.0]);
    let Kind::Row { children } = &node.kind else { panic!() };
    assert!(matches!(&children[2].kind, Kind::Column { children } if children.is_empty()));
    assert!(node.accepts(&Action::activate("go", "")));
    assert!(!node.accepts(&Action::change("go", "")));
    let value = lua
        .to_value_with(
            &node,
            mlua::serde::SerializeOptions::new()
                .set_array_metatable(false)
                .serialize_none_to_null(false),
        )
        .unwrap();
    assert_eq!(node, Node::read(&lua, value.as_table().unwrap().clone()).unwrap());
}

#[test]
fn button_feedback_is_passive_and_shares_view_resource_limits() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {kind = "button", id = "copy", feedback = {
        child = {kind = "button", id = "hidden", text = "Done"},
        tooltip = {content = {kind = "button", id = "hidden_tip", text = "Copied"}},
    }}"#,
    )
    .unwrap();
    assert!(node.accepts(&Action::activate("copy", "")));
    assert!(!node.accepts(&Action::activate("hidden", "")));
    assert!(!node.accepts(&Action::activate("hidden_tip", "")));
    for feedback in [
        r#"{child = {kind = "image", image = {width = 1, height = 1, rgba = "rgba"}}}"#,
        r#"{child = {kind = "text"}, tooltip = {gap = -1, content = {kind = "text"}}}"#,
        r#"{child = {kind = "column", children = table.create(32768, {kind = "text"})}}"#,
        r#"{child = {kind = "text"}, tooltip = {content = {kind = "column", children = table.create(32768, {kind = "text"})}}}"#,
    ] {
        assert!(
            read(
                &lua,
                &format!("return {{kind = 'button', id = 'copy', feedback = {feedback}}}")
            )
            .is_err(),
            "{feedback}"
        );
    }
}

#[test]
fn serde_reads_buffer_pixels_and_nested_drawing_commands() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {
        kind = "canvas", canvas_width = 32, canvas_height = 16,
        commands = {
            {kind = "rect", width = 10, height = 5, fill = "primary"},
            {kind = "ellipse", rx = 2, ry = 3},
            {kind = "path", points = {{0, 0}, {1, 1}}, closed = true, line_cap = "square"},
            {kind = "text", text = "Test"},
            {kind = "image", width = 1, height = 1, image = {width = 1, height = 1, rgba = buffer.fromstring("rgba")}},
        },
    }"#,
    )
    .unwrap();
    let Kind::Canvas { commands, .. } = node.kind else {
        panic!()
    };
    let Draw::Image { image, nearest, .. } = &commands[4] else {
        panic!()
    };
    assert!(*nearest);
    assert_eq!(&*image.rgba, b"rgba");
    assert_ne!(image.digest, [0; 32]);
    let Draw::Rect { paint, .. } = &commands[0] else {
        panic!()
    };
    assert_eq!(paint.stroke, Color::transparent());
    assert_eq!(paint.stroke_width, 1.0);
    assert_eq!(paint.line_cap, LineCap::Butt);
    let Draw::Path { paint, .. } = &commands[2] else {
        panic!()
    };
    assert_eq!(paint.line_cap, LineCap::Square);
}

#[test]
fn rasterized_canvases_share_image_budgets_and_require_pixel_dimensions() {
    let lua = crate::runtime::new_lua();
    read(
        &lua,
        r#"return {kind = "canvas", canvas_width = 4096, canvas_height = 4096,
        commands = {}, rasterize = {corner_radius = 4, nearest = true}}"#,
    )
    .unwrap();
    for fields in [
        "canvas_width = 4097, canvas_height = 1",
        "canvas_width = 1.5, canvas_height = 1",
        "canvas_width = 1, canvas_height = 0/0",
        "canvas_width = 1, canvas_height = 1, rasterize = {corner_radius = 4097}",
        "canvas_width = 1, canvas_height = 1, rasterize = {corner_radius = -1}",
    ] {
        assert!(
            read(
                &lua,
                &format!("return {{kind = 'canvas', commands = {{}}, rasterize = {{}}, {fields}}}")
            )
            .is_err(),
            "{fields}"
        );
    }
    let error = read(
        &lua,
        r#"return {kind = "column", children = {
        {kind = "canvas", canvas_width = 4096, canvas_height = 4096, commands = {}, rasterize = {}},
        {kind = "image", image = {width = 1, height = 1, rgba = buffer.create(4)}},
    }}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("image budget"), "{error}");
    let error = read(
        &lua,
        r#"return {kind = "canvas", canvas_width = 4096, canvas_height = 4096, rasterize = {}, commands = {
        {kind = "rect", width = 4096, height = 4096, fill = "primary"},
        {kind = "rect", width = 4096, height = 4096, fill = "primary"},
    }}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("rasterization work budget"), "{error}");
}

#[test]
fn nested_actions_respect_subtree_and_button_action_disabling() {
    let lua = crate::runtime::new_lua();
    let mut node = read(
        &lua,
        r#"return {kind = "button", id = "row", action_enabled = false,
        child = {kind = "button", id = "rotate", text = "Rotate"}}"#,
    )
    .unwrap();
    assert!(!node.accepts(&Action::activate("row", "")));
    assert!(node.accepts(&Action::activate("rotate", "")));
    node.enabled = false;
    assert!(!node.accepts(&Action::activate("rotate", "")));
}

#[test]
fn serde_rejects_wrong_shapes_and_semantics_reject_invalid_values() {
    let lua = crate::runtime::new_lua();
    for source in [
        r#"return {kind = "unknown"}"#,
        r#"return {kind = "input"}"#,
        r#"return {kind = "text", text = false}"#,
        r#"return {kind = "text", text = "", color = "unknown"}"#,
        r#"return {kind = "space", cursor = "unknown"}"#,
        r#"return {kind = "space", width = {portion = 0}}"#,
        r#"return {kind = "space", padding = {1, 2, 3}}"#,
        r#"return {kind = "space", width = -1}"#,
        r#"return {kind = "slider", id = "s", min = 2, max = 1, number = 1}"#,
        r#"return {kind = "image", image = {width = 2, height = 1, rgba = buffer.create(4)}}"#,
        r#"return {kind = "image", opacity = -0.1, image = {width = 1, height = 1, rgba = buffer.create(4)}}"#,
        r#"return {kind = "canvas", canvas_width = 1, canvas_height = 1, commands = {}, pointer_capture = {{phase = "invalid"}}}"#,
        r#"return {kind = "row", children = {{kind = "input", id = "same"}, {kind = "input", id = "same"}}}"#,
        r#"return {kind = "select", id = "s", value = "absent", choices = {}}"#,
    ] {
        assert!(read(&lua, source).is_err(), "{source}");
    }
}

#[test]
fn preflight_rejects_cycles_metatables_sparse_arrays_and_large_values() {
    let lua = crate::runtime::new_lua();
    for source in [
        r#"local n = {kind = "column", children = {}}; n.children[1] = n; return n"#,
        r#"return setmetatable({kind = "space"}, {__index = function() error("must not run") end})"#,
        r#"return {kind = "column", children = {[2] = {kind = "space"}}}"#,
        r#"return {kind = "column", children = {{kind = "space"}, extra = true}}"#,
        r#"return {kind = "text", text = string.rep("a", 16385)}"#,
        r#"local n = {kind = "space"}; for i = 1, 100 do n = {kind = "container", child = n} end; return n"#,
        r#"return {kind = "space", extra = function() end}"#,
    ] {
        assert!(read(&lua, source).is_err(), "{source}");
    }
    // Shared immutable values are legal; only ancestor cycles are rejected.
    read(
        &lua,
        r#"local n = {kind = "text", text = "Shared"}; return {kind = "row", children = {n, n}}"#,
    )
    .unwrap();
}

#[test]
fn events_are_typed_and_disabled_ancestors_reject_actions() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {kind = "row", children = {
        {kind = "checkbox", id = "check", text = "Check", checked = false},
        {kind = "slider", id = "slide", number = 1, min = 0, max = 5, step = 0.5},
        {kind = "container", enabled = false, child = {kind = "button", id = "disabled"}},
    }}"#,
    )
    .unwrap();
    let action = Action {
        id: "check".into(),
        event: Event::Toggle { checked: true },
    };
    assert!(node.accepts(&action));
    assert!(!node.accepts(&Action::change("check", "true")));
    assert!(!node.accepts(&Action::activate("disabled", "")));
    for (number, valid) in [(1.5, true), (1.3, false), (6.0, false), (f64::NAN, false)] {
        assert_eq!(
            node.accepts(&Action {
                id: "slide".into(),
                event: Event::Slide { number }
            }),
            valid
        );
    }
    let table = lua.to_value(&action).unwrap();
    assert_eq!(table.as_table().unwrap().get::<String>("kind").unwrap(), "toggle");
    assert_eq!(action, lua.from_value::<Action>(table).unwrap());
    let slider = read(
        &lua,
        r#"return {kind = "slider", id = "s", number = 0, max = 1, step = 0.3}"#,
    )
    .unwrap();
    assert!(slider.accepts(&Action {
        id: "s".into(),
        event: Event::Slide { number: 1.0 }
    }));
}

#[test]
fn rich_tooltips_share_validation_budgets_but_cannot_dispatch_actions() {
    let lua = crate::runtime::new_lua();
    let node = read(
        &lua,
        r#"return {
        kind = "button", id = "choose", tone = {row = 1, selected = true},
        content_padding = {3, 12, 3, 12}, appearance = {text_color = "danger"},
        hovered = {border_color = "primary", border_width = 2},
        pressed = {background = {theme = "primary", alpha = 0.4}},
        disabled = {text_color = "muted"},
        child = {kind = "text", text = "Choose"},
        tooltip = {position = "follow_cursor", gap = 8, content = {
            kind = "column", appearance = {background = {0, 0, 0, 0.85}, text_color = {1, 1, 1, 1}, radius = 4},
            children = {
                {kind = "image", image = {width = 1, height = 1, rgba = buffer.fromstring("rgba")}},
                {kind = "button", id = "tooltip-action", text = "Presentation only"},
            },
        }},
    }"#,
    )
    .unwrap();
    assert!(node.accepts(&Action::activate("choose", "")));
    assert!(!node.accepts(&Action::activate("tooltip-action", "")));
    let Kind::Button { child: Some(child), .. } = &node.kind else {
        panic!()
    };
    assert!(matches!(child.kind, Kind::Text { color: None, .. }));
    let Some(Tooltip::Rich { content, .. }) = &node.tooltip else {
        panic!()
    };
    let Kind::Column { children } = &content.kind else {
        panic!()
    };
    let Kind::Image { image, .. } = &children[0].kind else {
        panic!()
    };
    assert_ne!(image.digest, [0; 32]);
    let value = lua
        .to_value_with(
            &node,
            mlua::serde::SerializeOptions::new()
                .set_array_metatable(false)
                .serialize_none_to_null(false),
        )
        .unwrap();
    assert_eq!(node, Node::read(&lua, value.as_table().unwrap().clone()).unwrap());

    for source in [
        // A tooltip participates in the same ID/key namespace and resource limits.
        r#"return {kind = "button", id = "same", tooltip = {content = {kind = "button", id = "same"}}}"#,
        r#"return {kind = "space", key = "same", tooltip = {content = {kind = "space", key = "same"}}}"#,
        r#"return {kind = "space", tooltip = {content = {kind = "image", image = {width = 2, height = 2, rgba = buffer.create(4)}}}}"#,
        r#"local n = {kind = "space"}; n.tooltip = {content = n}; return n"#,
        r#"local n = {kind = "space"}; for _ = 1, 33 do n = {kind = "space", tooltip = {content = n}} end; return n"#,
        r#"return {kind = "space", tooltip = {content = {kind = "space"}, gap = -1}}"#,
        r#"return {kind = "space", tooltip = {content = {kind = "space"}, position = "outside"}}"#,
    ] {
        assert!(read(&lua, source).is_err(), "{source}");
    }
}

#[test]
fn large_catalogs_count_shared_rows_and_tooltips_toward_the_node_limit() {
    let lua = crate::runtime::new_lua();
    // A shared Luau table still becomes a separate rendered node at each use.
    let source = format!(
        r#"local row = {{kind = "space", tooltip = {{content = {{kind = "space"}}}}}}
        return {{kind = "column", children = table.create({}, row)}}"#,
        (MAX_UI_NODES - 1) / 2,
    );
    let table: Table = lua.load(source).eval().unwrap();
    let children: Table = table.get("children").unwrap();
    children
        .push(lua.load(r#"return {kind = "space"}"#).eval::<Table>().unwrap())
        .unwrap();
    Node::read(&lua, table.clone()).unwrap();
    // Tooltip nodes share the root's budget, including when that tooltip is hidden.
    table
        .set(
            "tooltip",
            lua.load(r#"return {content = {kind = "space"}}"#)
                .eval::<Table>()
                .unwrap(),
        )
        .unwrap();
    assert!(Node::read(&lua, table).unwrap_err().to_string().contains("node limit"));
}

#[test]
fn appearance_and_control_metrics_reject_invalid_values() {
    let lua = crate::runtime::new_lua();
    for source in [
        r#"return {kind = "space", appearance = {background = {theme = "background", alpha = 2}}}"#,
        r#"return {kind = "space", appearance = {background = {theme = "background", tint = {1, 0, 0, 1}, mix = 1.1}}}"#,
        r#"return {kind = "space", appearance = {background = {theme = "background", tint = {1, -1, 0, 1}, mix = 0.5}}}"#,
        r#"return {kind = "space", appearance = {background = {theme = "background", tint = {1, 0, 0, 1}, mix = 0.5, alpha = 1}}}"#,
        r#"return {kind = "space", appearance = {shadow = {color = {0, 0, 0, 2}}}}"#,
        r#"return {kind = "space", appearance = {shadow = {color = "text", offset = {1, 0/0}}}}"#,
        r#"return {kind = "space", appearance = {shadow = {color = "text", blur = -1}}}"#,
        r#"return {kind = "space", appearance = {shadow = {color = "text", unknown = true}}}"#,
        r#"return {kind = "text", text = "Test", wrapping = "invalid"}"#,
        r#"return {kind = "space", clip = "true"}"#,
        r#"return {kind = "checkbox", id = "c", text = "Group", checked = true, size = 0}"#,
        r#"return {kind = "checkbox", id = "c", text = "Group", checked = true, text_size = 0/0}"#,
        r#"return {kind = "space", appearance = {border_width = -1}}"#,
        r#"return {kind = "space", appearance = {radius = 0/0}}"#,
        r#"return {kind = "space", appearance = {unknown = true}}"#,
        r#"return {kind = "space", tone = {row = -1}}"#,
        r#"return {kind = "space", tone = {row = 0}}"#,
        r#"return {kind = "space", tone = {row = 1, tint = "primary"}}"#,
        r#"return {kind = "button", id = "b", tone = {tint = {1, 2, 0, 1}}}"#,
        r#"return {kind = "button", id = "b", hovered = {border_width = 0/0}}"#,
        r#"return {kind = "button", id = "b", disabled = {text_color = {theme = "muted", alpha = -1}}}"#,
        r#"return {kind = "button", id = "b", pressed = {background = false}}"#,
        r#"return {kind = "input", id = "i", content_padding = {1, 2}}"#,
        r#"return {kind = "input", id = "i", size = -1}"#,
        r#"return {kind = "select", id = "s", value = "a", choices = {{value = "a", label = "A"}}, content_padding = -1}"#,
    ] {
        assert!(read(&lua, source).is_err(), "{source}");
    }
}
