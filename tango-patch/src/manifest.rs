//! `manifest.toml` — the one metadata file inside a `.tangopatch`.
//!
//! A package is exactly *one version* of one patch, so the manifest is
//! flat: there is no `[versions.'x.y.z']` nesting and nothing in it is
//! repeated per version.
//!
//! ```toml
//! format = 1
//! name = "bn6_allstars"
//! version = "1.1.0"
//! title = "BN6 All-Stars + BBN6"
//! authors = ["Someone <someone@example.com>"]
//! license = "MIT"
//! source = "https://github.com/luckytyphlosion/bn6-all-stars"
//! netplay = "group:bn6allstars"
//!
//! [rom_overrides]
//! language = "en-US"
//! charset = [" ", "0", "1", ...]
//! ```
//!
//! Note what is *absent*: the manifest never names a game, ROM code, or
//! family. Which games a package patches is read off the archive's `roms/`
//! entries, and the netplay family comes from the game being played. That
//! is what makes [`Compatibility::Vanilla`] and
//! [`Compatibility::Group`] impossible to state ambiguously — see [`crate::tag`].

use crate::overrides::Overrides;
use crate::Error;
use serde::{Deserialize, Serialize};

/// The only manifest format version this crate reads or writes.
pub const FORMAT: u32 = 1;

/// Longest accepted patch name / netplay group.
pub const MAX_NAME_LEN: usize = 64;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
// A misspelled or stale key (`netplay_compatibility`, say) is an author
// mistake that would otherwise be silently ignored and leave the patch
// matching nothing.
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Manifest format version. Bumped only for breaking schema changes;
    /// [`Manifest::parse`] rejects anything it doesn't understand rather
    /// than silently ignoring fields.
    pub format: u32,
    /// Stable identifier, unique within a repo. Also the on-disk and URL
    /// stem — see [`crate::validate_name`] for the accepted charset.
    pub name: String,
    pub version: semver::Version,
    /// Human-readable name shown in the UI.
    pub title: String,
    /// `Display Name <email@address>` strings, as in `Cargo.toml`. Kept
    /// raw here; consumers that want just the display name parse them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<String>,
    /// SPDX identifier. `None` means UNLICENSED.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    /// URL to the patch's source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Who this version can netplay against. Defaults to
    /// [`Compatibility::Isolated`] — the conservative choice, since a
    /// gameplay-affecting patch that silently claimed broader
    /// compatibility would desync rather than fail to match.
    #[serde(default, skip_serializing_if = "Compatibility::is_default")]
    pub netplay: Compatibility,
    /// Asset name/description overrides applied on top of the patched
    /// ROM's own data.
    #[serde(default, skip_serializing_if = "Overrides::is_empty")]
    pub rom_overrides: Overrides,
}

impl Manifest {
    /// Parse and validate a manifest. Rejects unknown format versions,
    /// bad names, and bad group names, so a package that opens is a
    /// package whose metadata is usable.
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let manifest: Manifest = toml::from_str(raw)?;
        if manifest.format != FORMAT {
            return Err(Error::UnsupportedFormat(manifest.format));
        }
        crate::validate_name(&manifest.name).map_err(|e| Error::Invalid(format!("name: {e}")))?;
        if manifest.title.trim().is_empty() {
            return Err(Error::Invalid("title: must not be empty".into()));
        }
        if let Compatibility::Group(group) = &manifest.netplay {
            crate::validate_name(group).map_err(|e| Error::Invalid(format!("netplay group: {e}")))?;
        }
        Ok(manifest)
    }

    pub fn to_toml(&self) -> Result<String, Error> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// `<name>-<version>` — the package's file stem and index key.
    pub fn stem(&self) -> String {
        format!("{}-{}", self.name, self.version)
    }

    /// `<name>-<version>.tangopatch`.
    pub fn file_name(&self) -> String {
        format!("{}.{}", self.stem(), crate::EXTENSION)
    }
}

