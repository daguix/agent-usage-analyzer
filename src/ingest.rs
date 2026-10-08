use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::activity::BlockKind;
use crate::tools::ToolKind;

#[derive(Clone, Debug, Default, Serialize)]
pub struct UsageEvent {
    pub captured_at: DateTime<Utc>,
    pub total_tokens: u64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub cache_write_5m_tokens: u64,
    pub cache_write_1h_tokens: u64,
    pub speed: Option<String>,
    pub context_window: Option<u64>,
    pub primary_limit: Option<RateLimitWindow>,
    pub secondary_limit: Option<RateLimitWindow>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub directory: Option<String>,
    pub branch: Option<String>,
    pub origin: Option<String>,
    pub session_id: Option<String>,
    pub codex_version: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct LatencyEvent {
    pub captured_at: DateTime<Utc>,
    pub duration_ms: Option<u64>,
    pub time_to_first_token_ms: Option<u64>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub directory: Option<String>,
    pub branch: Option<String>,
    pub origin: Option<String>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub tool_intervals: Option<Vec<(DateTime<Utc>, DateTime<Utc>)>>,
    pub tool_calls: Vec<ToolCall>,
    pub model_blocks: Vec<ModelBlock>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ModelBlock {
    pub kind: BlockKind,
    pub tool: Option<String>,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ToolCall {
    pub kind: ToolKind,
    pub command: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RateLimitWindow {
    pub percent_left: f64,
    pub window_minutes: Option<u64>,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    pub events: Vec<UsageEvent>,
    pub latencies: Vec<LatencyEvent>,
    pub files: usize,
    pub invalid_lines: usize,
}

#[derive(Debug, Default)]
struct FileResult {
    events: Vec<UsageEvent>,
    latencies: Vec<LatencyEvent>,
    invalid_lines: usize,
}

#[derive(Clone, Debug, Default)]
struct Context {
    model: Option<String>,
    effort: Option<String>,
    directory: Option<String>,
    branch: Option<String>,
    origin: Option<String>,
    session_id: Option<String>,
    codex_version: Option<String>,
}

pub fn scan_rollouts(root: &Path) -> Result<ScanResult> {
    if !root.exists() {
        return Ok(ScanResult::default());
    }
    let files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy();
            name.starts_with("rollout-") && name.ends_with(".jsonl")
        })
        .map(|entry| entry.into_path())
        .collect();

    let parsed: Vec<Result<FileResult>> = files.par_iter().map(|path| scan_file(path)).collect();
    let mut result = ScanResult {
        files: files.len(),
        ..ScanResult::default()
    };
    for file in parsed {
        let file = file?;
        result.invalid_lines += file.invalid_lines;
        result.events.extend(file.events);
        result.latencies.extend(file.latencies);
    }
    Ok(result)
}

fn scan_file(path: &Path) -> Result<FileResult> {
    let reader = BufReader::new(File::open(path)?);
    let mut context = Context::default();
    let mut result = FileResult::default();
    for line in reader.lines() {
        let line = match line {
            Ok(line) => line,
            Err(_) => {
                result.invalid_lines += 1;
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => {
                result.invalid_lines += 1;
                continue;
            }
        };
        parse_value(
            &value,
            &mut context,
            &mut result.events,
            &mut result.latencies,
        );
    }
    let mut seen = HashSet::new();
    result.events.retain(|event| {
        seen.insert((
            event.captured_at,
            event.total_tokens,
            event.input_tokens,
            event.cached_input_tokens,
            event.output_tokens,
            event.reasoning_output_tokens,
            event.session_id.clone(),
        ))
    });
    let mut seen_latencies = HashSet::new();
    result.latencies.retain(|event| {
        seen_latencies.insert((
            event.captured_at,
            event.duration_ms,
            event.time_to_first_token_ms,
            event.session_id.clone(),
            event.turn_id.clone(),
        ))
    });
    Ok(result)
}

fn parse_value(
    value: &Value,
    context: &mut Context,
    events: &mut Vec<UsageEvent>,
    latencies: &mut Vec<LatencyEvent>,
) {
    let Some(item_type) = value.get("type").and_then(Value::as_str) else {
        return;
    };
    let payload = value.get("payload").unwrap_or(&Value::Null);
    match item_type {
        "session_meta" => {
            replace_string(&mut context.session_id, payload.get("id"));
            replace_string(&mut context.directory, payload.get("cwd"));
            replace_string(&mut context.codex_version, payload.get("cli_version"));
            replace_string(
                &mut context.branch,
                payload.get("git").and_then(|git| git.get("branch")),
            );
            if payload
                .get("source")
                .is_some_and(|source| source.get("subagent").is_some())
            {
                context.origin = Some("subagent".to_owned());
            }
        }
        "turn_context" => {
            replace_string(&mut context.model, payload.get("model"));
            replace_string(
                &mut context.effort,
                payload
                    .get("effort")
                    .or_else(|| payload.get("reasoning_effort")),
            );
            replace_string(&mut context.directory, payload.get("cwd"));
        }
        "event_msg" if payload.get("type").and_then(Value::as_str) == Some("token_count") => {
            let event_payload = payload
                .get("payload")
                .filter(|value| value.is_object())
                .unwrap_or(payload);
            let Some(timestamp) = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_timestamp)
            else {
                return;
            };
            let info = event_payload.get("info").unwrap_or(&Value::Null);
            let last = info.get("last_token_usage").unwrap_or(&Value::Null);
            let limits = event_payload.get("rate_limits").unwrap_or(&Value::Null);
            events.push(UsageEvent {
                captured_at: timestamp,
                total_tokens: number(last, "total_tokens"),
                input_tokens: number(last, "input_tokens"),
                cached_input_tokens: number(last, "cached_input_tokens"),
                output_tokens: number(last, "output_tokens"),
                reasoning_output_tokens: number(last, "reasoning_output_tokens"),
                cache_write_5m_tokens: 0,
                cache_write_1h_tokens: 0,
                speed: None,
                context_window: optional_number(info, "model_context_window"),
                primary_limit: rate_limit_window(limits.get("primary")),
                secondary_limit: rate_limit_window(limits.get("secondary")),
                model: context.model.clone(),
                effort: context.effort.clone(),
                directory: context.directory.clone(),
                branch: context.branch.clone(),
                origin: context.origin.clone(),
                session_id: context.session_id.clone(),
                codex_version: context.codex_version.clone(),
            });
        }
        "event_msg" if payload.get("type").and_then(Value::as_str) == Some("task_complete") => {
            let duration_ms = optional_number(payload, "duration_ms");
            let time_to_first_token_ms = optional_number(payload, "time_to_first_token_ms");
            if duration_ms.is_none() && time_to_first_token_ms.is_none() {
                return;
            }
            let captured_at = value
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(parse_timestamp)
                .or_else(|| {
                    optional_number(payload, "completed_at")
                        .and_then(|value| i64::try_from(value).ok())
                        .and_then(|value| DateTime::from_timestamp(value, 0))
                });
            let Some(captured_at) = captured_at else {
                return;
            };
            latencies.push(LatencyEvent {
                captured_at,
                duration_ms,
                time_to_first_token_ms,
                model: context.model.clone(),
                effort: context.effort.clone(),
                directory: context.directory.clone(),
                branch: context.branch.clone(),
                origin: context.origin.clone(),
                session_id: context.session_id.clone(),
                turn_id: payload
                    .get("turn_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                tool_intervals: None,
                tool_calls: Vec::new(),
                model_blocks: Vec::new(),
            });
        }
        _ => {}
    }
}

fn replace_string(target: &mut Option<String>, value: Option<&Value>) {
    if let Some(value) = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        *target = Some(value.to_owned());
    }
}

pub(crate) fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.to_utc())
}

