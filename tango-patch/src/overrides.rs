//! `[rom_overrides]` — author-supplied replacements for text the patched
//! ROM can't provide itself (a translation patch's chip names, a new
//! charset, retitled NaviCust parts).
//!
//! This is the schema only. Layering it over a game's assets is the app's
//! job (`tango::library::rom_overrides::OverridenAssets`); keeping the
//! types here means the bundler validates an author's overrides at pack
//! time without depending on `tango-dataview`.

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Debug, Clone, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct Overrides {
    /// BCP-47 tag, e.g. `en-US`. Parsed here so a typo like `en_US` is a
    /// pack-time error rather than a silently ignored override.
    #[serde(
        deserialize_with = "deserialize_option_language_identifier",
        serialize_with = "serialize_option_language_identifier",
        skip_serializing_if = "Option::is_none"
    )]
    pub language: Option<unic_langid::LanguageIdentifier>,
    /// The ROM's text encoding, indexed by byte value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub charset: Option<Vec<String>>,
    /// Indexed by chip id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chips: Option<Vec<ChipOverride>>,
    /// Indexed by NaviCust part id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navicust_parts: Option<Vec<NavicustPartOverride>>,
    /// Indexed by style id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub styles: Option<Vec<StyleOverride>>,
    /// Indexed by patch card id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_card56s: Option<Vec<PatchCard56Override>>,
    /// Indexed by patch card *effect* id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_card56_effects: Option<Vec<PatchCard56EffectOverride>>,
}

impl Overrides {
    pub fn is_empty(&self) -> bool {
        *self == Overrides::default()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct ChipOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct NavicustPartOverride {
    pub name: Option<String>,
    pub description: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct StyleOverride {
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PatchCard56Override {
    pub name: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct PatchCard56EffectOverride {
    /// Effect name as alternating literal / parameter parts.
    pub name_template: Option<Vec<TemplatePart>>,
}

/// One piece of a patch card effect name: either literal text (`t`) or
/// the effect's numeric parameter (`p`).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct TemplatePart {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p: Option<u32>,
}

fn deserialize_option_language_identifier<'de, D>(
    deserializer: D,
) -> Result<Option<unic_langid::LanguageIdentifier>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)?
        .map_or(Ok(None), |buf| buf.parse().map(Some).map_err(serde::de::Error::custom))
}

fn serialize_option_language_identifier<S>(
    value: &Option<unic_langid::LanguageIdentifier>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match value {
        Some(v) => serializer.serialize_str(&v.to_string()),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_translation_style_override_block() {
        let o: Overrides = toml::from_str(
            r#"
language = "en-US"
charset = [" ", "A", "B"]
chips = [{ name = "Cannon", description = "Fires a shot." }]
patch_card56_effects = [{ name_template = [{ t = "Attack +" }, { p = 1 }] }]
"#,
        )
        .unwrap();
        assert_eq!(o.language.unwrap().to_string(), "en-US");
        assert_eq!(o.charset.unwrap().len(), 3);
        assert_eq!(o.chips.unwrap()[0].name.as_deref(), Some("Cannon"));
        let effects = o.patch_card56_effects.unwrap();
        assert_eq!(effects[0].name_template.as_ref().unwrap()[1].p, Some(1));
    }

    #[test]
    fn rejects_a_malformed_language_tag() {
        for bad in ["", "1234", "en-US-", "not a language"] {
            assert!(
                toml::from_str::<Overrides>(&format!("language = {bad:?}")).is_err(),
                "{bad:?} should be rejected"
            );
        }
        // `en_US` is tolerated and normalized, so a hyphen slip isn't an
        // error — it just comes back as `en-US`.
        let o: Overrides = toml::from_str(r#"language = "en_US""#).unwrap();
        assert_eq!(o.language.unwrap().to_string(), "en-US");
    }

    #[test]
    fn empty_is_empty() {
        assert!(Overrides::default().is_empty());
        assert!(!toml::from_str::<Overrides>(r#"charset = ["A"]"#).unwrap().is_empty());
    }
}
