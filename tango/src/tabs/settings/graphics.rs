//! Graphics pane: window, emulator display, DS screen arrangement.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::column;

fn ds_screen_stacking_choice(
    lang: &LanguageIdentifier,
    stacking: config::DsScreenStacking,
) -> Choice<config::DsScreenStacking> {
    Choice::new(
        stacking,
        match stacking {
            config::DsScreenStacking::Horizontal => t!(lang, "settings-ds-screen-stacking-horizontal"),
            config::DsScreenStacking::Vertical => t!(lang, "settings-ds-screen-stacking-vertical"),
            config::DsScreenStacking::PrimaryOnly => t!(lang, "settings-ds-screen-stacking-primary-only"),
        },
    )
}

fn ds_primary_screen_choice(
    lang: &LanguageIdentifier,
    primary: config::DsPrimaryScreen,
) -> Choice<config::DsPrimaryScreen> {
    Choice::new(
        primary,
        match primary {
            config::DsPrimaryScreen::Upper => t!(lang, "settings-ds-primary-screen-upper"),
            config::DsPrimaryScreen::Touch => t!(lang, "settings-ds-primary-screen-touch"),
        },
    )
}

/// A window resolution as a pick_list [`Choice`]. PartialEq is exact
/// f32 — fine since the values come straight from
/// [`window::STANDARD_RESOLUTIONS`] constants and matched by equality.
fn resolution_choice(width: f32, height: f32) -> Choice<(f32, f32)> {
    Choice::new((width, height), format!("{}×{}", width as u32, height as u32))
}

/// UI scale presets surfaced in the graphics-settings pick-list.
/// Multiplies on top of the OS DPI scale.
const UI_SCALE_PRESETS: &[f32] = &[0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

/// A UI scale multiplier as a pick_list [`Choice`]. Integer percent —
/// 25%-step decimals would just clutter the dropdown. `PartialEq` is
/// exact on f32, which is fine since values come from the
/// `UI_SCALE_PRESETS` constants and matched by equality.
fn ui_scale_choice(scale: f32) -> Choice<f32> {
    Choice::new(scale, format!("{}%", (scale * 100.0).round() as u32))
}

pub(super) fn settings_graphics<'a>(lang: &'a LanguageIdentifier, config: &'a config::Config) -> Element<'a, Message> {
    let resolution_options: Vec<Choice<(f32, f32)>> = window::STANDARD_RESOLUTIONS
        .iter()
        .map(|&(w, h)| resolution_choice(w as f32, h as f32))
        .collect();
    // Match the current windowed size against the preset list so
    // the picker shows a selected value when it lines up exactly.
    // No match (custom drag-resized size) renders as blank.
    let selected_resolution = config
        .last_window_size
        .and_then(|size| resolution_options.iter().find(|o| o.value == size).cloned());
    // Disable the window-size picker while fullscreen is on:
    // picking a sub-monitor size while fullscreen is meaningless
    // (the live window stays at monitor resolution). Render the
    // shared disabled-dropdown placeholder so it reads as the
    // same control family as the live picker.
    let window_size_picker: Element<'a, Message> = if config.fullscreen {
        let label = selected_resolution.map(|r| r.label).unwrap_or_else(|| "—".into());
        widgets::disabled_pick_list(label).into()
    } else {
        widgets::picker(resolution_options, selected_resolution, |c: Choice<(f32, f32)>| {
            Message::ResolutionChanged(c.value)
        })
        .into()
    };
    let ui_scale_options: Vec<Choice<f32>> = UI_SCALE_PRESETS.iter().copied().map(ui_scale_choice).collect();
    let selected_ui_scale = ui_scale_options
        .iter()
        .find(|c| (c.value - config.ui_scale).abs() < f32::EPSILON)
        .cloned();
    column![
        settings_group(
            t!(lang, "settings-group-window"),
            vec![
                option_row::<Message>(t!(lang, "settings-window-size"), window_size_picker),
                option_row(
                    t!(lang, "settings-fullscreen"),
                    toggle(config.fullscreen, Message::ToggleFullscreen),
                ),
                option_row::<Message>(
                    t!(lang, "settings-ui-scale"),
                    widgets::picker(ui_scale_options, selected_ui_scale, |c: Choice<f32>| {
                        Message::UiScaleChanged(c.value)
                    }),
                ),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-emulator"),
            vec![
                option_row::<Message>(t!(lang, "settings-video-filter"), {
                    // `value` is the `config.video_filter` key (`""`, `"hq2x"`, …).
                    let options: Vec<Choice<String>> = crate::platform::video::effects::EFFECTS
                        .iter()
                        .map(|effect| Choice::new(effect.id.into(), effect.name))
                        .collect();
                    let selected = options.iter().find(|c| c.value == config.video_filter).cloned();
                    widgets::picker(options, selected, |c: Choice<String>| {
                        Message::VideoFilterChanged(c.value)
                    })
                }),
                option_row(
                    t!(lang, "settings-fractional-scaling"),
                    toggle(config.fractional_scaling, Message::ToggleFractionalScaling),
                ),
                option_row(
                    t!(lang, "settings-hide-emulator-border"),
                    toggle(config.hide_emulator_border, Message::ToggleHideEmulatorBorder),
                ),
            ],
        ),
        settings_group(
            t!(lang, "settings-group-ds"),
            vec![
                option_row::<Message>(t!(lang, "settings-ds-screen-stacking"), {
                    let options = vec![
                        ds_screen_stacking_choice(lang, config::DsScreenStacking::Vertical),
                        ds_screen_stacking_choice(lang, config::DsScreenStacking::Horizontal),
                        ds_screen_stacking_choice(lang, config::DsScreenStacking::PrimaryOnly),
                    ];
                    let selected = options.iter().find(|c| c.value == config.ds_screen_stacking).cloned();
                    widgets::picker(options, selected, |c: Choice<config::DsScreenStacking>| {
                        Message::DsScreenStackingChanged(c.value)
                    })
                }),
                option_row::<Message>(t!(lang, "settings-ds-primary-screen"), {
                    let options = vec![
                        ds_primary_screen_choice(lang, config::DsPrimaryScreen::Upper),
                        ds_primary_screen_choice(lang, config::DsPrimaryScreen::Touch),
                    ];
                    let selected = options.iter().find(|c| c.value == config.ds_primary_screen).cloned();
                    widgets::picker(options, selected, |c: Choice<config::DsPrimaryScreen>| {
                        Message::DsPrimaryScreenChanged(c.value)
                    })
                }),
            ],
        ),
    ]
    .spacing(24)
    .padding(style::PANE_PADDING)
    .into()
}
