//! Retained failures own catalog data, never Lua closures or an operation's VM.
use super::*;
use mlua::{AnyUserData, Table, UserData};

pub(crate) const TRANSLATORS: &str = "tango:translators";

pub(crate) fn initialize(lua: &Lua) -> mlua::Result<()> {
    let registry = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.set("__mode", "k")?;
    registry.set_metatable(Some(metatable))?;
    lua.set_named_registry_value(TRANSLATORS, registry)
}

#[derive(Clone)]
pub(super) struct Chain(Vec<Arc<Catalog>>);
impl UserData for Chain {}

impl Chain {
    pub fn new(lua: &Lua, catalog: Arc<Catalog>, fallback: Option<&Function>) -> mlua::Result<Option<Self>> {
        let mut catalogs = vec![catalog];
        if let Some(fallback) = fallback {
            let registry: Table = lua.named_registry_value(TRANSLATORS)?;
            let Some(chain) = registry.raw_get::<Option<AnyUserData>>(fallback.clone())? else {
                // Arbitrary Lua fallback functions still work for eager labels,
                // but cannot be retained safely after the operation ends.
                return Ok(None);
            };
            let chain = chain.borrow::<Self>()?;
            if chain.0.len() >= 33 {
                return Ok(None);
            }
            catalogs.extend(chain.0.iter().cloned());
        }
        Ok(Some(Self(catalogs)))
    }

    fn text(&self, locale: &LanguageIdentifier, key: &str, args: &Arguments) -> Result<String> {
        for catalog in &self.0 {
            if let Some(text) = catalog.for_locale(locale).text_if_present(key, args.clone())? {
                return Ok(text);
            }
        }
        Ok(format!("⟦{key}⟧"))
    }
}

struct Failure {
    chain: Chain,
    key: String,
    args: Arguments,
    original: String,
}

impl fmt::Debug for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LocalizedFailure")
            .field("key", &self.key)
            .field("args", &self.args)
            .finish()
    }
}
impl fmt::Display for Failure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.original)
    }
}
impl std::error::Error for Failure {}

pub(super) fn function(lua: &Lua, budget: Arc<AtomicUsize>, locale: LanguageIdentifier) -> mlua::Result<Function> {
    lua.create_function(
        move |lua, (translator, key, args): (Function, String, Value)| -> mlua::Result<()> {
            crate::runtime::charge(&budget, MAX_TEXT * 33).map_err(mlua::Error::external)?;
            let registry: Table = lua.named_registry_value(TRANSLATORS)?;
            let chain = registry.raw_get::<Option<AnyUserData>>(translator)?.ok_or_else(|| {
                mlua::Error::runtime("fail requires a load_catalog translator with at most 32 catalog fallbacks")
            })?;
            let chain = chain.borrow::<Chain>()?.clone();
            let args = arguments(lua, args).map_err(mlua::Error::external)?;
            let original = chain.text(&locale, &key, &args).map_err(mlua::Error::external)?;
            Err(mlua::Error::external(Failure {
                chain,
                key,
                args,
                original,
            }))
        },
    )
}

/// Rebuild only error wrappers. Preserve the original traceback and never call
/// back into Lua. Error context and traceback wrappers remain intact.
pub(crate) fn localized(error: &mlua::Error, locale: &LanguageIdentifier) -> mlua::Error {
    use mlua::Error;
    match error {
        Error::ExternalError(source) => {
            if let Some(failure) = source.downcast_ref::<Failure>() {
                let text = failure
                    .chain
                    .text(locale, &failure.key, &failure.args)
                    .unwrap_or_else(|_| failure.original.clone());
                Error::external(Rendered(text))
            } else {
                error.clone()
            }
        }
        Error::CallbackError { traceback, cause } => Error::CallbackError {
            traceback: traceback.clone(),
            cause: Arc::new(localized(cause, locale)),
        },
        Error::WithContext { context, cause } => Error::WithContext {
            context: context.clone(),
            cause: Arc::new(localized(cause, locale)),
        },
        Error::BadArgument { to, pos, name, cause } => Error::BadArgument {
            to: to.clone(),
            pos: *pos,
            name: name.clone(),
            cause: Arc::new(localized(cause, locale)),
        },
        _ => error.clone(),
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
struct Rendered(String);
