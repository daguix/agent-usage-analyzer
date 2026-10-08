use std::cmp::Ordering;
use std::collections::BTreeMap;

use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use chrono_tz::Tz;
use serde::Serialize;
use serde_json::Value;

use crate::ingest::LatencyEvent;
use crate::latency::{format_integer, format_optional_duration};
use crate::report::{GroupBy, PeriodGroup, ReportFormat};
use crate::workflow::{group_label, period_key, period_segments};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolKind {
    Build,
    Test,
    Lint,
    VersionControl,
    SearchRead,
    Edit,
    Web,
    Subagent,
    UserInput,
    Wait,
    Mcp,
    Shell,
    Other,
}

impl ToolKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Build => "Build",
            Self::Test => "Test",
            Self::Lint => "Lint / format",
            Self::VersionControl => "Version control",
            Self::SearchRead => "Search / read",
            Self::Edit => "Edit",
            Self::Web => "Web",
            Self::Subagent => "Subagent",
            Self::UserInput => "User input",
            Self::Wait => "Wait / background",
            Self::Mcp => "MCP",
            Self::Shell => "Other shell",
            Self::Other => "Other",
        }
    }

    fn command_priority(self) -> u8 {
        match self {
            Self::Test => 7,
            Self::Build => 6,
            Self::Lint => 5,
            Self::Web => 4,
            Self::VersionControl => 3,
            Self::Wait => 2,
            Self::SearchRead => 1,
            _ => 0,
        }
    }
}

pub fn classify(name: &str, input: &Value) -> (ToolKind, String) {
    let field = |key: &str| input.get(key).and_then(Value::as_str);
    match name {
        "Bash" | "PowerShell" => classify_command(field("command").unwrap_or_default()),
        "Read" | "Grep" | "Glob" | "LS" | "NotebookRead" => (ToolKind::SearchRead, name.to_owned()),
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => (ToolKind::Edit, name.to_owned()),
        "WebFetch" | "WebSearch" => (ToolKind::Web, name.to_owned()),
        "Agent" | "Task" => (
            ToolKind::Subagent,
            field("subagent_type")
                .map_or_else(|| name.to_owned(), |agent| format!("{name} ({agent})")),
        ),
        "AskUserQuestion" | "ExitPlanMode" => (ToolKind::UserInput, name.to_owned()),
        "BashOutput" | "TaskOutput" | "KillShell" | "KillBash" | "TaskStop" | "Monitor" => {
            (ToolKind::Wait, name.to_owned())
        }
        _ if name.starts_with("mcp__") => (
            ToolKind::Mcp,
            name.split("__").nth(1).unwrap_or(name).to_owned(),
        ),
        "" => (ToolKind::Other, "<unknown>".to_owned()),
        _ => (ToolKind::Other, name.to_owned()),
    }
}

pub fn classify_command(command: &str) -> (ToolKind, String) {
    let lowered = command.to_ascii_lowercase();
    lowered
        .split(['\n', ';', '|', '&'])
        .filter_map(classify_segment)
        .reduce(|best, candidate| {
            if candidate.0.command_priority() > best.0.command_priority() {
                candidate
            } else {
                best
            }
        })
        .unwrap_or_else(|| (ToolKind::Shell, "<empty>".to_owned()))
}

