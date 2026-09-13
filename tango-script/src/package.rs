use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use semver::Version;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::{invalid, Checker, Result};

pub const MAX_SOURCE: usize = 256 * 1024;
pub const MAX_SOURCES: usize = 2 * 1024 * 1024;
pub const MAX_PACKAGE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_PACKAGE_FILES: usize = 4096;
pub(crate) const ENTRY_PATH: &str = "init.luau";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackageRef {
    pub name: String,
    pub version: Version,
}

/// Named, package-local capability export. Each gamemode declaration is one
/// selectable match type. The path uses normal relative require semantics,
/// including directory/init.luau modules.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModuleExport {
    pub name: String,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ExportKind {
    Editor,
    #[serde(rename = "gamemode")]
    GameMode,
    Telemetry,
}

impl ExportKind {
    pub const ALL: [Self; 3] = [Self::Editor, Self::GameMode, Self::Telemetry];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::GameMode => "gamemode",
            Self::Telemetry => "telemetry",
        }
    }

    pub(crate) fn contract(self) -> &'static str {
        match self {
            Self::Editor => "Editor",
            Self::GameMode => "GameMode",
            Self::Telemetry => "Telemetry",
        }
    }
}

/// Package composition uses ordinary modules and declared dependencies.
/// Files are discovered from the snapshot, without per-file declarations.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub api: u32,
    pub name: String,
    pub version: Version,
    /// Default selectable gamemode, required when this package exports any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_gamemode: Option<String>,
    /// Ordinary imports always resolve to init.luau, independently of exports.
    #[serde(default, rename = "editor", skip_serializing_if = "Vec::is_empty")]
    pub editors: Vec<ModuleExport>,
    #[serde(default, rename = "gamemode", skip_serializing_if = "Vec::is_empty")]
    pub gamemodes: Vec<ModuleExport>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub telemetry: Vec<ModuleExport>,
    #[serde(default)]
    pub dependencies: BTreeMap<String, Version>,
}

impl Manifest {
    pub fn exports(&self, kind: ExportKind) -> &[ModuleExport] {
        match kind {
            ExportKind::Editor => &self.editors,
            ExportKind::GameMode => &self.gamemodes,
            ExportKind::Telemetry => &self.telemetry,
        }
    }
}

/// Immutable source and assets. Cross-package checking runs when the complete
/// dependency graph is resolved, before any package is executed.
#[derive(Clone)]
pub struct Package {
    pub(crate) manifest: Manifest,
    pub(crate) modules: Arc<BTreeMap<String, Arc<str>>>,
    pub(crate) files: Arc<BTreeMap<String, Vec<u8>>>,
    pub(crate) catalogs: Arc<crate::i18n::Catalogs>,
    pub(crate) export_paths: BTreeMap<(ExportKind, String), String>,
    digest: [u8; 32],
}

/// Portable relative names, including Unicode and embedded spaces.
pub(crate) fn valid_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 240
        && !path.chars().any(|c| c.is_control() || "\\:".contains(c))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']))
}

pub(crate) fn valid_name(name: &str) -> bool {
    valid_path(name) && !name.contains('/') && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}

/// Normalize a root-relative file request entirely in the virtual tree.
pub(crate) fn file_path(path: &str) -> Result<String> {
    if path.len() > 240 || path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return Err(invalid("invalid package file path"));
    }
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop().ok_or_else(|| invalid("file path escapes package"))?;
            }
            "" => return Err(invalid("empty file path component")),
            part => parts.push(part),
        }
    }
    let path = parts.join("/");
    if !valid_path(&path) {
        return Err(invalid("invalid package file path"));
    }
    Ok(path)
}

