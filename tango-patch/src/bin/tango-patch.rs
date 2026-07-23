//! The patch bundler: build `.tangopatch` packages and the repo index.
//!
//! Patch authors run `pack` on a source directory and commit the
//! resulting package; the repo's CI runs `index` over the committed
//! packages at deploy time.

use anyhow::Context as _;
use clap::Parser;
use std::path::{Path, PathBuf};
use tango_patch::{bundle, Index, Manifest, Package};

#[derive(Parser)]
#[command(name = "tango-patch", about = "Build and index Tango patch packages")]
enum Command {
    /// Build a .tangopatch from a source directory.
    Pack {
        /// Directories laid out like a package: manifest.toml, optional
        /// README.md, roms/CODE_RR.bps, optional saves/CODE_RR[.name].sav
        #[arg(required = true)]
        src: Vec<PathBuf>,
        /// Where to write. A package always names itself
        /// `<name>-<version>.tangopatch`.
        #[arg(short, long, default_value = ".")]
        out: PathBuf,
    },
    /// Check that packages (or source directories) are well-formed.
    Validate {
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// Print what a package contains.
    Info { package: PathBuf },
    /// Build index.json for a directory tree of packages.
    Index {
        root: PathBuf,
        /// Defaults to <root>/index.json.
        #[arg(short, long)]
        out: Option<PathBuf>,
        /// Also extract each package's README beside it, so the app can
        /// show it before downloading the package itself.
        #[arg(long)]
        readmes: bool,
    },
    /// Convert an old-style patch repo (per-patch info.toml + v*/
    /// directories) into packages. Transitional — delete once the repo
    /// has been converted.
    Migrate {
        /// The old repo's root.
        src: PathBuf,
        /// Where the `<name>/<name>-<version>.tangopatch` tree goes.
        #[arg(short, long)]
        out: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    match Command::parse() {
        Command::Pack { src, out } => {
            for src in &src {
                let built = bundle::read_dir(src)
                    .and_then(|b| b.write_file(&out))
                    .with_context(|| format!("packing {}", src.display()))?;
                println!("{} ({})", built.path.display(), human_size(built.size));
            }
        }

        Command::Validate { paths } => {
            let mut failed = 0;
            for path in &paths {
                match validate(path) {
                    Ok(summary) => println!("ok   {}: {summary}", path.display()),
                    Err(e) => {
                        failed += 1;
                        println!("FAIL {}: {e:#}", path.display());
                    }
                }
            }
            if failed > 0 {
                anyhow::bail!("{failed} of {} failed validation", paths.len());
            }
        }

        Command::Info { package } => {
            let mut package = Package::open(&package)?;
            let manifest = package.manifest().clone();
            println!("{}", manifest.to_toml()?.trim_end());
            println!();
            for target in package.targets().collect::<Vec<_>>() {
                let templates: Vec<String> = package
                    .save_templates(target)
                    .map(|t| if t.is_empty() { "(default)".into() } else { t.to_owned() })
                    .collect();
                let saves = if templates.is_empty() {
                    String::new()
                } else {
                    format!("  saves: {}", templates.join(", "))
                };
                println!("{target}{saves}");
            }
            if let Some(readme) = package.readme()? {
                println!("\nREADME ({} bytes)", readme.len());
            }
        }

        Command::Index { root, out, readmes } => {
            let index = Index::build(&root, readmes)?;
            let out = out.unwrap_or_else(|| root.join(tango_patch::index::FILE_NAME));
            std::fs::write(&out, index.to_json()?).with_context(|| format!("writing {}", out.display()))?;
            println!(
                "{}: {} versions of {} patches",
                out.display(),
                index.len(),
                index.patches.len()
            );
        }

        Command::Migrate { src, out } => migrate::run(&src, &out)?,
    }
    Ok(())
}

/// Accepts either a built package or a source directory, so the same
/// command works before and after packing.
fn validate(path: &Path) -> anyhow::Result<String> {
    let (manifest, targets) = if path.is_dir() {
        let builder = bundle::read_dir(path)?;
        (builder.manifest().clone(), builder.targets().collect::<Vec<_>>())
    } else {
        let package = Package::open(path)?;
        (package.manifest().clone(), package.targets().collect::<Vec<_>>())
    };
    Ok(format!(
        "{} netplay={} [{}]",
        manifest.stem(),
        manifest.netplay,
        targets.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(" ")
    ))
}

fn human_size(bytes: u64) -> String {
    match bytes {
        0..=1023 => format!("{bytes} B"),
        1024..=1048575 => format!("{:.1} KiB", bytes as f64 / 1024.0),
        _ => format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0)),
    }
}

