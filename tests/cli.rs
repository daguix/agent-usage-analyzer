use std::process::Command;

#[test]
fn report_matches_fixture_totals() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--by",
            "model",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["period"], "All");
    assert_eq!(rows[0]["total_tokens"], 155);
    assert_eq!(rows[1]["period"], "All");
    assert_eq!(rows[1]["total_tokens"], 310);
}

#[test]
fn report_can_aggregate_the_entire_range() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--group",
            "all",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["period"], "All");
    assert_eq!(rows[0]["total_tokens"], 465);
    assert!(rows[0].get("duration_samples").is_none());

    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--group",
            "all",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        text.lines().filter(|line| line.starts_with("All ")).count(),
        1
    );
}

#[test]
fn latency_view_groups_by_model() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "latency",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--by",
            "model",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["group"], "gpt-5.2-codex");
    assert_eq!(rows[0]["average_duration_ms"], 5000.0);
    assert_eq!(rows[0]["average_ttft_ms"], 1000.0);
    assert_eq!(rows[1]["group"], "gpt-5.6-luna");
    assert_eq!(rows[1]["average_duration_ms"], 15000.0);
    assert_eq!(rows[1]["average_ttft_ms"], 3000.0);
}

#[test]
fn latency_view_aggregates_the_entire_range() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "latency",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["period"], "All");
    assert_eq!(rows[0]["duration_samples"], 2);
    assert_eq!(rows[0]["average_duration_ms"], 10000.0);
    assert_eq!(rows[0]["p50_duration_ms"], 5000);
    assert_eq!(rows[0]["p95_duration_ms"], 15000);
    assert_eq!(rows[0]["ttft_samples"], 2);
    assert_eq!(rows[0]["average_ttft_ms"], 2000.0);
    assert_eq!(rows[0]["p50_ttft_ms"], 1000);
    assert_eq!(rows[0]["p95_ttft_ms"], 3000);
}

#[test]
fn workflow_defaults_to_all_periods() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "workflow",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--timezone",
            "UTC",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["period"], "All");
    assert_eq!(rows[0]["group"], "all");
    assert_eq!(rows[0]["agent_hours"], 20.0 / 3600.0);
    assert_eq!(rows[0]["wall_clock_active_hours"], 20.0 / 3600.0);
    assert_eq!(rows[0]["effective_parallelism"], 1.0);
}

#[test]
fn workflow_groups_by_day_and_model() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "workflow",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--timezone",
            "UTC",
            "--group",
            "day",
            "--by",
            "model",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["period"], "2026-09-22");
    assert_eq!(rows[0]["group"], "gpt-5.2-codex");
    assert_eq!(rows[0]["agent_hours"], 5.0 / 3600.0);
    assert_eq!(rows[1]["group"], "gpt-5.6-luna");
    assert_eq!(rows[1]["agent_hours"], 15.0 / 3600.0);
}

#[test]
fn report_groups_usage_by_effort() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--by",
            "effort",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["group"], "high");
    assert_eq!(rows[0]["total_tokens"], 155);
    assert_eq!(rows[1]["group"], "medium");
    assert_eq!(rows[1]["total_tokens"], 310);
}

#[test]
fn report_groups_usage_by_model_and_effort() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--by",
            "model,effort",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["group"], "gpt-5.2-codex / high");
    assert_eq!(rows[0]["total_tokens"], 155);
    assert_eq!(rows[1]["group"], "gpt-5.6-luna / medium");
    assert_eq!(rows[1]["total_tokens"], 310);
}

#[test]
fn report_emits_telemetry_json_with_structured_dimensions() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "codex",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--by",
            "model,effort,directory,session",
            "--format",
            "telemetry-json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["schema_version"], 2);
    assert_eq!(document["event_type"], "agent.usage.report");
    assert_eq!(document["window"]["start"], "2026-09-22T08:00:02Z");
    assert_eq!(document["window"]["end"], "2026-09-22T09:00:01Z");
    assert_eq!(document["aggregation"]["period"], "all");
    assert_eq!(document["aggregation"]["dimensions"][2], "directory");
    assert_eq!(
        document["records"][0]["dimensions"]["model"],
        "gpt-5.2-codex"
    );
    assert_eq!(
        document["records"][0]["dimensions"]["directory"],
        "/tmp/project-alpha"
    );
    assert_eq!(
        document["records"][0]["dimensions"]["session"],
        "session-alpha"
    );
    assert_eq!(document["records"][0]["metrics"]["total_tokens"], 155);
    assert_eq!(document["records"][1]["metrics"]["total_tokens"], 310);
}

#[test]
fn csv_has_one_header() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "all",
            "--format",
            "csv",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert_eq!(text.matches("period,group,total_tokens").count(), 1);
}

#[test]
fn status_shows_reasoning_output_tokens() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "status",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--timezone",
            "UTC",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("Token usage: total=310 input=240 cached=40 output=60 reasoning=10"));
    assert!(text.contains("Context window: 99% left (310 used / 22,000)"));
    assert!(text.contains("5h limit: 75% left (resets 2026-09-21T14:13:20+00:00)"));
    assert!(text.contains("7d limit: 85% left (resets 2026-09-21T14:13:20+00:00)"));
}

#[test]
fn breakdown_last_emits_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "breakdown",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--last",
            "99999d",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows = document["rows"].as_array().unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row["estimated_input_tokens"].as_u64().unwrap())
            .sum::<u64>(),
        360
    );
    assert_eq!(document["cached_input_tokens"], 60);
    assert_eq!(document["output_tokens"], 90);
    assert_eq!(document["reasoning_output_tokens"], 15);
}

