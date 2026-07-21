//! `agent-tokenizer` — concrete [`Tokenizer`] backends behind the seam in
//! `agent-core`, plus the price table for the cost model.
//!
//! The default backend, [`ApproxTokenizer`], is a dependency-free, deterministic,
//! Unicode-aware segmenter: a real improvement over the crate-private `~chars/4`
//! heuristic in `agent-context` (which counts *bytes* and is model-agnostic),
//! while shipping no BPE vocab and needing no network — so the default build stays
//! hermetic under Nix. Higher-fidelity backends (`tiktoken` BPE, HuggingFace
//! `tokenizers`, a provider count-tokens endpoint) drop in behind cargo features
//! against the same seam; they are reserved for a follow-up.
//!
//! The cost math itself lives in `agent-core` (`calculate_cost`, `Cost`,
//! `ModelPrices`) so every crate shares one definition; this crate owns the
//! concrete [`PriceTable`] that supplies the rates. See parity spec 23.

#[cfg(feature = "tokenizer-approx")]
mod approx;
#[cfg(feature = "tokenizer-approx")]
pub use approx::ApproxTokenizer;

pub mod cost;
pub use cost::PriceTable;
