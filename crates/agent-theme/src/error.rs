//! The theme error vocabulary (docs/design/visual/06-security-and-validation.md).

use std::fmt;

/// Which rule a theme broke. Tests match on the kind; people read the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeErrorKind {
    /// The document is over [`crate::MAX_THEME_BYTES`].
    TooLarge,
    /// The textproto does not parse as an `agent.v1.Theme`.
    Parse,
    /// The id is not a safe path segment.
    BadId,
    /// A name, author or description is empty, too long, or has control or bidi characters.
    BadName,
    /// A colour is not `#RRGGBB` / `#RRGGBBAA`.
    BadColor,
    /// A required value is absent.
    Missing,
    /// A map key is not in its allowlist, or the map is too big.
    UnknownKey,
    /// A font family is not bundled with the portal.
    UnknownFont,
    /// A number or enum is outside its range.
    OutOfRange,
    /// A colour pair is below the theme's contrast level.
    Contrast,
}

impl ThemeErrorKind {
    /// The kind's name, as used in the shared case table.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TooLarge => "TooLarge",
            Self::Parse => "Parse",
            Self::BadId => "BadId",
            Self::BadName => "BadName",
            Self::BadColor => "BadColor",
            Self::Missing => "Missing",
            Self::UnknownKey => "UnknownKey",
            Self::UnknownFont => "UnknownFont",
            Self::OutOfRange => "OutOfRange",
            Self::Contrast => "Contrast",
        }
    }
}

/// A rejected theme: the rule ([`ThemeErrorKind`]) plus a message naming the field.
#[derive(Debug, Clone, PartialEq)]
pub struct ThemeError {
    kind: ThemeErrorKind,
    message: String,
}

impl ThemeError {
    /// A new error of `kind`.
    pub fn new(kind: ThemeErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The rule that failed.
    pub fn kind(&self) -> ThemeErrorKind {
        self.kind
    }

    /// Prefix the message with the field path, e.g. `light.primary: …`.
    pub(crate) fn at(mut self, field: &str) -> Self {
        self.message = format!("{field}: {}", self.message);
        self
    }
}

impl fmt::Display for ThemeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "theme {}: {}", self.kind.as_str(), self.message)
    }
}

impl std::error::Error for ThemeError {}

impl From<ThemeError> for agent_core::Error {
    fn from(e: ThemeError) -> Self {
        agent_core::Error::Config(e.to_string())
    }
}
