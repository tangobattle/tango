use super::{Message, State};
use crate::i18n::t;
use crate::library::{package, Scanners};
use crate::ui::style::{self, STANDARD_PADDING, TEXT_BODY, TEXT_CAPTION, TEXT_TITLE};
use crate::ui::widgets;
use iced::widget::{button, container, scrollable, text};
use iced::{Alignment, Element, Fill};
use lucide_icons::Icon;
use sweeten::widget::{column, row, text_input};
use tango_script::{ExportKind, PackageRef};
use unic_langid::LanguageIdentifier;

impl State {
    pub fn view<'a>(
        &'a self,
        lang: &'a LanguageIdentifier,
        scanners: &Scanners,
        scanning: bool,
    ) -> Element<'a, Message> {
        let catalog = scanners.packages.read();
        let idle = !self.busy && !scanning;
        let top_row = row![
            text_input(&t!(lang, "packages-search"), &self.search)
                .on_input(Message::SearchChanged)
                .padding(STANDARD_PADDING)
                .width(Fill)
                .style(widgets::chunky_text_input),
            button(text(t!(lang, "packages-install")).size(TEXT_BODY))
                .on_press_maybe(idle.then_some(Message::Install))
                .padding(STANDARD_PADDING)
                .style(widgets::primary_button),
            widgets::icon_button_maybe(
                Icon::RefreshCw,
                t!(lang, "packages-refresh"),
                idle.then_some(Message::Refresh),
                STANDARD_PADDING
            ),
            widgets::icon_button(
                Icon::FolderOpen,
                t!(lang, "packages-open-folder"),
                Message::OpenFolder,
                STANDARD_PADDING
            ),
            button(text(t!(lang, "packages-legacy-patches")).size(TEXT_CAPTION))
                .on_press(Message::LegacyPatches)
                .padding(STANDARD_PADDING)
                .style(widgets::neutral),
        ]
        .spacing(8)
        .align_y(Alignment::Center);
        let mut top = column![top_row].spacing(6);
        if self.busy || scanning {
            top = top.push(
                text(t!(lang, "packages-working"))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style),
            );
        }
        if let Some(error) = &self.error {
            top = top.push(
                text(t!(lang, "packages-operation-failed", error = error.clone()))
                    .size(TEXT_CAPTION)
                    .style(widgets::danger_text_style),
            );
        }
        let top = container(top)
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane);
        let query = self.search.trim().to_lowercase();
        let entries: Vec<_> = catalog
            .entries()
            .iter()
            .filter(|entry| entry.package.manifest().name.to_lowercase().contains(&query))
            .collect();
        let left: Element<'_, Message> = if entries.is_empty() {
            widgets::pane_prompt(if scanning {
                t!(lang, "packages-working")
            } else {
                t!(lang, "packages-empty")
            })
        } else {
            let mut list = column![].spacing(2).padding([8, 0]);
            for (index, entry) in entries.iter().enumerate() {
                let reference = entry.reference();
                let selected = self.selected.as_ref() == Some(&reference);
                let status = if entry.error.is_some() {
                    t!(lang, "packages-unavailable")
                } else {
                    source_label(lang, entry)
                };
                list = list.push(
                    button(
                        column![
                            text(reference.name.clone()).size(TEXT_BODY),
                            text(format!("{} · {status}", reference.version))
                                .size(TEXT_CAPTION)
                                .style(widgets::list_caption_style(selected)),
                        ]
                        .spacing(2),
                    )
                    .on_press(Message::Selected(reference))
                    .padding(style::ROW_PADDING)
                    .width(Fill)
                    .style(widgets::list_item(selected, index)),
                );
            }
            container(scrollable(list).style(widgets::chunky_scrollable))
                .width(Fill)
                .height(Fill)
                .style(widgets::pane)
                .into()
        };
        let mut right = column![].spacing(style::PANE_GAP);
        if let Some(entry) = self.selected.as_ref().and_then(|reference| catalog.entry(reference)) {
            right = right.push(self.detail(lang, &catalog, entry, idle));
        } else {
            right = right.push(widgets::pane_prompt(t!(lang, "packages-select-prompt")));
        }
        if !catalog.issues().is_empty() {
            let mut issues = column![text(t!(lang, "packages-issues")).size(TEXT_BODY)].spacing(6);
            for issue in catalog.issues() {
                issues = issues.push(text(issue.clone()).size(TEXT_CAPTION).style(widgets::danger_text_style));
            }
            right = right.push(
                container(issues)
                    .padding(style::PANE_PADDING)
                    .width(Fill)
                    .style(widgets::pane),
            );
        }
        widgets::top_split_pane(top, left, scrollable(right).style(widgets::chunky_scrollable))
    }

    fn detail<'a>(
        &self,
        lang: &LanguageIdentifier,
        catalog: &package::Catalog,
        entry: &package::Entry,
        idle: bool,
    ) -> Element<'a, Message> {
        let reference = entry.reference();
        let manifest = entry.package.manifest();
        let mut body = column![
            text(format!("{} {}", reference.name, reference.version)).size(TEXT_TITLE),
            text(source_label(lang, entry))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style),
        ]
        .spacing(10);
        if let Some(error) = &entry.error {
            body = body.push(text(error.clone()).size(TEXT_CAPTION).style(widgets::danger_text_style));
        }
        let mut actions = row![].spacing(8).align_y(Alignment::Center);
        let blockers = package::removal_blockers(catalog, &reference);
        let removable = entry
            .sources
            .iter()
            .any(|source| matches!(source, package::Source::Archive(_)));
        for source in &entry.sources {
            if let package::Source::Archive(path) | package::Source::Directory(path) = source {
                actions = actions.push(widgets::icon_button(
                    Icon::FolderOpen,
                    t!(lang, "packages-reveal"),
                    Message::Reveal(path.clone()),
                    STANDARD_PADDING,
                ));
            }
        }
        if removable {
            actions = actions.push(
                button(text(t!(lang, "packages-remove")).size(TEXT_BODY))
                    .on_press_maybe((idle && blockers.is_empty()).then_some(Message::Uninstall(reference.clone())))
                    .padding(STANDARD_PADDING)
                    .style(widgets::neutral),
            );
        }
        body = body.push(actions);
        if !blockers.is_empty() && removable {
            body = body.push(text(t!(lang, "packages-required-by", packages = labels(&blockers))).size(TEXT_CAPTION));
        }
        body = body.push(text(t!(lang, "packages-exports")).size(TEXT_BODY));
        let mut exported = false;
        for kind in ExportKind::ALL {
            for export in manifest.exports(kind) {
                exported = true;
                let label = match kind {
                    ExportKind::Editor => t!(lang, "packages-editor"),
                    ExportKind::GameMode => t!(lang, "packages-gamemode"),
                    ExportKind::Telemetry => t!(lang, "packages-telemetry"),
                };
                let name = if kind == ExportKind::GameMode && manifest.default_gamemode.as_ref() == Some(&export.name) {
                    t!(lang, "packages-default-export", name = export.name.clone())
                } else {
                    export.name.clone()
                };
                body = body.push(text(format!("{label}: {name}")).size(TEXT_CAPTION));
            }
        }
        if !exported {
            body = body.push(text(t!(lang, "packages-library")).size(TEXT_CAPTION));
        }
        body = body.push(text(t!(lang, "packages-dependencies")).size(TEXT_BODY));
        if manifest.dependencies.is_empty() {
            body = body.push(text(t!(lang, "packages-no-dependencies")).size(TEXT_CAPTION));
        }
        for (name, version) in &manifest.dependencies {
            let reference = PackageRef {
                name: name.clone(),
                version: version.clone(),
            };
            let found = catalog.entry(&reference);
            let label = format!("{name} {version}");
            let label = if found.is_none() {
                t!(lang, "packages-missing-dependency", package = label)
            } else {
                label
            };
            body = body.push(
                button(text(label).size(TEXT_CAPTION))
                    .on_press_maybe(found.map(|_| Message::Selected(reference)))
                    .padding(STANDARD_PADDING)
                    .style(widgets::neutral),
            );
        }
        container(body)
            .padding(style::PANE_PADDING)
            .width(Fill)
            .style(widgets::pane)
            .into()
    }
}

fn source_label(lang: &LanguageIdentifier, entry: &package::Entry) -> String {
    let mut labels = Vec::new();
    for source in &entry.sources {
        labels.push(match source {
            package::Source::Bundled => t!(lang, "packages-source-bundled"),
            package::Source::Directory(_) => t!(lang, "packages-source-development"),
            package::Source::Archive(_) => t!(lang, "packages-source-installed"),
        });
    }
    labels.join(" · ")
}

fn labels(references: &[PackageRef]) -> String {
    references
        .iter()
        .map(|reference| format!("{} {}", reference.name, reference.version))
        .collect::<Vec<_>>()
        .join(", ")
}
