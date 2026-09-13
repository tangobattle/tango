//! Game packages and their extensions, the successor to `.tangopatch`.
//! All file access goes through the same storage adapter as the rest of the
//! library. Directory packages support development; `.tangopkg` is distributable.
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::storage::{Storage, TreeLimits};
use tango_script::{Package, PackageRef, Profile};

mod catalog;
pub use catalog::{Catalog, Entry, Export, ExportRef, Source};
mod management;
pub use management::{install_many, removal_blockers, uninstall};
pub type Scanner = crate::scanner::Scanner<Catalog>;

fn read(storage: &dyn Storage, path: &Path, limit: usize) -> tango_script::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    storage.open(path)?.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(tango_script::Error::Invalid(format!(
            "package file too large: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

pub async fn load(storage: &dyn Storage, path: &Path) -> tango_script::Result<Package> {
    if path.extension().is_some_and(|ext| ext == "tangopkg") {
        return Package::from_archive(&read(storage, path, 64 * 1024 * 1024)?);
    }
    Package::load(
        storage
            .snapshot_tree(
                path,
                TreeLimits {
                    bytes: tango_script::MAX_PACKAGE_BYTES,
                    entries: tango_script::MAX_PACKAGE_FILES,
                    depth: 32,
                },
            )
            .await?,
    )
}

/// Resolve the last selected package and its exact dependency versions. All
/// dependencies must be among the supplied installed/development packages.
pub async fn load_profile(storage: &dyn Storage, paths: &[PathBuf]) -> tango_script::Result<Profile> {
    if paths.is_empty() {
        return Err(tango_script::Error::Invalid("select at least one package".into()));
    }
    let mut packages = Vec::new();
    for path in paths {
        packages.push(load(storage, path).await?);
    }
    let manifest = packages.last().unwrap().manifest();
    let selected = PackageRef {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
    };
    Profile::resolve(&packages, &selected)
}

/// Package creation and installation share the validated, canonical container.
pub async fn bundle(storage: &dyn Storage, source: &Path, output: &Path) -> tango_script::Result<()> {
    let package = load(storage, source).await?;
    crate::storage::write_atomic(storage, output, &package.to_archive()?)?;
    Ok(())
}

/// Install an immutable package version. Reinstalling the same contents is
/// idempotent; changing contents under an existing version requires a bump.
pub async fn install(
    storage: &dyn Storage,
    source: &Path,
    root: &Path,
    bundled: &[Package],
) -> tango_script::Result<PathBuf> {
    let references = install_many(storage, &[source.to_owned()], root, bundled).await?;
    let reference = &references[0];
    Ok(root
        .join(&reference.name)
        .join(format!("{}.tangopkg", reference.version)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{Entry, ListFuture, Listing, ReadSeek};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryStorage(Mutex<BTreeMap<PathBuf, Vec<u8>>>, Mutex<Option<PathBuf>>);

    impl Storage for MemoryStorage {
        fn snapshot_tree<'a>(&'a self, root: &'a Path, limits: TreeLimits) -> crate::storage::TreeFuture<'a> {
            Box::pin(async move {
                crate::storage::snapshot_files(
                    root,
                    self.0.lock().unwrap().iter().map(|(p, b)| (p.as_path(), b.as_slice())),
                    limits,
                )
            })
        }

        fn read(&self, path: &Path) -> std::io::Result<Vec<u8>> {
            self.0
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or(std::io::ErrorKind::NotFound.into())
        }
        fn open(&self, path: &Path) -> std::io::Result<Box<dyn ReadSeek>> {
            Ok(Box::new(std::io::Cursor::new(self.read(path)?)))
        }
        fn write(&self, path: &Path, data: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().insert(path.to_owned(), data.to_vec());
            Ok(())
        }
        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .remove(path)
                .ok_or(std::io::ErrorKind::NotFound)?;
            Ok(())
        }
        fn create_dir_all(&self, _: &Path) -> std::io::Result<()> {
            Ok(())
        }
        fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
            if self.1.lock().unwrap().as_deref() == Some(to) {
                return Err(std::io::Error::other("injected rename failure"));
            }
            let mut files = self.0.lock().unwrap();
            let bytes = files.remove(from).ok_or(std::io::ErrorKind::NotFound)?;
            files.insert(to.to_owned(), bytes);
            Ok(())
        }
        fn is_file(&self, path: &Path) -> bool {
            self.0.lock().unwrap().contains_key(path)
        }
        fn list<'a>(&'a self, roots: &'a [PathBuf]) -> ListFuture<'a> {
            Box::pin(async move {
                Listing::new(
                    self.0
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(path, _)| roots.iter().any(|root| path.starts_with(root)))
                        .map(|(path, bytes)| Entry {
                            path: path.clone(),
                            len: bytes.len() as u64,
                            modified: None,
                        })
                        .collect(),
                )
            })
        }
    }

    fn library(version: &str, value: u8) -> Package {
        Package::load(
            [
                (
                    "package.toml".into(),
                    format!("api=1\nname='common'\nversion='{version}'\n").into_bytes(),
                ),
                (
                    "init.luau".into(),
                    format!("--!strict\nreturn {{value = {value}}}").into_bytes(),
                ),
            ]
            .into(),
        )
        .unwrap()
    }

    fn editor_files(name: &str, dependency: Option<&str>) -> BTreeMap<String, Vec<u8>> {
        let dependencies = dependency
            .map(|v| format!("\n[dependencies]\ncommon='{v}'\n"))
            .unwrap_or_default();
        let value = if dependency.is_some() {
            "require('@common').value"
        } else {
            "0"
        };
        [
            (
                "package.toml".into(),
                format!(
                    "api=1\nname='{name}'\nversion='1.0.0'\n[[editor]]\nname='main'\npath='./init'\n{dependencies}"
                )
                .into_bytes(),
            ),
            (
                "init.luau".into(),
                format!(
                    r#"--!strict
local value = {value}
local editor: Editor = {{
    decode = function(b: buffer): buffer return b end,
    encode = function(b: buffer): buffer return b end,
    validate = function(_b: buffer): {{string}} return {{}} end,
    view = function(_b: buffer, _state: ViewState): Node return {{kind = "text", text = tostring(value)}} end,
    update = function(_b: buffer, _state: ViewState, _action: Action) return nil end,
}}
return editor
"#
                )
                .into_bytes(),
            ),
        ]
        .into()
    }

    async fn catalog(storage: &MemoryStorage, bundled: &[Package]) -> Catalog {
        let root = Path::new("packages");
        let listing = storage.list(&[root.to_owned()]).await;
        Catalog::scan(storage, root, &listing, bundled).await
    }

    #[test]
    fn catalog_resolves_installed_dependencies_and_retains_immutable_profiles() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let common = library("1.0.0", 7);
            let package = Package::load(editor_files("extension", Some("1.0.0"))).unwrap();
            storage
                .write(
                    Path::new("packages/extension/1.0.0.tangopkg"),
                    &package.to_archive().unwrap(),
                )
                .unwrap();
            let first = catalog(&storage, &[common.clone()]).await;
            assert!(first.issues().is_empty(), "{:?}", first.issues());
            assert_eq!(first.exports().len(), 1);
            let reference = first.exports()[0].reference.clone();
            let profile = first.export(&reference).unwrap().profile.clone();
            // The archive scan supplies dependencies without caller-specified paths.
            storage
                .write(
                    Path::new("packages/common/1.0.0.tangopkg"),
                    &common.to_archive().unwrap(),
                )
                .unwrap();
            let second = catalog(&storage, &[]).await;
            assert!(second.issues().is_empty());
            assert_eq!(second.export(&reference).unwrap().profile.digest(), profile.digest());
            storage
                .remove_file(Path::new("packages/common/1.0.0.tangopkg"))
                .unwrap();
            let third = catalog(&storage, &[]).await;
            assert!(third.exports().is_empty());
            assert!(third
                .issues()
                .iter()
                .any(|issue| issue.contains("missing package common 1.0.0")));
            // Already opened environments still own their original dependency bytes.
            let doc = tango_script::Document::open(profile, &[0]).unwrap();
            assert_eq!(serde_json::to_value(doc.view()).unwrap()["text"], "7");
        });
    }

    #[test]
    fn catalog_quarantines_identity_conflicts_without_losing_unrelated_exports() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let common = library("1.0.0", 7);
            let changed = library("1.0.0", 8);
            for (name, dependency) in [("extension", Some("1.0.0")), ("independent", None)] {
                let package = Package::load(editor_files(name, dependency)).unwrap();
                storage
                    .write(
                        &Path::new("packages").join(name).join("1.0.0.tangopkg"),
                        &package.to_archive().unwrap(),
                    )
                    .unwrap();
            }
            storage
                .write(
                    Path::new("packages/common/1.0.0.tangopkg"),
                    &changed.to_archive().unwrap(),
                )
                .unwrap();
            let result = catalog(&storage, &[common]).await;
            assert_eq!(result.exports().len(), 1);
            assert_eq!(result.exports()[0].reference.package.name, "independent");
            assert!(result
                .issues()
                .iter()
                .any(|issue| issue.contains("conflicting contents")));
            assert!(result
                .issues()
                .iter()
                .any(|issue| issue.contains("missing package common")));
        });
    }

    #[test]
    fn catalog_checks_flat_package_paths_and_reports_bad_sources_independently() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            for (name, contents) in editor_files("directory", None) {
                storage
                    .write(&Path::new("packages/directory").join(name), &contents)
                    .unwrap();
            }
            let other = Package::load(editor_files("other", None))
                .unwrap()
                .to_archive()
                .unwrap();
            storage
                .write(Path::new("packages/wrong-name/1.0.0.tangopkg"), &other)
                .unwrap();
            storage
                .write(Path::new("packages/other/2.0.0.tangopkg"), &other)
                .unwrap();
            storage
                .write(Path::new("packages/broken/1.0.0.tangopkg"), b"not an archive")
                .unwrap();
            storage
                .write(Path::new("packages/nested/other/1.0.0.tangopkg"), &other)
                .unwrap();
            storage
                .write(Path::new("outside/other/1.0.0.tangopkg"), &other)
                .unwrap();
            let mut entries = storage.list(&[PathBuf::from("packages")]).await.entries().to_vec();
            entries.push(Entry {
                path: "outside/other/1.0.0.tangopkg".into(),
                len: other.len() as u64,
                modified: None,
            });
            let result = Catalog::scan(&storage, Path::new("packages"), &Listing::new(entries), &[]).await;
            assert_eq!(result.exports().len(), 1);
            assert_eq!(result.exports()[0].reference.package.name, "directory");
            assert_eq!(result.issues().len(), 3, "{:?}", result.issues());
        });
    }

    #[test]
    fn catalog_keeps_capability_names_and_versions_separate() {
        futures::executor::block_on(async {
            let mut files = editor_files("capabilities", None);
            files
                .get_mut("package.toml")
                .unwrap()
                .splice(0..0, b"default_gamemode='main'\n".iter().copied());
            files.get_mut("package.toml").unwrap().extend_from_slice(
                b"\n[[gamemode]]\nname='main'\npath='./mode'\n[[telemetry]]\nname='main'\npath='./telemetry'\n",
            );
            files.insert(
                "mode.luau".into(),
                b"--!strict\nreturn {patch_rom = function(b: buffer): buffer buffer.writeu8(b, 0, 42); return b end}"
                    .to_vec(),
            );
            files.insert(
                "telemetry.luau".into(),
                b"--!strict\nreturn {poll = function(): TelemetryFrame return {values = {}, events = {}} end}".to_vec(),
            );
            let result = catalog(&MemoryStorage::default(), &[Package::load(files).unwrap()]).await;
            assert!(result.issues().is_empty(), "{:?}", result.issues());
            assert_eq!(result.exports().len(), 3);
            for kind in tango_script::ExportKind::ALL {
                let exports = result.latest_exports(kind);
                assert_eq!(exports.len(), 1);
                assert_eq!(exports[0].reference.kind, kind);
                assert_eq!(exports[0].reference.name, "main");
                assert!(result.export(&exports[0].reference).is_some());
            }
            let mode = result.latest_exports(tango_script::ExportKind::GameMode)[0];
            assert_eq!(mode.profile.patch_rom(&[0]).unwrap(), [42]);
        });
    }

    #[test]
    fn gamemode_resolution_checks_content_pins_and_never_falls_back_to_latest() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let mut files = editor_files("mode", Some("1.0.0"));
            files
                .get_mut("package.toml")
                .unwrap()
                .splice(0..0, b"default_gamemode='main'\n".iter().copied());
            files
                .get_mut("package.toml")
                .unwrap()
                .extend_from_slice(b"\n[[gamemode]]\nname='main'\npath='./mode'\n");
            files.insert("mode.luau".into(), b"--!strict\nlocal common = require('@common')\nreturn {patch_rom = function(b: buffer): buffer buffer.writeu8(b, 0, buffer.readu8(b, 0) + common.value); return b end}".to_vec());
            let first = Package::load(files.clone()).unwrap();
            let manifest = String::from_utf8(files.remove("package.toml").unwrap()).unwrap();
            files.insert(
                "package.toml".into(),
                manifest.replace("version='1.0.0'", "version='2.0.0'").into_bytes(),
            );
            let second = Package::load(files).unwrap();
            let common = library("1.0.0", 7);
            let catalog = catalog(&storage, &[first.clone(), second.clone(), common.clone()]).await;
            assert!(catalog.issues().is_empty(), "{:?}", catalog.issues());
            let reference = ExportRef {
                kind: tango_script::ExportKind::GameMode,
                package: PackageRef {
                    name: "mode".into(),
                    version: "1.0.0".parse().unwrap(),
                },
                name: "main".into(),
            };
            let engine = tango_match::gamemode::identity::Runtime {
                name: "test".into(),
                revision: 1,
            };
            let prepared = tango_script::PreparedGameMode::prepare(
                catalog.export(&reference).unwrap().profile.clone(),
                engine.clone(),
                vec![2],
                false,
                BTreeMap::new(),
            )
            .unwrap();
            let identity = prepared.identity();
            let configuration = prepared.configuration().unwrap();
            let resolved = catalog
                .resolve_gamemode(&configuration, engine.clone(), vec![2])
                .unwrap();
            assert_eq!(resolved.rom(), [9]);
            assert_eq!(resolved.identity(), identity);
            assert_eq!(
                catalog.latest_exports(tango_script::ExportKind::GameMode)[0]
                    .reference
                    .package
                    .version
                    .to_string(),
                "2.0.0"
            );
            for rom in [vec![3], vec![9]] {
                // Also catches accidentally passing the already patched ROM.
                assert!(catalog.resolve_gamemode(&configuration, engine.clone(), rom).is_err());
            }
            let mut changed_options = configuration.clone();
            changed_options
                .options
                .insert("x".into(), tango_match::gamemode::OptionValue::Boolean(true));
            assert!(catalog
                .resolve_gamemode(&changed_options, engine.clone(), vec![2])
                .is_err());
            let missing = self::catalog(&storage, &[second, common]).await;
            assert!(missing
                .resolve_gamemode(&configuration, engine.clone(), vec![2])
                .err()
                .unwrap()
                .to_string()
                .contains("missing gamemode"));
            let modified = self::catalog(&storage, &[first, library("1.0.0", 8)]).await;
            assert!(modified.resolve_gamemode(&configuration, engine, vec![2]).is_err());
            // Existing sessions retain the exact source snapshot after a rescan.
            assert_eq!(prepared.rom(), [9]);
            prepared.verify(identity).unwrap();
        });
    }

    #[test]
    fn catalog_automatic_versions_do_not_replace_explicit_pins() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            for version in ["1.0.0", "1.1.0", "2.0.0-beta.1"] {
                let mut files = editor_files("versions", None);
                let manifest = String::from_utf8(files.remove("package.toml").unwrap()).unwrap();
                files.insert(
                    "package.toml".into(),
                    manifest
                        .replace("version='1.0.0'", &format!("version='{version}'"))
                        .into_bytes(),
                );
                let package = Package::load(files).unwrap();
                storage
                    .write(
                        &Path::new("packages/versions").join(format!("{version}.tangopkg")),
                        &package.to_archive().unwrap(),
                    )
                    .unwrap();
            }
            let result = catalog(&storage, &[]).await;
            assert!(result.issues().is_empty(), "{:?}", result.issues());
            assert_eq!(result.exports().len(), 3);
            let latest = result.latest_exports(tango_script::ExportKind::Editor);
            assert_eq!(latest.len(), 1);
            assert_eq!(latest[0].reference.package.version.to_string(), "1.1.0");
            assert!(result
                .export(&ExportRef {
                    kind: tango_script::ExportKind::Editor,
                    package: PackageRef {
                        name: "versions".into(),
                        version: "1.0.0".parse().unwrap()
                    },
                    name: "main".into()
                })
                .is_some());
        });
    }

    #[test]
    fn package_bundle_install_and_load_share_the_storage_adapter() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let source = Path::new("development");
            let common = Path::new("common");
            // Keep storage coverage independent of the bundled game packages.
            for (path, contents) in [
                (
                    "development/package.toml",
                    "api = 1\nname = 'storage-test'\nversion = '1.0.0'\n[[editor]]\nname = 'main'\npath = './entry'\n[dependencies]\ncommon = '1.0.0'\n",
                ),
                ("development/init.luau", "--!strict\nreturn {label = 'Storage test'}"),
                ("development/entry.luau", "--!strict\nreturn require('./editor')"),
                (
                    "development/editor.luau",
                    r#"--!strict
local common = require("@common")
local t = tango.load_catalog("./locales")
return {
    decode = common.identity,
    encode = common.identity,
    validate = function(_bytes: buffer): {string} return {} end,
    view = function(_bytes: buffer, _state: ViewState): Node
        return {kind = "text", text = t("value")}
    end,
    update = function(_bytes: buffer, _state: ViewState, _action: Action) return nil end,
}
"#,
                ),
                ("development/locales/en-US/main.ftl", "package-name = Storage test\nvalue = Value\n"),
                ("development/locales/ja-JP/main.ftl", "package-name = ストレージテスト\nvalue = 値\n"),
                (
                    "common/package.toml",
                    "api = 1\nname = 'common'\nversion = '1.0.0'\n",
                ),
                (
                    "common/init.luau",
                    "--!strict\nreturn {identity = function(bytes: buffer): buffer return bytes end}",
                ),
            ] {
                storage.write(Path::new(path), contents.as_bytes()).unwrap();
            }
            storage
                .write(&source.join("notes/read me.txt"), b"unlisted data")
                .unwrap();
            let original = load(&storage, source).await.unwrap();
            let archive = Path::new("export.tangopkg");
            bundle(&storage, source, archive).await.unwrap();
            let bundled = [load(&storage, common).await.unwrap()];
            let installed = install(&storage, archive, Path::new("library"), &bundled)
                .await
                .unwrap();
            assert_eq!(installed, Path::new("library/storage-test/1.0.0.tangopkg"));
            assert_eq!(
                install(&storage, source, Path::new("library"), &bundled).await.unwrap(),
                installed
            );
            assert_eq!(load(&storage, &installed).await.unwrap().digest(), original.digest());
            let profile = load_profile(&storage, &[common.to_owned(), installed.clone()])
                .await
                .unwrap();
            let mut doc = tango_script::Document::open(profile, &[7, 8, 9]).unwrap();
            doc.set_locale("ja-JP").unwrap();
            assert_eq!(doc.title(), "ストレージテスト");
            assert_eq!(serde_json::to_value(doc.view()).unwrap()["text"], "値");
            assert_eq!(doc.encode().unwrap(), [7, 8, 9]);

            // An identity collision cannot overwrite the previously installed code.
            let mut modified = storage.read(&source.join("init.luau")).unwrap();
            modified.extend_from_slice(b"\n-- new revision\n");
            storage.write(&source.join("init.luau"), &modified).unwrap();
            assert!(install(&storage, source, Path::new("library"), &bundled).await.is_err());
            assert_eq!(load(&storage, &installed).await.unwrap().digest(), original.digest());
            assert!(load_profile(&storage, &[]).await.is_err());
        });
    }
    fn reference(name: &str, version: &str) -> PackageRef {
        PackageRef {
            name: name.into(),
            version: version.parse().unwrap(),
        }
    }

    fn input(storage: &MemoryStorage, package: &Package) -> PathBuf {
        let manifest = package.manifest();
        let path = PathBuf::from(format!("downloads/{}-{}.tangopkg", manifest.name, manifest.version));
        storage.write(&path, &package.to_archive().unwrap()).unwrap();
        path
    }

    #[test]
    fn inventory_includes_libraries_and_unresolved_graphs() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let library = library("1.0.0", 7);
            let extension = Package::load(editor_files("extension", Some("2.0.0"))).unwrap();
            let catalog = catalog(&storage, &[library, extension]).await;
            assert_eq!(catalog.entries().len(), 2);
            assert!(catalog.exports().is_empty());
            let common = catalog.entry(&reference("common", "1.0.0")).unwrap();
            assert!(common.error.is_none());
            assert_eq!(common.sources, [Source::Bundled].into());
            let extension = catalog.entry(&reference("extension", "1.0.0")).unwrap();
            assert!(extension
                .error
                .as_ref()
                .unwrap()
                .contains("missing package common 2.0.0"));
        });
    }

    #[test]
    fn batch_install_resolves_exact_dependencies_before_any_write() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let root = Path::new("packages");
            let extension = Package::load(editor_files("extension", Some("1.0.0"))).unwrap();
            let extension_path = input(&storage, &extension);
            let common = library("1.0.0", 7);
            let common_path = input(&storage, &common);
            let newer = library("2.0.0", 9);
            let before = storage.0.lock().unwrap().clone();
            let error = install_many(&storage, &[extension_path.clone()], root, &[newer.clone()])
                .await
                .unwrap_err();
            assert!(error.to_string().contains("missing package common 1.0.0"));
            assert_eq!(*storage.0.lock().unwrap(), before);
            // The selected extension comes first; installation order cannot
            // control whether its selected library dependency is visible.
            let sources = [extension_path, common_path];
            let installed = install_many(&storage, &sources, root, &[newer]).await.unwrap();
            assert_eq!(
                installed,
                [reference("common", "1.0.0"), reference("extension", "1.0.0")]
            );
            let first = storage.0.lock().unwrap().clone();
            install_many(&storage, &sources, root, &[]).await.unwrap();
            assert_eq!(*storage.0.lock().unwrap(), first);
            let catalog = catalog(&storage, &[]).await;
            assert!(catalog.issues().is_empty());
            assert_eq!(catalog.exports().len(), 1);
        });
    }

    #[test]
    fn batch_install_checks_transitive_imports_and_content_conflicts() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let root = Path::new("packages");
            let common = library("1.0.0", 7);
            let mut files = editor_files("extension", Some("1.0.0"));
            files.insert(
                "init.luau".into(),
                b"--!strict\nlocal value: string = require('@common').value\nreturn { value = value }".to_vec(),
            );
            let extension = Package::load(files).unwrap();
            let sources = [input(&storage, &common), input(&storage, &extension)];
            let before = storage.0.lock().unwrap().clone();
            assert!(install_many(&storage, &sources, root, &[]).await.is_err());
            assert_eq!(*storage.0.lock().unwrap(), before);
            let altered = library("1.0.0", 8);
            // Collision with bundled bytes is rejected, even with no archive.
            assert!(install_many(&storage, &sources[..1], root, &[altered]).await.is_err());
            assert_eq!(*storage.0.lock().unwrap(), before);
            let mut files = editor_files("extension", Some("1.0.0"));
            files.insert(
                "package.toml".into(),
                b"api=1\nname='extension'\nversion='1.0.0'\n[dependencies]\ncommon='1.0.0'\nother='1.0.0'".to_vec(),
            );
            let extension = Package::load(files).unwrap();
            let sources = [input(&storage, &common), input(&storage, &extension)];
            let before = storage.0.lock().unwrap().clone();
            assert!(install_many(&storage, &sources, root, &[]).await.is_err());
            assert_eq!(*storage.0.lock().unwrap(), before);
        });
    }

    #[test]
    fn failed_batch_rolls_back_only_new_archives() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let root = Path::new("packages");
            let common = library("1.0.0", 7);
            let extension = Package::load(editor_files("extension", Some("1.0.0"))).unwrap();
            let sources = [input(&storage, &common), input(&storage, &extension)];
            *storage.1.lock().unwrap() = Some(root.join("extension/1.0.0.tangopkg"));
            let before = storage.0.lock().unwrap().clone();
            assert!(install_many(&storage, &sources, root, &[])
                .await
                .unwrap_err()
                .to_string()
                .contains("injected rename failure"));
            assert_eq!(*storage.0.lock().unwrap(), before);
            // Retrying with a preexisting identical library must keep it.
            install_many(&storage, &sources[..1], root, &[]).await.unwrap();
            let before = storage.0.lock().unwrap().clone();
            assert!(install_many(&storage, &sources, root, &[]).await.is_err());
            assert_eq!(*storage.0.lock().unwrap(), before);
        });
    }

    #[test]
    fn removal_rechecks_exact_dependents_and_protects_non_archive_sources() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let root = Path::new("packages");
            let common = library("1.0.0", 7);
            let newer = library("2.0.0", 9);
            let common_ref = reference("common", "1.0.0");
            let extension_ref = reference("extension", "1.0.0");
            let extension = Package::load(editor_files("extension", Some("1.0.0"))).unwrap();
            let sources = [
                input(&storage, &common),
                input(&storage, &extension),
                input(&storage, &newer),
            ];
            install_many(&storage, &sources, root, &[]).await.unwrap();
            let snapshot = catalog(&storage, &[]).await;
            assert_eq!(removal_blockers(&snapshot, &common_ref), [extension_ref.clone()]);
            assert!(uninstall(&storage, root, &[], &common_ref)
                .await
                .unwrap_err()
                .to_string()
                .contains("extension 1.0.0"));
            uninstall(&storage, root, &[], &reference("common", "2.0.0"))
                .await
                .unwrap();
            // An identical bundled source continues to satisfy the dependency.
            uninstall(&storage, root, &[common.clone()], &common_ref).await.unwrap();
            assert!(uninstall(&storage, root, &[common.clone()], &common_ref).await.is_err());
            assert!(catalog(&storage, &[common]).await.issues().is_empty());
            uninstall(&storage, root, &[], &extension_ref).await.unwrap();
            storage
                .write(
                    &root.join("common/package.toml"),
                    b"api=1\nname='common'\nversion='1.0.0'",
                )
                .unwrap();
            storage
                .write(&root.join("common/init.luau"), b"--!strict\nreturn {value=7}")
                .unwrap();
            assert!(uninstall(&storage, root, &[], &common_ref).await.is_err());
            let before = storage.0.lock().unwrap().clone();
            assert!(uninstall(&storage, root, &[], &reference("../outside", "1.0.0"))
                .await
                .is_err());
            assert_eq!(*storage.0.lock().unwrap(), before);
        });
    }

    #[test]
    fn removing_a_conflicting_archive_restores_the_bundled_graph() {
        futures::executor::block_on(async {
            let storage = MemoryStorage::default();
            let root = Path::new("packages");
            let common = library("1.0.0", 7);
            let altered = library("1.0.0", 8);
            let extension = Package::load(editor_files("extension", Some("1.0.0"))).unwrap();
            storage
                .write(&root.join("common/1.0.0.tangopkg"), &altered.to_archive().unwrap())
                .unwrap();
            let bundled = [common, extension];
            let snapshot = catalog(&storage, &bundled).await;
            assert!(snapshot.exports().is_empty());
            let common_ref = reference("common", "1.0.0");
            assert_eq!(snapshot.entry(&common_ref).unwrap().sources.len(), 2);
            uninstall(&storage, root, &bundled, &common_ref).await.unwrap();
            let snapshot = catalog(&storage, &bundled).await;
            assert!(snapshot.issues().is_empty());
            assert_eq!(snapshot.exports().len(), 1);
        });
    }
}
