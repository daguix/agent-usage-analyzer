use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use rayon::prelude::*;
use serde_json::Value;
use walkdir::WalkDir;

use crate::ingest::{
    LatencyEvent, ScanResult, UsageEvent, number, optional_number, parse_timestamp,
};

#[derive(Debug, Default)]
struct FileResult {
    usage: Vec<(String, UsageEvent)>,
    turns: Vec<LatencyEvent>,
    invalid_lines: usize,
}

#[derive(Debug)]
struct Turn {
    id: Option<String>,
    started_at: DateTime<Utc>,
    ended_at: Option<DateTime<Utc>>,
    model: Option<String>,
    effort: Option<String>,
    directory: Option<String>,
    branch: Option<String>,
    origin: Option<String>,
    session_id: Option<String>,
    tool_starts: HashMap<String, DateTime<Utc>>,
    tool_intervals: Vec<(DateTime<Utc>, DateTime<Utc>)>,
}

impl Turn {
    fn finish(self) -> Option<LatencyEvent> {
        let ended_at = self.ended_at?;
        let duration_ms = u64::try_from((ended_at - self.started_at).num_milliseconds()).ok()?;
        let tool_intervals = merge_intervals(self.tool_intervals, self.started_at, ended_at);
        Some(LatencyEvent {
            captured_at: ended_at,
            duration_ms: Some(duration_ms),
            time_to_first_token_ms: None,
            model: self.model,
            effort: self.effort,
            directory: self.directory,
            branch: self.branch,
            origin: self.origin,
            session_id: self.session_id,
            turn_id: self.id,
            tool_intervals: Some(tool_intervals),
        })
    }
}

fn merge_intervals(
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

pub fn scan_projects(root: &Path) -> Result<ScanResult> {
    if !root.exists() {
        return Ok(ScanResult::default());
    }
    let files: Vec<PathBuf> = WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".jsonl"))
        .map(|entry| entry.into_path())
        .collect();

    let parsed: Vec<Result<FileResult>> = files.par_iter().map(|path| scan_file(path)).collect();
    let mut result = ScanResult {
        files: files.len(),
        ..ScanResult::default()
    };
    let mut usage = HashMap::<String, UsageEvent>::new();
    let mut turns = HashMap::<String, LatencyEvent>::new();
    for file in parsed {
        let file = file?;
        result.invalid_lines += file.invalid_lines;
        for (id, event) in file.usage {
            keep_largest(&mut usage, id, event, |event| event.output_tokens);
        }
        for turn in file.turns {
            match turn.turn_id.clone() {
                Some(id) => keep_largest(&mut turns, id, turn, |turn| {
                    turn.duration_ms.unwrap_or_default()
                }),
                None => result.latencies.push(turn),
            }
        }
    }
    result.events.extend(usage.into_values());
    result.latencies.extend(turns.into_values());
    Ok(result)
}

fn keep_largest<T>(map: &mut HashMap<String, T>, key: String, value: T, size: impl Fn(&T) -> u64) {
    match map.entry(key) {
        Entry::Occupied(mut entry) => {
            if size(&value) > size(entry.get()) {
                entry.insert(value);
            }
        }
        Entry::Vacant(entry) => {
            entry.insert(value);
        }
    }
}

fn scan_file(path: &Path) -> Result<FileResult> {
    let reader = BufReader::new(File::open(path)?);
    let mut result = FileResult::default();
    let mut usage = HashMap::<String, UsageEvent>::new();
    let mut turn = None::<Turn>;
    for line in reader.lines() {
        let Ok(line) = line else {
            result.invalid_lines += 1;
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            result.invalid_lines += 1;
            continue;
        };
        parse_value(&value, &mut usage, &mut turn, &mut result.turns);
    }
    if let Some(event) = turn.and_then(Turn::finish) {
        result.turns.push(event);
    }
    result.usage = usage.into_iter().collect();
    Ok(result)
}

