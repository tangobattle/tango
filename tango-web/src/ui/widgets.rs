//! Cross-target form controls. Blitz (dioxus-native 0.7.9) doesn't
//! implement `<select>` dropdowns or `<input type=range>` sliders, so
//! these components keep the real elements on the web build and render
//! working custom equivalents on native: [`Select`] becomes a button +
//! popover option list, [`Slider`] a −/readout/+ stepper. Call sites
//! use them identically on both targets.

use dioxus::prelude::*;

/// One entry of a [`Select`].
#[derive(Clone, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
    pub disabled: bool,
}

impl SelectOption {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            disabled: false,
        }
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// A dropdown picker: `<select>` on the web, a custom popover on
/// native. `value` is the currently-selected option's value; the
/// handler receives the picked option's value.
#[component]
pub fn Select(
    #[props(default)] class: String,
    #[props(default = false)] disabled: bool,
    options: Vec<SelectOption>,
    value: String,
    onchange: EventHandler<String>,
) -> Element {
    // Hook order must match across targets — declared unconditionally.
    let mut open = use_signal(|| false);

    #[cfg(target_arch = "wasm32")]
    {
        let _ = &mut open;
        rsx! {
            select {
                class: "{class}",
                disabled,
                onchange: move |evt: FormEvent| onchange.call(evt.value()),
                for o in options.iter() {
                    option {
                        value: "{o.value}",
                        selected: o.value == value,
                        disabled: o.disabled,
                        "{o.label}"
                    }
                }
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        let current = options
            .iter()
            .find(|o| o.value == value)
            .or_else(|| options.first())
            .map(|o| o.label.clone())
            .unwrap_or_default();
        rsx! {
            div {
                class: "dd {class}",
                class: if open() { "open" },
                button {
                    class: "btn dd-btn",
                    disabled,
                    onclick: move |_| {
                        let was = *open.peek();
                        open.set(!was);
                    },
                    span { class: "dd-label", "{current}" }
                    span { class: "dd-caret", "▾" }
                }
                if open() {
                    // Click-away sheet: a giant-inset absolute cover
                    // (Blitz has no position:fixed).
                    div { class: "dd-backdrop", onclick: move |_| open.set(false) }
                    div { class: "dd-list",
                        for o in options.iter() {
                            button {
                                class: "dd-item",
                                class: if o.value == value { "active" },
                                disabled: o.disabled,
                                onclick: {
                                    let v = o.value.clone();
                                    move |_| {
                                        open.set(false);
                                        onchange.call(v.clone());
                                    }
                                },
                                "{o.label}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// A ranged number input: `<input type=range>` on the web, a
/// −/readout/+ stepper on native. Values flow as f64; integer-stepped
/// sites just get whole numbers back.
#[component]
pub fn Slider(
    min: f64,
    max: f64,
    #[props(default = 1.0)] step: f64,
    value: f64,
    oninput: EventHandler<f64>,
) -> Element {
    #[cfg(target_arch = "wasm32")]
    {
        rsx! {
            input {
                r#type: "range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |evt: FormEvent| {
                    if let Ok(v) = evt.value().parse::<f64>() {
                        oninput.call(v.clamp(min, max));
                    }
                },
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    {
        // A click-stepper wants ~20 clicks across the range, however
        // fine the web slider's step is.
        let click = step.max((max - min) / 20.0);
        let display = if step >= 1.0 {
            format!("{}", value.round() as i64)
        } else {
            format!("{value:.2}")
        };
        rsx! {
            div { class: "stepper",
                button {
                    class: "btn icon-btn stepper-btn",
                    disabled: value <= min,
                    onclick: move |_| oninput.call((value - click).clamp(min, max)),
                    "−"
                }
                span { class: "stepper-value mono", "{display}" }
                button {
                    class: "btn icon-btn stepper-btn",
                    disabled: value >= max,
                    onclick: move |_| oninput.call((value + click).clamp(min, max)),
                    "+"
                }
            }
        }
    }
}
