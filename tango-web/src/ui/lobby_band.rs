//! The Play tab's bottom band, mirroring the desktop's two states: the
//! idle link-code strip (input + Fight) and the in-flight lobby band
//! (matchup cards, compat verdict, ready ladder, latency, leave).

use dioxus::prelude::*;

use super::{icons, use_ctx, Ctx};
use crate::t;
use crate::library::{self, GameRef};
use crate::netplay::{self, PhaseView};
use tango_net_protocol::control as protocol;

/// Build our Settings from the current selection.
fn local_settings(
    nick: &str,
    game: Option<GameRef>,
    patch: Option<&(String, String)>,
    match_type: (u8, u8),
) -> protocol::Settings {
    protocol::Settings {
        nickname: nick.to_string(),
        match_type,
        game_info: game.map(|g| {
            let (family, variant) = g.family_and_variant();
            protocol::GameInfo {
                family_and_variant: (family.to_string(), variant),
                patch: patch.and_then(|(name, ver)| {
                    Some(protocol::PatchInfo {
                        name: name.clone(),
                        version: semver::Version::parse(ver).ok()?,
                    })
                }),
            }
        }),
        blind_setup: false,
    }
}

#[component]
pub fn BottomBand(
    /// The Play tab's active pick: the game the next boot uses and the
    /// save row's value (`None` while nothing bootable is selected).
    active_game: Option<GameRef>,
    active_save: Option<String>,
    /// The picked patch (name, version string), already validated.
    active_patch: Option<(String, String)>,
) -> Element {
    let Ctx {
        runtime,
        config,
        storage,
        patches,
        ..
    } = use_ctx();
    let mut link_code = use_signal(String::new);
    let mut match_type = use_signal(|| (0u8, 0u8));
    let phase = netplay::PHASE.read().clone();
    let lang = crate::i18n::LANG.read().clone();

    // Selection changes while a lobby is up re-announce our settings
    // (material changes drop commits on both ends, like the desktop).
    {
        let active_game = active_game;
        let active_patch2 = active_patch.clone();
        use_effect(move || {
            let mt = *match_type.read();
            if matches!(&*netplay::PHASE.peek(), PhaseView::Lobby(_)) {
                let nick = config.peek().nick.clone();
                netplay::send_command(netplay::Command::SetSettings(local_settings(
                    &nick,
                    active_game,
                    active_patch2.as_ref(),
                    mt,
                )));
            }
        });
    }

    match phase {
        PhaseView::Idle | PhaseView::Failed { .. } => {
            let failed = if let PhaseView::Failed { error } = &phase {
                Some(error.clone())
            } else {
                None
            };
            let can_fight = active_game.is_some() && active_save.is_some();
            let code = link_code.read().clone();
            rsx! {
                div { class: "bottom-band",
                    if let Some(e) = failed {
                        span { class: "flash bad", "{e}" }
                    }
                    input {
                        r#type: "text",
                        placeholder: t!(&lang, "play-link-code"),
                        spellcheck: "false",
                        autocomplete: "off",
                        maxlength: "40",
                        value: "{code}",
                        oninput: move |evt: FormEvent| {
                            link_code.set(
                                evt.value()
                                    .to_lowercase()
                                    .chars()
                                    .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                                    .collect(),
                            )
                        },
                    }
                    button {
                        class: "btn primary",
                        disabled: !can_fight,
                        title: if can_fight { String::new() } else { t!(&lang, "lobby-pick-game-first") },
                        onclick: move |_| {
                            // The Fight click is the user gesture the
                            // audio sink needs; create it now so the
                            // eventual match has sound.
                            {
                                let runtime = runtime.clone();
                                spawn(async move {
                                    crate::web::ensure_audio(&runtime).await;
                                });
                            }
                            let mut code = link_code.peek().clone();
                            if code.is_empty() {
                                // Empty input auto-generates a code, like
                                // the desktop's Fight button.
                                code = randomcode();
                                link_code.set(code.clone());
                            }
                            let nick = config.peek().nick.clone();
                            let settings = local_settings(
                                &nick,
                                active_game,
                                active_patch.as_ref(),
                                *match_type.peek(),
                            );
                            // Snapshot the synced patches' tags for the
                            // compat gate.
                            let patch_tags = patches
                                .peek()
                                .clone()
                                .unwrap_or_default()
                                .iter()
                                .flat_map(|p| {
                                    p.versions.iter().map(|(v, pv)| {
                                        (
                                            (p.name.clone(), v.to_string()),
                                            pv.netplay_compatibility.clone(),
                                        )
                                    })
                                })
                                .collect();
                            netplay::connect(code, settings, patch_tags);
                        },
                        icons::Swords {}
                        {t!(&lang, "play-fight")}
                    }
                }
            }
        }
        PhaseView::Connecting { link_code: code } => rsx! {
            div { class: "bottom-band",
                span { class: "sub",
                    {t!(&lang, "play-status-waiting-opponent")}
                    " · {code}"
                }
                div { style: "flex:1" }
                button {
                    class: "btn danger",
                    onclick: move |_| {
                        netplay::disconnect();
                        *netplay::PHASE.write() = PhaseView::Idle;
                    },
                    {t!(&lang, "play-cancel")}
                }
            }
        },
        PhaseView::Starting => rsx! {
            div { class: "bottom-band",
                span { class: "flash ok", {t!(&lang, "lobby-match-starting")} }
                div { style: "flex:1" }
                button {
                    class: "btn danger",
                    onclick: move |_| {
                        *netplay::PHASE.write() = PhaseView::Idle;
                    },
                    {t!(&lang, "play-cancel")}
                }
            }
        },
        PhaseView::Lobby(lobby) => {
            let remote_nick = lobby
                .remote_settings
                .as_ref()
                .map(|s| s.nickname.clone())
                .unwrap_or_else(|| t!(&lang, "lobby-waiting"));
            let remote_game = lobby.remote_settings.as_ref().and_then(|s| {
                s.game_info.as_ref().map(|g| {
                    library::find_by_family_and_variant(
                        &g.family_and_variant.0,
                        g.family_and_variant.1,
                    )
                    .map(library::display_name)
                    .unwrap_or_else(|| g.family_and_variant.0.clone())
                })
            });
            // The verdict names the actual mismatch, like the desktop.
            let verdict = match lobby.compatible {
                None => None,
                Some(true) => Some(("flash ok", t!(&lang, "lobby-compat-ok"))),
                Some(false) => {
                    let key = match (&lobby.local_settings.game_info, &lobby.remote_settings.as_ref().and_then(|s| s.game_info.clone())) {
                        (Some(lg), Some(rg))
                            if lg.family_and_variant.0 == rg.family_and_variant.0 =>
                        {
                            "lobby-compat-match-mismatch"
                        }
                        _ => "lobby-compat-missing-game",
                    };
                    Some(("flash bad", crate::i18n::t(&lang, key)))
                }
            };
            let latency = lobby
                .latency_ms
                .map(|l| t!(&lang, "lobby-latency", ms = l.round() as i64));
            let local_ready = lobby.local_ready;
            let starting = lobby.match_ready;
            let modes: Vec<usize> = active_game
                .map(|g| g.match_types.to_vec())
                .unwrap_or_default();
            let mt = *match_type.read();
            let ready_gate = lobby.compatible == Some(true);
            let storage = storage;
            let active_save2 = active_save.clone();
            rsx! {
                div { class: "bottom-band lobby",
                    button {
                        class: "btn danger icon-btn",
                        title: "Leave",
                        onclick: move |_| {
                            netplay::disconnect();
                            *netplay::PHASE.write() = PhaseView::Idle;
                        },
                        icons::X {}
                    }
                    div { class: "matchup",
                        div { class: "side you",
                            span { class: "ready-dot", class: if local_ready { "on" } }
                            span { class: "nick", "{lobby.local_settings.nickname}" }
                        }
                        span { class: "vs", "VS" }
                        div { class: "side them",
                            span { class: "ready-dot", class: if lobby.remote_ready { "on" } }
                            span { class: "nick", "{remote_nick}" }
                            if let Some(game) = remote_game {
                                span { class: "sub", "{game}" }
                            }
                        }
                    }
                    span { class: "sub code", "@ {lobby.link_code}" }
                    if let Some((class, text)) = verdict {
                        span { class: "{class}", "{text}" }
                    }
                    if let Some(l) = latency {
                        span { class: "sub", "{l}" }
                    }
                    // Match type: mode / subtype, from the game's own
                    // mode table.
                    select {
                        disabled: local_ready,
                        onchange: move |evt: FormEvent| {
                            let v = evt.value();
                            let mut parts = v.split('.');
                            let mode: u8 =
                                parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                            let sub: u8 =
                                parts.next().and_then(|p| p.parse().ok()).unwrap_or(0);
                            match_type.set((mode, sub));
                        },
                        for (mode, subs) in modes.iter().enumerate() {
                            for sub in 0..*subs {
                                option {
                                    value: "{mode}.{sub}",
                                    selected: mt == (mode as u8, sub as u8),
                                    {
                                        active_game
                                            .map(|g| library::match_type_name(g, mode as u8, sub as u8))
                                            .unwrap_or_else(|| format!("{mode}.{sub}"))
                                    }
                                }
                            }
                        }
                    }
                    button {
                        class: "btn primary",
                        disabled: (!ready_gate && !local_ready) || starting,
                        onclick: move |_| {
                            if local_ready {
                                netplay::send_command(netplay::Command::Unready);
                                return;
                            }
                            let storage = storage.read().clone().flatten();
                            let save = active_save2.clone();
                            spawn(async move {
                                let (Some(storage), Some(save)) = (storage, save) else {
                                    return;
                                };
                                let Ok(Some(bytes)) =
                                    crate::storage::read(storage.saves(), &save).await
                                else {
                                    return;
                                };
                                netplay::send_command(netplay::Command::Ready {
                                    save_data: bytes,
                                });
                            });
                        },
                        if starting {
                            {t!(&lang, "lobby-match-starting")}
                        } else if local_ready {
                            {t!(&lang, "lobby-unready")}
                        } else {
                            {t!(&lang, "lobby-ready")}
                        }
                    }
                }
            }
        }
    }
}

/// A short random link code, the desktop's auto-generate analog.
fn randomcode() -> String {
    const WORDS: &[&str] = &[
        "aqua", "bass", "blast", "blues", "burn", "cross", "dash", "delta", "flame", "gale",
        "guts", "heat", "iris", "meteor", "navi", "pulse", "roll", "shade", "spark", "storm",
        "tango", "tomahawk", "wave", "zero",
    ];
    let a = WORDS[(rand::random::<u32>() as usize) % WORDS.len()];
    let b = WORDS[(rand::random::<u32>() as usize) % WORDS.len()];
    let n: u16 = rand::random::<u16>() % 10000;
    format!("{a}-{b}-{n:04}")
}
