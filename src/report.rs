use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Datelike, Utc, Weekday};
use chrono_tz::Tz;
use serde::Serialize;

use crate::ingest::{RateLimitWindow, UsageEvent};
use crate::pricing::Pricing;

#[derive(Clone, Copy, Debug)]
pub enum PeriodGroup {
    All,
    Hour,
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy, Debug)]
pub enum GroupBy {
    Model,
    Effort,
    Directory,
    Branch,
    Origin,
    Session,
}

#[derive(Clone, Copy, Debug)]
pub enum ReportFormat {
    Table,
    Json,
    Csv,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct ReportRow {
    pub period: String,
    pub group: String,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub input_cost: f64,
    pub cached_input_cost: f64,
    pub output_cost: f64,
    pub estimated_cost: f64,
    pub cache_write_tokens: u64,
    pub cache_write_cost: f64,
    #[serde(skip)]
    pub dimensions: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct TelemetryReport<'a> {
    schema_version: u8,
    event_type: &'static str,
    generated_at: DateTime<Utc>,
    source: TelemetrySource<'a>,
    window: TelemetryWindow,
    aggregation: TelemetryAggregation<'a>,
    records: Vec<TelemetryRecord<'a>>,
}

#[derive(Serialize)]
struct TelemetrySource<'a> {
    name: &'static str,
    version: &'a str,
}

#[derive(Serialize)]
struct TelemetryWindow {
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct TelemetryAggregation<'a> {
    period: &'a str,
    dimensions: &'a [&'a str],
}

#[derive(Serialize)]
struct TelemetryRecord<'a> {
    period: &'a str,
    dimensions: &'a BTreeMap<String, String>,
    metrics: TelemetryMetrics,
}

#[derive(Serialize)]
struct TelemetryMetrics {
    total_tokens: u64,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
    input_cost_usd: f64,
    cached_input_cost_usd: f64,
    output_cost_usd: f64,
    estimated_cost_usd: f64,
    cache_write_tokens: u64,
    cache_write_cost_usd: f64,
}

pub fn aggregate(
    events: impl Iterator<Item = UsageEvent>,
    period: PeriodGroup,
    by: &[GroupBy],
    timezone: Tz,
    pricing: &Pricing,
) -> Vec<ReportRow> {
    let mut rows: BTreeMap<(String, String), ReportRow> = BTreeMap::new();
    for event in events {
        let local = event.captured_at.with_timezone(&timezone);
        let period_key = match period {
            PeriodGroup::All => "All".to_owned(),
            PeriodGroup::Hour => local.format("%Y-%m-%d %H:00").to_string(),
            PeriodGroup::Day => local.format("%Y-%m-%d").to_string(),
            PeriodGroup::Week => {
                let offset = match local.weekday() {
                    Weekday::Mon => 0,
                    Weekday::Tue => 1,
                    Weekday::Wed => 2,
                    Weekday::Thu => 3,
                    Weekday::Fri => 4,
                    Weekday::Sat => 5,
                    Weekday::Sun => 6,
                };
                (local - chrono::Duration::days(offset))
                    .format("%Y-%m-%d")
                    .to_string()
            }
            PeriodGroup::Month => local.format("%Y-%m").to_string(),
        };
        let dimensions = by
            .iter()
            .map(|group| {
                let value = match group {
                    GroupBy::Model => event.model.as_deref(),
                    GroupBy::Effort => event.effort.as_deref(),
                    GroupBy::Directory => event.directory.as_deref(),
                    GroupBy::Branch => event.branch.as_deref(),
                    GroupBy::Origin => event.origin.as_deref(),
                    GroupBy::Session => event.session_id.as_deref(),
                }
                .unwrap_or("<unknown>")
                .to_owned();
                (group.name().to_owned(), value)
            })
            .collect::<BTreeMap<_, _>>();
        let group_key = if by.is_empty() {
            "all".to_owned()
        } else {
            by.iter()
                .map(|group| {
                    dimensions
                        .get(group.name())
                        .map(String::as_str)
                        .unwrap_or("<unknown>")
                })
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let row = rows
            .entry((period_key.clone(), group_key.clone()))
            .or_insert_with(|| ReportRow {
                period: period_key,
                group: group_key,
                dimensions,
                ..ReportRow::default()
            });
        row.total_tokens += event.total_tokens;
        row.input_tokens += event.input_tokens;
        row.cached_input_tokens += event.cached_input_tokens;
        row.output_tokens += event.output_tokens;
        row.reasoning_output_tokens += event.reasoning_output_tokens;
        let cache_write_tokens = event.cache_write_5m_tokens + event.cache_write_1h_tokens;
        row.cache_write_tokens += cache_write_tokens;
        if let Some(rates) = event
            .model
            .as_deref()
            .and_then(|model| pricing.rates_for(model, event.captured_at))
        {
            let rates = if event.speed.as_deref() == Some("fast") {
                rates.scaled(2.0)
            } else {
                rates
            };
            let non_cached = event
                .input_tokens
                .saturating_sub(event.cached_input_tokens)
                .saturating_sub(cache_write_tokens);
            row.input_cost += non_cached as f64 * rates.input / 1_000_000.0;
            row.cached_input_cost += event.cached_input_tokens as f64 * rates.cached / 1_000_000.0;
            row.cache_write_cost += (event.cache_write_5m_tokens as f64 * rates.cache_write_5m
                + event.cache_write_1h_tokens as f64 * rates.cache_write_1h)
                / 1_000_000.0;
            row.output_cost += event.output_tokens as f64 * rates.output / 1_000_000.0;
            row.estimated_cost =
                row.input_cost + row.cached_input_cost + row.cache_write_cost + row.output_cost;
        }
    }
    rows.into_values().collect()
}

impl GroupBy {
    fn name(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Effort => "effort",
            Self::Directory => "directory",
            Self::Branch => "branch",
            Self::Origin => "origin",
            Self::Session => "session",
        }
    }
}

