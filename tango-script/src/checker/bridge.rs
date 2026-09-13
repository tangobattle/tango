//! A synchronous C ABI: upstream owns its ASTs; Rust owns source and resolution.
use std::collections::BTreeSet;
use std::ffi::c_void;

use crate::{modules::Graph, Error, Result};

const CONTRACT_PREFIX: &str = "@tango-contract/";

#[repr(C)]
#[derive(Clone, Copy)]
struct Text {
    data: *const u8,
    len: usize,
}
impl Text {
    fn new(text: &str) -> Self {
        Self {
            data: text.as_ptr(),
            len: text.len(),
        }
    }
    fn missing() -> Self {
        Self {
            data: std::ptr::null(),
            len: 0,
        }
    }
    unsafe fn as_str<'a>(self) -> Option<&'a str> {
        if self.data.is_null() {
            return None;
        }
        // The bridge lends valid byte spans only for the synchronous callback.
        std::str::from_utf8(unsafe { std::slice::from_raw_parts(self.data, self.len) }).ok()
    }
}

#[repr(C)]
struct Source {
    name: Text,
    source: Text,
}

struct Context<'a> {
    graph: &'a Graph,
    errors: BTreeSet<String>,
}

unsafe extern "C" {
    fn tango_luau_initialize();
    fn tango_luau_globals_index() -> i32;
    fn tango_luau_check(
        sources: *const Source,
        len: usize,
        definitions: Text,
        context: *mut c_void,
        resolve: unsafe extern "C" fn(*mut c_void, Text, Text, u32, u32) -> Text,
        emit: unsafe extern "C" fn(*mut c_void, Text, u32, u32, Text),
    );
}

pub(crate) fn initialize() {
    // The upstream flag list is process-wide; call_once in the bridge ensures
    // initialization completes before any of our VMs or frontends can run.
    unsafe { tango_luau_initialize() }
}

unsafe extern "C" fn resolve(context: *mut c_void, from: Text, request: Text, line: u32, column: u32) -> Text {
    // Neither Rust panics nor C++ exceptions may cross the ABI.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &mut *context.cast::<Context<'_>>() };
        let (Some(from), Some(request)) = (unsafe { from.as_str() }, unsafe { request.as_str() }) else {
            return Text::missing();
        };
        let origin = contract_origin(from).map(|(_, name)| name).unwrap_or(from);
        match context.graph.resolve(origin, request) {
            Ok(name) => {
                // Return an interned graph key, never the temporary name.
                let (name, _) = context.graph.modules.get_key_value(&name).unwrap();
                Text::new(name)
            }
            Err(error) => {
                context
                    .errors
                    .insert(format!("{from}:{}:{}: {error}", line + 1, column + 1));
                Text::missing()
            }
        }
    }))
    .unwrap_or(Text::missing())
}

unsafe extern "C" fn emit(context: *mut c_void, name: Text, line: u32, column: u32, message: Text) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let context = unsafe { &mut *context.cast::<Context<'_>>() };
        let name = unsafe { name.as_str() }.unwrap_or("package");
        let message = unsafe { message.as_str() }.unwrap_or("invalid checker diagnostic");
        let (name, prefix, line, column) = match contract_origin(name) {
            Some((kind, name)) => (name, format!("{kind} contract: "), 0, 0),
            None => (name, String::new(), line, column),
        };
        context
            .errors
            .insert(format!("{name}:{}:{}: {prefix}{message}", line + 1, column + 1));
    }));
}

fn contract_origin(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix(CONTRACT_PREFIX)?.split_once('/')
}

pub(super) fn check(graph: &Graph) -> Result<()> {
    initialize();
    // Fail before running any type function if the linked VM's configuration
    // diverges from the headers used to compile Analysis.
    if unsafe { tango_luau_globals_index() } != mlua::ffi::LUA_GLOBALSINDEX {
        return Err(Error::Typecheck(
            "Luau Analysis and VM pseudo-indices do not match".into(),
        ));
    }
    let contracts: Vec<_> = graph
        .modules
        .iter()
        .flat_map(|(name, module)| {
            let path = format!("./{}", module.path.rsplit('/').next().unwrap());
            module.contracts.iter().map(move |kind| {
                (
                    format!("{CONTRACT_PREFIX}{}/{name}", kind.as_str()),
                    format!(
                        "--!strict\nlocal export: _TangoHost{} = require({path:?})\nreturn export\n",
                        kind.contract()
                    ),
                )
            })
        })
        .collect();
    let sources: Vec<_> = graph
        .modules
        .iter()
        .map(|(name, module)| Source {
            name: Text::new(name),
            source: Text::new(&module.source),
        })
        .chain(contracts.iter().map(|(name, source)| Source {
            name: Text::new(name),
            source: Text::new(source),
        }))
        .collect();
    let mut context = Context {
        graph,
        errors: BTreeSet::new(),
    };
    // All source spans and the context live until this call finishes. The C++
    // bridge copies borrowed strings and catches exceptions before returning.
    unsafe {
        tango_luau_check(
            sources.as_ptr(),
            sources.len(),
            Text::new(crate::SDK),
            (&mut context as *mut Context<'_>).cast(),
            resolve,
            emit,
        );
    }
    if context.errors.is_empty() {
        Ok(())
    } else {
        Err(Error::Typecheck(
            context.errors.into_iter().collect::<Vec<_>>().join("\n"),
        ))
    }
}