fn classify_segment(segment: &str) -> Option<(ToolKind, String)> {
    let mut words = segment
        .split_whitespace()
        .skip_while(|word| {
            word.contains('=')
                || matches!(
                    *word,
                    "sudo"
                        | "time"
                        | "timeout"
                        | "nice"
                        | "env"
                        | "command"
                        | "exec"
                        | "xargs"
                        | "do"
                        | "then"
                        | "else"
                        | "!"
                        | "("
                        | "{"
                )
                || word
                    .trim_end_matches(['s', 'm', 'h'])
                    .parse::<f64>()
                    .is_ok()
        })
        .map(|word| {
            word.trim_matches(|character| matches!(character, '(' | ')' | '{' | '}' | '"' | '\''))
        })
        .filter(|word| !word.is_empty());
    let mut program = words.next()?.rsplit('/').next()?.to_owned();
    if matches!(program.as_str(), "npx" | "pnpx" | "bunx" | "uvx") {
        program = words.next()?.rsplit('/').next()?.to_owned();
    }
    let rest: Vec<&str> = words.collect();
    let arguments = || {
        rest.iter()
            .enumerate()
            .filter(|(index, word)| {
                !word.starts_with(['-', '+'])
                    && !index.checked_sub(1).is_some_and(|previous| {
                        matches!(
                            rest[previous],
                            "-c" | "--git-dir" | "--work-tree" | "--manifest-path" | "--prefix"
                        )
                    })
            })
            .map(|(_, word)| *word)
            .filter(|word| {
                word.chars().all(|character| {
                    character.is_ascii_alphanumeric() || "-_:.".contains(character)
                })
            })
    };
    let subcommand = arguments().next().unwrap_or_default();
    let with_subcommand = |kind| {
        if subcommand.is_empty() {
            (kind, program.clone())
        } else {
            (kind, format!("{program} {subcommand}"))
        }
    };
    let kind = match program.as_str() {
        "cargo" => match subcommand {
            "test" | "t" | "nextest" | "bench" | "miri" => ToolKind::Test,
            "build" | "b" | "check" | "c" | "doc" | "install" => ToolKind::Build,
            "clippy" | "fmt" => ToolKind::Lint,
            _ => ToolKind::Shell,
        },
        "npm" | "pnpm" | "yarn" | "bun" => {
            let script = if subcommand == "run" {
                arguments().nth(1).unwrap_or_default()
            } else {
                subcommand
            };
            let kind = script_kind(script);
            return Some(if subcommand == "run" && !script.is_empty() {
                (kind, format!("{program} run {script}"))
            } else {
                with_subcommand(kind)
            });
        }
        "go" => match subcommand {
            "test" => ToolKind::Test,
            "build" | "install" => ToolKind::Build,
            "vet" | "fmt" => ToolKind::Lint,
            _ => ToolKind::Shell,
        },
        "dotnet" | "swift" | "mvn" | "gradle" | "gradlew" | "bazel" | "make" | "just" => {
            match subcommand {
                "test" | "check" | "verify" => ToolKind::Test,
                "build" | "compile" | "package" | "install" | "assemble" | "all" => ToolKind::Build,
                "" if program == "make" => ToolKind::Build,
                _ => match script_kind(subcommand) {
                    ToolKind::Shell if program == "make" => ToolKind::Build,
                    kind => kind,
                },
            }
        }
        "python" | "python3" | "uv" => {
            if rest.contains(&"pytest") {
                return Some((ToolKind::Test, "pytest".to_owned()));
            }
            return Some((ToolKind::Shell, program));
        }
        "cmake" | "ninja" | "meson" | "gcc" | "g++" | "clang" | "clang++" | "cc" | "c++"
        | "rustc" | "javac" | "tsc" | "swiftc" | "webpack" | "esbuild" | "rollup" | "vite"
        | "wasm-pack" | "trunk" | "maturin" => {
            if program == "tsc" && rest.contains(&"--noemit") {
                ToolKind::Lint
            } else {
                ToolKind::Build
            }
        }
        "pytest" | "jest" | "vitest" | "mocha" | "ctest" | "phpunit" | "rspec" | "tox" | "nox"
        | "playwright" | "cypress" => ToolKind::Test,
        "eslint" | "prettier" | "ruff" | "mypy" | "pyright" | "black" | "flake8" | "pylint"
        | "isort" | "rustfmt" | "golangci-lint" | "shellcheck" | "biome" | "stylelint"
        | "clang-format" | "clang-tidy" | "taplo" => ToolKind::Lint,
        "git" | "gh" | "jj" => ToolKind::VersionControl,
        "curl" | "wget" | "http" | "xh" => return Some((ToolKind::Web, program)),
        "rg" | "grep" | "find" | "fd" | "ls" | "cat" | "sed" | "head" | "tail" | "wc" | "tree"
        | "less" | "bat" | "awk" => return Some((ToolKind::SearchRead, program)),
        "sleep" | "wait" => return Some((ToolKind::Wait, program)),
        _ => return Some((ToolKind::Shell, program)),
    };
    Some(with_subcommand(kind))
}