pub fn render_telemetry(
    rows: &[ReportRow],
    window_start: Option<DateTime<Utc>>,
    window_end: Option<DateTime<Utc>>,
    period: &str,
    dimensions: &[&str],
) -> Result<String> {
    let records = rows
        .iter()
        .map(|row| TelemetryRecord {
            period: &row.period,
            dimensions: &row.dimensions,
            metrics: TelemetryMetrics {
                total_tokens: row.total_tokens,
                input_tokens: row.input_tokens,
                cached_input_tokens: row.cached_input_tokens,
                output_tokens: row.output_tokens,
                reasoning_output_tokens: row.reasoning_output_tokens,
                input_cost_usd: row.input_cost,
                cached_input_cost_usd: row.cached_input_cost,
                output_cost_usd: row.output_cost,
                estimated_cost_usd: row.estimated_cost,
                cache_write_tokens: row.cache_write_tokens,
                cache_write_cost_usd: row.cache_write_cost,
            },
        })
        .collect();
    Ok(serde_json::to_string_pretty(&TelemetryReport {
        schema_version: 2,
        event_type: "agent.usage.report",
        generated_at: Utc::now(),
        source: TelemetrySource {
            name: "agent-usage-analyzer",
            version: env!("CARGO_PKG_VERSION"),
        },
        window: TelemetryWindow {
            start: window_start,
            end: window_end,
        },
        aggregation: TelemetryAggregation { period, dimensions },
        records,
    })?)
}

pub fn render(rows: &[ReportRow], include_group: bool, format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Table => Ok(render_table(rows, include_group)),
        ReportFormat::Json => Ok(serde_json::to_string_pretty(rows)?),
        ReportFormat::Csv => render_csv(rows),
    }
}

fn render_csv(rows: &[ReportRow]) -> Result<String> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    writer.write_record([
        "period",
        "group",
        "total_tokens",
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "reasoning_output_tokens",
        "input_cost",
        "cached_input_cost",
        "output_cost",
        "estimated_cost",
        "cache_write_tokens",
        "cache_write_cost",
    ])?;
    for row in rows {
        writer.serialize(row)?;
    }
    let bytes = writer.into_inner()?;
    Ok(String::from_utf8(bytes)?.trim_end_matches('\n').to_owned())
}