fn parse_value(
    value: &Value,
    usage: &mut HashMap<String, UsageEvent>,
    turn: &mut Option<Turn>,
    turns: &mut Vec<LatencyEvent>,
) {
    let Some(timestamp) = value
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_timestamp)
    else {
        return;
    };
    match value.get("type").and_then(Value::as_str) {
        Some("user") if is_prompt(value) => {
            if let Some(event) = turn.take().and_then(Turn::finish) {
                turns.push(event);
            }
            *turn = Some(Turn {
                id: string(value, "uuid"),
                started_at: timestamp,
                ended_at: None,
                model: None,
                effort: None,
                directory: string(value, "cwd"),
                branch: string(value, "gitBranch"),
                origin: sidechain_origin(value).or_else(|| string(value, "turnOrigin")),
                session_id: string(value, "sessionId"),
                tool_starts: HashMap::new(),
                tool_intervals: Vec::new(),
            });
        }
        Some("user") => {
            if let Some(turn) = turn.as_mut() {
                for id in content_blocks(value, "tool_result", "tool_use_id") {
                    if let Some(start) = turn.tool_starts.remove(id) {
                        turn.tool_intervals.push((start, timestamp));
                    }
                }
            }
        }
        Some("assistant") => {
            let message = value.get("message").unwrap_or(&Value::Null);
            let model = string(message, "model").filter(|model| model != "<synthetic>");
            let effort = string(value, "effort");
            let origin = sidechain_origin(value)
                .or_else(|| turn.as_ref().and_then(|turn| turn.origin.clone()));
            if let Some(turn) = turn.as_mut() {
                turn.ended_at = Some(timestamp);
                for id in content_blocks(value, "tool_use", "id") {
                    turn.tool_starts.entry(id.to_owned()).or_insert(timestamp);
                }
                if model.is_some() {
                    turn.model.clone_from(&model);
                }
                if effort.is_some() {
                    turn.effort.clone_from(&effort);
                }
            }
            let Some(model) = model else {
                return;
            };
            let Some(tokens) = message.get("usage").filter(|usage| usage.is_object()) else {
                return;
            };
            let Some(id) = string(message, "id").or_else(|| string(value, "uuid")) else {
                return;
            };
            let event = usage_event(value, tokens, timestamp, model, effort, origin);
            keep_largest(usage, id, event, |event| event.output_tokens);
        }
        _ => {}
    }
}

fn usage_event(
    value: &Value,
    tokens: &Value,
    captured_at: DateTime<Utc>,
    model: String,
    effort: Option<String>,
    origin: Option<String>,
) -> UsageEvent {
    let uncached = number(tokens, "input_tokens");
    let cache_read = number(tokens, "cache_read_input_tokens");
    let cache_write = number(tokens, "cache_creation_input_tokens");
    let split = tokens
        .get("cache_creation")
        .filter(|split| split.is_object());
    let cache_write_1h = split.map_or(0, |split| number(split, "ephemeral_1h_input_tokens"));
    let cache_write_5m = split
        .and_then(|split| optional_number(split, "ephemeral_5m_input_tokens"))
        .unwrap_or_else(|| cache_write.saturating_sub(cache_write_1h));
    let input_tokens = uncached + cache_read + cache_write_5m + cache_write_1h;
    let output_tokens = number(tokens, "output_tokens");
    UsageEvent {
        captured_at,
        total_tokens: input_tokens + output_tokens,
        input_tokens,
        cached_input_tokens: cache_read,
        output_tokens,
        reasoning_output_tokens: tokens
            .get("output_tokens_details")
            .map_or(0, |details| number(details, "thinking_tokens")),
        cache_write_5m_tokens: cache_write_5m,
        cache_write_1h_tokens: cache_write_1h,
        speed: string(tokens, "speed"),
        model: Some(model),
        effort,
        directory: string(value, "cwd"),
        branch: string(value, "gitBranch"),
        origin,
        session_id: string(value, "sessionId"),
        ..UsageEvent::default()
    }
}

fn is_prompt(value: &Value) -> bool {
    if value.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
        || value.get("toolUseResult").is_some()
        || (value.get("isMeta").and_then(Value::as_bool) == Some(true)
            && value.get("turnOrigin").is_none())
    {
        return false;
    }
    match value
        .get("message")
        .and_then(|message| message.get("content"))
    {
        Some(Value::String(_)) => true,
        Some(Value::Array(blocks)) => blocks
            .iter()
            .all(|block| block.get("type").and_then(Value::as_str) != Some("tool_result")),
        _ => false,
    }
}

fn sidechain_origin(value: &Value) -> Option<String> {
    (value.get("isSidechain").and_then(Value::as_bool) == Some(true)).then(|| "subagent".to_owned())
}

