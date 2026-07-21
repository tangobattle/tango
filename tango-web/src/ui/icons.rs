//! Inline lucide glyphs (<https://lucide.dev>, ISC license). Rendered
//! as 1em SVGs stroked with `currentColor`, so they follow the
//! surrounding text's size and colour with no per-icon styling.

use dioxus::prelude::*;

/// The shared frame: every lucide icon is a 24×24 outline with the
/// same stroke settings.
#[component]
fn Lucide(children: Element) -> Element {
    rsx! {
        svg {
            class: "icon",
            view_box: "0 0 24 24",
            width: "1em",
            height: "1em",
            fill: "none",
            stroke: "currentColor",
            stroke_width: "2",
            stroke_linecap: "round",
            stroke_linejoin: "round",
            {children}
        }
    }
}

#[component]
pub fn Check() -> Element {
    rsx! {
        Lucide { path { d: "M20 6 9 17l-5-5" } }
    }
}

/// lucide `film` — the Replays tab.
#[component]
pub fn Film() -> Element {
    rsx! {
        Lucide {
            rect { x: "3", y: "3", width: "18", height: "18", rx: "2" }
            path { d: "M7 3v18" }
            path { d: "M3 7.5h4" }
            path { d: "M3 12h18" }
            path { d: "M3 16.5h4" }
            path { d: "M17 3v18" }
            path { d: "M17 7.5h4" }
            path { d: "M17 16.5h4" }
        }
    }
}

/// lucide `puzzle` — the Patches tab.
#[component]
pub fn Puzzle() -> Element {
    rsx! {
        Lucide {
            path { d: "M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.501 2.501 0 1 0-3.214 3.214c.446.166.855.497.925.968a.979.979 0 0 1-.276.837l-1.61 1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 0 0 0-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 1 1-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 1.026 0 0 0-.289-.877l-1.568-1.568A2.402 2.402 0 0 1 1.998 12c0-.617.236-1.234.706-1.704L4.23 8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.5 2.5 0 1 0 3.259-3.259c-.482-.196-.933-.558-1.01-1.073-.05-.336.062-.676.303-.917l1.525-1.525A2.402 2.402 0 0 1 12 1.998c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 1.02-.968a2.5 2.5 0 1 1 3.237 3.237c-.464.18-.894.527-.967 1.02Z" }
        }
    }
}

/// lucide `settings` — the Settings tab.
#[component]
pub fn Settings() -> Element {
    rsx! {
        Lucide {
            path { d: "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" }
            circle { cx: "12", cy: "12", r: "3" }
        }
    }
}

/// lucide `swords` — the Fight button.
#[component]
pub fn Swords() -> Element {
    rsx! {
        Lucide {
            path { d: "M14.5 17.5 3 6V3h3l11.5 11.5" }
            path { d: "M13 19l6-6" }
            path { d: "M16 16l4 4" }
            path { d: "M19 21l2-2" }
            path { d: "M9.5 6.5 21 18v3h-3L6.5 9.5" }
            path { d: "M5 14l-2 2" }
            path { d: "M7 17l-4 4" }
        }
    }
}

#[component]
pub fn Pencil() -> Element {
    rsx! {
        Lucide {
            path { d: "M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z" }
            path { d: "m15 5 4 4" }
        }
    }
}

/// lucide `wand-sparkles`.
#[component]
pub fn Wand() -> Element {
    rsx! {
        Lucide {
            path { d: "m21.64 3.64-1.28-1.28a1.21 1.21 0 0 0-1.72 0L2.36 18.64a1.21 1.21 0 0 0 0 1.72l1.28 1.28a1.2 1.2 0 0 0 1.72 0L21.64 5.36a1.2 1.2 0 0 0 0-1.72" }
            path { d: "m14 7 3 3" }
            path { d: "M5 6v4" }
            path { d: "M19 14v4" }
            path { d: "M10 2v2" }
            path { d: "M7 8H3" }
        }
    }
}

#[component]
pub fn User() -> Element {
    rsx! {
        Lucide {
            path { d: "M19 21v-2a4 4 0 0 0-4-4H9a4 4 0 0 0-4 4v2" }
            circle { cx: "12", cy: "7", r: "4" }
        }
    }
}