fn render_table(rows: &[ReportRow], include_group: bool) -> String {
    let show_summary = rows.len() != 1 || rows[0].period != "All" || include_group;
    let include_cache_writes = rows.iter().any(|row| row.cache_write_tokens > 0);
    let mut headers = vec!["Period".to_owned()];
    if include_group {
        headers.push("Group".to_owned());
    }
    headers.extend(["Total", "Input", "Cached"].map(str::to_owned));
    if include_cache_writes {
        headers.push("Cache write".to_owned());
    }
    headers.extend(["Output", "Reasoning", "Input cost", "Cached cost"].map(str::to_owned));
    if include_cache_writes {
        headers.push("Cache write cost".to_owned());
    }
    headers.extend(["Output cost", "Est. cost"].map(str::to_owned));

    let values: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row_values(row, include_group, include_cache_writes))
        .collect();
    let total_values = row_values(&summarize(rows), include_group, include_cache_writes);
    let mut widths: Vec<usize> = headers.iter().map(String::len).collect();
    for row in values.iter().chain(std::iter::once(&total_values)) {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.len());
        }
    }
    let format_row = |row: &[String]| {
        row.iter()
            .enumerate()
            .map(|(index, value)| format!("{value:<width$}", width = widths[index]))
            .collect::<Vec<_>>()
            .join("  ")
    };
    let separator: Vec<String> = widths.iter().map(|width| "-".repeat(*width)).collect();
    let mut lines = vec![format_row(&headers), format_row(&separator)];
    lines.extend(values.iter().map(|row| format_row(row)));
    if !rows.is_empty() && show_summary {
        lines.push(format_row(&separator));
    }
    if show_summary {
        lines.push(format_row(&total_values));
    }
    lines.join("\n")
}

fn summarize(rows: &[ReportRow]) -> ReportRow {
    let mut total = ReportRow {
        period: "Total".to_owned(),
        ..ReportRow::default()
    };
    for row in rows {
        total.total_tokens += row.total_tokens;
        total.input_tokens += row.input_tokens;
        total.cached_input_tokens += row.cached_input_tokens;
        total.output_tokens += row.output_tokens;
        total.reasoning_output_tokens += row.reasoning_output_tokens;
        total.input_cost += row.input_cost;
        total.cached_input_cost += row.cached_input_cost;
        total.output_cost += row.output_cost;
        total.estimated_cost += row.estimated_cost;
        total.cache_write_tokens += row.cache_write_tokens;
        total.cache_write_cost += row.cache_write_cost;
    }
    total
}

fn row_values(row: &ReportRow, include_group: bool, include_cache_writes: bool) -> Vec<String> {
    let mut values = vec![row.period.clone()];
    if include_group {
        values.push(row.group.clone());
    }
    values.extend([
        format_integer(row.total_tokens),
        format_integer(row.input_tokens),
        format_integer(row.cached_input_tokens),
    ]);
    if include_cache_writes {
        values.push(format_integer(row.cache_write_tokens));
    }
    values.extend([
        format_integer(row.output_tokens),
        format_integer(row.reasoning_output_tokens),
        format!("${:.2}", row.input_cost),
        format!("${:.2}", row.cached_input_cost),
    ]);
    if include_cache_writes {
        values.push(format!("${:.2}", row.cache_write_cost));
    }
    values.extend([
        format!("${:.2}", row.output_cost),
        format!("${:.2}", row.estimated_cost),
    ]);
    values
}

fn format_integer(value: u64) -> String {
    let source = value.to_string();
    let mut result = String::with_capacity(source.len() + source.len() / 3);
    for (index, character) in source.chars().enumerate() {
        if index > 0 && (source.len() - index).is_multiple_of(3) {
            result.push(',');
        }
        result.push(character);
    }
    result
}

pub fn render_status(event: &UsageEvent, timezone: Tz) {
    println!(
        "Captured: {}",
        event
            .captured_at
            .with_timezone(&timezone)
            .format("%Y-%m-%dT%H:%M:%S%.3f%:z")
    );
    if let Some(model) = &event.model {
        println!("Model: {model}");
    }
    if let Some(directory) = &event.directory {
        println!("Directory: {directory}");
    }
    if let Some(session) = &event.session_id {
        println!("Session: {session}");
    }
    if let Some(version) = &event.codex_version {
        println!("Codex version: {version}");
    }
    println!(
        "Token usage: total={} input={} cached={} output={} reasoning={}",
        format_integer(event.total_tokens),
        format_integer(event.input_tokens),
        format_integer(event.cached_input_tokens),
        format_integer(event.output_tokens),
        format_integer(event.reasoning_output_tokens),
    );
    if let Some(total) = event.context_window.filter(|total| *total > 0) {
        let used = event.total_tokens;
        let used_percent = used
            .saturating_mul(100)
            .checked_div(total)
            .unwrap_or_default();
        let left = 100_u64.saturating_sub(used_percent);
        println!(
            "Context window: {left}% left ({} used / {})",
            format_integer(used),
            format_integer(total)
        );
    }
    if let Some(limit) = &event.primary_limit {
        render_rate_limit(limit, "Primary", timezone);
    }
    if let Some(limit) = &event.secondary_limit {
        render_rate_limit(limit, "Secondary", timezone);
    }
}

