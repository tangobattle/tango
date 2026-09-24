//! Central message dispatch and screen transition bookkeeping.

use super::{App, EnterScope, Message, ScreenKey, PANE_SLIDE, ROOT_SLIDE};
use crate::tabs;

impl App {
    pub fn update(&mut self, message: Message) -> iced::Task<Message> {
        let screen_before = self.screen_key();
        let family_before = self.loadout.family();
        // Candidate snapshot for the lobby's exit animation — taken
        // before dispatch (the handler about to run may reset the
        // phase/lobby), kept only if the lobby actually left.
        let lobby_live = self.lobby_on_screen().then(|| {
            (
                self.netplay.phase.clone(),
                self.netplay.lobby.clone(),
                self.netplay.ready_view(),
            )
        });
        let task = self.update_inner(message);
        let now = iced::time::Instant::now();
        let screen_after = self.screen_key();
        if screen_before != screen_after {
            self.screen_enter.start(now);
            self.screen_enter_scope = match (screen_before, screen_after) {
                (ScreenKey::Tabs(t1), ScreenKey::Tabs(t2)) => {
                    // The nav strip lays the tabs out in declaration
                    // order, so the discriminants double as nav
                    // positions: moving right brings the new pane in
                    // from the right, moving left from the left.
                    let dx = if (t2 as u8) > (t1 as u8) {
                        PANE_SLIDE
                    } else {
                        -PANE_SLIDE
                    };
                    EnterScope::Body { dx }
                }
                // Closing a session descends — the menu comes back
                // in from above, mirroring the rise that brought
                // the session up.
                (ScreenKey::Session, _) => EnterScope::Root { dy: -ROOT_SLIDE },
                _ => EnterScope::Root { dy: ROOT_SLIDE },
            };
        }
        // The Play tab's band swap follows the netplay phase: the
        // view morphs the link-code strip into the lobby (and back)
        // off this transition. When the lobby leaves, freeze its last
        // live state for the exit half to render from.
        let lobby_after = self.lobby_on_screen();
        if let (Some(snap), false) = (lobby_live, lobby_after) {
            self.lobby_exit_snapshot = Some(snap);
            // A Fight-generated code never touched the input on the way in
            // (it debuted in the lobby band) — drop it in now that the
            // strip is coming back, so a retry re-hosts the same code.
            self.play.restore_generated_link_code();
        }
        self.lobby_swap.set(lobby_after, now);
        // A different family swaps the entire bottom of the tab —
        // rise the whole save-view pane in. A different game or save
        // within the family only re-renders the save's content, and
        // that entrance rides in with the reloaded save itself: its
        // view state is minted by the load and starts its own rise.
        if family_before != self.loadout.family() {
            self.play.animate_family_switch(now);
        }
        task
    }

    fn update_inner(&mut self, message: Message) -> iced::Task<Message> {
        match message {
            Message::NoOp => iced::Task::none(),
            Message::AnimTick => iced::Task::none(),
            Message::Quit => self.quit(),
            Message::Exit => iced::exit(),
            Message::TabSelected(t) => {
                self.tab = t;
                // A tab switch unmounts the input settings pane's capture
                // wrapper, so key/button releases stop arriving — drop the
                // held set rather than show stale-lit binding chips on the
                // way back.
                self.settings.held = Default::default();
                // Clicking a scanner-backed tab re-runs the scan in the
                // background — there are no Rescan buttons; this is how
                // new files on disk get noticed. Cheap when nothing
                // changed (stat-fingerprint gated, see Catalog::rescan).
                // Deliberately not limited to *entering* the tab: pressing
                // the tab you are already on is what a user reaches for
                // when they have just dropped a file in, and it is the
                // only gesture left that means "look again".
                // Play additionally throws away the loaded bundle after
                // the scan, even when the selected paths did not change:
                // the click is an explicit reload from disk, including
                // discarding staged edits and rebuilding ROM/patch assets.
                // Settings doesn't read the scanners, so skip it there.
                if !self.is_rescanning() {
                    if let Some(followup) = t.rescan_followup() {
                        return self.rescan_off_thread(followup);
                    }
                }
                iced::Task::none()
            }
            // Loadout strip interactions route to the shared
            // App-level Loadout — the tab never sees them.
            // Every dispatch below is followed by a Settings resend —
            // the netplay handler dedupes against the last-sent value
            // via `Settings: Eq`, so unchanged dispatches are free.
            Message::Play(tabs::play::Message::Loadout(m)) => {
                let task = self.update_loadout(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Play(m) => {
                let task = self.update_play(m);
                iced::Task::batch([task, self.resend_settings_if_lobby()])
            }
            Message::Patches(m) => self.update_patches(m),
            Message::Download(event) => {
                if self.downloads.apply(&event) {
                    self.update_patches(event.message)
                } else {
                    iced::Task::none()
                }
            }
            Message::DiscordTick => {
                self.handle_discord_tick();
                iced::Task::none()
            }
            Message::Window(id, ev) => self.update_window(id, ev),
            Message::WindowScaleQueried { id, scale } => {
                // Cap how big the window can get dragged. A surface is
                // the window size in *physical* pixels, so on a wide
                // enough desktop a drag-resize can walk it past what
                // the GPU will configure — and that isn't an error the
                // app gets to handle, it's a panic from inside wgpu.
                // iced converts this cap back to physical with the
                // same `scale × ui_scale` it reports sizes with, so
                // what winit ends up enforcing is exactly
                // `MAX_SURFACE_SIZE` physical pixels — which stays the
                // right cap even if either scale changes later.
                let (min_w, min_h) = crate::window::MINIMUM_RESOLUTION;
                let scale = (scale * self.config.ui_scale).max(0.01);
                let cap = crate::MAX_SURFACE_SIZE / scale;
                let max = iced::Size::new(cap.max(min_w as f32), cap.max(min_h as f32));
                iced::window::set_max_size(id, Some(max))
            }
            Message::WindowMaximizedQueried { size, maximized } => {
                if !maximized {
                    self.config.last_window_size = Some((size.width, size.height));
                }
                self.config.last_window_maximized = maximized;
                self.persist_config();
                iced::Task::none()
            }
            Message::Replays(m) => self.update_replays(m),
            Message::Settings(m) => self.update_settings(m),
            Message::Welcome(m) => self.update_welcome(m),
            Message::Session(m) => self.update_session(m),
            Message::Netplay(delivery) => self.update_netplay(delivery),
            Message::PvpSessionBuilt(attempt, slot) => {
                let Some(result) = slot.lock().unwrap().take() else {
                    return iced::Task::none();
                };
                self.finish_pvp_handoff(attempt, result)
            }
            Message::Rescanned(followup) => self.finish_rescan(followup),
        }
    }
}
