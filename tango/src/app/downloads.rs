//! Owns download lifetime and progress independently of application navigation.
use crate::library::patch;
use crate::tabs;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Event {
    generation: u64,
    pub message: tabs::patches::Message,
}
#[derive(Default)]
pub struct Coordinator {
    entries: patch::Downloads,
    flights: HashMap<patch::VersionKey, (u64, tokio_util::sync::CancellationToken)>,
    next_generation: u64,
}
impl Coordinator {
    pub fn entries(&self) -> &patch::Downloads {
        &self.entries
    }
    pub fn cancel(&mut self, key: &patch::VersionKey) {
        if let Some((_, token)) = self.flights.remove(key) {
            token.cancel();
        }
        self.entries.remove(key);
    }
    pub fn apply(&mut self, event: &Event) -> bool {
        use tabs::patches::Message as M;
        let key = match &event.message {
            M::InstallProgress(key, ..) | M::InstallFinished(key, _) | M::InstallCancelled(key) => key,
            _ => return false,
        };
        if !self.flights.get(key).is_some_and(|(id, _)| *id == event.generation) {
            return false;
        }
        match &event.message {
            M::InstallProgress(_, downloaded, total) => {
                self.entries.insert(
                    key.clone(),
                    patch::Download::Running(patch::Progress {
                        downloaded: *downloaded,
                        total: *total,
                    }),
                );
            }
            M::InstallFinished(_, Err(_)) => {
                self.flights.remove(key);
                self.entries.insert(key.clone(), patch::Download::Failed);
            }
            _ => {
                self.flights.remove(key);
                self.entries.remove(key);
            }
        }
        true
    }
    pub fn start(
        &mut self,
        key: patch::VersionKey,
        catalog: &patch::Catalog,
        url: String,
        root: std::path::PathBuf,
    ) -> iced::Task<Event> {
        if self.flights.contains_key(&key) {
            return iced::Task::none();
        }
        self.next_generation += 1;
        let generation = self.next_generation;
        let token = tokio_util::sync::CancellationToken::new();
        self.flights.insert(key.clone(), (generation, token.clone()));
        let (name, version) = key.clone();
        let Some(entry) = catalog.entry(&name, &version).cloned() else {
            return iced::Task::done(Event {
                generation,
                message: tabs::patches::Message::InstallFinished(key, Err("not offered by this patch repo".into())),
            });
        };
        self.entries.insert(
            key.clone(),
            patch::Download::Running(patch::Progress {
                downloaded: 0,
                total: 0,
            }),
        );
        let (tx, rx) = futures::channel::mpsc::unbounded();
        let progress_tx = tx.clone();
        let progress_key = key.clone();
        tokio::spawn(async move {
            let result = patch::download(
                crate::library::http(),
                crate::library::storage(),
                &url,
                &root,
                &name,
                &version,
                &entry,
                move |p| {
                    let _ = progress_tx.unbounded_send(Event {
                        generation,
                        message: tabs::patches::Message::InstallProgress(progress_key.clone(), p.downloaded, p.total),
                    });
                    !token.is_cancelled()
                },
            )
            .await;
            let message = match result {
                Ok(patch::Outcome::Cancelled) => tabs::patches::Message::InstallCancelled(key),
                Ok(patch::Outcome::Installed) => tabs::patches::Message::InstallFinished(key, Ok(())),
                Err(e) => tabs::patches::Message::InstallFinished(key, Err(format!("{e:#}"))),
            };
            let _ = tx.unbounded_send(Event { generation, message });
        });
        iced::Task::stream(rx)
    }
}
impl Drop for Coordinator {
    fn drop(&mut self) {
        for (_, token) in self.flights.values() {
            token.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancelled_attempt_cannot_overwrite_a_retry() {
        let mut owner = Coordinator::default();
        let key = ("patch".into(), semver::Version::new(1, 0, 0));
        let first = tokio_util::sync::CancellationToken::new();
        owner.flights.insert(key.clone(), (1, first.clone()));
        owner.cancel(&key);
        assert!(first.is_cancelled());
        owner
            .flights
            .insert(key.clone(), (2, tokio_util::sync::CancellationToken::new()));
        assert!(!owner.apply(&Event {
            generation: 1,
            message: tabs::patches::Message::InstallFinished(key.clone(), Err("old".into()))
        }));
        assert!(owner.apply(&Event {
            generation: 2,
            message: tabs::patches::Message::InstallProgress(key.clone(), 5, 10)
        }));
        assert!(owner.entries()[&key].is_running());
    }
}
