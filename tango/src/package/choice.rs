//! Localized choices for independently exported package capabilities.
use tango_library::package::ExportRef;

#[derive(Clone)]
pub struct Choice {
    pub reference: ExportRef,
    pub profile: tango_script::Profile,
}

impl Choice {
    pub fn label(&self, locale: &str) -> String {
        self.profile
            .clone()
            .with_locale(locale)
            .and_then(|profile| {
                Ok(format!(
                    "{} · {} (v{})",
                    profile.display_name(),
                    profile.export_display_name(self.reference.kind, &self.reference.name)?,
                    self.reference.package.version
                ))
            })
            .unwrap_or_else(|_| self.reference.name.clone())
    }
}
