//! About pane: credits markdown and updater status.

use super::*;
// Explicit: macros reached only through the glob above are ambiguous.
use sweeten::widget::{column, row};

/// Holder for the parsed-once markdown Content. Tab state field
/// because `markdown::Content` is `!Sync` (interior mutability
/// for incremental parsing) and the parsed Items must outlive
/// the `Element<'_>` that `markdown::view` borrows from.
/// `OnceCell` so the parse only runs the first time the About
/// tab renders.
#[derive(Default)]
pub struct AboutMarkdown(std::cell::OnceCell<iced::widget::markdown::Content>);

impl AboutMarkdown {
    fn content(&self) -> &iced::widget::markdown::Content {
        self.0.get_or_init(|| {
            iced::widget::markdown::Content::parse(&format!(
                "# Tango {}\n{}",
                env!("CARGO_PKG_VERSION"),
                include_str!("../../../../CREDITS.md")
            ))
        })
    }
}

pub(super) fn settings_about<'a>(
    lang: &'a LanguageIdentifier,
    config: &'a config::Config,
    about: &'a AboutMarkdown,
    updater_status: crate::updater::Status,
) -> Element<'a, Message> {
    use iced::widget::image::{Handle, Image};
    use iced::widget::markdown;
    use std::sync::LazyLock;

    static EMBLEM: LazyLock<Handle> = LazyLock::new(|| {
        let raw: &'static [u8] = include_bytes!("../../emblem.png");
        Handle::from_bytes(raw)
    });

    let _ = lang; // about screen is English-only, matching legacy.

    let emblem = iced::widget::container(Image::new(EMBLEM.clone()).width(Length::Fixed(200.0)))
        .width(Fill)
        .align_x(iced::alignment::Horizontal::Center);

    // Pull the live Theme from the same `crate::ui::theme::theme_for` the
    // App's theme callback uses — keeps link color in sync with
    // the rest of the app instead of pinning to DARK + TANGO_GREEN
    // by hand. `Settings::from(&Theme)` defaults to text-size 16,
    // so wrap to also pin the app's body text size.
    let theme = crate::ui::theme::theme_for(config);
    let style = crate::ui::theme::markdown_style(&theme);
    let settings = markdown::Settings::with_text_size(TEXT_BODY, style);
    let body: Element<'a, Message> = markdown::view(about.content().items(), settings).map(Message::OpenUrl);

    column![
        emblem,
        body,
        updater_section(lang, updater_status, config.enable_updater)
    ]
    .spacing(12)
    // Symmetric inner padding so the emblem doesn't rub
    // the nav scanline at the top, the updater section
    // breathes at the page end, and link text doesn't slam
    // the left edge.
    .padding(style::PANE_PADDING)
    .into()
}

/// Bottom-of-About updater status panel. Current version
/// vs. latest, a one-line status, and an Update Now button
/// when a download is ready. Hidden release notes/download
/// progress until they're actually relevant.
fn updater_section<'a>(
    lang: &'a LanguageIdentifier,
    status: crate::updater::Status,
    enable_updater: bool,
) -> Element<'a, Message> {
    use crate::updater::Status as S;

    let current = env!("CARGO_PKG_VERSION");
    let is_ready = matches!(status, S::ReadyToUpdate { .. });

    let latest_label: String = match &status {
        S::UpToDate { release: None } | S::UpToDate { release: Some(None) } => t!(lang, "updater-loading"),
        S::UpToDate { release: Some(Some(r)) } => t!(lang, "updater-up-to-date", version = r.version.to_string()),
        S::UpdateAvailable { release: r } | S::Downloading { release: r, .. } | S::ReadyToUpdate { release: r } => {
            format!("v{}", r.version)
        }
    };

    let status_line: Option<Element<'a, Message>> = match &status {
        S::Downloading { current, total, .. } => {
            let pct = if *total > 0 {
                (*current as f32 / *total as f32 * 100.0).round() as u32
            } else {
                0
            };
            Some(
                text(t!(lang, "updater-downloading", pct = pct as i64))
                    .size(TEXT_CAPTION)
                    .style(widgets::muted_text_style)
                    .into(),
            )
        }
        S::ReadyToUpdate { .. } => Some(
            text(t!(lang, "updater-ready-to-update"))
                .size(TEXT_CAPTION)
                .style(widgets::muted_text_style)
                .into(),
        ),
        _ => None,
    };

    let action: Option<Element<'a, Message>> = is_ready.then(|| {
        widgets::labeled_icon_button(
            Icon::Download,
            t!(lang, "updater-update-now"),
            Message::UpdateNow,
            STANDARD_PADDING,
            widgets::primary_button,
        )
    });

    // The "latest version" readout only makes sense when the updater is on; with
    // it disabled we never fetch a release, so show just the current version and
    // drop the latest-version line.
    let mut version_row =
        row![text(t!(lang, "updater-current-version", version = format!("v{current}"))).size(TEXT_CAPTION),]
            .spacing(8)
            .align_y(Alignment::Center);
    if enable_updater {
        version_row = version_row.push(horizontal_space());
        version_row =
            version_row.push(text(t!(lang, "updater-latest-version", version = latest_label)).size(TEXT_CAPTION));
    }

    let mut col = column![accent_hairline(), version_row].spacing(8);
    match (status_line, action) {
        // Update ready: the button sits to the right of the status message on the
        // same row, rather than on its own row below it.
        (Some(s), Some(a)) => {
            col = col.push(row![s, horizontal_space(), a].spacing(8).align_y(Alignment::Center));
        }
        (Some(s), None) => col = col.push(s),
        (None, Some(a)) => col = col.push(row![horizontal_space(), a].spacing(8)),
        (None, None) => {}
    }
    col.into()
}
