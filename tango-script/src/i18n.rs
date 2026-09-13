//! Package-owned Fluent catalogs. No filesystem access or ambient translations.
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use mlua::{Function, Lua, Value};

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use fluent_langneg::{negotiate_languages, NegotiationStrategy};
use serde::Deserialize;
use unic_langid::LanguageIdentifier;

use crate::{invalid, Result};

mod failure;
mod validate;

pub(crate) use failure::{initialize, localized, TRANSLATORS};

const MAX_TEXT: usize = 16 * 1024;
type Bundle = FluentBundle<Arc<FluentResource>>;

pub(crate) fn locale(name: &str) -> Result<LanguageIdentifier> {
    if name.is_empty() || name.len() > 128 || name.contains('_') {
        return Err(invalid(
            "locale must be a BCP 47 language identifier, for example en-US",
        ));
    }
    name.parse().map_err(|_| invalid(format!("invalid locale: {name}")))
}

pub(crate) fn default_locale() -> LanguageIdentifier {
    "en-US".parse().expect("default locale")
}

/// Parsed immutable FTL resources. Catalog directories are explicit, and can
/// be nested anywhere in the package. A bounded cache shares compiled bundles.
pub(crate) struct Catalogs {
    resources: BTreeMap<String, Arc<FluentResource>>,
    directories: Mutex<BTreeMap<String, Arc<Catalog>>>,
    bytes: usize,
}

impl Catalogs {
    pub fn load(files: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        let mut resources = BTreeMap::new();
        let mut total = 0usize;
        for (path, bytes) in files.iter().filter(|(path, _)| path.ends_with(".ftl")) {
            total += bytes.len();
            if bytes.len() > 256 * 1024 || total > 2 * 1024 * 1024 {
                return Err(invalid("translations exceed 256 KiB per file or 2 MiB per package"));
            }
            let source =
                std::str::from_utf8(bytes).map_err(|_| invalid(format!("translation must be UTF-8: {path}")))?;
            let resource = FluentResource::try_new(source.to_owned())
                .map_err(|(_, errors)| invalid(format!("invalid translation {path}: {errors:?}")))?;
            validate::resource(&resource).map_err(|e| invalid(format!("invalid translation {path}: {e}")))?;
            resources.insert(path.clone(), Arc::new(resource));
        }
        let result = Self {
            resources,
            directories: Mutex::new(BTreeMap::new()),
            bytes: total,
        };
        // The conventional metadata catalog supplies package-name even before
        // script execution. Other catalog directories load on explicit request.
        if result.resources.keys().any(|p| p.starts_with("locales/")) {
            result.directory("locales")?;
        }
        Ok(result)
    }

    pub fn package_name(&self, locale: &LanguageIdentifier) -> Result<Option<String>> {
        self.metadata(locale, "package-name")
    }

    pub fn metadata(&self, locale: &LanguageIdentifier, key: &str) -> Result<Option<String>> {
        if !self.resources.keys().any(|p| p.starts_with("locales/")) {
            return Ok(None);
        }
        self.directory("locales")?
            .for_locale(locale)
            .text_if_present(key, Arguments::new())
    }

    fn directory(&self, directory: &str) -> Result<Arc<Catalog>> {
        let mut cache = self
            .directories
            .lock()
            .map_err(|_| invalid("catalog cache lock poisoned"))?;
        if let Some(catalog) = cache.get(directory) {
            return Ok(catalog.clone());
        }
        let prefix = if directory.is_empty() {
            String::new()
        } else {
            format!("{directory}/")
        };
        let mut locales = BTreeMap::<String, Bundle>::new();
        for (path, resource) in &self.resources {
            let Some(relative) = path.strip_prefix(&prefix) else {
                continue;
            };
            let (name, _) = relative
                .split_once('/')
                .ok_or_else(|| invalid(format!("expected <catalog>/<language>/<file>.ftl: {path}")))?;
            let language = locale(name)?;
            if language.to_string() != name {
                return Err(invalid(format!(
                    "locale folder must use canonical spelling: {language}"
                )));
            }
            let bundle = locales.entry(name.into()).or_insert_with(|| {
                let mut bundle = Bundle::new_concurrent(vec![language]);
                // Match Tango's existing labels, including native dialogs.
                bundle.set_use_isolating(false);
                // Bound native precision/formatting outside the VM quota.
                bundle.add_function("NUMBER", number).expect("new bundle");
                bundle
            });
            bundle
                .add_resource(resource.clone())
                .map_err(|errors| invalid(format!("duplicate translation in {path}: {errors:?}")))?;
            if locales.len() > 64 {
                return Err(invalid("catalog exceeds 64 translation locales"));
            }
        }
        if locales.is_empty() {
            return Err(invalid(format!(
                "catalog directory not found or contains no FTL files: {directory}"
            )));
        }
        let (locales, bundles) = locales
            .into_iter()
            .map(|(name, bundle)| (name.parse().expect("validated locale"), Arc::new(bundle)))
            .unzip();
        let catalog = Arc::new(Catalog { locales, bundles });
        // Cache occupancy must not make an otherwise valid operation depend
        // on earlier documents' catalog loads. Evict, rather than rejecting.
        if cache.len() >= 64 {
            cache.pop_first();
        }
        cache.insert(directory.to_owned(), catalog.clone());
        Ok(catalog)
    }
}

