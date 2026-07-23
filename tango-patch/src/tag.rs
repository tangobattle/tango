//! Netplay compatibility tags.
//!
//! Two players may play each other when their tags are equal. A tag is
//! resolved locally on each side from `(ROM family, chosen patch)` — it
//! is never sent over the wire, so this is a pure function both peers
//! evaluate over the same inputs.
//!
//! # Why this is a type and not a string
//!
//! The old format had one free-form `netplay_compatibility` string per
//! patch version, and every distinct meaning had to be squeezed into that
//! one namespace:
//!
//! - "only this exact release" → authors hand-suffixed the version
//!   (`bn6allstars_v1.1.0`), and typos silently created islands;
//! - "plays with the unpatched game" → authors typed the ROM family name
//!   (`bn6`), which was pure convention;
//! - "plays with these other patches" → a shared made-up word.
//!
//! Because all three lived in one string space they could collide: a
//! group named `bn6` was indistinguishable from vanilla-on-bn6, and
//! nothing stopped a bn4 patch and a bn6 patch from claiming the same
//! tag and "matching" into a guaranteed desync.
//!
//! [`Tag`] gives each meaning its own variant, and *every* variant is
//! family-scoped from the game rather than from anything an author typed.
//! Both classes of collision become unrepresentable rather than merely
//! discouraged.

use crate::manifest::Compatibility;

/// A resolved compatibility identity. Compare with `==`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Tag {
    /// The unpatched game, or a patch that declares itself cosmetic.
    Vanilla { family: String },
    /// An author-declared group.
    Group { family: String, group: String },
    /// One specific patch release.
    Exact {
        family: String,
        patch: String,
        version: semver::Version,
    },
}

impl Tag {
    /// The tag for playing `family` unpatched.
    pub fn vanilla(family: &str) -> Tag {
        Tag::Vanilla {
            family: family.to_owned(),
        }
    }

    /// The tag for playing `family` with a patch.
    ///
    /// `family` comes from the ROM being played (`bn6`, `exe45`, …), not
    /// from the patch — a package that patches ROMs from two families
    /// resolves to a different tag for each, which is what keeps a bn4
    /// player and a bn6 player using the same package from matching.
    pub fn patched(family: &str, name: &str, version: &semver::Version, compatibility: &Compatibility) -> Tag {
        match compatibility {
            Compatibility::Vanilla => Tag::vanilla(family),
            Compatibility::Group(group) => Tag::Group {
                family: family.to_owned(),
                group: group.clone(),
            },
            Compatibility::Isolated => Tag::Exact {
                family: family.to_owned(),
                patch: name.to_owned(),
                version: version.clone(),
            },
        }
    }

    pub fn family(&self) -> &str {
        match self {
            Tag::Vanilla { family } | Tag::Group { family, .. } | Tag::Exact { family, .. } => family,
        }
    }
}

/// Human-readable and, because names and groups are restricted to
/// `[A-Za-z0-9_-]` (see [`crate::validate_name`]), unambiguous: the `+`
/// and `@` separators can't occur inside a family, group, or patch name.
impl std::fmt::Display for Tag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Tag::Vanilla { family } => write!(f, "{family}"),
            Tag::Group { family, group } => write!(f, "{family}+{group}"),
            Tag::Exact { family, patch, version } => write!(f, "{family}+{patch}@{version}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> semver::Version {
        s.parse().unwrap()
    }

    #[test]
    fn a_group_named_after_a_family_is_not_vanilla() {
        // The exact collision the old string format allowed.
        let vanilla = Tag::vanilla("bn6");
        let group = Tag::patched("bn6", "someone_patch", &v("1.0.0"), &Compatibility::Group("bn6".into()));
        assert_ne!(vanilla, group);
        assert_ne!(vanilla.to_string(), group.to_string());
    }

    #[test]
    fn vanilla_patches_play_the_unpatched_game_and_each_other() {
        let unpatched = Tag::vanilla("bn6");
        let soundmod = Tag::patched("bn6", "bn6_soundmod", &v("1.0.0"), &Compatibility::Vanilla);
        let widescreen = Tag::patched("bn6", "bn6_hud", &v("3.2.1"), &Compatibility::Vanilla);
        assert_eq!(unpatched, soundmod);
        assert_eq!(soundmod, widescreen);
    }

    #[test]
    fn isolated_is_per_version() {
        let a = Tag::patched("bn6", "p", &v("1.0.0"), &Compatibility::Isolated);
        let b = Tag::patched("bn6", "p", &v("1.0.1"), &Compatibility::Isolated);
        assert_ne!(a, b);
        assert_eq!(a, Tag::patched("bn6", "p", &v("1.0.0"), &Compatibility::Isolated));
    }

    #[test]
    fn a_group_spans_versions_but_not_families() {
        let group = Compatibility::Group("allstars".into());
        assert_eq!(
            Tag::patched("bn6", "p", &v("1.0.0"), &group),
            Tag::patched("bn6", "p", &v("2.0.0"), &group)
        );
        // One package can patch ROMs from two families; each side still
        // resolves against its own game.
        assert_ne!(
            Tag::patched("bn4", "p", &v("1.0.0"), &group),
            Tag::patched("bn6", "p", &v("1.0.0"), &group)
        );
    }

    #[test]
    fn different_patches_only_match_by_joining_a_group() {
        let mine = Tag::patched("bn6", "mine", &v("1.0.0"), &Compatibility::Isolated);
        let yours = Tag::patched("bn6", "yours", &v("1.0.0"), &Compatibility::Isolated);
        assert_ne!(mine, yours);
        let group = Compatibility::Group("shared".into());
        assert_eq!(
            Tag::patched("bn6", "mine", &v("1.0.0"), &group),
            Tag::patched("bn6", "yours", &v("9.9.9"), &group)
        );
    }
}