pub(crate) fn number(value: &Value, key: &str) -> u64 {
    optional_number(value, key).unwrap_or(0)
}

pub(crate) fn optional_number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_i64().and_then(|value| u64::try_from(value).ok()))
    })
}

fn percent_left(value: Option<&Value>) -> Option<f64> {
    value?
        .get("used_percent")?
        .as_f64()
        .map(|used| (100.0 - used).clamp(0.0, 100.0))
}

fn rate_limit_window(value: Option<&Value>) -> Option<RateLimitWindow> {
    let value = value?;
    Some(RateLimitWindow {
        percent_left: percent_left(Some(value))?,
        window_minutes: optional_number(value, "window_minutes"),
        resets_at: optional_number(value, "resets_at")
            .and_then(|value| i64::try_from(value).ok())
            .and_then(|value| DateTime::from_timestamp(value, 0)),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_token_payload_and_context() {
        let mut context = Context::default();
        let mut events = Vec::new();
        let mut latencies = Vec::new();
        parse_value(
            &serde_json::json!({"type":"session_meta","payload":{
                "id":"s","cwd":"/tmp/p","git":{"branch":"feature"},"source":{"subagent":{"other":"guardian"}}
            }}),
            &mut context,
            &mut events,
            &mut latencies,
        );
        parse_value(
            &serde_json::json!({"type":"turn_context","payload":{"model":"gpt-5.2","effort":"high"}}),
            &mut context,
            &mut events,
            &mut latencies,
        );
        parse_value(
            &serde_json::json!({
                "timestamp":"2026-09-22T10:00:00Z",
                "type":"event_msg",
                "payload":{"type":"token_count","payload":{"info":{"last_token_usage":{
                    "total_tokens":16,"input_tokens":12,"cached_input_tokens":2,"output_tokens":4
                }}}}
            }),
            &mut context,
            &mut events,
            &mut latencies,
        );
        assert_eq!(events[0].total_tokens, 16);
        assert_eq!(events[0].model.as_deref(), Some("gpt-5.2"));
        assert_eq!(events[0].effort.as_deref(), Some("high"));
        assert_eq!(events[0].session_id.as_deref(), Some("s"));
        assert_eq!(events[0].branch.as_deref(), Some("feature"));
        assert_eq!(events[0].origin.as_deref(), Some("subagent"));
    }

    #[test]
    fn parses_task_latency_and_context() {
        let mut context = Context {
            model: Some("gpt-5.2".to_owned()),
            effort: Some("high".to_owned()),
            directory: Some("/tmp/p".to_owned()),
            session_id: Some("s".to_owned()),
            ..Context::default()
        };
        let mut events = Vec::new();
        let mut latencies = Vec::new();
        parse_value(
            &serde_json::json!({
                "timestamp":"2026-09-22T10:00:05Z",
                "type":"event_msg",
                "payload":{
                    "type":"task_complete",
                    "turn_id":"t",
                    "duration_ms":5000,
                    "time_to_first_token_ms":1200
                }
            }),
            &mut context,
            &mut events,
            &mut latencies,
        );
        assert_eq!(latencies[0].duration_ms, Some(5000));
        assert_eq!(latencies[0].time_to_first_token_ms, Some(1200));
        assert_eq!(latencies[0].model.as_deref(), Some("gpt-5.2"));
        assert_eq!(latencies[0].turn_id.as_deref(), Some("t"));
    }
}
