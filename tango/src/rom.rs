use serde::Deserialize;

use crate::{game, scanner};

pub type Scanner = scanner::Scanner<std::collections::HashMap<&'static (dyn game::Game + Send + Sync), Vec<u8>>>;

fn deserialize_option_language_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<unic_langid::LanguageIdentifier>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.map_or(Ok(None), |buf| {
        buf.parse().map(|v| Some(v)).map_err(serde::de::Error::custom)
    })?)
}

fn deserialize_option_patch_card56_effect_template<'de, D>(
    deserializer: D,
) -> Result<Option<tango_dataview::rom::PatchCard56EffectTemplate>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(serde::Deserialize)]
    struct ShortTemplatePart {
        t: Option<String>,
        p: Option<u32>,
    }

    Ok(Option::<Vec<ShortTemplatePart>>::deserialize(deserializer)?.map(|v| {
        v.into_iter()
            .flat_map(|v| {
                if let Some(t) = v.t {
                    vec![tango_dataview::rom::PatchCard56EffectTemplatePart::String(t)]
                } else if let Some(p) = v.p {
                    vec![tango_dataview::rom::PatchCard56EffectTemplatePart::PrintVar(p as usize)]
                } else {
                    vec![]
                }
            })
            .collect()
    }))
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct ChipOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct NavicustPartOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct PatchCard56Override {
    pub name: Option<String>,
}

#[derive(serde::Deserialize, Debug, Clone)]
pub struct PatchCard56EffectOverride {
    #[serde(deserialize_with = "deserialize_option_patch_card56_effect_template")]
    pub name_template: Option<tango_dataview::rom::PatchCard56EffectTemplate>,
}

#[derive(serde::Deserialize, Default, Debug, Clone)]
#[serde(default)]
pub struct Overrides {
    #[serde(deserialize_with = "deserialize_option_language_identifier")]
    pub language: Option<unic_langid::LanguageIdentifier>,
    pub charset: Option<Vec<String>>,
    pub chips: Option<Vec<ChipOverride>>,
    pub navicust_parts: Option<Vec<NavicustPartOverride>>,
    pub patch_card56s: Option<Vec<PatchCard56Override>>,
    pub patch_card56_effects: Option<Vec<PatchCard56EffectOverride>>,
}

pub struct OverridenAssets<A> {
    assets: A,
    overrides: Overrides,
}

impl<A> OverridenAssets<A> {
    pub fn new(assets: A, overrides: &Overrides) -> Self {
        Self {
            assets,
            overrides: overrides.clone(),
        }
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
            .map(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .flatten()
            .map_or_else(|| self.chip.name(), Some)
    }

    fn description(&self) -> Option<String> {
        self.overrides
            .map(|v| v.get(self.id).and_then(|v| v.description.clone()))
            .flatten()
            .map_or_else(|| self.chip.description(), Some)
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
            .map(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .flatten()
            .map_or_else(|| self.navicust_part.name(), Some)
    }

    fn description(&self) -> Option<String> {
        self.overrides
            .map(|v| v.get(self.id).and_then(|v| v.description.clone()))
            .flatten()
            .map_or_else(|| self.navicust_part.description(), Some)
    }

    fn color(&self) -> Option<tango_dataview::rom::NavicustPartColor> {
        self.navicust_part.color()
    }

    fn is_solid(&self) -> bool {
        self.navicust_part.is_solid()
    }

    fn compressed_bitmap(&self) -> tango_dataview::rom::NavicustBitmap {
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
    effect_overrides: Option<&'a Vec<PatchCard56EffectOverride>>,
}

impl<'a> tango_dataview::rom::PatchCard56 for OverridenPatchCard56<'a> {
    fn name(&self) -> Option<String> {
        self.overrides
            .map(|v| v.get(self.id).and_then(|v| v.name.clone()))
            .flatten()
            .map_or_else(|| self.patch_card56.name(), Some)
    }

    fn mb(&self) -> u8 {
        self.patch_card56.mb()
    }

    fn effects(&self) -> Vec<tango_dataview::rom::PatchCard56Effect> {
        self.patch_card56
            .effects()
            .into_iter()
            .map(|e| tango_dataview::rom::PatchCard56Effect {
                id: e.id,
                name: self
                    .effect_overrides
                    .map(|v| v.get(e.id).and_then(|v| v.name_template.as_ref()))
                    .flatten()
                    .map(|parts| {
                        parts
                            .iter()
                            .map(|p| match p {
                                tango_dataview::rom::PatchCard56EffectTemplatePart::String(s) => s.clone(),
                                tango_dataview::rom::PatchCard56EffectTemplatePart::PrintVar(v) => {
                                    if *v == 1 {
                                        let mut parameter = e.parameter as u32;
                                        if e.id == 0x00 || e.id == 0x02 {
                                            parameter = parameter * 10;
                                        }
                                        format!("{}", parameter)
                                    } else {
                                        "".to_string()
                                    }
                                }
                            })
                            .collect::<String>()
                    })
                    .map_or_else(|| e.name, Some),
                parameter: e.parameter,
                is_ability: e.is_ability,
                is_debuff: e.is_debuff,
            })
            .collect()
    }
}

impl<A> tango_dataview::rom::Assets for OverridenAssets<A>
where
    A: tango_dataview::rom::Assets,
{
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

    fn navicust_part<'a>(
        &'a self,
        id: usize,
        variant: usize,
    ) -> Option<Box<dyn tango_dataview::rom::NavicustPart + 'a>> {
        self.assets.navicust_part(id, variant).map(|navicust_part| {
            Box::new(OverridenNavicustPart {
                navicust_part,
                id,
                overrides: self.overrides.navicust_parts.as_ref(),
            }) as Box<dyn tango_dataview::rom::NavicustPart + 'a>
        })
    }

    fn num_navicust_parts(&self) -> (usize, usize) {
        self.assets.num_navicust_parts()
    }

    fn style<'a>(&'a self, id: usize) -> Option<Box<dyn tango_dataview::rom::Style + 'a>> {
        self.assets.style(id)
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

    fn navicust_layout(&self) -> Option<tango_dataview::rom::NavicustLayout> {
        self.assets.navicust_layout()
    }

    fn can_set_regular_chip(&self) -> bool {
        self.assets.can_set_regular_chip()
    }

    fn can_set_tag_chips(&self) -> bool {
        self.assets.can_set_tag_chips()
    }

    fn regular_chip_is_in_place(&self) -> bool {
        self.assets.regular_chip_is_in_place()
    }

    fn chips_have_mb(&self) -> bool {
        self.assets.chips_have_mb()
    }
}
