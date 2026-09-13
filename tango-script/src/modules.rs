//! Package-local import resolution, shared by the checker and VM.
use crate::{invalid, package::ENTRY_PATH, Result};
use mlua::{Lua, Table, Value};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::Mutex;

pub(crate) struct Module {
    pub package: usize,
    pub path: String,
    pub source: Arc<str>,
    pub contracts: BTreeSet<crate::ExportKind>,
    // Host-compiled bytecode belongs to the immutable graph, never the package
    // archive. Each operation still creates fresh environments and upvalues.
    bytecode: std::sync::OnceLock<std::result::Result<Vec<u8>, String>>,
}

/// One canonical graph feeds static import resolution and runtime loading.
pub(crate) struct Graph {
    pub machine: Mutex<Option<Lua>>,
    pub modules: BTreeMap<String, Module>,
    pub entries: Vec<String>,
    pub exports: Vec<BTreeMap<(crate::ExportKind, String), String>>,
    prefixes: Vec<String>,
    dependencies: BTreeMap<(usize, String), usize>,
}

impl Graph {
    pub fn new(packages: &[crate::Package]) -> Result<Self> {
        let prefixes: Vec<_> = packages
            .iter()
            .map(|p| format!("{}@{}/", p.manifest.name, p.manifest.version))
            .collect();
        let mut graph = Self {
            machine: Default::default(),
            modules: BTreeMap::new(),
            entries: Vec::new(),
            exports: Vec::new(),
            prefixes,
            dependencies: BTreeMap::new(),
        };
        let mut source_bytes = 0;
        for (index, package) in packages.iter().enumerate() {
            for (name, version) in &package.manifest.dependencies {
                let target = packages
                    .iter()
                    .position(|p| &p.manifest.name == name && &p.manifest.version == version)
                    .ok_or_else(|| invalid(format!("missing dependency {name} {version}")))?;
                graph.dependencies.insert((index, name.to_ascii_lowercase()), target);
            }
            graph.entries.push(format!("{}{ENTRY_PATH}", graph.prefixes[index]));
            graph.exports.push(
                package
                    .export_paths
                    .iter()
                    .map(|(name, path)| (name.clone(), format!("{}{path}", graph.prefixes[index])))
                    .collect(),
            );
            for (path, source) in package.modules.iter() {
                source_bytes += source.len();
                if source_bytes > 16 * 1024 * 1024 {
                    return Err(invalid("profile source exceeds 16 MiB"));
                }
                graph.modules.insert(
                    format!("{}{path}", graph.prefixes[index]),
                    Module {
                        package: index,
                        path: path.clone(),
                        source: source.clone(),
                        contracts: package
                            .export_paths
                            .iter()
                            .filter_map(|((kind, _), export)| (export == path).then_some(*kind))
                            .collect(),
                        bytecode: Default::default(),
                    },
                );
            }
        }
        Ok(graph)
    }

    pub fn resolve(&self, from: &str, request: &str) -> Result<String> {
        let (package, path) = self.resolve_path(from, request)?;
        let prefix = &self.prefixes[package];
        let path = module_file(path, request, |path| {
            self.modules.contains_key(&format!("{prefix}{path}"))
        })?;
        Ok(format!("{prefix}{path}"))
    }

    /// Resolve a module-relative path or an explicitly declared dependency
    /// path. Catalogs keep module-relative, package-local file semantics.
    pub fn resolve_path(&self, from: &str, request: &str) -> Result<(usize, String)> {
        if request.len() > 512 || request.starts_with('/') || request.contains('\\') || request.contains(':') {
            return Err(invalid("invalid package path"));
        }
        let origin = self
            .modules
            .get(from)
            .ok_or_else(|| invalid("unknown importing module"))?;
        if let Some(request) = request.strip_prefix('@') {
            let (name, path) = match request.split_once('/') {
                Some((name, path)) => (name, Some(path)),
                None => (request, None),
            };
            if !crate::package::valid_name(name) {
                return Err(invalid("invalid package name"));
            }
            let target = *self
                .dependencies
                .get(&(origin.package, name.to_ascii_lowercase()))
                .ok_or_else(|| invalid(format!("undeclared package dependency: {name}")))?;
            match path {
                None => Ok((target, String::new())),
                Some(path) => {
                    // A dependency name is a package root, not permission to navigate
                    // into sibling packages or the importing package's files.
                    if !crate::package::valid_path(path) {
                        return Err(invalid("invalid dependency path"));
                    }
                    Ok((target, path.to_owned()))
                }
            }
        } else {
            Ok((origin.package, resolve_local_path(&origin.path, request)?))
        }
    }
}

pub(crate) const CACHE: &str = "tango:modules";
pub(crate) const COMPILED: &str = "tango:compiled-modules";
pub(crate) fn host_key(index: usize) -> String {
    format!("tango:host:{index}")
}

pub(crate) struct Loader {
    graph: Arc<Graph>,
    active: Mutex<BTreeSet<String>>,
    translations: Arc<crate::i18n::Context>,
}

impl Loader {
    pub fn new(graph: Arc<Graph>, translations: Arc<crate::i18n::Context>) -> Arc<Self> {
        Arc::new(Self {
            graph,
            active: Mutex::new(BTreeSet::new()),
            translations,
        })
    }

