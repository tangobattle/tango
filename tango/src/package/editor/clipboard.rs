//! Desktop transport for validated, package-owned clipboard effects.
use tango_script::Effect;

pub(super) fn write(renderer: &tango_script_iced::Renderer, theme: &iced::Theme, effect: Effect) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    match effect {
        Effect::Edit { .. } | Effect::ScrollTo { .. } | Effect::Invoke { .. } => {
            anyhow::bail!("expected a clipboard effect")
        }
        Effect::CopyText { text } => clipboard.set_text(text)?,
        Effect::CopyHtml { text, html } => {
            if clipboard.set_html(html, Some(text.clone())).is_err() {
                clipboard.set_text(text)?;
            }
        }
        Effect::CopyImage { image } => {
            let handle = renderer.render_image(&image, theme, iced::Font::with_name("Noto Sans"))?;
            let iced::widget::image::Handle::Rgba {
                width, height, pixels, ..
            } = handle
            else {
                unreachable!("package images always render to RGBA")
            };
            clipboard.set_image(arboard::ImageData {
                width: width as usize,
                height: height as usize,
                bytes: std::borrow::Cow::Borrowed(&pixels),
            })?;
        }
    }
    Ok(())
}