/// Who a patch version may netplay against.
///
/// Serialized as a single string in both TOML and JSON so there is
/// exactly one way to write each case:
///
/// | value | meaning |
/// |---|---|
/// | `"isolated"` | only the identical patch at the identical version (default) |
/// | `"vanilla"`  | the unpatched game, and any other `vanilla` patch for it |
/// | `"group:NAME"` | anything else declaring the same group |
///
/// The old `netplay_compatibility` string could express all three, but
/// only by convention — vanilla meant "type out the ROM family name",
/// which then collided with any group that happened to be named after a
/// family. Here the cases are distinct variants, and the family is never
/// author-supplied at all.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Compatibility {
    /// This exact `(name, version)` and nothing else.
    #[default]
    Isolated,
    /// Cosmetic-only: interchangeable with the unpatched game.
    Vanilla,
    /// A named group, opted into deliberately. Shared across versions of
    /// one patch (so `1.0.1` can play `1.0.0`) or across patches that
    /// deliberately stay in lockstep.
    Group(String),
}

impl Compatibility {
    const ISOLATED: &'static str = "isolated";
    const VANILLA: &'static str = "vanilla";
    const GROUP_PREFIX: &'static str = "group:";

    fn is_default(&self) -> bool {
        *self == Compatibility::Isolated
    }

    pub fn as_str(&self) -> std::borrow::Cow<'static, str> {
        match self {
            Compatibility::Isolated => Self::ISOLATED.into(),
            Compatibility::Vanilla => Self::VANILLA.into(),
            Compatibility::Group(g) => format!("{}{g}", Self::GROUP_PREFIX).into(),
        }
    }
}

impl std::fmt::Display for Compatibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.as_str())
    }
}

impl std::str::FromStr for Compatibility {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        if let Some(group) = s.strip_prefix(Self::GROUP_PREFIX) {
            crate::validate_name(group).map_err(|e| Error::Invalid(format!("netplay group: {e}")))?;
            return Ok(Compatibility::Group(group.to_owned()));
        }
        match s {
            Self::ISOLATED => Ok(Compatibility::Isolated),
            Self::VANILLA => Ok(Compatibility::Vanilla),
            other => Err(Error::Invalid(format!(
                "netplay: expected \"isolated\", \"vanilla\", or \"group:NAME\", got {other:?}"
            ))),
        }
    }
}

impl Serialize for Compatibility {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_str())
    }
}

impl<'de> Deserialize<'de> for Compatibility {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        raw.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
format = 1
name = "bn6_allstars"
version = "1.1.0"
title = "BN6 All-Stars"
"#;

    #[test]
    fn minimal_manifest_defaults_to_isolated() {
        let m = Manifest::parse(MINIMAL).unwrap();
        assert_eq!(m.netplay, Compatibility::Isolated);
        assert!(m.authors.is_empty());
        assert_eq!(m.stem(), "bn6_allstars-1.1.0");
        assert_eq!(m.file_name(), "bn6_allstars-1.1.0.tangopatch");
    }

    #[test]
    fn compatibility_round_trips_through_strings() {
        for c in [
            Compatibility::Isolated,
            Compatibility::Vanilla,
            Compatibility::Group("bn6allstars".into()),
        ] {
            assert_eq!(c.as_str().parse::<Compatibility>().unwrap(), c);
        }
    }

    #[test]
    fn bad_netplay_values_are_rejected() {
        for bad in ["", "group:", "Vanilla", "group:has spaces", "bn6"] {
            assert!(
                bad.parse::<Compatibility>().is_err(),
                "{bad:?} should not parse as a compatibility"
            );
        }
    }

    #[test]
    fn manifest_round_trips_through_toml() {
        let mut m = Manifest::parse(MINIMAL).unwrap();
        m.netplay = Compatibility::Group("bn6allstars".into());
        m.authors = vec!["Someone <someone@example.com>".into()];
        m.license = Some("MIT".into());
        assert_eq!(Manifest::parse(&m.to_toml().unwrap()).unwrap(), m);
    }

    #[test]
    fn unknown_format_is_rejected() {
        let raw = MINIMAL.replace("format = 1", "format = 2");
        assert!(matches!(Manifest::parse(&raw), Err(Error::UnsupportedFormat(2))));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let raw = format!("{MINIMAL}netplay_compatibility = \"bn6\"\n");
        assert!(Manifest::parse(&raw).is_err());
    }
}