#[test]
fn breakdown_without_range_analyzes_everything() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "breakdown",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["calls"], 2);
    assert_eq!(document["input_tokens"], 360);
}

#[test]
fn breakdown_applies_start_and_end() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "breakdown",
            "--rollouts",
            "tests/fixtures/breakdown",
            "--from",
            "2026-09-22T10:00:02Z",
            "--to",
            "2026-09-22T10:00:05Z",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(document["calls"], 2);
}

#[test]
fn breakdown_emits_structured_paths_for_real_tool_activity() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "breakdown",
            "--rollouts",
            "tests/fixtures/breakdown",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let document: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows = document["rows"].as_array().unwrap();
    assert_eq!(document["calls"], 3);
    assert_eq!(document["input_tokens"], 420);
    assert!(document["estimated_code_input_tokens"].as_u64().unwrap() > 0);
    assert!(rows.iter().any(|row| {
        row["category"]["family"] == "tool_calls" && row["category"]["kind"] == "patch_edit_payload"
    }));
    assert!(rows.iter().any(|row| {
        row["category"]["family"] == "tool_outputs"
            && row["category"]["kind"] == "repository_source"
            && row["category"]["source"] == "rg"
    }));
}

#[test]
fn breakdown_table_renders_family_kind_and_source_levels() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "breakdown",
            "--rollouts",
            "tests/fixtures/breakdown",
            "--format",
            "table",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let table = String::from_utf8(output.stdout).unwrap();
    assert!(table.lines().any(|line| line.starts_with("Tool outputs")));
    assert!(
        table
            .lines()
            .any(|line| line.starts_with("  Repository source"))
    );
    assert!(table.lines().any(|line| line.starts_with("    rg")));
}

#[test]
fn claude_report_deduplicates_streamed_and_relocated_messages() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "claude",
            "--claude-projects",
            "tests/fixtures/claude",
            "--last",
            "all",
            "--by",
            "model",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["group"], "claude-haiku-4-5-20251001");
    assert_eq!(rows[0]["total_tokens"], 120);
    assert_eq!(rows[1]["group"], "claude-opus-5-5");
    assert_eq!(rows[1]["total_tokens"], 2385);
    assert_eq!(rows[1]["input_tokens"], 2225);
    assert_eq!(rows[1]["cached_input_tokens"], 1000);
    assert_eq!(rows[1]["cache_write_tokens"], 1200);
    assert_eq!(rows[1]["output_tokens"], 160);
    assert!((rows[1]["cache_write_cost"].as_f64().unwrap() - 0.0096).abs() < 1e-12);
    assert!((rows[1]["estimated_cost"].as_f64().unwrap() - 0.0131).abs() < 1e-12);
}

#[test]
fn claude_workflow_counts_subagent_turns_in_parallel() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "workflow",
            "--source",
            "claude",
            "--claude-projects",
            "tests/fixtures/claude",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["agent_hours"], 35.0 / 3600.0);
    assert_eq!(rows[0]["wall_clock_active_hours"], 30.0 / 3600.0);
    assert_eq!(rows[0]["tool_hours"], 2.0 / 3600.0);
    assert_eq!(rows[0]["tool_share"], 2.0 / 35.0);
}

#[test]
fn claude_report_groups_by_branch_and_origin() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--source",
            "claude",
            "--claude-projects",
            "tests/fixtures/claude",
            "--last",
            "all",
            "--by",
            "branch,origin",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 2);
    assert_eq!(rows[0]["group"], "feature / human");
    assert_eq!(rows[0]["total_tokens"], 2490);
    assert_eq!(rows[1]["group"], "feature / subagent");
    assert_eq!(rows[1]["total_tokens"], 15);
}

#[test]
fn default_source_combines_codex_and_claude_usage() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "report",
            "--rollouts",
            "tests/fixtures/rollouts",
            "--claude-projects",
            "tests/fixtures/claude",
            "--last",
            "all",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(output.status.success());
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows[0]["total_tokens"], 465 + 2505);
}

#[test]
fn status_rejects_claude_source() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args(["status", "--source", "claude"])
        .output()
        .expect("binary should run");
    assert!(!output.status.success());
}

#[test]
fn claude_tools_attributes_tool_time_to_build_commands() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "tools",
            "--source",
            "claude",
            "--claude-projects",
            "tests/fixtures/claude",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(rows.as_array().unwrap().len(), 1);
    assert_eq!(rows[0]["kind"], "build");
    assert_eq!(rows[0]["command"], "cargo build");
    assert_eq!(rows[0]["calls"], 1);
    assert_eq!(rows[0]["hours"], 2.0 / 3600.0);
    assert_eq!(rows[0]["share_of_agent_hours"], 2.0 / 35.0);
}

#[test]
fn claude_time_partitions_agent_hours() {
    let output = Command::new(env!("CARGO_BIN_EXE_agent-usage-analyzer"))
        .args([
            "time",
            "--source",
            "claude",
            "--claude-projects",
            "tests/fixtures/claude",
            "--format",
            "json",
        ])
        .output()
        .expect("binary should run");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows = rows.as_array().unwrap();
    let hours: f64 = rows.iter().map(|row| row["hours"].as_f64().unwrap()).sum();
    assert!((hours - 35.0 / 3600.0).abs() < 1e-12);
    assert!(rows.iter().any(|row| {
        row["family"] == "model" && row["kind"] == "tool_call" && row["command"] == "Bash"
    }));
    assert!(rows.iter().any(|row| {
        row["family"] == "tools" && row["kind"] == "build" && row["command"] == "cargo build"
    }));
}
