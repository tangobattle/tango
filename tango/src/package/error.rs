//! Format package failures at presentation time, retaining structured causes.
pub(super) fn localized(error: &anyhow::Error, locale: &unic_langid::LanguageIdentifier) -> String {
    let mut context = Vec::new();
    for cause in error.chain() {
        if let Some(script) = cause.downcast_ref::<tango_script::Error>() {
            context.push(script.localized(locale));
            return context.join(": ");
        }
        context.push(cause.to_string());
    }
    format!("{error:#}")
}
