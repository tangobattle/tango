//! Applies a patch's `[rom_overrides]` on top of a game's own assets.
//!
//! The schema itself lives in `tango_patch::Overrides` (it's part of the
//! package format, and the bundler validates it at pack time). This
//! module is only the runtime layering.

use tango_patch::overrides::{ChipOverride, NavicustPartOverride, PatchCard56Override, StyleOverride};

/// A patch card effect name template, resolved against an effect's
/// parameter. The format stores parts as `{ t = "text" }` or
/// `{ p = 1 }`; `p == 1` is the effect's parameter, doubled by ten for
/// the two id's that record it in tens.
fn render_effect_name(
    parts: &[tango_patch::overrides::TemplatePart],
    effect: &tango_dataview::rom::PatchCard56Effect,
) -> String {
    parts
        .iter()
        .map(|part| {
            if let Some(t) = &part.t {
                t.clone()
            } else if part.p == Some(1) {
                let mut parameter = effect.parameter as u32;
                if effect.id == 0x00 || effect.id == 0x02 {
                    parameter *= 10;
                }
                format!("{parameter}")
            } else {
                String::new()
            }
        })
        .collect()
}

pub struct OverridenAssets {
    assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>,
    overrides: tango_patch::Overrides,
}

impl OverridenAssets {
    pub fn new(assets: Box<dyn tango_dataview::rom::Assets + Send + Sync>, overrides: tango_patch::Overrides) -> Self {
        Self { assets, overrides }
    }
}

pub struct OverridenChip<'a> {
    chip: Box<dyn tango_dataview::rom::Chip + 'a>,
    id: usize,
    overrides: Option<&'a Vec<ChipOverride>>,
}

impl<'a> tango_dataview::rom::Chip for OverridenChip<'a> {
    fn name(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .or_else(|| self.chip.name())
    }
    fn description(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.description.clone()))
            .or_else(|| self.chip.description())
    }
    fn icon(&self) -> image::RgbaImage {
        self.chip.icon()
    }
    fn image(&self) -> image::RgbaImage {
        self.chip.image()
    }
    fn codes(&self) -> Vec<char> {
        self.chip.codes()
    }
    fn element(&self) -> usize {
        self.chip.element()
    }
    fn class(&self) -> tango_dataview::rom::ChipClass {
        self.chip.class()
    }
    fn dark(&self) -> bool {
        self.chip.dark()
    }
    fn mb(&self) -> u8 {
        self.chip.mb()
    }
    fn attack_power(&self) -> u32 {
        self.chip.attack_power()
    }
    fn library_sort_order(&self) -> Option<usize> {
        self.chip.library_sort_order()
    }
}

pub struct OverridenNavicustPart<'a> {
    navicust_part: Box<dyn tango_dataview::rom::NavicustPart + 'a>,
    id: usize,
    overrides: Option<&'a Vec<NavicustPartOverride>>,
}

impl<'a> tango_dataview::rom::NavicustPart for OverridenNavicustPart<'a> {
    fn name(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .or_else(|| self.navicust_part.name())
    }
    fn description(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.description.clone()))
            .or_else(|| self.navicust_part.description())
    }
    fn color(&self) -> Option<tango_dataview::rom::NavicustPartColor> {
        self.navicust_part.color()
    }
    fn is_solid(&self) -> bool {
        self.navicust_part.is_solid()
    }
    fn compressed_bitmap(&self) -> Option<tango_dataview::rom::NavicustBitmap> {
        self.navicust_part.compressed_bitmap()
    }
    fn uncompressed_bitmap(&self) -> tango_dataview::rom::NavicustBitmap {
        self.navicust_part.uncompressed_bitmap()
    }
}

pub struct OverridenPatchCard56<'a> {
    patch_card56: Box<dyn tango_dataview::rom::PatchCard56 + 'a>,
    id: usize,
    overrides: Option<&'a Vec<PatchCard56Override>>,
    effect_overrides: Option<&'a Vec<tango_patch::overrides::PatchCard56EffectOverride>>,
}

