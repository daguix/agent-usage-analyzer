use std::cmp::Ordering;
use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::Serialize;

use crate::ingest::LatencyEvent;
use crate::latency::{format_integer, format_optional_duration};
use crate::report::{GroupBy, PeriodGroup, ReportFormat};
use crate::tools::ToolKind;
use crate::workflow::{group_label, period_key, period_segments};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Thinking,
    Text,
    ToolCall,
}

impl BlockKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Thinking => "Thinking",
            Self::Text => "Text",
            Self::ToolCall => "Tool call writing",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Model,
    Tools,
    Other,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Self::Model => "Model response",
            Self::Tools => "Tool execution",
            Self::Other => "Other",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum Kind {
    Model(BlockKind),
    Tool(ToolKind),
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Model(kind) => kind.label(),
            Self::Tool(kind) => kind.label(),
        }
    }

    fn name(self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
struct Activity {
    family: Family,
    kind: Option<Kind>,
    command: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ActivityRow {
    pub period: String,
    pub group: String,
    pub family: Family,
    pub kind: Option<Kind>,
    pub command: Option<String>,
    pub count: Option<u64>,
    pub hours: f64,
    pub average_ms: Option<f64>,
    pub share_of_agent_hours: Option<f64>,
}

#[derive(Default)]
struct Totals {
    count: u64,
    raw_ms: i64,
    ms: f64,
}

pub fn aggregate(
    events: impl IntoIterator<Item = LatencyEvent>,
    range_start: Option<DateTime<Utc>>,
    range_end: Option<DateTime<Utc>>,
    timezone: Tz,
    period: PeriodGroup,
    by: &[GroupBy],
) -> Vec<ActivityRow> {
    let lower = range_start.unwrap_or(DateTime::<Utc>::MIN_UTC);
    let upper = range_end.unwrap_or(DateTime::<Utc>::MAX_UTC);
    let other = Activity {
        family: Family::Other,
        kind: None,
        command: None,
    };
    let mut agent_ms = BTreeMap::<(String, String), f64>::new();
    let mut entries = BTreeMap::<(String, String, Activity), Totals>::new();
    for event in events {
        let Some(start) = event
            .duration_ms
            .and_then(|value| i64::try_from(value).ok())
            .and_then(|value| {
                event
                    .captured_at
                    .checked_sub_signed(Duration::milliseconds(value))
            })
        else {
            continue;
        };
        let group = group_label(&event, by);
        let spans: Vec<(Activity, DateTime<Utc>, DateTime<Utc>)> = event
            .model_blocks
            .iter()
            .map(|block| {
                (
                    Activity {
                        family: Family::Model,
                        kind: Some(Kind::Model(block.kind)),
                        command: block.tool.clone(),
                    },
                    block.started_at,
                    block.ended_at,
                )
            })
            .chain(event.tool_calls.iter().map(|call| {
                (
                    Activity {
                        family: Family::Tools,
                        kind: Some(Kind::Tool(call.kind)),
                        command: Some(call.command.clone()),
                    },
                    call.started_at,
                    call.ended_at,
                )
            }))
            .collect();
        for (activity, span_start, span_end) in &spans {
            if *span_start < lower || *span_start >= upper {
                continue;
            }
            let entry = entries
                .entry((
                    period_key(*span_start, timezone, period),
                    group.clone(),
                    activity.clone(),
                ))
                .or_default();
            entry.count += 1;
            entry.raw_ms += (*span_end - *span_start).num_milliseconds();
        }
        let turn_start = start.max(lower);
        let turn_end = event.captured_at.min(upper);
        if turn_end <= turn_start {
            continue;
        }
        let mut bounds = vec![turn_start, turn_end];
        for (_, span_start, span_end) in &spans {
            for bound in [*span_start, *span_end] {
                if bound > turn_start && bound < turn_end {
                    bounds.push(bound);
                }
            }
        }
        bounds.sort_unstable();
        bounds.dedup();
        for window in bounds.windows(2) {
            let (window_start, window_end) = (window[0], window[1]);
            let covers = |(_, span_start, span_end): &&(Activity, DateTime<Utc>, DateTime<Utc>)| {
                *span_start <= window_start && *span_end >= window_end
            };
            let shares: Vec<(&Activity, f64)> = match spans
                .iter()
                .filter(|span| span.0.family == Family::Model)
                .find(covers)
            {
                Some((activity, _, _)) => vec![(activity, 1.0)],
                None => {
                    let active: Vec<&Activity> = spans
                        .iter()
                        .filter(|span| span.0.family == Family::Tools)
                        .filter(covers)
                        .map(|(activity, _, _)| activity)
                        .collect();
                    if active.is_empty() {
                        vec![(&other, 1.0)]
                    } else {
                        let weight = 1.0 / active.len() as f64;
                        active
                            .into_iter()
                            .map(|activity| (activity, weight))
                            .collect()
                    }
                }
            };
            for (key, segment_start, segment_end) in
                period_segments(window_start, window_end, timezone, period)
            {
                let ms = (segment_end - segment_start).num_milliseconds() as f64;
                *agent_ms.entry((key.clone(), group.clone())).or_default() += ms;
                for (activity, weight) in &shares {
                    entries
                        .entry((key.clone(), group.clone(), (*activity).clone()))
                        .or_default()
                        .ms += ms * weight;
                }
            }
        }
    }
    let mut kind_ms = BTreeMap::<(String, String, Family, Option<Kind>), f64>::new();
    for ((period, group, activity), totals) in &entries {
        *kind_ms
            .entry((
                period.clone(),
                group.clone(),
                activity.family,
                activity.kind,
            ))
            .or_default() += totals.ms;
    }
    let mut rows: Vec<(f64, ActivityRow)> = entries
        .into_iter()
        .map(|((period, group, activity), totals)| {
            let agent = agent_ms
                .get(&(period.clone(), group.clone()))
                .copied()
                .unwrap_or_default();
            let kind_total = kind_ms[&(
                period.clone(),
                group.clone(),
                activity.family,
                activity.kind,
            )];
            let counted = activity.family != Family::Other;
            (
                kind_total,
                ActivityRow {
                    hours: totals.ms / 3_600_000.0,
                    count: counted.then_some(totals.count),
                    average_ms: (counted && totals.count > 0)
                        .then(|| totals.raw_ms as f64 / totals.count as f64),
                    share_of_agent_hours: (agent > 0.0).then(|| totals.ms / agent),
                    period,
                    group,
                    family: activity.family,
                    kind: activity.kind,
                    command: activity.command,
                },
            )
        })
        .collect();
    rows.sort_by(|(left_kind, left), (right_kind, right)| {
        (&left.period, &left.group, left.family)
            .cmp(&(&right.period, &right.group, right.family))
            .then_with(|| right_kind.partial_cmp(left_kind).unwrap_or(Ordering::Equal))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| {
                right
                    .hours
                    .partial_cmp(&left.hours)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| left.command.cmp(&right.command))
    });
    rows.into_iter().map(|(_, row)| row).collect()
}

pub fn render(rows: &[ActivityRow], format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(rows)?),
        ReportFormat::Csv => render_csv(rows),
        ReportFormat::Table => Ok(render_table(rows)),
    }
}

