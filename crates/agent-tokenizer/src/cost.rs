//! The concrete per-model [`PriceTable`] backing the cost model.
//!
//! The cost *math* is in `agent-core` (`calculate_cost`); this is just the data
//! source it consults. A table is keyed by model id and resolves an exact match
//! first, then the **longest family prefix** (so `claude-3-5-sonnet-20241022`
//! resolves to the `claude-3-5-sonnet` row). A miss returns `None`, and
//! `calculate_cost` turns that into a zero-priced [`agent_core::CostStatus::Estimated`]
//! result rather than a wrong bill.
//!
//! `builtin()` ships a small, illustrative set of current public rates ($/MTok).
//! The real deployment loads rates from config; the numbers here are a sane
//! default, not a pricing source of truth.

use agent_core::{ModelPrices, Prices};
use std::collections::BTreeMap;

/// A model → [`ModelPrices`] map with exact-then-longest-prefix lookup.
#[derive(Debug, Clone, Default)]
pub struct PriceTable {
    rows: BTreeMap<String, ModelPrices>,
}

impl PriceTable {
    /// An empty table (every model falls back to `Estimated`/zero-priced).
    pub fn empty() -> Self {
        Self::default()
    }

    /// Build from an iterator of `(model_id, prices)` rows.
    pub fn from_rows(rows: impl IntoIterator<Item = (String, ModelPrices)>) -> Self {
        Self {
            rows: rows.into_iter().collect(),
        }
    }

    /// Insert/overwrite one model's rates.
    pub fn insert(&mut self, model: impl Into<String>, prices: ModelPrices) {
        self.rows.insert(model.into(), prices);
    }

    /// A small default table of current public rates ($/MTok). Illustrative — the
    /// deployment overrides these from config.
    pub fn builtin() -> Self {
        let p = |input, output, cache_read, cache_write| ModelPrices {
            input,
            output,
            cache_read,
            cache_write,
        };
        Self::from_rows([
            // Anthropic bills cache-read at 0.1× input and cache-write at 1.25×.
            ("claude-3-5-sonnet".into(), p(3.0, 15.0, 0.3, 3.75)),
            ("claude-3-5-haiku".into(), p(0.8, 4.0, 0.08, 1.0)),
            ("claude-3-opus".into(), p(15.0, 75.0, 1.5, 18.75)),
            // OpenAI reuses the prompt cache at 0.5× input; writes are billed as
            // normal input (no separate premium), so cache_write = 0.
            ("gpt-4o".into(), p(2.5, 10.0, 1.25, 0.0)),
            ("gpt-4o-mini".into(), p(0.15, 0.6, 0.075, 0.0)),
        ])
    }
}