fn content_blocks<'a>(
    value: &'a Value,
    kind: &'a str,
    key: &'a str,
) -> impl Iterator<Item = &'a str> + 'a {
    value
        .get("message")
        .and_then(|message| message.get("content"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(move |block| block.get("type").and_then(Value::as_str) == Some(kind))
        .filter_map(move |block| block.get(key).and_then(Value::as_str))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(lines: &[Value]) -> (HashMap<String, UsageEvent>, Vec<LatencyEvent>) {
        let mut usage = HashMap::new();
        let mut turn = None;
        let mut turns = Vec::new();
        for line in lines {
            parse_value(line, &mut usage, &mut turn, &mut turns);
        }
        turns.extend(turn.and_then(Turn::finish));
        (usage, turns)
    }

    fn assistant(timestamp: &str, output_tokens: u64) -> Value {
        serde_json::json!({
            "type": "assistant",
            "timestamp": timestamp,
            "cwd": "/tmp/p",
            "sessionId": "s",
            "effort": "high",
            "message": {
                "id": "msg_1",
                "model": "claude-opus-5-5",
                "usage": {
                    "input_tokens": 2,
                    "cache_creation_input_tokens": 100,
                    "cache_read_input_tokens": 1000,
                    "output_tokens": output_tokens,
                    "output_tokens_details": {"thinking_tokens": 3},
                    "cache_creation": {
                        "ephemeral_5m_input_tokens": 40,
                        "ephemeral_1h_input_tokens": 60
                    },
                    "speed": "standard"
                }
            }
        })
    }

    #[test]
    fn keeps_the_final_usage_of_a_streamed_message() {
        let (usage, _) = feed(&[
            assistant("2026-09-22T10:00:01Z", 7),
            assistant("2026-09-22T10:00:02Z", 50),
        ]);
        let event = &usage["msg_1"];
        assert_eq!(usage.len(), 1);
        assert_eq!(event.input_tokens, 1102);
        assert_eq!(event.cached_input_tokens, 1000);
        assert_eq!(event.cache_write_5m_tokens, 40);
        assert_eq!(event.cache_write_1h_tokens, 60);
        assert_eq!(event.output_tokens, 50);
        assert_eq!(event.total_tokens, 1152);
        assert_eq!(event.reasoning_output_tokens, 3);
        assert_eq!(event.effort.as_deref(), Some("high"));
        assert_eq!(event.directory.as_deref(), Some("/tmp/p"));
    }

    #[test]
    fn records_branch_origin_and_tool_time() {
        let (usage, turns) = feed(&[
            serde_json::json!({
                "type": "user",
                "uuid": "u1",
                "timestamp": "2026-09-22T10:00:00Z",
                "gitBranch": "feature",
                "turnOrigin": "task_notification",
                "message": {"role": "user", "content": "done"}
            }),
            serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-09-22T10:00:02Z",
                "gitBranch": "feature",
                "message": {
                    "id": "msg_1",
                    "model": "claude-opus-5-5",
                    "content": [
                        {"type": "tool_use", "id": "a"},
                        {"type": "tool_use", "id": "b"}
                    ],
                    "usage": {"output_tokens": 1}
                }
            }),
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-09-22T10:00:05Z",
                "toolUseResult": {},
                "message": {"content": [{"type": "tool_result", "tool_use_id": "a"}]}
            }),
            serde_json::json!({
                "type": "user",
                "timestamp": "2026-09-22T10:00:07Z",
                "toolUseResult": {},
                "message": {"content": [{"type": "tool_result", "tool_use_id": "b"}]}
            }),
            serde_json::json!({
                "type": "assistant",
                "timestamp": "2026-09-22T10:00:10Z",
                "message": {"id": "msg_2", "model": "claude-opus-5-5", "content": []}
            }),
            serde_json::json!({
                "type": "user",
                "uuid": "u2",
                "timestamp": "2026-09-22T10:01:00Z",
                "isMeta": true,
                "turnOrigin": "peer",
                "message": {"role": "user", "content": "hand-back"}
            }),
        ]);
        assert_eq!(usage["msg_1"].branch.as_deref(), Some("feature"));
        assert_eq!(usage["msg_1"].origin.as_deref(), Some("task_notification"));
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].branch.as_deref(), Some("feature"));
        assert_eq!(turns[0].origin.as_deref(), Some("task_notification"));
        let tools = turns[0].tool_intervals.as_ref().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!((tools[0].1 - tools[0].0).num_seconds(), 5);
    }

    #[test]
    fn sidechain_messages_are_attributed_to_subagents() {
        let mut line = assistant("2026-09-22T10:00:01Z", 7);
        line["isSidechain"] = serde_json::json!(true);
        let (usage, _) = feed(&[line]);
        assert_eq!(usage["msg_1"].origin.as_deref(), Some("subagent"));
    }

    #[test]
    fn measures_turns_from_prompt_to_last_assistant_message() {
        let (_, turns) = feed(&[
            serde_json::json!({
                "type": "user",
                "uuid": "u1",
                "timestamp": "2026-09-22T10:00:00Z",
                "sessionId": "s",
                "message": {"role": "user", "content": "hello"}
            }),
            assistant("2026-09-22T10:00:05Z", 10),
            serde_json::json!({
                "type": "user",
                "uuid": "u2",
                "timestamp": "2026-09-22T10:00:06Z",
                "toolUseResult": {},
                "message": {"role": "user", "content": [{"type": "tool_result"}]}
            }),
            assistant("2026-09-22T10:00:09Z", 20),
            serde_json::json!({
                "type": "user",
                "uuid": "u3",
                "timestamp": "2026-09-22T10:05:00Z",
                "message": {"role": "user", "content": [{"type": "text", "text": "next"}]}
            }),
        ]);
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].duration_ms, Some(9_000));
        assert_eq!(turns[0].turn_id.as_deref(), Some("u1"));
        assert_eq!(turns[0].model.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(turns[0].effort.as_deref(), Some("high"));
    }
}
