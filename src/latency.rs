use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{Datelike, Weekday};
use chrono_tz::Tz;
use serde::Serialize;

use crate::ingest::LatencyEvent;
use crate::report::{GroupBy, PeriodGroup, ReportFormat};

#[derive(Clone, Debug, Default, Serialize)]
pub struct LatencyRow {
    pub period: String,
    pub group: String,
    pub duration_samples: u64,
    pub average_duration_ms: Option<f64>,
    pub p50_duration_ms: Option<u64>,
    pub p95_duration_ms: Option<u64>,
    pub ttft_samples: u64,
    pub average_ttft_ms: Option<f64>,
    pub p50_ttft_ms: Option<u64>,
    pub p95_ttft_ms: Option<u64>,
    #[serde(skip)]
    duration_values: Vec<u64>,
    #[serde(skip)]
    ttft_values: Vec<u64>,
}

pub fn aggregate(
    events: impl Iterator<Item = LatencyEvent>,
    period: PeriodGroup,
    by: &[GroupBy],
    timezone: Tz,
) -> Vec<LatencyRow> {
    let mut rows = BTreeMap::<(String, String), LatencyRow>::new();
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
        let group_key = if by.is_empty() {
            "all".to_owned()
        } else {
            by.iter()
                .map(|group| {
                    match group {
                        GroupBy::Model => event.model.as_deref(),
                        GroupBy::Effort => event.effort.as_deref(),
                        GroupBy::Directory => event.directory.as_deref(),
                        GroupBy::Branch => event.branch.as_deref(),
                        GroupBy::Origin => event.origin.as_deref(),
                        GroupBy::Session => event.session_id.as_deref(),
                    }
                    .unwrap_or("<unknown>")
                })
                .collect::<Vec<_>>()
                .join(" / ")
        };
        let row = rows
            .entry((period_key.clone(), group_key.clone()))
            .or_insert_with(|| LatencyRow {
                period: period_key,
                group: group_key,
                ..LatencyRow::default()
            });
        if let Some(value) = event.duration_ms {
            row.duration_values.push(value);
        }
        if let Some(value) = event.time_to_first_token_ms {
            row.ttft_values.push(value);
        }
    }
    rows.into_values()
        .map(|mut row| {
            update_statistics(&mut row);
            row
        })
        .collect()
}

pub fn render(rows: &[LatencyRow], include_group: bool, format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Table => Ok(render_table(rows, include_group)),
        ReportFormat::Json => Ok(serde_json::to_string_pretty(rows)?),
        ReportFormat::Csv => render_csv(rows),
    }
}

fn render_csv(rows: &[LatencyRow]) -> Result<String> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    writer.write_record([
        "period",
        "group",
        "duration_samples",
        "average_duration_ms",
        "p50_duration_ms",
        "p95_duration_ms",
        "ttft_samples",
        "average_ttft_ms",
        "p50_ttft_ms",
        "p95_ttft_ms",
    ])?;
    for row in rows {
        writer.serialize(row)?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?
        .trim_end_matches('\n')
        .to_owned())
}

fn render_table(rows: &[LatencyRow], include_group: bool) -> String {
    let show_summary = rows.len() != 1 || rows[0].period != "All" || include_group;
    let mut headers = vec!["Period".to_owned()];
    if include_group {
        headers.push("Group".to_owned());
    }
    headers.extend(
        [
            "Duration n",
            "Duration avg",
            "Duration p50",
            "Duration p95",
            "TTFT n",
            "TTFT avg",
            "TTFT p50",
            "TTFT p95",
        ]
        .map(str::to_owned),
    );
    let values: Vec<Vec<String>> = rows
        .iter()
        .map(|row| row_values(row, include_group))
        .collect();
    let total_values = row_values(&summarize(rows), include_group);
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

fn summarize(rows: &[LatencyRow]) -> LatencyRow {
    let mut total = LatencyRow {
        period: "Total".to_owned(),
        ..LatencyRow::default()
    };
    for row in rows {
        total
            .duration_values
            .extend_from_slice(&row.duration_values);
        total.ttft_values.extend_from_slice(&row.ttft_values);
    }
    update_statistics(&mut total);
    total
}

fn update_statistics(row: &mut LatencyRow) {
    row.duration_values.sort_unstable();
    row.ttft_values.sort_unstable();
    row.duration_samples = row.duration_values.len() as u64;
    row.average_duration_ms = average(&row.duration_values);
    row.p50_duration_ms = percentile(&row.duration_values, 50);
    row.p95_duration_ms = percentile(&row.duration_values, 95);
    row.ttft_samples = row.ttft_values.len() as u64;
    row.average_ttft_ms = average(&row.ttft_values);
    row.p50_ttft_ms = percentile(&row.ttft_values, 50);
    row.p95_ttft_ms = percentile(&row.ttft_values, 95);
}

fn average(values: &[u64]) -> Option<f64> {
    (!values.is_empty())
        .then(|| values.iter().map(|value| *value as f64).sum::<f64>() / values.len() as f64)
}

fn percentile(values: &[u64], percentile: usize) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let rank = (percentile * values.len()).div_ceil(100);
    Some(values[rank.saturating_sub(1)])
}

fn row_values(row: &LatencyRow, include_group: bool) -> Vec<String> {
    let mut values = vec![row.period.clone()];
    if include_group {
        values.push(row.group.clone());
    }
    values.extend([
        format_integer(row.duration_samples),
        format_optional_duration(row.average_duration_ms),
        format_optional_duration(row.p50_duration_ms.map(|value| value as f64)),
        format_optional_duration(row.p95_duration_ms.map(|value| value as f64)),
        format_integer(row.ttft_samples),
        format_optional_duration(row.average_ttft_ms),
        format_optional_duration(row.p50_ttft_ms.map(|value| value as f64)),
        format_optional_duration(row.p95_ttft_ms.map(|value| value as f64)),
    ]);
    values
}

pub(crate) fn format_optional_duration(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "-".to_owned();
    };
    if value < 1_000.0 {
        format!("{value:.0}ms")
    } else if value < 60_000.0 {
        format!("{:.2}s", value / 1_000.0)
    } else {
        format!("{:.2}m", value / 60_000.0)
    }
}

pub(crate) fn format_integer(value: u64) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calculates_nearest_rank_percentiles() {
        assert_eq!(percentile(&[1_000, 3_000], 50), Some(1_000));
        assert_eq!(percentile(&[1_000, 3_000], 95), Some(3_000));
        assert_eq!(percentile(&[], 95), None);
    }
}
