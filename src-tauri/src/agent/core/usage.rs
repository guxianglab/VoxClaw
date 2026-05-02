use serde::{Deserialize, Serialize};

/// Token usage and cost reported by an LLM provider for a single response.
///
/// Mirrors the shape used by pi-mono's `Usage` so the UI / session log can
/// display the same numbers regardless of provider.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Prompt tokens (input).
    #[serde(default)]
    pub input: u64,
    /// Completion tokens (output).
    #[serde(default)]
    pub output: u64,
    /// Tokens served from provider-side prompt cache.
    #[serde(default)]
    pub cache_read: u64,
    /// Tokens written into the provider-side prompt cache.
    #[serde(default)]
    pub cache_write: u64,
    /// Native total tokens reported by the provider, when available.
    /// Falls back to `input + output + cache_read + cache_write`.
    #[serde(default)]
    pub total: u64,

    /// Costs in USD. All zero by default; computed via `compute_cost` once
    /// model pricing metadata is available (Phase 3).
    #[serde(default)]
    pub cost_input: f64,
    #[serde(default)]
    pub cost_output: f64,
    #[serde(default)]
    pub cost_cache_read: f64,
    #[serde(default)]
    pub cost_cache_write: f64,
    #[serde(default)]
    pub cost_total: f64,
}

/// Pricing metadata, expressed as USD per 1 million tokens.
///
/// Optional fields default to zero so providers without cache pricing
/// (e.g. open-source models) only contribute input/output costs.
#[derive(Debug, Clone, Default, Copy)]
pub struct ModelPricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_read_per_mtok: f64,
    pub cache_write_per_mtok: f64,
}

impl Usage {
    /// Sum of all token kinds. Uses `total` if non-zero, otherwise computes.
    pub fn effective_total(&self) -> u64 {
        if self.total > 0 {
            self.total
        } else {
            self.input + self.output + self.cache_read + self.cache_write
        }
    }

    /// Compute and store per-bucket and total cost given pricing.
    pub fn compute_cost(&mut self, pricing: ModelPricing) {
        let scale = 1_000_000.0;
        self.cost_input = self.input as f64 * pricing.input_per_mtok / scale;
        self.cost_output = self.output as f64 * pricing.output_per_mtok / scale;
        self.cost_cache_read = self.cache_read as f64 * pricing.cache_read_per_mtok / scale;
        self.cost_cache_write = self.cache_write as f64 * pricing.cache_write_per_mtok / scale;
        self.cost_total =
            self.cost_input + self.cost_output + self.cost_cache_read + self.cost_cache_write;
    }

    /// Add another usage entry into this one. Costs are added directly;
    /// callers must ensure both sides used the same pricing.
    pub fn add(&mut self, other: &Usage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        // Recompute total from components to stay consistent.
        self.total = self.effective_total() + other.effective_total();

        self.cost_input += other.cost_input;
        self.cost_output += other.cost_output;
        self.cost_cache_read += other.cost_cache_read;
        self.cost_cache_write += other.cost_cache_write;
        self.cost_total += other.cost_total;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_total_uses_native_when_present() {
        let u = Usage {
            input: 10,
            output: 20,
            total: 99,
            ..Default::default()
        };
        assert_eq!(u.effective_total(), 99);
    }

    #[test]
    fn effective_total_falls_back_to_sum() {
        let u = Usage {
            input: 10,
            output: 20,
            cache_read: 3,
            cache_write: 4,
            ..Default::default()
        };
        assert_eq!(u.effective_total(), 37);
    }

    #[test]
    fn compute_cost_basic() {
        let mut u = Usage {
            input: 1_000_000,
            output: 500_000,
            ..Default::default()
        };
        let pricing = ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            ..Default::default()
        };
        u.compute_cost(pricing);
        assert!((u.cost_input - 3.0).abs() < 1e-9);
        assert!((u.cost_output - 7.5).abs() < 1e-9);
        assert!((u.cost_total - 10.5).abs() < 1e-9);
        assert_eq!(u.cost_cache_read, 0.0);
    }

    #[test]
    fn compute_cost_with_cache() {
        let mut u = Usage {
            input: 200_000,
            output: 100_000,
            cache_read: 800_000,
            cache_write: 50_000,
            ..Default::default()
        };
        let pricing = ModelPricing {
            input_per_mtok: 3.0,
            output_per_mtok: 15.0,
            cache_read_per_mtok: 0.30,
            cache_write_per_mtok: 3.75,
        };
        u.compute_cost(pricing);
        // 200k * 3 / 1M = 0.6
        // 100k * 15 / 1M = 1.5
        // 800k * 0.3 / 1M = 0.24
        // 50k * 3.75 / 1M = 0.1875
        assert!((u.cost_input - 0.6).abs() < 1e-9);
        assert!((u.cost_output - 1.5).abs() < 1e-9);
        assert!((u.cost_cache_read - 0.24).abs() < 1e-9);
        assert!((u.cost_cache_write - 0.1875).abs() < 1e-9);
        assert!((u.cost_total - 2.5275).abs() < 1e-9);
    }

    #[test]
    fn add_accumulates_tokens_and_costs() {
        let mut a = Usage {
            input: 100,
            output: 50,
            cost_input: 0.1,
            cost_output: 0.5,
            cost_total: 0.6,
            ..Default::default()
        };
        let b = Usage {
            input: 200,
            output: 60,
            cost_input: 0.2,
            cost_output: 0.6,
            cost_total: 0.8,
            ..Default::default()
        };
        a.add(&b);
        assert_eq!(a.input, 300);
        assert_eq!(a.output, 110);
        assert!((a.cost_total - 1.4).abs() < 1e-9);
    }

    #[test]
    fn serializes_with_snake_case_fields() {
        let u = Usage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            total: 10,
            ..Default::default()
        };
        let json = serde_json::to_string(&u).unwrap();
        assert!(json.contains(r#""input":1"#));
        assert!(json.contains(r#""cache_read":3"#));
        assert!(json.contains(r#""cost_total":0.0"#));
    }
}