/// Conversion from the pre-`.tangopatch` repo layout.
///
/// Transitional: the old format kept every version's metadata in one
/// `info.toml` per patch, and expressed netplay compatibility as a single
/// free-form string. Both go away here.
mod migrate {
    use super::*;
    use std::collections::HashMap;
    use tango_patch::Compatibility;

    /// Tango's ROM families. An old `netplay_compatibility` equal to one
    /// of these was the convention for "plays with the unpatched game",
    /// which is now [`Compatibility::Vanilla`].
    const FAMILIES: &[&str] = &[
        "bn1", "bn2", "bn3", "bn4", "bn5", "bn6", "exe1", "exe2", "exe3", "exe4", "exe45", "exe5", "exe6",
    ];

    #[derive(serde::Deserialize)]
    struct LegacyInfo {
        patch: LegacyPatch,
        versions: HashMap<String, LegacyVersion>,
    }

    #[derive(serde::Deserialize)]
    struct LegacyPatch {
        title: String,
        #[serde(default)]
        authors: Vec<String>,
        license: Option<String>,
        source: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct LegacyVersion {
        netplay_compatibility: String,
        #[serde(default)]
        rom_overrides: tango_patch::Overrides,
    }

    pub fn run(src: &Path, out: &Path) -> anyhow::Result<()> {
        let mut patches: Vec<(String, LegacyInfo)> = Vec::new();
        for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !entry.path().is_dir() {
                continue;
            }
            let info_path = entry.path().join("info.toml");
            let raw =
                std::fs::read_to_string(&info_path).with_context(|| format!("reading {}", info_path.display()))?;
            let info: LegacyInfo = toml::from_str(&raw).with_context(|| format!("parsing {}", info_path.display()))?;
            patches.push((name, info));
        }
        patches.sort_by(|(a, _), (b, _)| a.cmp(b));

        // A tag used by exactly one version means the same thing as
        // "isolated", which is what most of these were faking with a
        // hand-written version suffix.
        let mut tag_uses: HashMap<&str, usize> = HashMap::new();
        for (_, info) in &patches {
            for version in info.versions.values() {
                *tag_uses.entry(version.netplay_compatibility.as_str()).or_default() += 1;
            }
        }

