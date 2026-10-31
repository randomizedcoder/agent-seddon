//! The built-in themes, compiled in from `config/themes/*.textproto`.

use agent_proto::pb;

use crate::{textproto, validate, ThemeError};

/// `(id, textproto)` for every built-in, in picker order.
pub const BUILTIN_SOURCES: &[(&str, &str)] = &[
    (
        "prototype",
        include_str!("../../../config/themes/prototype.textproto"),
    ),
    (
        "tn3270",
        include_str!("../../../config/themes/tn3270.textproto"),
    ),
    (
        "windows",
        include_str!("../../../config/themes/windows.textproto"),
    ),
    (
        "macos",
        include_str!("../../../config/themes/macos.textproto"),
    ),
    (
        "accessible",
        include_str!("../../../config/themes/accessible.textproto"),
    ),
];

/// The id of the theme used when nothing else resolves. It cannot be deleted.
pub const FALLBACK_THEME: &str = "prototype";

/// Parse and validate every built-in, marking each `builtin = true`.
///
/// An error here means a shipped file is broken; the crate's tests make that a
/// build failure, so at runtime this only fails if the binary was built without them.
pub fn builtins() -> Result<Vec<pb::Theme>, ThemeError> {
    BUILTIN_SOURCES
        .iter()
        .map(|(id, text)| {
            let mut theme = textproto::parse(text).map_err(|e| e.at(id))?;
            validate(&theme).map_err(|e| e.at(id))?;
            theme.builtin = true;
            Ok(theme)
        })
        .collect()
}