fn hash_bytes(hash: &mut Sha3_256, bytes: &[u8]) {
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

impl Package {
    /// Load an immutable host-supplied file snapshot. The host must confine
    /// collection to its chosen package root. Execution never reads live paths.
    pub fn load(files: BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if files.len() > MAX_PACKAGE_FILES {
            return Err(invalid("package exceeds 4096 files"));
        }
        let mut total = 0usize;
        for (path, bytes) in &files {
            if !valid_path(path) {
                return Err(invalid(format!("invalid package file path: {path}")));
            }
            for (slash, _) in path.match_indices('/') {
                if files.contains_key(&path[..slash]) {
                    return Err(invalid("package path is both a file and a directory"));
                }
            }
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("package too large"))?;
            if total > MAX_PACKAGE_BYTES {
                return Err(invalid("package files exceed 64 MiB"));
            }
        }
        let bytes = files
            .get("package.toml")
            .ok_or_else(|| invalid("missing package.toml"))?;
        if bytes.len() > 64 * 1024 {
            return Err(invalid("package manifest exceeds 64 KiB"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid("package manifest must be UTF-8"))?;
        let manifest: Manifest = toml::from_str(text)?;
        match (&manifest.default_gamemode, manifest.gamemodes.is_empty()) {
            (None, true) => {}
            (Some(name), false) if manifest.gamemodes.iter().any(|export| &export.name == name) => {}
            _ => {
                return Err(invalid(
                    "packages with gamemodes must name an exported default_gamemode",
                ))
            }
        }
        if manifest.api != 1 {
            return Err(invalid("unsupported package API version"));
        }
        if !valid_name(&manifest.name) {
            return Err(invalid("package name must be a single folder name, for example bn6"));
        }
        if manifest.dependencies.len() > 64 {
            return Err(invalid("too many package dependencies"));
        }
        let mut aliases = BTreeSet::new();
        for name in manifest.dependencies.keys() {
            if !valid_name(name) {
                return Err(invalid("dependency keys must be canonical package names"));
            }
            if !aliases.insert(name.to_ascii_lowercase()) {
                return Err(invalid(
                    "dependency names must be unique ignoring ASCII case, like Luau aliases",
                ));
            }
        }
        let mut modules = BTreeMap::new();
        let mut source_bytes = 0usize;
        for (path, bytes) in &files {
            if !path.ends_with(".luau") {
                continue;
            }
            let source = source(bytes)?;
            source_bytes += source.len();
            if source_bytes > MAX_SOURCES {
                return Err(invalid("package source exceeds 2 MiB"));
            }
            modules.insert(path.clone(), Arc::<str>::from(source));
        }
        if !modules.contains_key(ENTRY_PATH) {
            return Err(invalid("missing package entry point: init.luau"));
        }
        if ExportKind::ALL
            .iter()
            .map(|&kind| manifest.exports(kind).len())
            .sum::<usize>()
            > 64
        {
            return Err(invalid("package exceeds 64 capability exports"));
        }
        let mut export_paths = BTreeMap::new();
        for kind in ExportKind::ALL {
            for export in manifest.exports(kind) {
                if !valid_name(&export.name) {
                    return Err(invalid("export names must be single folder names"));
                }
                let path = crate::modules::resolve_local_path(ENTRY_PATH, &export.path)?;
                let path =
                    crate::modules::module_file(path, &export.path, |candidate| modules.contains_key(candidate))?;
                if export_paths.insert((kind, export.name.clone()), path).is_some() {
                    return Err(invalid(format!("duplicate {} name: {}", kind.as_str(), export.name)));
                }
            }
        }
        let mut hash = Sha3_256::new();
        hash_bytes(&mut hash, b"tango-package-files-v1");
        for (path, bytes) in &files {
            hash_bytes(&mut hash, path.as_bytes());
            hash_bytes(&mut hash, bytes);
        }
        let package = Self {
            catalogs: Arc::new(crate::i18n::Catalogs::load(&files)?),
            export_paths,
            manifest,
            modules: Arc::new(modules),
            files: Arc::new(files),
            digest: hash.finalize().into(),
        };
        if package.manifest.dependencies.is_empty() {
            Checker::new().check(&crate::modules::Graph::new(std::slice::from_ref(&package))?)?;
        } else {
            // Source may refer to dependency exports that are unavailable until
            // profile resolution. Syntax/strict mode are still checked here.
            for source in package.modules.values() {
                Checker::new().syntax(source)?;
            }
        }
        Ok(package)
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    /// Captured source and asset bytes, before runtime inputs are bound.
    pub fn byte_len(&self) -> usize {
        self.files.values().map(Vec::len).sum()
    }
}

fn source(bytes: &[u8]) -> Result<&str> {
    if bytes.len() > MAX_SOURCE {
        return Err(invalid("script exceeds 256 KiB"));
    }
    let source = std::str::from_utf8(bytes).map_err(|_| invalid("script must be UTF-8 source"))?;
    if source.trim_start().lines().next().map(str::trim) != Some("--!strict") {
        return Err(invalid("scripts must start with --!strict"));
    }
    if source
        .lines()
        .any(|line| matches!(line.trim(), "--!nocheck" | "--!nonstrict"))
    {
        return Err(invalid("packages must use strict type checking"));
    }
    Ok(source)
}

/// A resolved dependency graph with independent capability selections;
/// composition is explicit in Luau, not inferred by the host.
/// The digest includes every capability selection and the UI locale. Use a
/// prepared gamemode's identity when negotiating a simulation instead.
#[derive(Clone)]
pub struct Profile {
    pub(crate) packages: Vec<Package>,
    pub(crate) graph: Arc<crate::modules::Graph>,
    pub(crate) inputs: crate::Inputs,
    pub(crate) locale: unic_langid::LanguageIdentifier,
    selections: BTreeMap<ExportKind, String>,
    input_digest: [u8; 32],
    display_name: String,
    code_digest: [u8; 32],
    digest: [u8; 32],
}

impl Profile {
    pub fn resolve(packages: &[Package], selected: &PackageRef) -> Result<Self> {
        let mut index = BTreeMap::new();
        for package in packages {
            let key = (package.manifest.name.clone(), package.manifest.version.clone());
            if index.insert(key, package).is_some() {
                return Err(invalid("duplicate package name and version"));
            }
        }
        let mut active = BTreeSet::new();
        let mut complete = BTreeSet::new();
        let mut ordered = Vec::new();
        visit(selected, &index, &mut active, &mut complete, &mut ordered)?;
        let graph = Arc::new(crate::modules::Graph::new(&ordered)?);
        if ordered.iter().any(|p| !p.manifest.dependencies.is_empty()) {
            Checker::new().check(&graph)?;
        }
        let mut hash = Sha3_256::new();
        hash_bytes(&mut hash, b"tango-profile-dependencies-v2-luau-0.736-mlua-0.12.1-o2");
        hash_bytes(&mut hash, crate::SDK.as_bytes());
        for package in &ordered {
            hash.update(package.digest);
        }
        let digest = hash.finalize().into();
        let manifest = &ordered.last().expect("resolved profile").manifest;
        let selections = ExportKind::ALL
            .into_iter()
            .filter_map(|kind| {
                if kind == ExportKind::GameMode {
                    manifest.default_gamemode.clone().map(|name| (kind, name))
                } else {
                    match manifest.exports(kind) {
                        [export] => Some((kind, export.name.clone())),
                        _ => None,
                    }
                }
            })
            .collect();
        Self {
            display_name: String::new(),
            packages: ordered,
            graph,
            selections,
            inputs: crate::Inputs::default(),
            locale: crate::i18n::default_locale(),
            input_digest: [0; 32],
            code_digest: digest,
            digest,
        }
        .with_inputs(crate::Inputs::default())
        .with_locale("en-US")
    }

    /// Bind a fixed set of host-supplied input buffers to all operations and
    /// dependencies. Rebinding creates a new environment; existing documents
    /// retain their original snapshot. Inputs never enter package archives.
    pub fn with_inputs(mut self, inputs: crate::Inputs) -> Self {
        self.input_digest = inputs.digest();
        self.inputs = inputs;
        self.update_digest();
        self
    }

    /// Set the host's active language. Catalog lookup remains package-local,
    /// with regional negotiation and a per-message en-US fallback.
    pub fn with_locale(mut self, locale: &str) -> Result<Self> {
        self.locale = crate::i18n::locale(locale)?;
        let package = self.packages.last().expect("resolved profile");
        self.display_name = package
            .catalogs
            .package_name(&self.locale)?
            .unwrap_or_else(|| package.manifest.name.clone());
        self.update_digest();
        Ok(self)
    }

    pub fn locale(&self) -> &unic_langid::LanguageIdentifier {
        &self.locale
    }

    fn update_digest(&mut self) {
        let mut hash = Sha3_256::new();
        hash.update(b"tango-capability-environment-v4");
        hash.update(self.code_digest);
        hash.update(self.input_digest);
        hash_bytes(&mut hash, self.locale.to_string().as_bytes());
        for kind in ExportKind::ALL {
            hash_bytes(&mut hash, kind.as_str().as_bytes());
            hash_bytes(&mut hash, self.selected_export(kind).unwrap_or("").as_bytes());
        }
        self.digest = hash.finalize().into();
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Localized label for a named capability; the export name is its fallback.
    pub fn export_display_name(&self, kind: ExportKind, name: &str) -> Result<String> {
        let package = self.packages.last().expect("resolved profile");
        Ok(package
            .catalogs
            .metadata(&self.locale, &format!("{}-{name}", kind.as_str()))?
            .unwrap_or_else(|| name.to_owned()))
    }

    /// The selected gamemode's emulator platform, independent of native games.
    pub fn gamemode_platform(&self) -> Result<Option<String>> {
        crate::runtime::Runtime::for_export(self, ExportKind::GameMode)?.gamemode_platform()
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }

    pub fn packages(&self) -> impl Iterator<Item = &Manifest> {
        self.packages.iter().map(|package| &package.manifest)
    }

    /// Select only this capability; other selections remain unchanged. Exports
    /// from dependencies are never implicitly inherited by a consumer.
    pub fn with_export(mut self, kind: ExportKind, name: &str) -> Result<Self> {
        if !self
            .graph
            .exports
            .last()
            .expect("resolved profile")
            .contains_key(&(kind, name.to_owned()))
        {
            return Err(invalid(format!(
                "selected package does not expose {}: {name}",
                kind.as_str()
            )));
        }
        self.selections.insert(kind, name.to_owned());
        self.update_digest();
        Ok(self)
    }

    pub fn with_editor(self, name: &str) -> Result<Self> {
        self.with_export(ExportKind::Editor, name)
    }

    pub fn with_gamemode(self, name: &str) -> Result<Self> {
        self.with_export(ExportKind::GameMode, name)
    }

    /// Simulation cannot depend on a host's UI language or which editor it
    /// happens to have open. Package catalogs still work in the fixed locale.
    pub(crate) fn for_gamemode(mut self) -> Result<Self> {
        self.export_module(ExportKind::GameMode)?;
        self.selections.retain(|kind, _| *kind == ExportKind::GameMode);
        self.with_locale("en-US")
    }

    pub fn with_telemetry(self, name: &str) -> Result<Self> {
        self.with_export(ExportKind::Telemetry, name)
    }

    pub fn selected_export(&self, kind: ExportKind) -> Option<&str> {
        self.selections.get(&kind).map(String::as_str)
    }

    pub(crate) fn export_module(&self, kind: ExportKind) -> Result<&str> {
        let exports = self.graph.exports.last().expect("resolved profile");
        match self.selections.get(&kind) {
            Some(name) => Ok(exports.get(&(kind, name.clone())).expect("selected export").as_str()),
            None if !exports.keys().any(|(k, _)| *k == kind) => {
                Err(invalid(format!("selected package does not expose {}", kind.as_str())))
            }
            None => Err(invalid(format!(
                "selected package exposes multiple {} exports; select one by name",
                kind.as_str()
            ))),
        }
    }

    /// ROM transformation belongs to the selected gamemode. Packages with
    /// no gamemodes leave the ROM unchanged (including editor-only packages).
    pub fn patch_rom(&self, rom: &[u8]) -> Result<Vec<u8>> {
        if self
            .packages
            .last()
            .expect("resolved profile")
            .manifest
            .gamemodes
            .is_empty()
        {
            if rom.len() > crate::MAX_BUFFER {
                return Err(invalid("buffer exceeds 32 MiB"));
            }
            return Ok(rom.to_vec());
        }
        crate::runtime::Runtime::for_export(self, ExportKind::GameMode)?.transform("patch_rom", rom)
    }

    /// Recognition belongs to each export and cannot mutate the supplied ROM.
    /// A missing detector declines automatic selection.
    pub fn supports_export(&self, kind: ExportKind, rom: &[u8]) -> Result<bool> {
        crate::runtime::Runtime::for_export(self, kind)?.detect_rom(rom)
    }

    pub fn supports_editor(&self, rom: &[u8]) -> Result<bool> {
        self.supports_export(ExportKind::Editor, rom)
    }

    pub fn save_templates(&self) -> Result<Vec<crate::SaveTemplate>> {
        crate::runtime::Runtime::new(self)?.save_templates()
    }

    pub fn create_save(&self, name: &str) -> Result<Vec<u8>> {
        if !self.save_templates()?.iter().any(|template| template.name == name) {
            return Err(invalid("unknown save template"));
        }
        let bytes = crate::runtime::Runtime::new(self)?.create_save(name)?;
        self.check_save(&bytes)?;
        Ok(bytes)
    }

    /// Check whether the editor can decode a file, without rendering or running
    /// build validation. Editable diagnostics do not make a save unrecognizable.
    pub fn check_save(&self, input: &[u8]) -> Result<()> {
        if input.len() > crate::editor::MAX_DOCUMENT {
            return Err(invalid("document exceeds 8 MiB"));
        }
        let bytes = crate::runtime::Runtime::new(self)?.transform("decode", input)?;
        if bytes.len() > crate::editor::MAX_DOCUMENT {
            return Err(invalid("decoded document exceeds 8 MiB"));
        }
        Ok(())
    }

    /// Run the package's Luau conformance tests in the same restricted host.
    pub fn test(&self) -> Result<()> {
        crate::runtime::Runtime::for_test(self)?.test()
    }
}

type PackageKey = (String, Version);

fn visit(
    reference: &PackageRef,
    index: &BTreeMap<PackageKey, &Package>,
    active: &mut BTreeSet<PackageKey>,
    complete: &mut BTreeSet<PackageKey>,
    ordered: &mut Vec<Package>,
) -> Result<()> {
    let key = (reference.name.clone(), reference.version.clone());
    if complete.contains(&key) {
        return Ok(());
    }
    if !active.insert(key.clone()) {
        return Err(invalid("cyclic package dependency"));
    }
    if active.len() + complete.len() > 128 {
        return Err(invalid("dependency graph exceeds 128 packages"));
    }
    let package = index
        .get(&key)
        .ok_or_else(|| invalid(format!("missing package {} {}", key.0, key.1)))?;
    for (name, version) in &package.manifest.dependencies {
        let dependency = PackageRef {
            name: name.clone(),
            version: version.clone(),
        };
        visit(&dependency, index, active, complete, ordered)?;
    }
    active.remove(&key);
    complete.insert(key);
    ordered.push((*package).clone());
    Ok(())
}