struct Catalog {
    locales: Vec<LanguageIdentifier>,
    bundles: Vec<Arc<Bundle>>,
}

impl Catalog {
    pub fn for_locale(&self, locale: &LanguageIdentifier) -> Translator {
        let fallback = default_locale();
        let chain = negotiate_languages(
            std::slice::from_ref(locale),
            &self.locales,
            self.locales.iter().find(|l| **l == fallback),
            NegotiationStrategy::Filtering,
        );
        Translator {
            bundles: chain
                .iter()
                .map(|l| self.bundles[self.locales.iter().position(|available| available == *l).unwrap()].clone())
                .collect(),
        }
    }
}

fn number<'a>(positional: &[FluentValue<'a>], named: &FluentArgs) -> FluentValue<'a> {
    let Some(FluentValue::Number(value)) = positional.first() else {
        return FluentValue::Error;
    };
    for (key, value) in named.iter() {
        if key.ends_with("Digits") {
            match value {
                FluentValue::Number(n) if (0.0..=18.0).contains(&n.value) && n.value.fract() == 0.0 => {}
                _ => return FluentValue::Error,
            }
        }
    }
    let mut value = value.clone();
    value.options.merge(named);
    FluentValue::Number(value)
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum Argument {
    Text(String),
    Number(f64),
}

pub(crate) type Arguments = BTreeMap<String, Argument>;

fn arguments(lua: &Lua, value: Value) -> Result<Arguments> {
    if matches!(value, Value::Nil) {
        return Ok(Arguments::new());
    }
    crate::decode::from_value(
        lua,
        value,
        crate::decode::Limits {
            values: 65,
            bytes: 8 * 1024,
            depth: 1,
            string: 8 * 1024,
            root: crate::decode::Root::Map,
        },
        false,
    )
}

pub(crate) struct Translator {
    bundles: Vec<Arc<Bundle>>,
}

impl Translator {
    pub fn text_if_present(&self, key: &str, args: Arguments) -> Result<Option<String>> {
        if key.is_empty() || key.len() > 256 || key.chars().any(char::is_control) {
            return Err(invalid("translation key must contain 1–256 bytes without controls"));
        }
        let mut arguments = FluentArgs::new();
        for (name, value) in args {
            match value {
                Argument::Text(s) => arguments.set(name, s),
                Argument::Number(n) => {
                    validate::number(n)?;
                    arguments.set(name, n);
                }
            }
        }
        let (id, attribute) = key.split_once('.').map_or((key, None), |(id, attr)| (id, Some(attr)));
        for bundle in &self.bundles {
            let Some(message) = bundle.get_message(id) else {
                continue;
            };
            let pattern = match attribute {
                Some(attr) => message.get_attribute(attr).map(|a| a.value()),
                None => message.value(),
            };
            let Some(pattern) = pattern else { continue };
            let mut output = Text(String::new());
            let mut errors = Vec::new();
            bundle
                .write_pattern(&mut output, pattern, Some(&arguments), &mut errors)
                .map_err(|_| invalid("translated text exceeds 16 KiB"))?;
            if !errors.is_empty() {
                return Err(invalid(format!("cannot format translation {key}: {errors:?}")));
            }
            return Ok(Some(output.0));
        }
        Ok(None)
    }
}