impl Prices for PriceTable {
    fn get(&self, model: &str) -> Option<ModelPrices> {
        if let Some(p) = self.rows.get(model) {
            return Some(*p);
        }
        // Longest-prefix family match (e.g. dated model ids → the family row).
        self.rows
            .iter()
            .filter(|(k, _)| model.starts_with(k.as_str()))
            .max_by_key(|(k, _)| k.len())
            .map(|(_, p)| *p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_core::{calculate_cost, Cost, CostStatus, Usage};
    use agent_testkit::StaticPrices;
    use rstest::rstest;

    fn usage(input: u32, output: u32, cache_read: u32, cache_write: u32) -> Usage {
        Usage {
            prompt_tokens: input,
            completion_tokens: output,
            total_tokens: input + output,
            cache_read_tokens: cache_read,
            cache_write_tokens: cache_write,
            cost: None,
        }
    }

    fn approx_eq(a: &Cost, b: &Cost) {
        let close = |x: f64, y: f64| (x - y).abs() < 1e-9;
        assert!(
            close(a.input, b.input)
                && close(a.output, b.output)
                && close(a.cache_read, b.cache_read)
                && close(a.cache_write, b.cache_write)
                && close(a.total, b.total),
            "cost mismatch: got {a:?}, want {b:?}"
        );
    }

    // Prices in $/MTok: input=3.0, output=15.0, cache_read=0.3, cache_write=3.75.
    // `positive_` a normal bill, `corner_` a single cache line, `boundary_` zero.
    #[rstest]
    #[case::positive_input_output_cost(
        usage(1_000_000, 1_000_000, 0, 0),
        Cost { input: 3.0, output: 15.0, cache_read: 0.0, cache_write: 0.0, total: 18.0 })]
    #[case::corner_cache_read_discounted(
        usage(0, 0, 1_000_000, 0),
        Cost { cache_read: 0.3, total: 0.3, ..Cost::default() })]
    #[case::corner_cache_write_premium(
        usage(0, 0, 0, 1_000_000),
        Cost { cache_write: 3.75, total: 3.75, ..Cost::default() })]
    #[case::boundary_zero_usage(usage(0, 0, 0, 0), Cost::default())]
    fn cost_cases(#[case] u: Usage, #[case] expected: Cost) {
        let prices = StaticPrices::one("model", 3.0, 15.0, 0.3, 3.75);
        let (cost, status) = calculate_cost("model", &u, &prices);
        approx_eq(&cost, &expected);
        assert_eq!(status, CostStatus::Actual);
    }

    #[test]
    fn negative_unknown_model_zero_priced_estimated() {
        let prices = StaticPrices::one("model", 3.0, 15.0, 0.3, 3.75);
        let (cost, status) = calculate_cost("no-such-model", &usage(1_000_000, 0, 0, 0), &prices);
        approx_eq(&cost, &Cost::default());
        assert_eq!(status, CostStatus::Estimated);
    }

    // A price row is config/provider data and could be malformed or hostile; the
    // cost must stay finite and non-negative (so `total` isn't poisoned by `NaN` and
    // the Prometheus counter never sees a NaN/negative that would panic `inc_by`).
    // `expect_zero` ⇒ a bad rate yields 0 cost; else a valid rate on max tokens is >0.
    #[rstest]
    #[case::boundary_u32_max_tokens(u32::MAX, 3.0, false)]
    #[case::adversarial_nan_rate(1_000_000, f64::NAN, true)]
    #[case::adversarial_negative_rate(1_000_000, -5.0, true)]
    #[case::adversarial_inf_rate(1_000_000, f64::INFINITY, true)]
    #[case::boundary_zero_rate(1_000_000, 0.0, true)]
    fn hostile_or_extreme_input_stays_finite(
        #[case] tokens: u32,
        #[case] rate: f64,
        #[case] expect_zero: bool,
    ) {
        let prices = StaticPrices::one("model", rate, 0.0, 0.0, 0.0);
        let (cost, _status) = calculate_cost("model", &usage(tokens, 0, 0, 0), &prices);
        assert!(
            cost.total.is_finite() && cost.total >= 0.0,
            "total must stay finite & non-negative, got {}",
            cost.total
        );
        if expect_zero {
            assert_eq!(cost.input, 0.0, "a bad/zero rate must yield 0 cost");
        } else {
            assert!(cost.input > 0.0, "a valid rate on max tokens should be > 0");
        }
    }

    // --- PriceTable resolution: exact + longest-prefix family match ---------
    #[rstest]
    #[case::exact("gpt-4o", true)]
    #[case::family_prefix("claude-3-5-sonnet-20241022", true)]
    #[case::unknown("mistral-large", false)]
    fn price_table_lookup(#[case] model: &str, #[case] found: bool) {
        let t = PriceTable::builtin();
        assert_eq!(t.get(model).is_some(), found, "model={model}");
    }

    #[test]
    fn longest_prefix_wins() {
        let mut t = PriceTable::empty();
        t.insert("claude", ModelPrices::ZERO);
        let sonnet = ModelPrices {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write: 3.75,
        };
        t.insert("claude-3-5-sonnet", sonnet);
        // The dated id must resolve to the more specific `claude-3-5-sonnet` row.
        assert_eq!(t.get("claude-3-5-sonnet-20241022"), Some(sonnet));
    }
}