#[component]
pub fn Play() -> Element {
    rsx! {
        Lucide { polygon { points: "6 3 20 12 6 21 6 3" } }
    }
}

#[component]
pub fn Pause() -> Element {
    rsx! {
        Lucide {
            rect { x: "14", y: "4", width: "4", height: "16", rx: "1" }
            rect { x: "6", y: "4", width: "4", height: "16", rx: "1" }
        }
    }
}

/// lucide `settings-2`.
#[component]
pub fn Sliders() -> Element {
    rsx! {
        Lucide {
            path { d: "M20 7h-9" }
            path { d: "M14 17H5" }
            circle { cx: "17", cy: "17", r: "3" }
            circle { cx: "7", cy: "7", r: "3" }
        }
    }
}

#[component]
pub fn Gamepad2() -> Element {
    rsx! {
        Lucide {
            line { x1: "6", x2: "10", y1: "11", y2: "11" }
            line { x1: "8", x2: "8", y1: "9", y2: "13" }
            line { x1: "15", x2: "15.01", y1: "12", y2: "12" }
            line { x1: "18", x2: "18.01", y1: "10", y2: "10" }
            path { d: "M17.32 5H6.68a4 4 0 0 0-3.978 3.59c-.006.052-.01.101-.017.152C2.604 9.416 2 14.456 2 16a3 3 0 0 0 3 3c1 0 1.5-.5 2-1l1.414-1.414A2 2 0 0 1 9.828 16h4.344a2 2 0 0 1 1.414.586L17 18c.5.5 1 1 2 1a3 3 0 0 0 3-3c0-1.545-.604-6.584-.685-7.258-.007-.05-.011-.1-.017-.151A4 4 0 0 0 17.32 5z" }
        }
    }
}

#[component]
pub fn Save() -> Element {
    rsx! {
        Lucide {
            path { d: "M15.2 3a2 2 0 0 1 1.4.6l3.8 3.8a2 2 0 0 1 .6 1.4V19a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2z" }
            path { d: "M17 21v-7a1 1 0 0 0-1-1H8a1 1 0 0 0-1 1v7" }
            path { d: "M7 3v4a1 1 0 0 0 1 1h7" }
        }
    }
}

#[component]
pub fn Trash2() -> Element {
    rsx! {
        Lucide {
            path { d: "M3 6h18" }
            path { d: "M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6" }
            path { d: "M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2" }
            line { x1: "10", x2: "10", y1: "11", y2: "17" }
            line { x1: "14", x2: "14", y1: "11", y2: "17" }
        }
    }
}

/// lucide `files` — the save view's Folder tab.
#[component]
pub fn Files() -> Element {
    rsx! {
        Lucide {
            path { d: "M20 7h-3a2 2 0 0 1-2-2V2" }
            path { d: "M9 18a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h7l5 5v9a2 2 0 0 1-2 2Z" }
            path { d: "M3 7.6v12.8A1.6 1.6 0 0 0 4.6 22h9.8" }
        }
    }
}

/// lucide `credit-card` — the save view's Patch Cards tab.
#[component]
pub fn CreditCard() -> Element {
    rsx! {
        Lucide {
            rect { x: "2", y: "5", width: "20", height: "14", rx: "2" }
            line { x1: "2", x2: "22", y1: "10", y2: "10" }
        }
    }
}

/// lucide `clipboard-copy` — the save view's copy-as-text button.
#[component]
pub fn ClipboardCopy() -> Element {
    rsx! {
        Lucide {
            rect { x: "8", y: "2", width: "8", height: "4", rx: "1", ry: "1" }
            path { d: "M8 4H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h9a2 2 0 0 0 2-2v-2" }
            path { d: "M16 4h2a2 2 0 0 1 2 2v4" }
            path { d: "M21 14H11" }
            path { d: "m15 10-4 4 4 4" }
        }
    }
}

/// lucide `file-question-mark` — the save view's empty-tab placeholder.
#[component]
pub fn FileQuestion() -> Element {
    rsx! {
        Lucide {
            path { d: "M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" }
            path { d: "M9.1 9a3 3 0 0 1 5.82 1c0 2-3 3-3 3" }
            path { d: "M12 17h.01" }
        }
    }
}

