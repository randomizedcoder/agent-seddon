//! Fixture source file for the agent-search index tests.
//!
//! Planted tokens the table-driven tests assert on:
//!   * `FINDME_LITERAL`      — literal-mode target
//!   * "the quick brown fox" — phrase-mode target
//!   * `searchable`          — fuzzy-mode target (queried as `serchable`)

pub const FINDME_LITERAL: u32 = 42;

/// the quick brown fox jumps over the lazy dog
pub fn searchable_helper() -> u32 {
    FINDME_LITERAL
}
