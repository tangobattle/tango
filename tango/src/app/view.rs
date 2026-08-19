//! Rendering for the application shell.
//!
//! Keeping view composition separate from state construction and message
//! reduction makes the top-level app module an orchestrator instead of a
//! second home for every screen's presentation details.

use super::{App, EnterScope, Message, Tab, ROOT_SLIDE};
use crate::i18n::t;
use crate::session;
use crate::tabs;
use crate::ui::theme::theme_for;
use crate::ui::{anim, widgets};
use iced::widget::container;
use iced::widget::space::horizontal as horizontal_space;
use iced::{Alignment, Element, Fill, Theme};
use sweeten::widget::{column, row};
use unic_langid::LanguageIdentifier;

impl App {
    pub fn view(&self) -> Element<'_, Message> {
        let lang = &self.config.language;

        // Live entrance glide, `Some(progress)` while mid-flight.
        // Sampled once here; the branches below wrap whatever they
        // return (whole window or just the tab body, per
        // `screen_enter_scope`).
        let now = iced::time::Instant::now();
        let enter = self.screen_enter.progress(now);

        // First-run gate: no main UI until the user picks a nickname.
        // Sits on the same cyberworld backdrop as the main shell so
        // the first thing a new user sees is already the PET screen.
        if self.config.nickname.is_none() {
            let roms_count = self.scanners.roms.read().len();
            let welcome = tabs::welcome::view(
                lang,
                &self.welcome,
                roms_count,
                &self.config.roms_path(),
                self.is_rescanning(),
            )
            .map(Message::Welcome);
            return anim::slide_in_opt(
                iced::widget::stack![widgets::cyber_backdrop(), welcome]
                    .width(Fill)
                    .height(Fill),
                enter,
                iced::Vector::new(0.0, ROOT_SLIDE),
            );
        }

