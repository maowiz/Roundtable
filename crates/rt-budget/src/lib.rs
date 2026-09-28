//! Budget engine (backend plan section 9; PRD BUD-01…07).
//!
//! - Prices come from a versioned catalog frozen into the run. Unverified prices block dispatch.
//! - Token estimates are conservative and tokenizer-free.
//! - Every reservation is a maximum; settlement uses provider usage; unknown stays unresolved.
//! - All arithmetic is integer micro-USD.

use rt_core::MicroUsd;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const CATALOG_JSON: &str = include_str!("../../../contracts/prices/catalog.json");

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BudgetError {
    #[error("no price record for {provider}/{model}")]
    NoPrice { provider: String, model: String },
    #[error("price for {provider}/{model} is not verified; confirm it before paid dispatch")]
    Unverified { provider: String, model: String },
    #[error("reservation of {requested} would exceed the ceiling {ceiling} (committed {committed})")]
    OverCeiling { requested: MicroUsd, ceiling: MicroUsd, committed: MicroUsd },
    #[error("unknown reservation")]
    UnknownReservation,
    #[error("catalog parse error: {0}")]
    Catalog(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceRecord {
    pub provider: String,
    pub model: String,
    pub input_per_m: MicroUsd,
    pub output_per_m: MicroUsd,
    pub reasoning_per_m: Option<MicroUsd>,
    pub cached_input_per_m: Option<MicroUsd>,
    pub verified: bool,
    pub source_url: String,
    pub checked_on: Option<String>,
    pub catalog_version: String,
    /// Whether the provider enforces a JSON schema for this model; false means JSON mode plus native validation.
    #[serde(default = "default_true")]
    pub structured_output: bool,
}

fn default_true() -> bool {
    true
}

impl PriceRecord {
    pub fn key(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub version: String,
    records: HashMap<String, PriceRecord>,
}

impl Catalog {
    pub fn bundled() -> Result<Catalog, BudgetError> {
        Catalog::parse(CATALOG_JSON)
    }

    pub fn parse(json: &str) -> Result<Catalog, BudgetError> {
        let v: serde_json::Value = serde_json::from_str(json).map_err(|e| BudgetError::Catalog(e.to_string()))?;
        let version = v["catalog_version"].as_str().ok_or_else(|| BudgetError::Catalog("catalog_version missing".into()))?.to_string();
        let money = |x: &serde_json::Value| -> Result<Option<MicroUsd>, BudgetError> {
            match x.as_str() {
                None => Ok(None),
                Some(s) => MicroUsd::parse_dollars(s).map(Some).ok_or_else(|| BudgetError::Catalog(format!("bad price {s}"))),
            }
        };
        let mut records = HashMap::new();
        for r in v["records"].as_array().ok_or_else(|| BudgetError::Catalog("records missing".into()))? {
            let rec = PriceRecord {
                provider: r["provider"].as_str().unwrap_or_default().to_string(),
                model: r["model"].as_str().unwrap_or_default().to_string(),
                input_per_m: money(&r["input"])?.ok_or_else(|| BudgetError::Catalog("input price missing".into()))?,
                output_per_m: money(&r["output"])?.ok_or_else(|| BudgetError::Catalog("output price missing".into()))?,
                reasoning_per_m: money(&r["reasoning"])?,
                cached_input_per_m: money(&r["cached_input"])?,
                verified: r["verified"].as_bool().unwrap_or(false),
                source_url: r["source_url"].as_str().unwrap_or_default().to_string(),
                checked_on: r["checked_on"].as_str().map(str::to_string),
                catalog_version: version.clone(),
                structured_output: r["structured_output"].as_bool().unwrap_or(true),
            };
            records.insert(rec.key(), rec);
        }
        Ok(Catalog { version, records })
    }

    pub fn get(&self, provider: &str, model: &str) -> Option<&PriceRecord> {
        self.records.get(&format!("{provider}/{model}"))
    }

    /// Price usable for paid dispatch: must exist and be verified (BUD-04).
    pub fn for_dispatch(&self, provider: &str, model: &str) -> Result<&PriceRecord, BudgetError> {
        let rec = self.get(provider, model).ok_or_else(|| BudgetError::NoPrice { provider: provider.into(), model: model.into() })?;
        if !rec.verified {
            return Err(BudgetError::Unverified { provider: provider.into(), model: model.into() });
        }
        Ok(rec)
    }

    pub fn records(&self) -> impl Iterator<Item = &PriceRecord> {
        self.records.values()
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

/// Conservative token estimate: about 3.2 characters per token, plus 10 percent, plus a fixed overhead.
pub fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as f64;
    ((chars / 3.2) * 1.1).ceil() as u64 + 8
}

/// Maximum cost of one attempt given known input and allowed output bounds (BUD-04).
/// OpenRouter's Exa web search: a flat fee per request that used it (10 results included), on top of
/// token cost. Verified against openrouter.ai/docs/features/web-search on 2026-09-12.
pub const WEB_SEARCH_FEE: MicroUsd = MicroUsd(7_000);

pub fn max_attempt_cost(price: &PriceRecord, input_tokens: u64, output_bound: u64, reasoning_bound: u64) -> MicroUsd {
    let input = MicroUsd::for_tokens(price.input_per_m, input_tokens);
    let output = MicroUsd::for_tokens(price.output_per_m, output_bound);
    let reasoning = match price.reasoning_per_m {
        Some(p) => MicroUsd::for_tokens(p, reasoning_bound),
        None => MicroUsd::for_tokens(price.output_per_m, reasoning_bound),
    };
    input.saturating_add(output).saturating_add(reasoning)
}

/// Provider usage after a completed attempt. `None` fields mean the provider did not report them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// What the provider says it charged for this call, in micro-USD (OpenRouter reports this on every
    /// response). When present it is the settled cost; token prices are only the fallback.
    #[serde(default)]
    pub provider_cost_micro: Option<u64>,
    pub input_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// Actual cost from usage, or None when any billable component is unknown (BUD-02).
pub fn settle_cost(price: &PriceRecord, usage: &Usage) -> Option<MicroUsd> {
    if let Some(c) = usage.provider_cost_micro {
        return Some(MicroUsd(c as i64));
    }
    let input = usage.input_tokens?;
    let output = usage.output_tokens?;
    let cached = usage.cached_input_tokens.unwrap_or(0);
    let uncached = input.saturating_sub(cached);
    let mut total = MicroUsd::for_tokens(price.input_per_m, uncached);
    if cached > 0 {
        total += MicroUsd::for_tokens(price.cached_input_per_m.unwrap_or(price.input_per_m), cached);
    }
    total += MicroUsd::for_tokens(price.output_per_m, output);
    if let Some(r) = usage.reasoning_tokens {
        total += MicroUsd::for_tokens(price.reasoning_per_m.unwrap_or(price.output_per_m), r);
    }
    Some(total)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReservationId(pub u64);

/// In-memory accounting for one run. The ledger persists every movement as budget_entries;
/// this struct is the arithmetic the engine consults before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunBudget {
    pub ceiling: MicroUsd,
    pub settled: MicroUsd,
    pub unresolved: MicroUsd,
    reservations: HashMap<ReservationId, MicroUsd>,
    next_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSnapshot {
    pub ceiling: MicroUsd,
    pub settled: MicroUsd,
    pub reserved: MicroUsd,
    pub unresolved: MicroUsd,
    pub committed: MicroUsd,
    pub remaining: MicroUsd,
}

impl RunBudget {
    pub fn new(ceiling: MicroUsd) -> Self {
        RunBudget { ceiling, settled: MicroUsd::ZERO, unresolved: MicroUsd::ZERO, reservations: HashMap::new(), next_id: 1 }
    }

    pub fn reserved(&self) -> MicroUsd {
        self.reservations.values().fold(MicroUsd::ZERO, |a, b| a + *b)
    }

    /// Settled + unresolved + reserved.
    pub fn committed(&self) -> MicroUsd {
        self.settled + self.unresolved + self.reserved()
    }

    pub fn snapshot(&self) -> BudgetSnapshot {
        let committed = self.committed();
        BudgetSnapshot { ceiling: self.ceiling, settled: self.settled, reserved: self.reserved(), unresolved: self.unresolved, committed, remaining: self.ceiling - committed }
    }

    /// Reserve a maximum before dispatch. Fails without changing anything when over the ceiling.
    pub fn try_reserve(&mut self, max: MicroUsd) -> Result<ReservationId, BudgetError> {
        let committed = self.committed();
        if committed + max > self.ceiling {
            return Err(BudgetError::OverCeiling { requested: max, ceiling: self.ceiling, committed });
        }
        let id = ReservationId(self.next_id);
        self.next_id += 1;
        self.reservations.insert(id, max);
        Ok(id)
    }

    /// Settle a reservation with the actual cost (may be below or, rarely, above the reservation).
    pub fn settle(&mut self, id: ReservationId, actual: MicroUsd) -> Result<(), BudgetError> {
        self.reservations.remove(&id).ok_or(BudgetError::UnknownReservation)?;
        self.settled += actual;
        Ok(())
    }

    /// Release a reservation that never produced a charge (failed before send, non-retryable rejection).
    pub fn release(&mut self, id: ReservationId) -> Result<(), BudgetError> {
        self.reservations.remove(&id).ok_or(BudgetError::UnknownReservation)?;
        Ok(())
    }

    /// Keep the maximum as an unresolved liability (outcome unknown, usage unknown).
    pub fn mark_unresolved(&mut self, id: ReservationId) -> Result<(), BudgetError> {
        let max = self.reservations.remove(&id).ok_or(BudgetError::UnknownReservation)?;
        self.unresolved += max;
        Ok(())
    }

    /// The user confirmed the provider's bill for an unresolved attempt (or that no charge occurred).
    pub fn resolve(&mut self, previously_unresolved: MicroUsd, actual: MicroUsd) {
        self.unresolved = self.unresolved - previously_unresolved;
        self.settled += actual;
    }

    /// A user-authorised ceiling extension (BUD-06). Never lowers below committed.
    pub fn extend_ceiling(&mut self, new_ceiling: MicroUsd) -> bool {
        if new_ceiling < self.committed() {
            return false;
        }
        self.ceiling = new_ceiling;
        true
    }
}

/// Preflight estimate for a whole run (BUD-01).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEstimate {
    pub base_calls: usize,
    pub max_attempts: usize,
    pub low: MicroUsd,
    pub high: MicroUsd,
    pub unverified_prices: Vec<String>,
}

pub struct SeatPricing<'a> {
    pub price: &'a PriceRecord,
    pub output_bound: u64,
    pub reasoning_bound: u64,
}

/// Estimate a run: `low` assumes one attempt per call with output at a third of the bound;
/// `high` assumes the attempt cap with full output bounds. Input grows through the stages
/// because reviews and improvements resend earlier text (BUD-01).
pub fn estimate_run(seats: &[SeatPricing<'_>], synthesizer: &SeatPricing<'_>, question_tokens: u64, reference_tokens: u64) -> RunEstimate {
    let n = seats.len() as u64;
    let base_in = question_tokens + reference_tokens + 600; // instructions overhead
    let mut low = MicroUsd::ZERO;
    let mut high = MicroUsd::ZERO;
    let mut unverified = Vec::new();
    let avg_out: u64 = seats.iter().map(|s| s.output_bound).sum::<u64>() / n.max(1);
    for s in seats {
        if !s.price.verified {
            unverified.push(s.price.key());
        }
        // answer
        low += max_attempt_cost(s.price, base_in, s.output_bound / 3, s.reasoning_bound / 3);
        high += max_attempt_cost(s.price, base_in, s.output_bound, s.reasoning_bound);
        // reviews written by this seat: n-1 calls, each with one answer as input
        let review_in = base_in + avg_out;
        low += MicroUsd(max_attempt_cost(s.price, review_in, s.output_bound / 3, s.reasoning_bound / 3).0 * (n as i64 - 1));
        high += MicroUsd(max_attempt_cost(s.price, review_in, s.output_bound, s.reasoning_bound).0 * (n as i64 - 1));
        // improve: own answer + n-1 reviews
        let improve_in = base_in + avg_out + (n - 1) * (avg_out / 2);
        low += max_attempt_cost(s.price, improve_in, s.output_bound / 3, s.reasoning_bound / 3);
        high += max_attempt_cost(s.price, improve_in, s.output_bound, s.reasoning_bound);
    }
    if !synthesizer.price.verified && !unverified.contains(&synthesizer.price.key()) {
        unverified.push(synthesizer.price.key());
    }
    let synth_in = base_in + n * avg_out;
    low += max_attempt_cost(synthesizer.price, synth_in, synthesizer.output_bound / 3, synthesizer.reasoning_bound / 3);
    high += max_attempt_cost(synthesizer.price, synth_in, synthesizer.output_bound, synthesizer.reasoning_bound);
    let base_calls = rt_core::stage::base_calls(n as usize);
    RunEstimate { base_calls, max_attempts: base_calls * rt_core::bounds::MAX_ATTEMPTS_PER_CALL as usize, low, high: MicroUsd(high.0 * rt_core::bounds::MAX_ATTEMPTS_PER_CALL as i64), unverified_prices: unverified }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn price(verified: bool) -> PriceRecord {
        PriceRecord { provider: "p".into(), model: "m".into(), input_per_m: MicroUsd::parse_dollars("1.00").unwrap(), output_per_m: MicroUsd::parse_dollars("10.00").unwrap(), reasoning_per_m: None, cached_input_per_m: Some(MicroUsd::parse_dollars("0.10").unwrap()), verified, source_url: String::new(), checked_on: None, catalog_version: "t".into(), structured_output: true }
    }

    #[test]
    fn bundled_catalog_parses_and_blocks_unverified_dispatch() {
        let c = Catalog::bundled().unwrap();
        assert!(c.len() >= 6);
        let r = c.get("openai", "gpt-5.6").unwrap();
        assert!(!r.verified);
        assert!(matches!(c.for_dispatch("openai", "gpt-5.6"), Err(BudgetError::Unverified { .. })));
        let or = c.for_dispatch("openrouter", "nex-agi/nex-n2.5-mini:free").unwrap();
        assert_eq!(or.input_per_m, MicroUsd::ZERO);
        assert!(!c.get("openrouter", "qwen/qwen3.7-flash").unwrap().structured_output);
        assert!(matches!(c.for_dispatch("nobody", "x"), Err(BudgetError::NoPrice { .. })));
    }

    #[test]
    fn token_estimate_is_conservative() {
        assert_eq!(estimate_tokens(""), 8);
        let t = estimate_tokens(&"word ".repeat(100)); // 500 chars
        assert!((172..=180).contains(&t), "{t}");
    }

    #[test]
    fn max_cost_and_settlement() {
        let p = price(true);
        let max = max_attempt_cost(&p, 1_000, 2_000, 0);
        assert_eq!(max, MicroUsd(1_000 + 20_000));
        let actual = settle_cost(&p, &Usage { provider_cost_micro: None, input_tokens: Some(1_000), cached_input_tokens: Some(500), output_tokens: Some(300), reasoning_tokens: None }).unwrap();
        // 500 uncached at $1/M = 500, 500 cached at $0.1/M = 50, 300 out at $10/M = 3000
        assert_eq!(actual, MicroUsd(500 + 50 + 3_000));
        assert!(settle_cost(&p, &Usage { input_tokens: None, ..Default::default() }).is_none(), "unknown stays unknown");
    }

    #[test]
    fn reservations_respect_the_ceiling_and_lifecycle() {
        let mut b = RunBudget::new(MicroUsd(1_000));
        let r1 = b.try_reserve(MicroUsd(600)).unwrap();
        assert!(matches!(b.try_reserve(MicroUsd(500)), Err(BudgetError::OverCeiling { .. })));
        let r2 = b.try_reserve(MicroUsd(400)).unwrap();
        assert_eq!(b.snapshot().remaining, MicroUsd::ZERO);
        b.settle(r1, MicroUsd(150)).unwrap();
        assert_eq!(b.settled, MicroUsd(150));
        assert_eq!(b.snapshot().remaining, MicroUsd(450));
        b.mark_unresolved(r2).unwrap();
        assert_eq!(b.unresolved, MicroUsd(400));
        assert_eq!(b.reserved(), MicroUsd::ZERO);
        assert!(matches!(b.settle(r2, MicroUsd(1)), Err(BudgetError::UnknownReservation)));
        b.resolve(MicroUsd(400), MicroUsd(90));
        assert_eq!(b.unresolved, MicroUsd::ZERO);
        assert_eq!(b.settled, MicroUsd(240));
        assert!(!b.extend_ceiling(MicroUsd(100)), "cannot go below committed");
        assert!(b.extend_ceiling(MicroUsd(5_000)));
    }

    #[test]
    fn run_estimate_scales_with_seats_and_reports_unverified() {
        let verified = price(true);
        let unverified = price(false);
        let seats4: Vec<SeatPricing<'_>> = (0..4).map(|_| SeatPricing { price: &verified, output_bound: 1_680, reasoning_bound: 0 }).collect();
        let synth = SeatPricing { price: &unverified, output_bound: 2_000, reasoning_bound: 0 };
        let e4 = estimate_run(&seats4, &synth, 100, 0);
        assert_eq!(e4.base_calls, 21);
        assert_eq!(e4.max_attempts, 63);
        assert!(e4.low < e4.high);
        assert_eq!(e4.unverified_prices, vec!["p/m".to_string()]);
        let seats2: Vec<SeatPricing<'_>> = (0..2).map(|_| SeatPricing { price: &verified, output_bound: 1_680, reasoning_bound: 0 }).collect();
        let e2 = estimate_run(&seats2, &synth, 100, 0);
        assert!(e2.high < e4.high);
    }
}