/// lucide `grip-vertical` — drag-to-reorder affordance.
#[component]
pub fn GripVertical() -> Element {
    rsx! {
        Lucide {
            circle { cx: "9", cy: "12", r: "1" }
            circle { cx: "9", cy: "5", r: "1" }
            circle { cx: "9", cy: "19", r: "1" }
            circle { cx: "15", cy: "12", r: "1" }
            circle { cx: "15", cy: "5", r: "1" }
            circle { cx: "15", cy: "19", r: "1" }
        }
    }
}

/// lucide `rotate-cw` — the navicust palette's rotate control.
#[component]
pub fn RotateCw() -> Element {
    rsx! {
        Lucide {
            path { d: "M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" }
            path { d: "M21 3v5h-5" }
        }
    }
}

/// lucide `expand` — the navicust palette's uncompress control.
#[component]
pub fn Expand() -> Element {
    rsx! {
        Lucide {
            path { d: "m15 15 6 6" }
            path { d: "m15 9 6-6" }
            path { d: "M21 16v5h-5" }
            path { d: "M21 8V3h-5" }
            path { d: "M3 16v5h5" }
            path { d: "m3 21 6-6" }
            path { d: "M3 8V3h5" }
            path { d: "M9 9 3 3" }
        }
    }
}

/// lucide `shrink` — the navicust palette's compress control.
#[component]
pub fn Shrink() -> Element {
    rsx! {
        Lucide {
            path { d: "m15 15 6 6" }
            path { d: "m15 9 6-6" }
            path { d: "M15 21v-5h5" }
            path { d: "M15 3v5h5" }
            path { d: "M3 16h5v5" }
            path { d: "m3 21 6-6" }
            path { d: "M3 8h5V3" }
            path { d: "M9 9 3 3" }
        }
    }
}

#[component]
pub fn Download() -> Element {
    rsx! {
        Lucide {
            path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
            polyline { points: "7 10 12 15 17 10" }
            line { x1: "12", x2: "12", y1: "15", y2: "3" }
        }
    }
}

#[component]
pub fn Upload() -> Element {
    rsx! {
        Lucide {
            path { d: "M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" }
            polyline { points: "17 8 12 3 7 8" }
            line { x1: "12", x2: "12", y1: "3", y2: "15" }
        }
    }
}

#[component]
pub fn RefreshCw() -> Element {
    rsx! {
        Lucide {
            path { d: "M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" }
            path { d: "M21 3v5h-5" }
            path { d: "M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" }
            path { d: "M8 16H3v5" }
        }
    }
}

#[component]
pub fn X() -> Element {
    rsx! {
        Lucide {
            path { d: "M18 6 6 18" }
            path { d: "m6 6 12 12" }
        }
    }
}

#[component]
pub fn Keyboard() -> Element {
    rsx! {
        Lucide {
            path { d: "M10 8h.01" }
            path { d: "M12 12h.01" }
            path { d: "M14 8h.01" }
            path { d: "M16 12h.01" }
            path { d: "M18 8h.01" }
            path { d: "M6 8h.01" }
            path { d: "M7 16h10" }
            path { d: "M8 12h.01" }
            rect { width: "20", height: "16", x: "2", y: "4", rx: "2" }
        }
    }
}

#[component]
pub fn Cable() -> Element {
    rsx! {
        Lucide {
            path { d: "M17 21v-2a1 1 0 0 1-1-1v-1a2 2 0 0 1 2-2h2a2 2 0 0 1 2 2v1a1 1 0 0 1-1 1v2" }
            path { d: "M19 15V6.5a1 1 0 0 0-7 0v11a1 1 0 0 1-7 0V9" }
            path { d: "M21 21v-2h-4" }
            path { d: "M3 5h4V3" }
            path { d: "M7 5a1 1 0 0 1 1 1v1a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V6a1 1 0 0 1 1-1V3" }
        }
    }
}

#[component]
pub fn Unplug() -> Element {
    rsx! {
        Lucide {
            path { d: "m19 5 3-3" }
            path { d: "m2 22 3-3" }
            path { d: "M6.3 20.3a2.4 2.4 0 0 0 3.4 0L12 18l-6-6-2.3 2.3a2.4 2.4 0 0 0 0 3.4Z" }
            path { d: "M7.5 13.5 10 11" }
            path { d: "M10.5 16.5 13 14" }
            path { d: "m12 6 6 6 2.3-2.3a2.4 2.4 0 0 0 0-3.4l-2.6-2.6a2.4 2.4 0 0 0-3.4 0Z" }
        }
    }
}