fn render_csv(rows: &[ActivityRow]) -> Result<String> {
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record([
        "period",
        "group",
        "family",
        "kind",
        "command",
        "count",
        "hours",
        "average_ms",
        "share_of_agent_hours",
    ])?;
    let optional = |value: Option<String>| value.unwrap_or_default();
    for row in rows {
        writer.write_record([
            row.period.clone(),
            row.group.clone(),
            serde_json::to_value(row.family)?
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            optional(row.kind.map(Kind::name)),
            optional(row.command.clone()),
            optional(row.count.map(|count| count.to_string())),
            row.hours.to_string(),
            optional(row.average_ms.map(|value| value.to_string())),
            optional(row.share_of_agent_hours.map(|value| value.to_string())),
        ])?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?
        .trim_end_matches('\n')
        .to_owned())
}

struct Line {
    period: String,
    group: String,
    label: String,
    count: Option<u64>,
    raw_ms: f64,
    hours: f64,
    share: Option<f64>,
}

impl Line {
    fn new(row: &ActivityRow, label: String) -> Self {
        Self {
            period: row.period.clone(),
            group: row.group.clone(),
            label,
            count: None,
            raw_ms: 0.0,
            hours: 0.0,
            share: None,
        }
    }

    fn add(&mut self, row: &ActivityRow) {
        if let Some(count) = row.count {
            *self.count.get_or_insert(0) += count;
            self.raw_ms += row.average_ms.unwrap_or_default() * count as f64;
        }
        self.hours += row.hours;
        if let Some(share) = row.share_of_agent_hours {
            *self.share.get_or_insert(0.0) += share;
        }
    }