    pub fn load(self: &Arc<Self>, lua: &Lua, name: &str) -> mlua::Result<Value> {
        let cache: Table = lua.named_registry_value(CACHE)?;
        let cached: Value = cache.raw_get(name)?;
        if cached != Value::Nil {
            return Ok(cached);
        }
        {
            let mut active = self
                .active
                .lock()
                .map_err(|_| mlua::Error::runtime("module loader lock poisoned"))?;
            if active.len() >= 64 {
                return Err(mlua::Error::runtime("module import depth exceeded"));
            }
            if !active.insert(name.to_owned()) {
                return Err(mlua::Error::runtime(format!("cyclic module import: {name}")));
            }
        }
        let result = (|| {
            let module = self
                .graph
                .modules
                .get(name)
                .ok_or_else(|| mlua::Error::runtime("module not found"))?;
            let package_host: Table = lua.named_registry_value(&host_key(module.package))?;
            let host = lua.create_table()?;
            for entry in package_host.pairs::<Value, Value>() {
                let (key, value) = entry?;
                host.raw_set(key, value)?;
            }
            host.raw_set("load_catalog", self.translations.loader(lua, name)?)?;
            host.raw_set("fail", self.translations.fail(lua)?)?;
            host.set_readonly(true);
            let environment = lua.create_table()?;
            environment.set("tango", host)?;
            let context = name.to_owned();
            let loader = self.clone();
            environment.set(
                "require",
                lua.create_function(move |lua, request: String| {
                    let name = loader
                        .graph
                        .resolve(&context, &request)
                        .map_err(|e| mlua::Error::runtime(e.to_string()))?;
                    loader.load(lua, &name)
                })?,
            )?;
            let metatable = lua.create_table()?;
            metatable.set("__index", lua.globals())?;
            environment.set_metatable(Some(metatable))?;
            environment.set_readonly(true);
            environment.set_safeenv(true);
            let bytecode = module
                .bytecode
                .get_or_init(|| {
                    mlua::chunk::Compiler::new()
                        .set_optimization_level(2)
                        .compile(module.source.as_ref())
                        .map_err(|error| error.to_string())
                })
                .as_ref()
                .map_err(|error| mlua::Error::runtime(error.clone()))?;
            let compiled: Table = lua.named_registry_value(COMPILED)?;
            let template = match compiled.raw_get::<Option<mlua::Function>>(name)? {
                Some(template) => template,
                None => {
                    // Bind only immutable builtins while loading/JIT compiling.
                    // No operation's tango/require closures enter cached imports.
                    let template = lua
                        .load(bytecode)
                        .set_mode(mlua::chunk::ChunkMode::Binary)
                        .set_name(name)
                        .set_environment(lua.globals())
                        .into_function()?;
                    compiled.raw_set(name, template.clone())?;
                    template
                }
            };
            // Clone the never-executed module closure; running it creates fresh
            // captured state. Its immutable prototype and native code are shared.
            let function = template.deep_clone()?;
            function.set_environment(environment)?;
            let value: Value = function.call(())?;
            if value == Value::Nil {
                return Err(mlua::Error::runtime(format!("module returned nil: {name}")));
            }
            for &kind in &module.contracts {
                crate::runtime::validate_export(kind, &value).map_err(mlua::Error::external)?;
            }
            cache.raw_set(name, value.clone())?;
            Ok(value)
        })();
        self.active
            .lock()
            .map_err(|_| mlua::Error::runtime("module loader lock poisoned"))?
            .remove(name);
        result
    }
}

pub(crate) fn resolve_local_path(from: &str, request: &str) -> Result<String> {
    if !request.starts_with("./") && !request.starts_with("../") {
        return Err(invalid("imports must be relative package paths"));
    }
    if request.len() > 240 {
        return Err(invalid("module path too long"));
    }
    let mut parts: Vec<_> = from.split('/').collect();
    parts.pop();
    for part in request.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop().ok_or_else(|| invalid("import escapes package"))?;
            }
            "" => return Err(invalid("empty import path component")),
            part => parts.push(part),
        }
    }
    let path = parts.join("/");
    if !path.is_empty() && !crate::package::valid_path(&path) {
        return Err(invalid("invalid module import path"));
    }
    Ok(path)
}

/// Shared by manifest exports and require; both reject ambiguous files and
/// use the same directory/init.luau convention within the captured snapshot.
pub(crate) fn module_file(mut path: String, request: &str, mut exists: impl FnMut(&str) -> bool) -> Result<String> {
    if path.is_empty() {
        path = ENTRY_PATH.to_owned();
    }
    if !crate::package::valid_path(&path) {
        return Err(invalid("invalid module import path"));
    }
    let candidates = if path.ends_with(".luau") {
        vec![path]
    } else {
        vec![format!("{path}.luau"), format!("{path}/{ENTRY_PATH}")]
    };
    let mut matches = candidates.into_iter().filter(|path| exists(path));
    let name = matches
        .next()
        .ok_or_else(|| invalid(format!("module not found: {request}")))?;
    if matches.next().is_some() {
        return Err(invalid(format!(
            "ambiguous module import: {request}; use an explicit .luau path"
        )));
    }
    Ok(name)
}