fn render_rate_limit(limit: &RateLimitWindow, fallback: &str, timezone: Tz) {
    let label = limit
        .window_minutes
        .map(format_window_duration)
        .unwrap_or_else(|| fallback.to_owned());
    if let Some(resets_at) = limit.resets_at {
        println!(
            "{label} limit: {}% left (resets {})",
            limit.percent_left,
            resets_at
                .with_timezone(&timezone)
                .format("%Y-%m-%dT%H:%M:%S%:z")
        );
    } else {
        println!("{label} limit: {}% left", limit.percent_left);
    }
}

fn format_window_duration(minutes: u64) -> String {
    if minutes > 0 && minutes.is_multiple_of(1_440) {
        format!("{}d", minutes / 1_440)
    } else if minutes > 0 && minutes.is_multiple_of(60) {
        format!("{}h", minutes / 60)
    } else {
        format!("{minutes}min")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_format_matches_python_style() {
        assert_eq!(format_integer(1_234_567), "1,234,567");
    }

    #[test]
    fn rate_limit_windows_use_their_actual_duration() {
        assert_eq!(format_window_duration(300), "5h");
        assert_eq!(format_window_duration(10_080), "7d");
        assert_eq!(format_window_duration(90), "90min");
    }

    #[test]
    fn aggregate_prices_each_event_at_its_capture_date() {
        let events = [
            UsageEvent {
                captured_at: "2026-08-20T23:59:59Z".parse().unwrap(),
                output_tokens: 1_000_000,
                model: Some("gpt-5.6-sol".to_owned()),
                ..UsageEvent::default()
            },
            UsageEvent {
                captured_at: "2026-08-21T00:00:00Z".parse().unwrap(),
                output_tokens: 1_000_000,
                model: Some("gpt-5.6-sol".to_owned()),
                ..UsageEvent::default()
            },
        ];
        let rows = aggregate(
            events.into_iter(),
            PeriodGroup::All,
            &[],
            chrono_tz::UTC,
            &Pricing::default(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].output_cost, 50.0);
        assert_eq!(rows[0].estimated_cost, 50.0);
    }

    #[test]
    fn aggregate_preserves_claude_prices_across_generations() {
        let events = [
            ("claude-opus-4-1-20250805", "2025-08-05T00:00:00Z"),
            ("claude-opus-4-5", "2025-11-24T00:00:00Z"),
            ("claude-opus-5-5", "2026-09-22T00:00:00Z"),
        ]
        .map(|(model, captured_at)| UsageEvent {
            captured_at: captured_at.parse().unwrap(),
            input_tokens: 4_000_000,
            cached_input_tokens: 1_000_000,
            cache_write_5m_tokens: 1_000_000,
            cache_write_1h_tokens: 1_000_000,
            output_tokens: 1_000_000,
            model: Some(model.to_owned()),
            ..UsageEvent::default()
        });
        let rows = aggregate(
            events.into_iter(),
            PeriodGroup::All,
            &[],
            chrono_tz::UTC,
            &Pricing::default(),
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].input_cost, 24.0);
        assert_eq!(rows[0].output_cost, 120.0);
        assert_eq!(rows[0].cache_write_cost, 78.0);
        assert_eq!(rows[0].cached_input_cost, 2.2);
        assert_eq!(rows[0].estimated_cost, 224.2);
    }

    #[test]
    fn aggregate_prices_claude_cache_writes_separately() {
        let event = UsageEvent {
            captured_at: "2026-09-28T00:00:00Z".parse().unwrap(),
            input_tokens: 4_000_000,
            cached_input_tokens: 1_000_000,
            cache_write_5m_tokens: 1_000_000,
            cache_write_1h_tokens: 1_000_000,
            output_tokens: 1_000_000,
            model: Some("claude-opus-5-5".to_owned()),
            ..UsageEvent::default()
        };
        let fast = UsageEvent {
            speed: Some("fast".to_owned()),
            ..event.clone()
        };
        let pricing = Pricing::default();
        let standard = aggregate(
            [event].into_iter(),
            PeriodGroup::All,
            &[],
            chrono_tz::UTC,
            &pricing,
        );
        assert_eq!(standard[0].input_cost, 4.0);
        assert_eq!(standard[0].cached_input_cost, 0.2);
        assert_eq!(standard[0].cache_write_cost, 13.0);
        assert_eq!(standard[0].output_cost, 20.0);
        assert_eq!(standard[0].estimated_cost, 37.2);
        let fast = aggregate(
            [fast].into_iter(),
            PeriodGroup::All,
            &[],
            chrono_tz::UTC,
            &pricing,
        );
        assert_eq!(fast[0].estimated_cost, 74.4);
    }
}
