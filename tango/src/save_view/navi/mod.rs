use super::*;
use sweeten::widget::column;

pub(super) fn render_navi<M: 'static>(lang: &LanguageIdentifier, loaded: &Loaded) -> Element<'static, M> {
    let Some(nv) = loaded.save.view_navi() else {
        return placeholder(t!(lang, "save-empty"));
    };
    render_navi_card::<M>(loaded, nv.navi())
}

/// The equipped-navi card: emblem on an accent-tinted plate and the navi name.
fn render_navi_card<M: 'static>(loaded: &Loaded, navi_id: usize) -> Element<'static, M> {
    let assets = loaded.assets.as_ref();

    let name = assets
        .navi(navi_id)
        .and_then(|n| n.name())
        .unwrap_or_else(|| format!("Navi #{navi_id}"));
    // Plate/glow tint: the emblem's own signature color, with a
    // neutral slate fallback for monochrome emblems.
    let accent = loaded
        .navi_accents
        .get(&navi_id)
        .copied()
        .unwrap_or(iced::Color::from_rgb8(0x6b, 0x7a, 0x99));

    // Emblem at an integer multiple of its 15px crop so the
    // nearest-neighbor upscale lands on even pixels.
    let emblem: Element<'static, M> = loaded
        .navi_emblems
        .get(&navi_id)
        .cloned()
        .map(|h| {
            Image::new(h)
                .width(Length::Fixed(90.0))
                .height(Length::Fixed(90.0))
                .filter_method(iced_image::FilterMethod::Nearest)
                .content_fit(ContentFit::Contain)
                .into()
        })
        .unwrap_or_else(|| {
            Space::new()
                .width(Length::Fixed(90.0))
                .height(Length::Fixed(90.0))
                .into()
        });

    // Circular plate behind the emblem: accent-tinted fill, a
    // ring a shade brighter, and an accent glow lifting it off
    // the pane.
    let plate: Element<'static, M> = container(emblem)
        .width(Length::Fixed(140.0))
        .height(Length::Fixed(140.0))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .style(move |theme: &iced::Theme| {
            let bg = theme.palette().background;
            container::Style {
                background: Some(iced::Background::Color(crate::widgets::mix(bg, accent, 0.22))),
                border: iced::Border {
                    radius: 70.0.into(),
                    width: 2.0,
                    color: iced::Color { a: 0.8, ..accent },
                },
                shadow: iced::Shadow {
                    color: iced::Color { a: 0.45, ..accent },
                    offset: iced::Vector::new(0.0, 0.0),
                    blur_radius: 26.0,
                },
                ..Default::default()
            }
        })
        .into();

    let card = column![plate, text(name).size(TEXT_DISPLAY)]
        .spacing(16)
        .align_x(Alignment::Center);

    // The pane itself picks up a whisper of the accent, fading
    // back to the standard plate color toward the bottom.
    container(card)
        .width(Fill)
        .align_x(Alignment::Center)
        .padding([28.0, crate::style::PANE_PADDING])
        .style(move |theme: &iced::Theme| {
            let mut s = crate::widgets::pane(theme);
            if let Some(iced::Background::Color(plate_color)) = s.background {
                // Stop 0 sits at the bottom for a 0-radian linear
                // gradient, so the accent goes on stop 1 — the
                // tint halos the plate at the top of the card.
                s.background = Some(iced::Background::Gradient(iced::Gradient::Linear(
                    iced::gradient::Linear::new(0.0)
                        .add_stop(0.0, plate_color)
                        .add_stop(1.0, crate::widgets::mix(plate_color, accent, 0.10)),
                )));
            }
            s
        })
        .into()
}

/// The Navi tab as text: the equipped navi's name.
pub(crate) fn navi_as_text(loaded: &Loaded) -> Option<String> {
    let assets = loaded.assets.as_ref();
    let id = loaded.save.view_navi()?.navi();
    let name = assets.navi(id).and_then(|n| n.name()).unwrap_or_else(|| format!("#{id}"));
    Some(format!("{name}\n"))
}
