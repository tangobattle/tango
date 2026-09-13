//! Offscreen drawing for package-owned pixel artwork and scaled diagrams.
use iced::widget::image;
use sha3::{Digest, Sha3_256};
use tango_script::ui::{canvas::Rasterize, Draw};

impl crate::resources::Images {
    pub(crate) fn rasterize(
        &self,
        width: u32,
        height: u32,
        commands: &[Draw],
        options: Rasterize,
        theme: &iced::Theme,
        font: iced::Font,
    ) -> image::Handle {
        // Stream the description into the hash: embedded pixel buffers must
        // not allocate an expanded JSON copy merely to identify the drawing.
        struct HashWriter(Sha3_256);
        impl std::io::Write for HashWriter {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.update(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let mut hash = HashWriter(Sha3_256::new());
        serde_json::to_writer(
            &mut hash,
            &(
                "canvas",
                width,
                height,
                options,
                commands,
                format!("{theme:?}/{font:?}/{:?}", theme.extended_palette()),
            ),
        )
        .expect("validated canvas serialization");
        let key = hash.0.finalize().into();
        if let Some(handle) = self.cached(&key) {
            return handle;
        }

        let mut renderer = iced_tiny_skia::Renderer::new(font, iced::Pixels(tango_ui::style::TEXT_BODY));
        let size = iced::Size::new(width, height);
        let bounds = iced::Rectangle::with_size(iced::Size::new(width as f32, height as f32));
        let mut frame = iced::advanced::graphics::geometry::Frame::new(&renderer, bounds.size());
        for command in commands {
            crate::canvas::draw(&mut frame, theme, command, self, font);
        }
        iced::advanced::graphics::geometry::Renderer::draw_geometry(&mut renderer, frame.into_geometry());
        let mut pixels = tiny_skia::Pixmap::new(width, height).expect("validated canvas dimensions");
        let mut mask = tiny_skia::Mask::new(width, height).expect("validated canvas dimensions");
        let viewport = iced::advanced::graphics::Viewport::with_physical_size(size, 1.0);
        renderer.draw(
            &mut pixels.as_mut(),
            &mut mask,
            &viewport,
            &[bounds],
            iced::Color::TRANSPARENT,
        );
        // The software surface is premultiplied BGRA; image handles take
        // straight RGBA, including drawings on a transparent background.
        let mut rgba = pixels.take();
        for pixel in rgba.chunks_exact_mut(4) {
            pixel.swap(0, 2);
            let alpha = pixel[3] as u32;
            if alpha > 0 && alpha < 255 {
                for channel in &mut pixel[..3] {
                    *channel = ((*channel as u32 * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        let radius = options.corner_radius.min(width / 2).min(height / 2);
        let r = radius as f32;
        for (x0, y0, cx, cy) in [
            (0, 0, r, r),
            (width - radius, 0, width as f32 - r, r),
            (0, height - radius, r, height as f32 - r),
            (width - radius, height - radius, width as f32 - r, height as f32 - r),
        ] {
            for y in y0..y0 + radius {
                for x in x0..x0 + radius {
                    let dx = x as f32 + 0.5 - cx;
                    let dy = y as f32 + 0.5 - cy;
                    if dx * dx + dy * dy > r * r {
                        rgba[((y * width + x) * 4 + 3) as usize] = 0;
                    }
                }
            }
        }
        self.insert(key, width, height, rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tango_script::ui::{
        canvas::Paint,
        style::{Color, ThemeColor},
    };

    fn fill(color: Color) -> Vec<Draw> {
        vec![Draw::Rect {
            x: 0.0,
            y: 0.0,
            width: 8.0,
            height: 8.0,
            radius: 0.0,
            paint: Paint {
                fill: color,
                ..Default::default()
            },
        }]
    }

    #[test]
    fn rasterization_preserves_alpha_masks_corners_and_invalidates_cached_content() {
        let images = crate::resources::Images::default();
        let options = Rasterize {
            corner_radius: 3,
            nearest: false,
        };
        let red = fill(Color::Rgba([1.0, 0.0, 0.0, 0.5]));
        let render = |commands: &[Draw], theme: &iced::Theme, options| {
            images.rasterize(8, 8, commands, options, theme, iced::Font::DEFAULT)
        };
        let first = render(&red, &iced::Theme::Dark, options);
        let image::Handle::Rgba {
            width, height, pixels, ..
        } = &first
        else {
            panic!()
        };
        assert_eq!((*width, *height), (8, 8));
        assert_eq!(&pixels[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4], &[255, 0, 0, 128]);
        assert_eq!(pixels[3], 0);
        assert_eq!(pixels[(7 * 8 + 7) * 4 + 3], 0);
        assert_eq!(render(&red, &iced::Theme::Dark, options).id(), first.id());
        let unmasked = render(&red, &iced::Theme::Dark, Rasterize::default());
        assert_ne!(unmasked.id(), first.id());
        let image::Handle::Rgba { pixels, .. } = unmasked else {
            panic!()
        };
        assert_eq!(&pixels[..4], &[255, 0, 0, 128]);
        let green = render(&fill(Color::Rgba([0.0, 1.0, 0.0, 1.0])), &iced::Theme::Dark, options);
        assert_ne!(green.id(), first.id());
        let image::Handle::Rgba { pixels, .. } = green else {
            panic!()
        };
        assert_eq!(&pixels[(4 * 8 + 4) * 4..(4 * 8 + 4) * 4 + 4], &[0, 255, 0, 255]);
        let themed = fill(Color::Theme(ThemeColor::Primary));
        let dark = render(&themed, &iced::Theme::Dark, options);
        let light = render(&themed, &iced::Theme::Light, options);
        assert_ne!(dark.id(), light.id());
        assert_eq!(render(&themed, &iced::Theme::Dark, options).id(), dark.id());
    }
}