impl<'a> tango_dataview::rom::PatchCard56 for OverridenPatchCard56<'a> {
    fn name(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .or_else(|| self.patch_card56.name())
    }
    fn mb(&self) -> u8 {
        self.patch_card56.mb()
    }
    fn effects(&self) -> Vec<tango_dataview::rom::PatchCard56Effect> {
        self.patch_card56
            .effects()
            .into_iter()
            .map(|e| tango_dataview::rom::PatchCard56Effect {
                name: self
                    .effect_overrides
                    .and_then(|v| v.get(e.id).and_then(|v| v.name_template.as_ref()))
                    .map(|parts| render_effect_name(parts, &e))
                    .or_else(|| e.name.clone()),
                ..e
            })
            .collect()
    }
}

pub struct OverridenStyle<'a> {
    style: Box<dyn tango_dataview::rom::Style + 'a>,
    id: usize,
    overrides: Option<&'a Vec<StyleOverride>>,
}

impl tango_dataview::rom::Style for OverridenStyle<'_> {
    fn name(&self) -> Option<String> {
        self.overrides
            .and_then(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .or_else(|| self.style.name())
    }
    fn typ(&self) -> tango_dataview::rom::StyleType {
        self.style.typ()
    }
    fn element(&self) -> usize {
        self.style.element()
    }
    fn extra_ncp_color(&self) -> Option<tango_dataview::rom::NavicustPartColor> {
        self.style.extra_ncp_color()
    }
}

impl tango_dataview::rom::Assets for OverridenAssets {
    fn chip<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::Chip + 'a>> {
        self.assets.chip(id).map(|chip| {
            Box::new(OverridenChip {
                chip,
                id,
                overrides: self.overrides.chips.as_ref(),
            }) as Box<dyn tango_dataview::rom::Chip + 'a>
        })
    }
    fn num_chips(&self) -> usize {
        self.assets.num_chips()
    }
    fn element_icon(&self, id: usize) -> Option<image::RgbaImage> {
        self.assets.element_icon(id)
    }
    fn patch_card56<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::PatchCard56 + 'a>> {
        self.assets.patch_card56(id).map(|patch_card56| {
            Box::new(OverridenPatchCard56 {
                patch_card56,
                id,
                overrides: self.overrides.patch_card56s.as_ref(),
                effect_overrides: self.overrides.patch_card56_effects.as_ref(),
            }) as Box<dyn tango_dataview::rom::PatchCard56 + 'a>
        })
    }
    fn num_patch_card56s(&self) -> usize {
        self.assets.num_patch_card56s()
    }
    fn patch_card4<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::PatchCard4 + 'a>> {
        self.assets.patch_card4(id)
    }
    fn num_patch_card4s(&self) -> usize {
        self.assets.num_patch_card4s()
    }
    fn navicust_part<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::NavicustPart + 'a>> {
        self.assets.navicust_part(id).map(|navicust_part| {
            Box::new(OverridenNavicustPart {
                navicust_part,
                id,
                overrides: self.overrides.navicust_parts.as_ref(),
            }) as Box<dyn tango_dataview::rom::NavicustPart + 'a>
        })
    }
    fn num_navicust_parts(&self) -> usize {
        self.assets.num_navicust_parts()
    }
    fn style<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::Style + 'a>> {
        self.assets.style(id).map(|style| {
            Box::new(OverridenStyle {
                style,
                id,
                overrides: self.overrides.styles.as_ref(),
            }) as Box<dyn tango_dataview::rom::Style + 'a>
        })
    }
    fn num_styles(&self) -> usize {
        self.assets.num_styles()
    }
    fn navi<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::Navi + 'a>> {
        self.assets.navi(id)
    }
    fn num_navis(&self) -> usize {
        self.assets.num_navis()
    }
    fn navi_order(&self) -> &[&[usize]] {
        self.assets.navi_order()
    }
    fn navicust_layout(&self) -> Option<tango_dataview::rom::NavicustLayout> {
        self.assets.navicust_layout()
    }
    fn ex_code(&self, code: u8) -> Option<tango_dataview::rom::ExCode> {
        self.assets.ex_code(code)
    }
    fn chips_have_mb(&self) -> bool {
        self.assets.chips_have_mb()
    }
}
