use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, Utc};

#[derive(Clone, Copy, Debug)]
pub struct Rates {
    pub input: f64,
    pub cached: f64,
    pub output: f64,
    pub cache_write_5m: f64,
    pub cache_write_1h: f64,
}

impl Rates {
    pub fn scaled(self, factor: f64) -> Self {
        Self {
            input: self.input * factor,
            cached: self.cached * factor,
            output: self.output * factor,
            cache_write_5m: self.cache_write_5m * factor,
            cache_write_1h: self.cache_write_1h * factor,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct PricePeriod {
    effective_from: NaiveDate,
    rates: Rates,
}

#[derive(Debug)]
pub struct Pricing {
    rates: HashMap<&'static str, Vec<PricePeriod>>,
}

impl Default for Pricing {
    fn default() -> Self {
        let mut rates = HashMap::new();
        for (name, input, cached, output) in [
            ("gpt-5.2", 1.75, 0.175, 14.0),
            ("gpt-5.4", 2.5, 0.25, 15.0),
            ("gpt-5.4-mini", 0.75, 0.075, 4.5),
            ("gpt-5.5", 5.0, 0.5, 30.0),
            ("gpt-5.2-pro", 21.0, 21.0, 168.0),
            ("gpt-5.4-pro", 30.0, 30.0, 180.0),
            ("gpt-5.5-pro", 30.0, 30.0, 180.0),
            ("gpt-5", 5.0, 0.5, 30.0),
            ("gpt-5.1-codex-max", 1.25, 0.125, 10.0),
            ("gpt-5.1-codex", 1.25, 0.125, 10.0),
            ("gpt-5.1-codex-mini", 0.25, 0.025, 2.0),
            ("gpt-5.2-codex", 1.75, 0.175, 14.0),
            ("gpt-5-codex", 1.75, 0.175, 14.0),
            ("gpt-5.3-codex", 1.75, 0.175, 14.0),
        ] {
            rates.insert(
                name,
                vec![PricePeriod {
                    effective_from: NaiveDate::MIN,
                    rates: Rates {
                        input,
                        cached,
                        output,
                        cache_write_5m: input,
                        cache_write_1h: input,
                    },
                }],
            );
        }
        for (name, input, cache_write_5m, cache_write_1h, cached, output) in [
            ("claude-fable-5-1", 10.0, 12.5, 20.0, 0.25, 50.0),
            ("claude-mythos-5-1", 10.0, 12.5, 20.0, 0.25, 50.0),
            ("claude-fable-5", 10.0, 12.5, 20.0, 1.0, 50.0),
            ("claude-mythos-5", 10.0, 12.5, 20.0, 1.0, 50.0),
            ("claude-opus-5-5", 4.0, 5.0, 8.0, 0.2, 20.0),
            ("claude-opus-5", 5.0, 6.25, 10.0, 0.5, 25.0),
            ("claude-opus-4-8", 5.0, 6.25, 10.0, 0.5, 25.0),
            ("claude-opus-4-7", 5.0, 6.25, 10.0, 0.5, 25.0),
            ("claude-opus-4-6", 5.0, 6.25, 10.0, 0.5, 25.0),
            ("claude-opus-4-5", 5.0, 6.25, 10.0, 0.5, 25.0),
            ("claude-opus-4-1", 15.0, 18.75, 30.0, 1.5, 75.0),
            ("claude-opus-4", 15.0, 18.75, 30.0, 1.5, 75.0),
            ("claude-sonnet-5-5", 2.0, 2.5, 4.0, 0.2, 10.0),
            ("claude-sonnet-5", 2.0, 2.5, 4.0, 0.2, 10.0),
            ("claude-sonnet-4-6", 3.0, 3.75, 6.0, 0.3, 15.0),
            ("claude-sonnet-4-5", 3.0, 3.75, 6.0, 0.3, 15.0),
            ("claude-sonnet-4", 3.0, 3.75, 6.0, 0.3, 15.0),
            ("claude-haiku-4-5", 1.0, 1.25, 2.0, 0.1, 5.0),
            ("claude-3-5-haiku", 0.8, 1.0, 1.6, 0.08, 4.0),
        ] {
            rates.insert(
                name,
                vec![PricePeriod {
                    effective_from: NaiveDate::MIN,
                    rates: Rates {
                        input,
                        cached,
                        output,
                        cache_write_5m,
                        cache_write_1h,
                    },
                }],
            );
        }
        rates.insert(
            "gpt-6-astra",
            vec![gpt_6_period(2026, 9, 3, 10.0, 1.0, 50.0)],
        );
        rates.insert("gpt-6-sol", vec![gpt_6_period(2026, 9, 22, 2.0, 0.2, 10.0)]);
        rates.insert(
            "gpt-6.1-sol",
            vec![gpt_6_period(2026, 9, 29, 2.0, 0.1, 10.0)],
        );
        rates.insert(
            "gpt-6-luna",
            vec![gpt_6_period(2026, 9, 22, 0.1, 0.01, 0.5)],
        );
        rates.insert(
            "gpt-5.6-sol",
            vec![
                period(2026, 7, 9, 5.0, 0.5, 30.0),
                period(2026, 8, 21, 4.0, 0.4, 20.0),
            ],
        );
        rates.insert(
            "gpt-5.6-terra",
            vec![
                period(2026, 7, 9, 2.5, 0.25, 15.0),
                period(2026, 7, 30, 2.0, 0.2, 12.0),
            ],
        );
        rates.insert(
            "gpt-5.6-luna",
            vec![
                period(2026, 7, 9, 1.0, 0.1, 6.0),
                period(2026, 7, 30, 0.2, 0.02, 1.2),
            ],
        );
        Self { rates }
    }
}

impl Pricing {
    pub fn rates_for(&self, model: &str, captured_at: DateTime<Utc>) -> Option<Rates> {
        let cleaned = model.trim();
        if let Some(rates) = self.rates_at(cleaned, captured_at) {
            return Some(rates);
        }
        if cleaned.ends_with(')')
            && let Some((base, _)) = cleaned.split_once(" (")
            && let Some(rates) = self.rates_at(base, captured_at)
        {
            return Some(rates);
        }
        let undated = strip_dated_suffix(cleaned);
        if let Some(rates) = self.rates_at(undated, captured_at) {
            return Some(rates);
        }
        let alias = match undated {
            "codex-auto-review" => "gpt-5.6-luna",
            "gpt-5.6" => "gpt-5.6-sol",
            "gpt-5-codex" => "gpt-5.2-codex",
            "gpt-5" => "gpt-5.5",
            "gpt-5-pro" => "gpt-5.5-pro",
            _ => return None,
        };
        self.rates_at(alias, captured_at)
    }

    fn rates_at(&self, model: &str, captured_at: DateTime<Utc>) -> Option<Rates> {
        self.rates.get(model)?.iter().rev().find_map(|period| {
            (period.effective_from <= captured_at.date_naive()).then_some(period.rates)
        })
    }
}

fn period(year: i32, month: u32, day: u32, input: f64, cached: f64, output: f64) -> PricePeriod {
    PricePeriod {
        effective_from: NaiveDate::from_ymd_opt(year, month, day).unwrap(),
        rates: Rates {
            input,
            cached,
            output,
            cache_write_5m: input,
            cache_write_1h: input,
        },
    }
}

fn gpt_6_period(
    year: i32,
    month: u32,
    day: u32,
    input: f64,
    cached: f64,
    output: f64,
) -> PricePeriod {
    let mut price = period(year, month, day, input, cached, output);
    price.rates.cache_write_5m = input * 1.25;
    price.rates.cache_write_1h = input * 1.25;
    price
}

fn strip_dated_suffix(value: &str) -> &str {
    if value.len() > 9 {
        let split = value.len() - 9;
        let bytes = &value.as_bytes()[split..];
        if bytes[0] == b'-' && bytes[1..].iter().all(u8::is_ascii_digit) {
            return &value[..split];
        }
    }
    if value.len() < 11 {
        return value;
    }
    let split = value.len() - 11;
    let suffix = &value[split..];
    let bytes = suffix.as_bytes();
    if bytes[0] == b'-'
        && bytes[1..5].iter().all(u8::is_ascii_digit)
        && bytes[5] == b'-'
        && bytes[6..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'-'
        && bytes[9..11].iter().all(u8::is_ascii_digit)
    {
        &value[..split]
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::Pricing;

    fn at(value: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(value)
            .unwrap()
            .to_utc()
    }

    #[test]
    fn selects_sol_rates_by_capture_date() {
        let pricing = Pricing::default();
        let old = pricing
            .rates_for("gpt-5.6-sol", at("2026-08-20T23:59:59Z"))
            .unwrap();
        let current = pricing
            .rates_for("gpt-5.6-sol", at("2026-08-21T00:00:00Z"))
            .unwrap();
        assert_eq!((old.input, old.cached, old.output), (5.0, 0.5, 30.0));
        assert_eq!(
            (current.input, current.cached, current.output),
            (4.0, 0.4, 20.0)
        );
    }

    #[test]
    fn selects_terra_and_luna_rates_by_capture_date() {
        let pricing = Pricing::default();
        let old_terra = pricing
            .rates_for("gpt-5.6-terra", at("2026-07-29T23:59:59Z"))
            .unwrap();
        let current_terra = pricing
            .rates_for("gpt-5.6-terra", at("2026-07-30T00:00:00Z"))
            .unwrap();
        let old_luna = pricing
            .rates_for("gpt-5.6-luna", at("2026-07-29T23:59:59Z"))
            .unwrap();
        let current_luna = pricing
            .rates_for("gpt-5.6-luna", at("2026-07-30T00:00:00Z"))
            .unwrap();
        assert_eq!(
            (old_terra.input, old_terra.cached, old_terra.output),
            (2.5, 0.25, 15.0)
        );
        assert_eq!(
            (
                current_terra.input,
                current_terra.cached,
                current_terra.output
            ),
            (2.0, 0.2, 12.0)
        );
        assert_eq!(
            (old_luna.input, old_luna.cached, old_luna.output),
            (1.0, 0.1, 6.0)
        );
        assert_eq!(
            (current_luna.input, current_luna.cached, current_luna.output),
            (0.2, 0.02, 1.2)
        );
    }

    #[test]
    fn rejects_dates_before_a_models_first_price_period() {
        let pricing = Pricing::default();
        assert!(
            pricing
                .rates_for("gpt-5.6-luna", at("2026-07-08T23:59:59Z"))
                .is_none()
        );
    }

    #[test]
    fn resolves_dated_aliases_with_historical_rates() {
        let pricing = Pricing::default();
        let rates = pricing
            .rates_for("gpt-5.6-2026-07-09", at("2026-07-10T00:00:00Z"))
            .unwrap();
        assert_eq!((rates.input, rates.cached, rates.output), (5.0, 0.5, 30.0));
    }

    #[test]
    fn prices_gpt_6_models_from_their_release_dates() {
        let pricing = Pricing::default();
        assert!(
            pricing
                .rates_for("gpt-6-sol", at("2026-09-21T23:59:59Z"))
                .is_none()
        );
        let sol = pricing
            .rates_for("gpt-6-sol", at("2026-09-22T00:00:00Z"))
            .unwrap();
        let luna = pricing
            .rates_for("gpt-6-luna", at("2026-09-22T00:00:00Z"))
            .unwrap();
        assert_eq!((sol.input, sol.cached, sol.output), (2.0, 0.2, 10.0));
        assert_eq!((luna.input, luna.cached, luna.output), (0.1, 0.01, 0.5));
    }

    #[test]
    fn prices_gpt_6_1_sol_from_its_release_without_changing_gpt_6_sol() {
        let pricing = Pricing::default();
        assert!(
            pricing
                .rates_for("gpt-6.1-sol", at("2026-09-28T23:59:59Z"))
                .is_none()
        );
        for model in ["gpt-6.1-sol", "gpt-6.1-sol-2026-09-29"] {
            let rates = pricing
                .rates_for(model, at("2026-09-29T00:00:00Z"))
                .unwrap();
            assert_eq!((rates.input, rates.cached, rates.output), (2.0, 0.1, 10.0));
            assert_eq!((rates.cache_write_5m, rates.cache_write_1h), (2.5, 2.5));
        }
        let previous = pricing
            .rates_for("gpt-6-sol", at("2026-10-04T00:00:00Z"))
            .unwrap();
        assert_eq!(previous.cached, 0.2);
    }

    #[test]
    fn prices_gpt_6_cache_writes() {
        let pricing = Pricing::default();
        for (model, expected) in [
            ("gpt-6-astra", 12.5),
            ("gpt-6-sol", 2.5),
            ("gpt-6-luna", 0.125),
        ] {
            let rates = pricing
                .rates_for(model, at("2026-10-04T00:00:00Z"))
                .unwrap();
            assert_eq!(
                (rates.cache_write_5m, rates.cache_write_1h),
                (expected, expected)
            );
        }
    }

    #[test]
    fn prices_claude_models_including_cache_writes_and_dated_ids() {
        let pricing = Pricing::default();
        let opus = pricing
            .rates_for("claude-opus-5-5", at("2026-09-28T00:00:00Z"))
            .unwrap();
        let haiku = pricing
            .rates_for("claude-haiku-4-5-20251001", at("2026-09-28T00:00:00Z"))
            .unwrap();
        assert_eq!(
            (
                opus.input,
                opus.cache_write_5m,
                opus.cache_write_1h,
                opus.cached,
                opus.output
            ),
            (4.0, 5.0, 8.0, 0.2, 20.0)
        );
        assert_eq!((haiku.input, haiku.output), (1.0, 5.0));
        assert!(
            pricing
                .rates_for("<synthetic>", at("2026-09-28T00:00:00Z"))
                .is_none()
        );
    }
}