#[component]
pub fn Gauge() -> Element {
    rsx! {
        Lucide {
            path { d: "m12 14 4-4" }
            path { d: "M3.34 19a10 10 0 1 1 17.32 0" }
        }
    }
}

#[component]
pub fn ArrowLeftRight() -> Element {
    rsx! {
        Lucide {
            path { d: "M8 3 4 7l4 4" }
            path { d: "M4 7h16" }
            path { d: "m16 21 4-4-4-4" }
            path { d: "M20 17H4" }
        }
    }
}

#[component]
pub fn Footprints() -> Element {
    rsx! {
        Lucide {
            path { d: "M4 16v-2.38C4 11.5 2.97 10.5 3 8c.03-2.72 1.49-6 4.5-6C9.37 2 10 3.8 10 5.5c0 3.11-2 5.66-2 8.68V16a2 2 0 1 1-4 0Z" }
            path { d: "M20 20v-2.38c0-2.12 1.03-3.12 1-5.62-.03-2.72-1.49-6-4.5-6C14.63 6 14 7.8 14 9.5c0 3.11 2 5.66 2 8.68V20a2 2 0 1 0 4 0Z" }
            path { d: "M16 17h4" }
            path { d: "M4 13h4" }
        }
    }
}

#[component]
pub fn GitMerge() -> Element {
    rsx! {
        Lucide {
            circle { cx: "18", cy: "18", r: "3" }
            circle { cx: "6", cy: "6", r: "3" }
            path { d: "M6 21V9a9 9 0 0 0 9 9" }
        }
    }
}

#[component]
pub fn Wifi() -> Element {
    rsx! {
        Lucide {
            path { d: "M12 20h.01" }
            path { d: "M2 8.82a15 15 0 0 1 20 0" }
            path { d: "M5 12.859a10 10 0 0 1 14 0" }
            path { d: "M8.5 16.429a5 5 0 0 1 7 0" }
        }
    }
}

#[component]
pub fn SignalHigh() -> Element {
    rsx! {
        Lucide {
            path { d: "M2 20h.01" }
            path { d: "M7 20v-4" }
            path { d: "M12 20v-8" }
            path { d: "M17 20V8" }
        }
    }
}

#[component]
pub fn SignalMedium() -> Element {
    rsx! {
        Lucide {
            path { d: "M2 20h.01" }
            path { d: "M7 20v-4" }
            path { d: "M12 20v-8" }
        }
    }
}

#[component]
pub fn SignalLow() -> Element {
    rsx! {
        Lucide {
            path { d: "M2 20h.01" }
            path { d: "M7 20v-4" }
        }
    }
}

#[component]
pub fn Menu() -> Element {
    rsx! {
        Lucide {
            path { d: "M4 6h16" }
            path { d: "M4 12h16" }
            path { d: "M4 18h16" }
        }
    }
}

#[component]
pub fn ChevronUp() -> Element {
    rsx! {
        Lucide { path { d: "m18 15-6-6-6 6" } }
    }
}

#[component]
pub fn Timer() -> Element {
    rsx! {
        Lucide {
            line { x1: "10", x2: "14", y1: "2", y2: "2" }
            line { x1: "12", x2: "15", y1: "14", y2: "11" }
            circle { cx: "12", cy: "14", r: "8" }
        }
    }
}

#[component]
pub fn ChartLine() -> Element {
    rsx! {
        Lucide {
            path { d: "M3 3v16a2 2 0 0 0 2 2h16" }
            path { d: "m19 9-5 5-4-4-3 3" }
        }
    }
}

#[component]
pub fn Users() -> Element {
    rsx! {
        Lucide {
            path { d: "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" }
            circle { cx: "9", cy: "7", r: "4" }
            path { d: "M22 21v-2a4 4 0 0 0-3-3.87" }
            path { d: "M16 3.13a4 4 0 0 1 0 7.75" }
        }
    }
}