fn script_kind(script: &str) -> ToolKind {
    if script.contains("test") || script.contains("e2e") {
        ToolKind::Test
    } else if script.contains("build") || script.contains("compile") {
        ToolKind::Build
    } else if ["lint", "fmt", "format", "typecheck", "check"]
        .iter()
        .any(|name| script.contains(name))
    {
        ToolKind::Lint
    } else {
        ToolKind::Shell
    }
}

pub(crate) fn merge_intervals(
    mut intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
) -> Vec<(DateTime<Utc>, DateTime<Utc>)> {
    intervals.sort_unstable();
    let mut merged = Vec::<(DateTime<Utc>, DateTime<Utc>)>::new();
    for (interval_start, interval_end) in intervals {
        let interval_start = interval_start.max(start);
        let interval_end = interval_end.min(end);
        if interval_end <= interval_start {
            continue;
        }
        match merged.last_mut() {
            Some(last) if interval_start <= last.1 => last.1 = last.1.max(interval_end),
            _ => merged.push((interval_start, interval_end)),
        }
    }
    merged
}

#[derive(Debug, Serialize)]
pub struct ToolRow {
    pub period: String,
    pub group: String,
    pub kind: ToolKind,
    pub command: String,
    pub calls: u64,
    pub hours: f64,
    pub average_call_ms: Option<f64>,
    pub share_of_agent_hours: Option<f64>,
}

#[derive(Default)]
struct Entry {
    calls: u64,
    call_ms: i64,
    wall_ms: i64,
}

