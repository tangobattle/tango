//! The patch bundler: build `.tangopatch` packages and the repo index.
//!
//! Patch authors run `pack` on a source directory and commit the
//! resulting package; the repo's CI runs `index` over the committed
//! packages at deploy time.

use anyhow::Context as _;
use clap::Parser;
use std::path::{Path, PathBuf};
use tango_patch::{bundle, Index, Package};

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
