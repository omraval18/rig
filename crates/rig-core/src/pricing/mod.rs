//! What a request costs, from list prices. An estimate: discounts, plans, and tiers that depend on
//! the request's size are not modelled.

mod table;

use crate::provider::Usage;

/// USD per million tokens.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_write: f64,
}

/// The list price of `model` on rig provider `provider`, if we have one.
pub fn lookup(provider: &str, model: &str) -> Option<Price> {
    table::PRICES
        .iter()
        .find(|(p, m, _)| *p == provider && *m == model)
        .map(|(_, _, [input, output, cache_read, cache_write])| Price {
            input: *input,
            output: *output,
            cache_read: *cache_read,
            cache_write: *cache_write,
        })
}

impl Price {
    /// Cost of one request in USD. `usage.input` counts the cached and written tokens too.
    pub fn cost(&self, u: &Usage) -> f64 {
        let plain = u.input.saturating_sub(u.cached + u.cache_write);
        (plain as f64 * self.input
            + u.cached as f64 * self.cache_read
            + u.cache_write as f64 * self.cache_write
            + u.output as f64 * self.output)
            / 1_000_000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_known_model_is_priced_and_an_unknown_one_is_not() {
        let p = lookup("anthropic", "claude-opus-4-7").expect("priced");
        assert_eq!((p.input, p.output), (5.0, 25.0));
        assert!(lookup("anthropic", "no-such-model").is_none());
        assert!(lookup("chatgpt", "gpt-6-sol").is_none());
    }

    #[test]
    fn cached_and_written_tokens_are_billed_at_their_own_rates() {
        let p = lookup("anthropic", "claude-opus-4-7").expect("priced");
        // 1M prompt tokens: 400k plain, 500k read from cache, 100k written; 100k output.
        let u = Usage {
            input: 1_000_000,
            cached: 500_000,
            cache_write: 100_000,
            output: 100_000,
        };
        let expected = 0.4 * 5.0 + 0.5 * 0.5 + 0.1 * 6.25 + 0.1 * 25.0;
        assert!((p.cost(&u) - expected).abs() < 1e-9, "{}", p.cost(&u));
    }
}
