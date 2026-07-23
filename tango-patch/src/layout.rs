//! What lives where inside a `.tangopatch`, and the [`RomTarget`] type
//! that names it.
//!
//! ```text
//! manifest.toml               required
//! README.md                   optional
//! roms/BR6E_00.bps            at least one required
//! saves/BR6E_00.sav           optional — the default save template
//! saves/BR6E_00.gregar.sav    optional — a named save template
//! ```
//!
//! Always compiled, so the reader ([`crate::package`]), the writer
//! ([`crate::bundle`]) and the index ([`crate::index`]) can each be
//! feature-gated off without losing the vocabulary they share. The
//! formatters here are the writer's, and the parsers are their inverse —
//! the two can't drift.

use crate::Error;

pub const MANIFEST_PATH: &str = "manifest.toml";
pub const README_PATH: &str = "README.md";
pub const ROMS_DIR: &str = "roms";
pub const SAVES_DIR: &str = "saves";
pub const BPS_EXT: &str = ".bps";
pub const SAVE_EXT: &str = ".sav";

/// The default (unnamed) save template's name.
pub const DEFAULT_TEMPLATE: &str = "";

/// A specific ROM build a patch applies to: its 4-character game code
/// plus revision, e.g. `BR6E_00`.
///
/// Deliberately game-agnostic — this crate does no ROM identification, so
/// the bundler stays independent of `tango-gamesupport` (a private
/// submodule the public patch repo's CI can't pull).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RomTarget {
    pub code: [u8; 4],
    pub revision: u8,
}

impl RomTarget {
    pub fn new(code: [u8; 4], revision: u8) -> Self {
        Self { code, revision }
    }

    pub fn code_str(&self) -> &str {
        // `FromStr` validates; a non-ASCII code handed to `new` degrades
        // to a placeholder rather than panicking.
        std::str::from_utf8(&self.code).unwrap_or("????")
    }

    /// Where this target's BPS patch lives in the archive.
    pub fn rom_path(&self) -> String {
        format!("{ROMS_DIR}/{self}{BPS_EXT}")
    }

    /// Where one of this target's save templates lives in the archive.
    pub fn save_path(&self, template: &str) -> String {
        if template == DEFAULT_TEMPLATE {
            format!("{SAVES_DIR}/{self}{SAVE_EXT}")
        } else {
            format!("{SAVES_DIR}/{self}.{template}{SAVE_EXT}")
        }
    }
}

impl std::fmt::Display for RomTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}_{:02}", self.code_str(), self.revision)
    }
}

impl std::str::FromStr for RomTarget {
    type Err = Error;

    /// Parses `CODE_RR`: four ASCII alphanumerics, an underscore, then a
    /// two-digit revision.
    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Invalid(format!("bad rom target {s:?}, expected e.g. \"BR6E_00\""));
        let (code, revision) = s.split_once('_').ok_or_else(bad)?;
        if code.len() != 4 || !code.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(bad());
        }
        if revision.len() != 2 || !revision.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        Ok(RomTarget {
            code: code.as_bytes().try_into().map_err(|_| bad())?,
            revision: revision.parse().map_err(|_| bad())?,
        })
    }
}

impl serde::Serialize for RomTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for RomTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)?
            .parse()
            .map_err(serde::de::Error::custom)
    }
}

/// `dir/STEM.ext` → `STEM`, rejecting anything nested deeper.
pub fn strip_dir_ext<'a>(name: &'a str, dir: &str, ext: &str) -> Option<&'a str> {
    let rest = name.strip_prefix(dir)?.strip_prefix('/')?;
    let stem = rest.strip_suffix(ext)?;
    (!stem.is_empty() && !stem.contains('/')).then_some(stem)
}

/// The inverse of [`RomTarget::save_path`]'s stem: `BR6E_00` → (target,
/// default), `BR6E_00.gregar` → (target, `"gregar"`).
pub fn split_template_stem(stem: &str) -> Result<(RomTarget, String), Error> {
    let Some((target, name)) = stem.split_once('.') else {
        return Ok((stem.parse()?, DEFAULT_TEMPLATE.to_owned()));
    };
    crate::validate_name(name).map_err(|e| Error::Invalid(format!("save template {stem:?}: {e}")))?;
    Ok((target.parse()?, name.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(s: &str) -> RomTarget {
        s.parse().unwrap()
    }

    #[test]
    fn rom_targets_round_trip() {
        let t = target("BR6E_00");
        assert_eq!(t.code_str(), "BR6E");
        assert_eq!(t.revision, 0);
        assert_eq!(t.to_string(), "BR6E_00");
        assert_eq!(target("B4BE_01").to_string(), "B4BE_01");
    }

    #[test]
    fn malformed_rom_targets_are_rejected() {
        for bad in ["BR6E", "BR6E_0", "BR6E_000", "BR6_00", "BR6EE_00", "BR6E_XX", "_00"] {
            assert!(bad.parse::<RomTarget>().is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn paths_parse_back_to_what_built_them() {
        let t = target("BR6E_00");
        assert_eq!(t.rom_path(), "roms/BR6E_00.bps");
        assert_eq!(strip_dir_ext(&t.rom_path(), ROMS_DIR, BPS_EXT), Some("BR6E_00"));

        for template in [DEFAULT_TEMPLATE, "gregar"] {
            let path = t.save_path(template);
            let stem = strip_dir_ext(&path, SAVES_DIR, SAVE_EXT).unwrap();
            assert_eq!(split_template_stem(stem).unwrap(), (t, template.to_owned()));
        }
    }

    #[test]
    fn nested_paths_are_not_payloads() {
        assert_eq!(strip_dir_ext("roms/sub/BR6E_00.bps", ROMS_DIR, BPS_EXT), None);
        assert_eq!(strip_dir_ext("otherroms/BR6E_00.bps", ROMS_DIR, BPS_EXT), None);
        assert_eq!(strip_dir_ext("roms/.bps", ROMS_DIR, BPS_EXT), None);
    }

    #[test]
    fn template_stems_split_on_the_first_dot() {
        assert!(split_template_stem("BR6E_00.a.b").is_err());
        assert!(split_template_stem("BR6E_00.two words").is_err());
    }
}