        if self.session.is_active() {
            // Deliver keyboard + gamepad input through the
            // synchronous widget path so each event reaches
            // `program.update()` on the same winit iteration it
            // arrived in. Going through subscriptions would
            // round-trip through an `mpsc::try_send` and cost ~1
            // winit iteration of input lag per event.
            // The watched replay's export job (whole-replay or clip
            // alike), digested for the transport bar's clip strip —
            // the job itself stays owned by the replays tab.
            let clip_job = self
                .session
                .replay_path
                .as_ref()
                .and_then(|p| self.replays.job(p))
                .map(|j| session::view::ClipJob {
                    completed: j.completed,
                    total: j.total,
                    result: j.result.as_ref().map(|r| match r {
                        Ok(_) => Ok(()),
                        Err(e) => Err(e.as_str()),
                    }),
                    cancelling: j.canceller.is_cancelled() && j.result.is_none(),
                });
            let session_view = session::view::view(
                lang,
                &self.session,
                self.config.fractional_scaling,
                self.config.hide_emulator_border,
                self.config.show_replay_inputs,
                self.config.opponent_view,
                self.config.ds_screen_stacking,
                self.config.ds_primary_screen,
                self.replays.export_settings.scale,
                clip_job,
                self.replays.queue.len(),
                crate::platform::video::effects::effect_for(&self.config.video_filter),
            )
            .map(Message::Session);
            // In-session settings modal: floats centered over the
            // running session with a dimmed click-to-dismiss
            // backdrop. The emulator keeps running underneath.
            // Rendered while the open/close transition is in
            // flight too, so the panel eases in and out.
            let composed: Element<'_, Message> = if self.session.settings.visible(now) {
                let progress = self.session.settings.progress(now);
                // The session's own InputCapture wrapper + vblank pump
                // already track every key/button in `input_held`, so the
                // input pane's live binding highlight reads from that
                // instead of pumping its own.
                let body = tabs::settings::view(
                    lang,
                    &self.config,
                    &self.settings,
                    self.updater.status_blocking(),
                    Some(&self.session.input_held),
                )
                .map(Message::Settings);
                // Top header row carrying the X close button. The
                // close is the only affordance for dismissing the
                // modal — the backdrop is inert. Inline (not a
                // floating overlay) so the body lays out beneath.
                // Same chrome as the fullscreen top bar's app-close
                // X — both are window-dismissal affordances, so
                // they share the quiet-at-rest / red-on-hover look.
                let close_btn = widgets::icon_button_styled(
                    lucide_icons::Icon::X,
                    t!(lang, "playback-close"),
                    Some(Message::Session(session::Message::CloseSettings)),
                    [4.0, 8.0],
                    widgets::window_close,
                );
                let heading = iced::widget::text(t!(lang, "tab-settings")).size(crate::ui::style::TEXT_HEADING);
                let header = iced::widget::container(
                    row![heading, iced::widget::space::horizontal(), close_btn]
                        .padding(iced::Padding {
                            top: 8.0,
                            right: 8.0,
                            bottom: 0.0,
                            left: 14.0,
                        })
                        .align_y(iced::Alignment::Center),
                )
                .width(Fill);
                let modal_panel = iced::widget::container(column![header, body].spacing(0).width(Fill).height(Fill))
                    .width(iced::Length::Fixed(820.0))
                    .height(iced::Length::Fixed(560.0))
                    .style(widgets::panel);
                // Dim wash + click-swallow + centered placement come
                // from the shared scaffolding; the dismiss handler is
                // only armed while the modal is actually open so a
                // click mid-fade-out can't re-fire the close.
                let modal = widgets::modal_layer(
                    anim::pop(modal_panel, progress, 12.0),
                    0.45 * progress,
                    Message::NoOp,
                    self.session
                        .settings
                        .shown()
                        .then_some(Message::Session(session::Message::CloseSettings)),
                );
                iced::widget::stack![Element::from(session_view), modal].into()
            } else {
                session_view
            };
            // Session entry rises into place; the scope's dy also
            // covers the way back out (the menu descends — see the
            // screen-swap match in `update`).
            let composed = match (enter, self.screen_enter_scope) {
                (Some(p), EnterScope::Root { dy }) => anim::slide_in(composed, p, iced::Vector::new(0.0, dy)),
                _ => composed,
            };
            // Snapshot the replay's current rate into the input callback.
            // Shortcut messages rebuild the view after dispatch, so each
            // repeated Shift+, / Shift+. press steps from the freshly
            // selected preset.
            let replay_speed = self
                .session
                .active_as::<session::replay::ReplaySession>()
                .map(|replay| replay.speed());
            let opponent_view = self.config.opponent_view;
            return crate::platform::input_capture::InputCapture::new(composed, move |input| {
                // Esc is reserved as the in-session escape/menu key —
                // it never reaches the joyflag pipeline so the user
                // can't accidentally hide it behind a mapping. Both
                // edges are routed: press peels overlays and arms
                // hold-to-quit, release disarms it.
                let is_escape = |k: &iced::keyboard::key::Physical| {
                    matches!(
                        k,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape)
                    )
                };
                if let crate::platform::input_capture::Input::Keyboard(kb) = &input {
                    match kb {
                        iced::keyboard::Event::KeyPressed { physical_key, .. } if is_escape(physical_key) => {
                            return Some(Message::Session(session::Message::EscPressed));
                        }
                        iced::keyboard::Event::KeyReleased { physical_key, .. } if is_escape(physical_key) => {
                            return Some(Message::Session(session::Message::EscReleased));
                        }
                        _ => {}
                    }
                }
                // Give the replay view its raw keyboard event before
                // `to_event` strips modifiers and repeat state. It returns the
                // same messages as its on-screen controls; opponent view
                // therefore also follows the App's normal persistence path.
                if let (Some(speed), crate::platform::input_capture::Input::Keyboard(kb)) = (replay_speed, &input) {
                    if let Some(shortcut) = session::view::replay::keyboard_shortcut(kb, speed, opponent_view) {
                        return Some(Message::Session(session::Message::Replay(shortcut)));
                    }
                }
                input.to_event().map(|ev| Message::Session(session::Message::Input(ev)))
            })
            .into();
        }

        // Post-match results: a full-screen moment between the session and
        // the tabs — same chrome-less cyberworld composition as the welcome
        // screen. The ScreenKey change animates the swap in both directions.
        if let Some(results) = self.session.results.as_ref() {
            let results_view =
                session::view::results_view(lang, results).map(|m| Message::Session(session::Message::Results(m)));
            let composed: Element<'_, Message> = iced::widget::stack![widgets::cyber_backdrop(), results_view]
                .width(Fill)
                .height(Fill)
                .into();
            let composed = match (enter, self.screen_enter_scope) {
                (Some(p), EnterScope::Root { dy }) => anim::slide_in(composed, p, iced::Vector::new(0.0, dy)),
                _ => composed,
            };
            // Esc dismisses — through the same synchronous capture wrapper
            // the session uses, so it works without any widget focused.
            return crate::platform::input_capture::InputCapture::new(composed, |input| {
                if let crate::platform::input_capture::Input::Keyboard(iced::keyboard::Event::KeyPressed {
                    physical_key,
                    ..
                }) = &input
                {
                    if matches!(
                        physical_key,
                        iced::keyboard::key::Physical::Code(iced::keyboard::key::Code::Escape)
                    ) {
                        return Some(Message::Session(session::Message::Results(
                            session::view::results::Message::Dismiss,
                        )));
                    }
                }
                None
            })
            .into();
        }

        let body: Element<'_, Message> = match self.tab {
            Tab::Play => {
                let main = self
                    .play
                    .view(
                        lang,
                        &self.scanners,
                        &self.loadout,
                        self.loaded.as_ref(),
                        self.config.streamer_mode,
                        &self.config,
                        &self.downloads,
                        !self.library_scanned,
                        tabs::play::LobbyBandCtx {
                            phase: &self.netplay.phase,
                            lobby: &self.netplay.lobby,
                            ready: self.netplay.ready_view(),
                            handoff_pending: self.netplay.handoff_pending(),
                            swap: &self.lobby_swap,
                            exit_snapshot: self.lobby_exit_snapshot.as_ref(),
                        },
                    )
                    .map(Message::Play);
                container(main).width(Fill).height(Fill).into()
            }
            Tab::Replays => self
                .replays
                .view(
                    lang,
                    &self.scanners,
                    &self.config,
                    &self.netplay.phase,
                    &self.downloads,
                    !self.replays_scanned,
                )
                .map(Message::Replays),
            Tab::Patches => self
                .patches
                .view(
                    lang,
                    &self.scanners,
                    &self.config,
                    &self.downloads,
                    !self.library_scanned,
                )
                .map(Message::Patches),
            Tab::Settings => {
                tabs::settings::view(lang, &self.config, &self.settings, self.updater.status_blocking(), None)
                    .map(Message::Settings)
            }
        };

        // Body content rides on the drawn cyberworld backdrop (the
        // Legacy Collection's ring-and-hex PET screen). The content
        // container itself paints no background, and the backdrop
        // sits in a layer underneath — so tab switches slide just
        // the content sideways while the cyberworld stays fixed
        // (the top bar stays put too); welcome/session swaps glide
        // the whole window up.
        let mut body_content: Element<'_, Message> = container(body)
            .width(Fill)
            .height(Fill)
            .style(widgets::body_surface)
            .into();
        if let (Some(p), EnterScope::Body { dx }) = (enter, self.screen_enter_scope) {
            body_content = anim::slide_in(body_content, p, iced::Vector::new(dx, 0.0));
        }
        let body_surface: Element<'_, Message> = iced::widget::stack![widgets::cyber_backdrop(), body_content]
            .width(Fill)
            .height(Fill)
            .into();
        // While a lobby is live and the user is on another tab, the
        // Play tab's nav pill carries a small attention dot so the
        // open lobby isn't forgotten behind a tab switch.
        let lobby_badge = self.lobby_on_screen() && self.tab != Tab::Play;
        let root: Element<'_, Message> = column![
            top_bar(lang, self.tab, lobby_badge, self.config.fullscreen),
            widgets::hud_scanline_top(),
            body_surface,
        ]
        .spacing(0)
        .width(Fill)
        .height(Fill)
        .into();
        match (enter, self.screen_enter_scope) {
            (Some(p), EnterScope::Root { dy }) => anim::slide_in(root, p, iced::Vector::new(0.0, dy)),
            _ => root,
        }
    }

    pub fn theme(&self) -> Theme {
        // Single source of truth — anything else that needs the
        // active palette (markdown link colors etc.) calls this
        // free fn too so we never drift.
        theme_for(&self.config)
    }

    /// Global UI scale multiplier — fed to `iced::application().scale_factor`.
    /// Sourced from the user's pick in graphics settings; multiplies on
    /// top of the OS DPI scale.
    pub fn scale_factor(&self) -> f32 {
        self.config.ui_scale
    }
}