struct Text(String);
impl fmt::Write for Text {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if self.0.len() + value.len() > MAX_TEXT {
            return Err(fmt::Error);
        }
        self.0.push_str(value);
        Ok(())
    }
}

/// Each module gets a loader anchored at its own source path. The returned
/// function is explicitly bound to the requested catalog and active locale;
/// passing that function to another package cannot change either selection.
pub(crate) struct Context {
    graph: Arc<crate::modules::Graph>,
    packages: Vec<Arc<Catalogs>>,
    locale: LanguageIdentifier,
    budget: Arc<AtomicUsize>,
    fallback_depth: Arc<AtomicUsize>,
}

impl Context {
    pub fn new(profile: &crate::Profile, budget: Arc<AtomicUsize>) -> Arc<Self> {
        Arc::new(Self {
            graph: profile.graph.clone(),
            packages: profile.packages.iter().map(|p| p.catalogs.clone()).collect(),
            locale: profile.locale.clone(),
            budget,
            fallback_depth: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn loader(self: &Arc<Self>, lua: &Lua, from: &str) -> mlua::Result<Function> {
        let context = self.clone();
        let from = from.to_owned();
        lua.create_function(move |lua, (request, fallback): (String, Option<Function>)| {
            if !request.starts_with("./") && !request.starts_with("../") {
                return Err(mlua::Error::runtime(
                    "catalog paths must be package-local; import a shared translator with require",
                ));
            }
            let (package, directory) = context
                .graph
                .resolve_path(&from, &request)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let catalogs = &context.packages[package];
            // Charge the package's parsed catalog bytes even on cache hits,
            // bounding repeated directory scans and native bundle work.
            crate::runtime::charge(&context.budget, MAX_TEXT + catalogs.bytes)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let catalog = catalogs
                .directory(&directory)
                .map_err(|e| mlua::Error::runtime(e.to_string()))?;
            let translator = catalog.for_locale(&context.locale);
            let chain = failure::Chain::new(lua, catalog, fallback.as_ref())?;
            let budget = context.budget.clone();
            let lookup = lua.create_function(move |lua, (key, args): (String, Value)| {
                crate::runtime::charge(&budget, MAX_TEXT).map_err(|e| mlua::Error::runtime(e.to_string()))?;
                let arguments = arguments(lua, args).map_err(mlua::Error::external)?;
                translator
                    .text_if_present(&key, arguments)
                    .map_err(|e| mlua::Error::runtime(e.to_string()))
            })?;
            let depth = context.fallback_depth.clone();
            let enter = lua.create_function(move |_, ()| {
                depth
                    .try_update(Ordering::Relaxed, Ordering::Relaxed, |n| (n < 32).then_some(n + 1))
                    .map_err(|_| mlua::Error::runtime("translation fallback chain exceeds 32 calls"))?;
                Ok(())
            })?;
            let depth = context.fallback_depth.clone();
            let finish = lua.create_function(move |_, value: Value| {
                depth.fetch_sub(1, Ordering::Relaxed);
                let Value::String(text) = value else {
                    return Err(mlua::Error::runtime("translation fallback must return a string"));
                };
                if text.as_bytes().len() > MAX_TEXT {
                    return Err(mlua::Error::runtime("translated text exceeds 16 KiB"));
                }
                Ok(text.to_str()?.to_owned())
            })?;
            // Delegate in Luau: calling an imported translator from this Rust
            // callback would recursively nest native stacks. Callbacks only
            // perform bounded lookup/validation and return before delegation.
            // Errors abort the operation and discard its translation depth state.
            let function = lua
                .load(
                    r#"
local lookup, fallback, enter, finish = ...
return function(key, args)
    local text = lookup(key, args)
    if text ~= nil then return text end
    if fallback == nil then return "⟦" .. key .. "⟧" end
    enter()
    return finish(fallback(key, args))
end
"#,
                )
                .set_name("tango/catalog-translator")
                .call::<Function>((lookup, fallback, enter, finish))?;
            if let Some(chain) = chain {
                let registry: mlua::Table = lua.named_registry_value(TRANSLATORS)?;
                registry.raw_set(function.clone(), lua.create_userdata(chain)?)?;
            }
            Ok(function)
        })
    }

    pub fn fail(self: &Arc<Self>, lua: &Lua) -> mlua::Result<Function> {
        failure::function(lua, self.budget.clone(), self.locale.clone())
    }
}