        let mut groups: HashMap<String, &str> = HashMap::new();
        let mut packed = 0;
        let mut skipped_files = 0;
        let mut skipped_versions: Vec<String> = Vec::new();
        for (name, info) in &patches {
            tango_patch::validate_name(name).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
            let readme = ["README.md", "README"]
                .iter()
                .map(|f| src.join(name).join(f))
                .find(|p| p.is_file())
                .map(std::fs::read)
                .transpose()?
                .map(|raw| String::from_utf8_lossy(&raw).into_owned());

            let mut versions: Vec<(&String, &LegacyVersion)> = info.versions.iter().collect();
            versions.sort_by(|(a, _), (b, _)| a.cmp(b));
            for (version, legacy) in versions {
                let version: semver::Version = version
                    .parse()
                    .with_context(|| format!("{name}: bad version {version:?}"))?;
                let netplay = compatibility_for(
                    &legacy.netplay_compatibility,
                    tag_uses[legacy.netplay_compatibility.as_str()],
                )
                .with_context(|| format!("{name} {version}"))?;

                // Two different old tags must not collapse onto one
                // group: that would silently make incompatible patches
                // playable against each other.
                if let Compatibility::Group(group) = &netplay {
                    if let Some(other) = groups.insert(group.clone(), &legacy.netplay_compatibility) {
                        anyhow::ensure!(
                            other == legacy.netplay_compatibility,
                            "{name} {version}: tags {other:?} and {:?} both sanitize to group {group:?}",
                            legacy.netplay_compatibility
                        );
                    }
                }

                let manifest = Manifest {
                    format: tango_patch::manifest::FORMAT,
                    name: name.clone(),
                    version: version.clone(),
                    title: info.patch.title.clone(),
                    authors: info.patch.authors.clone(),
                    license: info.patch.license.clone(),
                    source: info.patch.source.clone(),
                    netplay,
                    rom_overrides: legacy.rom_overrides.clone(),
                };

                let mut builder = bundle::Builder::new(manifest);
                if let Some(readme) = &readme {
                    builder.set_readme(readme.clone());
                }
                let version_dir = src.join(name).join(format!("v{version}"));
                let mut entries: Vec<PathBuf> = std::fs::read_dir(&version_dir)
                    .with_context(|| format!("reading {}", version_dir.display()))?
                    .map(|e| e.map(|e| e.path()))
                    .collect::<Result<_, _>>()?;
                entries.sort();
                for path in entries {
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let raw = || std::fs::read(&path).with_context(|| format!("reading {}", path.display()));
                    // A file the old scanner's regex didn't match was
                    // already invisible to the app, so skipping it here
                    // changes nothing — but say so, since the fix is to
                    // rename it upstream and migrate again.
                    if let Some(stem) = file_name.strip_suffix(".bps") {
                        match stem.parse() {
                            Ok(target) => builder.add_rom(target, raw()?),
                            Err(_) => {
                                eprintln!("skipping {}: not a CODE_RR.bps", path.display());
                                skipped_files += 1;
                                continue;
                            }
                        };
                    } else if let Some(stem) = file_name.strip_suffix(".sav") {
                        // Old save templates were `CODE_RR[_name].sav`;
                        // packages separate the template name with a dot.
                        let (target, template) = split_legacy_save(stem);
                        match target.parse() {
                            Ok(target) => builder.add_save_template(target, template, raw()?)?,
                            Err(_) => {
                                eprintln!("skipping {}: not a CODE_RR[_name].sav", path.display());
                                skipped_files += 1;
                                continue;
                            }
                        };
                    } else {
                        eprintln!("skipping {}", path.display());
                        skipped_files += 1;
                    }
                }

                if builder.targets().len() == 0 {
                    eprintln!("skipping {name} {version}: no usable patches");
                    skipped_versions.push(format!("{name} {version}"));
                    continue;
                }
                builder
                    .write_file(&out.join(name))
                    .with_context(|| format!("packing {name} {version}"))?;
                packed += 1;
            }
        }

        println!("{packed} packages from {} patches → {}", patches.len(), out.display());
        if skipped_files > 0 {
            println!("{skipped_files} files skipped (the old scanner ignored them too)");
        }
        if !skipped_versions.is_empty() {
            println!(
                "{} versions dropped: {}",
                skipped_versions.len(),
                skipped_versions.join(", ")
            );
        }
        Ok(())
    }

    /// `BR6E_00` → (`BR6E_00`, default), `BR6E_00_gregar` → (`BR6E_00`,
    /// `gregar`).
    fn split_legacy_save(stem: &str) -> (&str, &str) {
        match stem.split_once('_').and_then(|(_, rest)| rest.split_once('_')) {
            Some((_, template)) => (&stem[..stem.len() - template.len() - 1], template),
            None => (stem, tango_patch::layout::DEFAULT_TEMPLATE),
        }
    }

    /// Map one old `netplay_compatibility` string onto the typed
    /// declaration that means the same thing.
    fn compatibility_for(tag: &str, uses: usize) -> anyhow::Result<Compatibility> {
        if FAMILIES.contains(&tag) {
            return Ok(Compatibility::Vanilla);
        }
        if uses == 1 {
            // Nothing else shares it, so a group would be a group of one.
            return Ok(Compatibility::Isolated);
        }
        let group: String = tag
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
            .collect();
        tango_patch::validate_name(&group).map_err(|e| anyhow::anyhow!("netplay tag {tag:?}: {e}"))?;
        Ok(Compatibility::Group(group))
    }
}