fn top_bar(lang: &LanguageIdentifier, active: Tab, lobby_badge: bool, fullscreen: bool) -> Element<'_, Message> {
    use iced::widget::image::{Handle, Image};
    use lucide_icons::Icon;
    use std::sync::LazyLock;

    // Small Tango logo at the left edge of the nav strip.
    // Uses `icon.png` (the standalone logo mark) — the emblem
    // image is the long About-page banner, not what we want
    // next to a button-sized tab strip. Parsed once via
    // LazyLock so the image bytes aren't re-decoded every
    // render.
    static LOGO: LazyLock<Handle> = LazyLock::new(|| {
        let raw: &'static [u8] = include_bytes!("../icon.png");
        Handle::from_bytes(raw)
    });

    let tab =
        |icon, label, target: Tab| widgets::nav_tab_button(icon, label, Message::TabSelected(target), target == active);
    let mut bar = row![
        iced::widget::container(
            Image::new(LOGO.clone())
                .width(iced::Length::Fixed(28.0))
                .height(iced::Length::Fixed(28.0))
                .content_fit(iced::ContentFit::Contain),
        )
        .padding([2, 8]),
        widgets::nav_tab_button_badged(
            Icon::Gamepad,
            t!(lang, "tab-play"),
            Message::TabSelected(Tab::Play),
            Tab::Play == active,
            lobby_badge,
        ),
        tab(Icon::Film, t!(lang, "tab-replays"), Tab::Replays),
        horizontal_space(),
        // Decorative hexagon burst — the Legacy Collection's
        // header motif, trailing off ahead of the utility tabs.
        // Sized just shy of the chips so it fills the band.
        widgets::hex_chain(32.0),
        // Patches + Settings = low-emphasis utility tabs.
        // Patch management is an occasional maintenance chore,
        // not a destination, so it doesn't get equal billing
        // with Play/Replays — icon-only on the right, with the
        // label exposed as a hover tooltip.
        widgets::nav_icon_tab_button(
            Icon::Puzzle,
            t!(lang, "tab-patches"),
            Message::TabSelected(Tab::Patches),
            Tab::Patches == active,
        ),
        widgets::nav_icon_tab_button(
            Icon::Settings,
            t!(lang, "tab-settings"),
            Message::TabSelected(Tab::Settings),
            Tab::Settings == active,
        ),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    if fullscreen {
        // Fullscreen is borderless — no OS title bar, so no native
        // X. Stand in for it at the same screen corner, in the
        // titlebar-close mood (quiet at rest, red on hover).
        bar = bar.push(widgets::icon_button_styled(
            Icon::X,
            t!(lang, "window-quit"),
            Some(Message::Quit),
            [8.0, 12.0],
            widgets::window_close,
        ));
    }
    container(bar.padding([10, 8]))
        .width(Fill)
        .style(widgets::hud_bar)
        .into()
}