pub fn aggregate(
    events: impl IntoIterator<Item = LatencyEvent>,
    range_start: Option<DateTime<Utc>>,
    range_end: Option<DateTime<Utc>>,
    timezone: Tz,
    period: PeriodGroup,
    by: &[GroupBy],
) -> Vec<ToolRow> {
    let lower = range_start.unwrap_or(DateTime::<Utc>::MIN_UTC);
    let upper = range_end.unwrap_or(DateTime::<Utc>::MAX_UTC);
    let mut agent_ms = BTreeMap::<(String, String), i64>::new();
    let mut entries = BTreeMap::<(String, String, ToolKind, String), Entry>::new();
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
        for (key, segment_start, segment_end) in period_segments(
            start.max(lower),
            event.captured_at.min(upper),
            timezone,
            period,
        ) {
            *agent_ms.entry((key, group.clone())).or_default() +=
                (segment_end - segment_start).num_milliseconds();
        }
        let mut intervals = BTreeMap::<(ToolKind, &str), Vec<_>>::new();
        for call in &event.tool_calls {
            intervals
                .entry((call.kind, call.command.as_str()))
                .or_default()
                .push((call.started_at, call.ended_at));
            if call.started_at < lower || call.started_at >= upper {
                continue;
            }
            let entry = entries
                .entry((
                    period_key(call.started_at, timezone, period),
                    group.clone(),
                    call.kind,
                    call.command.clone(),
                ))
                .or_default();
            entry.calls += 1;
            entry.call_ms += (call.ended_at - call.started_at).num_milliseconds();
        }
        for ((kind, command), calls) in intervals {
            for (interval_start, interval_end) in merge_intervals(calls, lower, upper) {
                for (key, segment_start, segment_end) in
                    period_segments(interval_start, interval_end, timezone, period)
                {
                    entries
                        .entry((key, group.clone(), kind, command.to_owned()))
                        .or_default()
                        .wall_ms += (segment_end - segment_start).num_milliseconds();
                }
            }
        }
    }
    let mut rows: Vec<ToolRow> = entries
        .into_iter()
        .map(|((period, group, kind, command), entry)| {
            let agent = agent_ms
                .get(&(period.clone(), group.clone()))
                .copied()
                .unwrap_or_default();
            ToolRow {
                hours: entry.wall_ms as f64 / 3_600_000.0,
                average_call_ms: (entry.calls > 0)
                    .then(|| entry.call_ms as f64 / entry.calls as f64),
                share_of_agent_hours: (agent > 0).then(|| entry.wall_ms as f64 / agent as f64),
                calls: entry.calls,
                period,
                group,
                kind,
                command,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        (&left.period, &left.group)
            .cmp(&(&right.period, &right.group))
            .then_with(|| {
                right
                    .hours
                    .partial_cmp(&left.hours)
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| (left.kind, &left.command).cmp(&(right.kind, &right.command)))
    });
    rows
}

pub fn render(rows: &[ToolRow], format: ReportFormat) -> Result<String> {
    match format {
        ReportFormat::Json => Ok(serde_json::to_string_pretty(rows)?),
        ReportFormat::Csv => {
            let mut writer = csv::WriterBuilder::new()
                .has_headers(false)
                .from_writer(Vec::new());
            writer.write_record([
                "period",
                "group",
                "kind",
                "command",
                "calls",
                "hours",
                "average_call_ms",
                "share_of_agent_hours",
            ])?;
            for row in rows {
                writer.serialize(row)?;
            }
            Ok(String::from_utf8(writer.into_inner()?)?
                .trim_end_matches('\n')
                .to_owned())
        }
        ReportFormat::Table => {
            let headers = [
                "Period",
                "Group",
                "Kind",
                "Command",
                "Calls",
                "Hours",
                "Avg call",
                "Agent-hour share",
            ];
            let values: Vec<[String; 8]> = rows
                .iter()
                .map(|row| {
                    [
                        row.period.clone(),
                        row.group.clone(),
                        row.kind.label().to_owned(),
                        row.command.clone(),
                        format_integer(row.calls),
                        format!("{:.3}", row.hours),
                        format_optional_duration(row.average_call_ms),
                        row.share_of_agent_hours.map_or_else(
                            || "-".to_owned(),
                            |share| format!("{:.1}%", share * 100.0),
                        ),
                    ]
                })
                .collect();
            let mut widths = headers.map(str::len);
            for row in &values {
                for (index, value) in row.iter().enumerate() {
                    widths[index] = widths[index].max(value.chars().count());
                }
            }
            let format_row = |row: &[&str; 8]| {
                row.iter()
                    .enumerate()
                    .map(|(index, value)| {
                        if index < 4 {
                            format!("{value:<width$}", width = widths[index])
                        } else {
                            format!("{value:>width$}", width = widths[index])
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("  ")
                    .trim_end()
                    .to_owned()
            };
            let mut lines = vec![format_row(&headers)];
            lines.push(widths.map(|width| "-".repeat(width)).join("  "));
            for row in &values {
                lines.push(format_row(&row.each_ref().map(String::as_str)));
            }
            Ok(lines.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingest::ToolCall;

    fn at(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value).unwrap().to_utc()
    }

    fn call(kind: ToolKind, command: &str, start: &str, end: &str) -> ToolCall {
        ToolCall {
            kind,
            command: command.to_owned(),
            started_at: at(start),
            ended_at: at(end),
        }
    }

    #[test]
    fn classifies_shell_commands_by_their_most_significant_step() {
        assert_eq!(
            classify_command("cd /tmp/p && cargo build --release 2>&1 | tail -20"),
            (ToolKind::Build, "cargo build".to_owned())
        );
        assert_eq!(
            classify_command("cargo fmt && cargo clippy && cargo +nightly test -q"),
            (ToolKind::Test, "cargo test".to_owned())
        );
        assert_eq!(
            classify_command("RUSTFLAGS=-Dwarnings cargo check"),
            (ToolKind::Build, "cargo check".to_owned())
        );
        assert_eq!(
            classify_command("npm run build"),
            (ToolKind::Build, "npm run build".to_owned())
        );
        assert_eq!(
            classify_command("npx tsc --noEmit"),
            (ToolKind::Lint, "tsc".to_owned())
        );
        assert_eq!(
            classify_command("python -m pytest tests"),
            (ToolKind::Test, "pytest".to_owned())
        );
        assert_eq!(
            classify_command("git diff --stat"),
            (ToolKind::VersionControl, "git diff".to_owned())
        );
        assert_eq!(
            classify_command("git -C /home/user/some/long/repository status --short"),
            (ToolKind::VersionControl, "git status".to_owned())
        );
        assert_eq!(
            classify_command("rg foo src | head"),
            (ToolKind::SearchRead, "rg".to_owned())
        );
        assert_eq!(
            classify_command("/usr/bin/make -j8"),
            (ToolKind::Build, "make".to_owned())
        );
        assert_eq!(
            classify_command("for f in a b; do timeout 600 cargo test; done"),
            (ToolKind::Test, "cargo test".to_owned())
        );
        assert_eq!(
            classify_command("cargo run -- report"),
            (ToolKind::Shell, "cargo run".to_owned())
        );
    }

    #[test]
    fn classifies_claude_code_tools() {
        let input = serde_json::json!({"command": "cargo test"});
        assert_eq!(
            classify("Bash", &input),
            (ToolKind::Test, "cargo test".to_owned())
        );
        assert_eq!(
            classify("Agent", &serde_json::json!({"subagent_type": "Explore"})),
            (ToolKind::Subagent, "Agent (Explore)".to_owned())
        );
        assert_eq!(
            classify("mcp__github__create_issue", &Value::Null),
            (ToolKind::Mcp, "github".to_owned())
        );
        assert_eq!(classify("Read", &Value::Null).0, ToolKind::SearchRead);
        assert_eq!(
            classify("AskUserQuestion", &Value::Null).0,
            ToolKind::UserInput
        );
    }

    #[test]
    fn concurrent_calls_of_one_command_count_once_and_share_uses_agent_hours() {
        let turn = LatencyEvent {
            captured_at: at("2026-09-22T11:00:00Z"),
            duration_ms: Some(3_600_000),
            tool_calls: vec![
                call(
                    ToolKind::Build,
                    "cargo build",
                    "2026-09-22T10:00:00Z",
                    "2026-09-22T10:10:00Z",
                ),
                call(
                    ToolKind::Build,
                    "cargo build",
                    "2026-09-22T10:05:00Z",
                    "2026-09-22T10:15:00Z",
                ),
                call(
                    ToolKind::Test,
                    "cargo test",
                    "2026-09-22T10:30:00Z",
                    "2026-09-22T10:36:00Z",
                ),
            ],
            ..LatencyEvent::default()
        };
        let rows = aggregate([turn], None, None, chrono_tz::UTC, PeriodGroup::All, &[]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, ToolKind::Build);
        assert_eq!(rows[0].calls, 2);
        assert_eq!(rows[0].hours, 0.25);
        assert_eq!(rows[0].average_call_ms, Some(600_000.0));
        assert_eq!(rows[0].share_of_agent_hours, Some(0.25));
        assert_eq!(rows[1].kind, ToolKind::Test);
        assert_eq!(rows[1].hours, 0.1);
    }

    #[test]
    fn tool_time_is_split_across_days_and_clipped_to_the_range() {
        let turn = LatencyEvent {
            captured_at: at("2026-09-23T02:00:00Z"),
            duration_ms: Some(14_400_000),
            tool_calls: vec![call(
                ToolKind::Build,
                "make",
                "2026-09-22T23:00:00Z",
                "2026-09-23T01:00:00Z",
            )],
            ..LatencyEvent::default()
        };
        let rows = aggregate(
            [turn],
            None,
            Some(at("2026-09-23T00:30:00Z")),
            chrono_tz::UTC,
            PeriodGroup::Day,
            &[],
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].period, "2026-09-22");
        assert_eq!(rows[0].calls, 1);
        assert_eq!(rows[0].hours, 1.0);
        assert_eq!(rows[0].share_of_agent_hours, Some(0.5));
        assert_eq!(rows[1].period, "2026-09-23");
        assert_eq!(rows[1].calls, 0);
        assert_eq!(rows[1].hours, 0.5);
        assert_eq!(rows[1].average_call_ms, None);
    }
}
