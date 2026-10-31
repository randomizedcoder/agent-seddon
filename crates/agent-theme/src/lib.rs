//! Portal themes as data (docs/design/visual/): the `agent.v1.Theme` validator,
//! colour and WCAG contrast maths, textproto/JSON encodings, and the built-ins.
//!
//! A theme is written by one user and rendered in another's session, so it is
//! untrusted input. [`validate`] is the gate: strict hex colours, bundled fonts
//! only, numbers rejected (not clamped) when out of range, size caps, and every
//! written-down text pair meeting the theme's declared contrast level.

mod builtin;
pub mod color;
mod error;
pub mod textproto;
mod validate;

pub use builtin::{builtins, BUILTIN_SOURCES, FALLBACK_THEME};
pub use color::{contrast_ratio, parse_color};
pub use error::{ThemeError, ThemeErrorKind};
pub use validate::{
    check_font, validate, EDGE_KINDS, FONT_FAMILIES, MAX_DESCRIPTION_CHARS, MAX_MAP_ENTRIES,
    MAX_NAME_CHARS, MAX_THEME_BYTES, NAV_TABS,
};