    fn values(&self) -> [String; 7] {
        [
            self.period.clone(),
            self.group.clone(),
            self.label.clone(),
            self.count.map_or_else(|| "-".to_owned(), format_integer),
            format!("{:.3}", self.hours),
            format_optional_duration(
                self.count
                    .filter(|count| *count > 0)
                    .map(|count| self.raw_ms / count as f64),
            ),
            self.share
                .map_or_else(|| "-".to_owned(), |share| format!("{:.1}%", share * 100.0)),
        ]
    }
}

fn render_table(rows: &[ActivityRow]) -> String {
    let mut lines = Vec::<Line>::new();
    let mut index = 0;
    while index < rows.len() {
        let first = &rows[index];
        let block_end = rows[index..]
            .iter()
            .position(|row| row.period != first.period || row.group != first.group)
            .map_or(rows.len(), |offset| index + offset);
        let block = &rows[index..block_end];
        let mut total = Line::new(first, "Total".to_owned());
        block.iter().for_each(|row| total.add(row));
        total.count = None;
        lines.push(total);
        let mut cursor = 0;
        while cursor < block.len() {
            let family = block[cursor].family;
            let family_end = block[cursor..]
                .iter()
                .position(|row| row.family != family)
                .map_or(block.len(), |offset| cursor + offset);
            let mut family_line = Line::new(&block[cursor], format!("  {}", family.label()));
            block[cursor..family_end]
                .iter()
                .for_each(|row| family_line.add(row));
            lines.push(family_line);
            let mut kind_cursor = cursor;
            while kind_cursor < family_end {
                let Some(kind) = block[kind_cursor].kind else {
                    kind_cursor += 1;
                    continue;
                };
                let kind_end = block[kind_cursor..family_end]
                    .iter()
                    .position(|row| row.kind != Some(kind))
                    .map_or(family_end, |offset| kind_cursor + offset);
                let mut kind_line = Line::new(&block[kind_cursor], format!("    {}", kind.label()));
                block[kind_cursor..kind_end]
                    .iter()
                    .for_each(|row| kind_line.add(row));
                lines.push(kind_line);
                for row in &block[kind_cursor..kind_end] {
                    if let Some(command) = &row.command {
                        let mut line = Line::new(row, format!("      {command}"));
                        line.add(row);
                        lines.push(line);
                    }
                }
                kind_cursor = kind_end;
            }
            cursor = family_end;
        }
        index = block_end;
    }
    let headers = [
        "Period",
        "Group",
        "Activity",
        "Count",
        "Hours",
        "Avg",
        "Agent-hour share",
    ];
    let values: Vec<[String; 7]> = lines.iter().map(Line::values).collect();
    let mut widths = headers.map(str::len);
    for row in &values {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.chars().count());
        }
    }
    let format_row = |row: &[&str; 7]| {
        row.iter()
            .enumerate()
            .map(|(index, value)| {
                if index < 3 {
                    format!("{value:<width$}", width = widths[index])
                } else {
                    format!("{value:>width$}", width = widths[index])
                }
            })
            .collect::<Vec<_>>()
            .join("  ")
    };
    let mut output = vec![
        format_row(&headers),
        widths.map(|width| "-".repeat(width)).join("  "),
    ];
    output.extend(
        values
            .iter()
            .map(|row| format_row(&row.each_ref().map(String::as_str))),
    );
    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::{ModelBlock, ToolCall};

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value).unwrap().to_utc()
    }

    fn block(kind: BlockKind, tool: Option<&str>, start: &str, end: &str) -> ModelBlock {
        ModelBlock {
            kind,
            tool: tool.map(str::to_owned),
            started_at: at(start),
            ended_at: at(end),
        }
    }

    fn call(kind: ToolKind, command: &str, start: &str, end: &str) -> ToolCall {
        ToolCall {
            kind,
            command: command.to_owned(),
            started_at: at(start),
            ended_at: at(end),
        }
    }

    fn find<'a>(
        rows: &'a [ActivityRow],
        kind: Option<Kind>,
        command: Option<&str>,
    ) -> &'a ActivityRow {
        rows.iter()
            .find(|row| row.kind == kind && row.command.as_deref() == command)
            .unwrap()
    }

    #[test]
    fn partitions_turn_time_exclusively() {
        let turn = LatencyEvent {
            captured_at: at("2026-09-22T10:01:40Z"),
            duration_ms: Some(100_000),
            model_blocks: vec![
                block(
                    BlockKind::Thinking,
                    None,
                    "2026-09-22T10:00:00Z",
                    "2026-09-22T10:00:10Z",
                ),
                block(
                    BlockKind::ToolCall,
                    Some("Bash"),
                    "2026-09-22T10:00:10Z",
                    "2026-09-22T10:00:20Z",
                ),
                block(
                    BlockKind::ToolCall,
                    Some("Bash"),
                    "2026-09-22T10:00:20Z",
                    "2026-09-22T10:00:30Z",
                ),
                block(
                    BlockKind::Text,
                    None,
                    "2026-09-22T10:01:20Z",
                    "2026-09-22T10:01:30Z",
                ),
            ],
            tool_calls: vec![
                call(
                    ToolKind::Build,
                    "cargo build",
                    "2026-09-22T10:00:20Z",
                    "2026-09-22T10:01:00Z",
                ),
                call(
                    ToolKind::SearchRead,
                    "rg",
                    "2026-09-22T10:00:30Z",
                    "2026-09-22T10:00:40Z",
                ),
            ],
            ..LatencyEvent::default()
        };
        let rows = aggregate([turn], None, None, chrono_tz::UTC, PeriodGroup::All, &[]);
        let total: f64 = rows.iter().map(|row| row.hours).sum();
        assert!((total - 100.0 / 3600.0).abs() < 1e-12);
        let share: f64 = rows.iter().filter_map(|row| row.share_of_agent_hours).sum();
        assert!((share - 1.0).abs() < 1e-12);
        let writing = find(&rows, Some(Kind::Model(BlockKind::ToolCall)), Some("Bash"));
        assert_eq!(writing.count, Some(2));
        assert!((writing.hours - 20.0 / 3600.0).abs() < 1e-12);
        let build = find(
            &rows,
            Some(Kind::Tool(ToolKind::Build)),
            Some("cargo build"),
        );
        assert!((build.hours - 25.0 / 3600.0).abs() < 1e-12);
        assert_eq!(build.average_ms, Some(40_000.0));
        let search = find(&rows, Some(Kind::Tool(ToolKind::SearchRead)), Some("rg"));
        assert!((search.hours - 5.0 / 3600.0).abs() < 1e-12);
        let other = find(&rows, None, None);
        assert_eq!(other.family, Family::Other);
        assert_eq!(other.count, None);
        assert!((other.hours - 30.0 / 3600.0).abs() < 1e-12);
        assert_eq!(rows[0].family, Family::Model);
    }

    #[test]
    fn table_nests_families_kinds_and_commands() {
        let turn = LatencyEvent {
            captured_at: at("2026-09-22T10:00:30Z"),
            duration_ms: Some(30_000),
            model_blocks: vec![block(
                BlockKind::ToolCall,
                Some("Write"),
                "2026-09-22T10:00:00Z",
                "2026-09-22T10:00:10Z",
            )],
            tool_calls: vec![call(
                ToolKind::Test,
                "cargo test",
                "2026-09-22T10:00:10Z",
                "2026-09-22T10:00:30Z",
            )],
            ..LatencyEvent::default()
        };
        let rows = aggregate([turn], None, None, chrono_tz::UTC, PeriodGroup::All, &[]);
        let table = render(&rows, ReportFormat::Table).unwrap();
        let labels: Vec<&str> = table
            .lines()
            .skip(2)
            .map(|line| {
                line.split("  ")
                    .filter(|part| !part.is_empty())
                    .nth(2)
                    .unwrap()
                    .trim()
            })
            .collect();
        assert_eq!(
            labels,
            [
                "Total",
                "Model response",
                "Tool call writing",
                "Write",
                "Tool execution",
                "Test",
                "cargo test"
            ]
        );
        assert!(table.lines().nth(2).unwrap().ends_with("100.0%"));
    }
}
