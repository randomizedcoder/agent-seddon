//! `agent-validate` — a concrete [`OutputSchema`] validator behind the seam in
//! `agent-core` (parity spec 16).
//!
//! [`Draft07Validator`] is a dependency-free validator for the **subset** of JSON
//! Schema draft-07 that matters for constraining a model completion: `type`
//! (incl. arrays + nested), `required`, `properties`, `additionalProperties:
//! false`, `enum`, `items`, and basic numeric/string/array bounds. It ships no
//! external jsonschema crate, so the default build stays hermetic under Nix.
//! Errors name the offending JSON path (`/outer/inner`), like pi's
//! `formatValidationPath`.

#[cfg(feature = "validate-draft07")]
mod draft07;
#[cfg(feature = "validate-draft07")]
#[doc(hidden)]
pub use draft07::bench_validate;
#[cfg(feature = "validate-draft07")]
pub use draft07::Draft07Validator;
