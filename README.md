# Agent Usage Analyzer

A fast, database-free Rust CLI for analyzing coding-agent usage, token
consumption, estimated costs, latency, and context composition from local
session files. It supports [OpenAI Codex CLI](https://github.com/openai/codex)
`rollout-*.jsonl` files and [Claude Code](https://claude.com/claude-code)
session transcripts.

Fast and lightweight: session files are processed in parallel, with no database
or background service required. The optimized Linux x86-64 binary is about
3.9 MB.

## Features

- Track Codex token usage and estimated API costs over custom time ranges
- Group results by model, reasoning effort, directory, git branch, turn origin, or session
- Inspect end-to-end latency, time to first token (TTFT), medians, and p95
- Measure agent-hours, effective parallelism, and time spent in tools from completed turns
- Analyze context composition and identify token-heavy tools and content
- Export reports as human-readable tables, JSON, or CSV
- Export versioned, structured JSON for telemetry ingestion
- Analyze Claude Code sessions, or Codex and Claude Code together, with `--source`
- Keep session data local with no database, account, or background service

Cost estimates select the price that was effective on each event's UTC date.
Historical price changes are sourced from the
[OpenAI API changelog](https://developers.openai.com/api/docs/changelog), while
current Codex rates come from the
[ChatGPT rate card](https://help.openai.com/en/articles/20001415-chatgpt-rate-card-enterprise-token-based-pricing).
When OpenAI publishes only an effective date, the new rate is applied from
00:00 UTC on that date. Claude model rates, including 5-minute and 1-hour cache
writes, come from the
[Claude API pricing page](https://platform.claude.com/docs/en/about-claude/pricing).
Rates were last checked on October 4, 2026. GPT-6.1 Sol is priced from
September 29, 2026 at $2 input, $0.10 cached input, $2.50 cache writes, and
$10 output per million tokens, as published in the
[OpenAI API pricing](https://developers.openai.com/api/docs/pricing).

Claude standard rates start on each model's API release date, using
00:00 UTC when only a date is published in the
[Claude Platform release notes](https://platform.claude.com/docs/en/release-notes/overview).
Events before that date have no price estimate. Retired versions keep their
historical rates; a newer version's lower price does not change older usage.
Sonnet 5 remains at $2 / $10: its planned September 1, 2026 increase was
cancelled on August 10, 2026.

Claude price history (USD per million tokens, standard mode):

| Effective from (UTC) | Model | Input | Cache read | Output |
| --- | --- | ---: | ---: | ---: |
| 2024-06-20 | `claude-3-5-sonnet` | $3 | $0.3 | $15 |
| 2024-10-22 | `claude-3-5-sonnet-20241022` | $3 | $0.3 | $15 |
| 2024-11-04 | `claude-3-5-haiku` | $0.8 | $0.08 | $4 |
| 2025-02-24 | `claude-3-7-sonnet` | $3 | $0.3 | $15 |
| 2025-05-22 | `claude-opus-4` | $15 | $1.5 | $75 |
| 2025-05-22 | `claude-sonnet-4` | $3 | $0.3 | $15 |
| 2025-08-05 | `claude-opus-4-1` | $15 | $1.5 | $75 |
| 2025-09-29 | `claude-sonnet-4-5` | $3 | $0.3 | $15 |
| 2025-10-15 | `claude-haiku-4-5` | $1 | $0.1 | $5 |
| 2025-11-24 | `claude-opus-4-5` | $5 | $0.5 | $25 |
| 2026-02-05 | `claude-opus-4-6` | $5 | $0.5 | $25 |
| 2026-02-17 | `claude-sonnet-4-6` | $3 | $0.3 | $15 |
| 2026-04-16 | `claude-opus-4-7` | $5 | $0.5 | $25 |
| 2026-05-28 | `claude-opus-4-8` | $5 | $0.5 | $25 |
| 2026-06-09 | `claude-fable-5` | $10 | $1 | $50 |
| 2026-06-09 | `claude-mythos-5` | $10 | $1 | $50 |
| 2026-06-30 | `claude-sonnet-5` | $2 | $0.2 | $10 |
| 2026-07-24 | `claude-opus-5` | $5 | $0.5 | $25 |
| 2026-09-01 | `claude-fable-5-1` | $10 | $0.25 | $50 |
| 2026-09-01 | `claude-mythos-5-1` | $10 | $0.25 | $50 |
| 2026-09-22 | `claude-opus-5-5` | $4 | $0.2 | $20 |
| 2026-09-28 | `claude-sonnet-5-5` | $2 | $0.2 | $10 |

For these models, 5-minute cache writes cost 1.25 times the input rate and
1-hour writes cost twice the input rate. Sonnet 3.5 and 3.7 launch prices
are also documented in the
[Sonnet 3.5 announcement](https://www.anthropic.com/news/claude-3-5-sonnet)
and [Sonnet 3.7 announcement](https://www.anthropic.com/news/claude-3-7-sonnet).

## Build

```bash
cargo build --release
```

The binary is written to `target/release/agent-usage-analyzer`.

## Usage

```bash
# Today's usage
agent-usage-analyzer --today

# Last seven days, broken down by model
agent-usage-analyzer --last 7d --by model

# Last seven days, broken down by model and reasoning effort
agent-usage-analyzer --last 7d --by model,effort

# JSON for all available rollouts
agent-usage-analyzer report --last all --format json

# Versioned JSON envelope for telemetry ingestion
agent-usage-analyzer report --last 1h --by model,effort,directory,session --format telemetry-json

# Latest captured usage snapshot
agent-usage-analyzer status

# Latency statistics for the last seven days, broken down by model
agent-usage-analyzer latency --last 7d --by model

# Agent-hours and effective parallelism over seven days
agent-usage-analyzer workflow --last 7d

# Daily results broken down by model
agent-usage-analyzer workflow --last 7d --group day --by model

# Estimated composition of input and cached-input context over seven days
agent-usage-analyzer breakdown --last 7d

# All available rollouts
agent-usage-analyzer breakdown
```

The default rollout directory is `~/.codex/sessions`. Override it with
`--rollouts PATH` or `AGENT_USAGE_ROLLOUTS`.

Supported report options include:

- `--today`, `--last`, `--from`, and `--to`
- `--group all|hour|day|week|month` (default: `all`)
- `--by model|effort|directory|branch|origin|session`, with comma-separated dimensions such as `--by model,effort`
- `--source codex|claude|all` (default: `all`)
- `--format table|json|csv|telemetry-json`
- `--timezone IANA_NAME`
- `--output PATH`

`telemetry-json` is available for `report`. It emits a versioned envelope with
the effective time window, aggregation settings, structured dimensions, and
numeric usage and cost metrics. Every dimension explicitly selected with
`--by`, including `directory` and `session`, is included unchanged. The output
contains no prompt, response, tool-output, or repository-file content.

`latency` accepts the same range, grouping, timezone, and output options as
`report`, with `table`, `json`, and `csv` formats. It shows sample counts,
averages, medians, and p95 values. Latency
fields are emitted in milliseconds in JSON and CSV; the table uses
human-readable durations. Older rollouts may not contain latency measurements,
so missing values are excluded from the sample counts and aggregates.

`workflow` accepts the same range, `--group all|hour|day|week|month`,
`--by model|effort|directory|branch|origin|session`, timezone, and
`--format table|json|csv` options as `latency`. The default grouping is `all`. Agent-hours sum the recorded
durations of completed turns within each period and group. Active wall-hours
measure the union of their time intervals, counting overlaps once within each
group. Effective parallelism is agent-hours divided by active wall-hours.
Intervals crossing a day or range boundary are split or clipped at that boundary.
Turns without a recorded duration are excluded. These measures describe recorded
agent activity, not verified human time saved.
For Claude Code, tool-hours measure the time between each tool call and its
result, counting concurrent tool calls once per turn, and tool share is
tool-hours divided by agent-hours; the remainder is spent generating model
output. Codex rollouts do not record tool timings, so these columns show `-`.

`tools` accepts the same options as `workflow` and splits Claude Code tool time
by kind and command, for example `Build` → `cargo build`, `Test` →
`cargo test`, or `Lint / format` → `cargo clippy`. Kinds are `build`, `test`,
`lint`, `version_control`, `search_read`, `edit`, `web`, `subagent`,
`user_input`, `wait`, `mcp`, `shell`, and `other`. A shell command chaining
several steps is attributed to its most significant one, in the order test,
build, lint, web, version control, wait, search/read; test runs therefore
include the compilation they trigger. `Hours` counts concurrent calls of the
same command once per turn, `Avg call` is the mean duration of the calls
started in the period, and `Agent-hour share` divides `Hours` by the agent-hours
of the same period and group. Tool time includes any wait for permission
approval, and `user_input` covers questions and plan approvals answered by the
user. Codex rollouts do not record tool timings and produce no rows.

`time` accepts the same options and gives an overview of Claude Code agent time:
every turn is split into exclusive slices, so the rows add up to the
agent-hours of `workflow` and to 100% of `Agent-hour share`. `Model response`
covers response generation, split into `Thinking`, `Text`, and `Tool call
writing` (by tool, for example the time spent writing the content of `Write`
or `Edit` calls). Claude Code records one transcript line per content block when
the block is complete, so the time between the previous event and a block is
attributed to that block; the first block of each response therefore also
includes request latency. `Tool execution` uses the same kinds and commands as
`tools`; while the model is generating, time is attributed to the model, and
concurrent tool calls share the remaining time equally. `Other` is turn time
covered by neither. `Count` is the number of blocks or calls started in the
period and `Avg` their mean duration. The table nests families, kinds, and
commands with subtotals; JSON and CSV contain the leaf rows with `family`,
`kind`, and `command` fields.

`branch` is the git branch recorded with each message (Claude Code) or at the
start of the session (Codex). `origin` describes what started a turn: for Claude
Code, `human`, `task_notification` (a background task finished), `peer` (a
message from another agent), `system`, or `subagent`; for Codex, `subagent` for
subagent sessions and `<unknown>` otherwise.

`breakdown` reads context items but does not store them. It allocates the exact
reported input, cached-input, output, and reasoning-output totals across
categories using the recorded context order. Tokenization, encrypted
compaction summaries, model-injected tool schemas, and protocol overhead make
the category split an estimate.
`reasoning_output_tokens` is shown separately; depending on the rollout schema,
it may be a subset of `output_tokens` rather than an additional token count.
Without a range option, every available rollout is analyzed. `breakdown` accepts
the same `--last`, `--today`, `--from`, and `--to` options as reports.
Use `--format table|json|csv` and `--output PATH` as with reports. Code-looking
output from file-reading/search commands is classified as repository source.
The table groups results hierarchically by family, content kind, and source;
for example, `Tool outputs` → `Repository source` → `rg`. JSON exposes the same
three-part path as a structured `category` object, while CSV keeps separate
`family`, `kind`, and `source` columns.
Detected-code counts are metrics on every hierarchy level. They scan every
observable text category, including fenced Markdown, diff hunks, compiler
excerpts, prompts, assistant messages, and otherwise mixed tool output. Opaque
protocol overhead and the unobserved portion of encrypted compaction summaries
cannot be classified.
Tool-output kinds include repository source, build/test/lint, search/listings,
version control, patches/edits, web/external data, UI/media, process control,
data/analysis, system/environment, diagnostics, generic shell, and
uncategorized output. Sources such as `rg`, `grep`, `find`, `ls`, `sed`, and
`cat` remain individually attributable below those kinds.
For patch/edit calls, the call wrapper and metadata are accounted separately
from the actual patch or replacement-code payload; the tool's confirmation is
reported as a third, distinct result category.

## Claude Code

```bash
# Claude Code usage over seven days, broken down by model
agent-usage-analyzer report --source claude --last 7d --by model

# Codex and Claude Code combined
agent-usage-analyzer report --source all --last 7d --by model

# Claude Code agent-hours, including subagents
agent-usage-analyzer workflow --source claude --last 7d --group day

# Time spent compiling, testing, and running other tools over seven days
agent-usage-analyzer tools --source claude --last 7d

# Overview of agent time: thinking, writing, tool execution
agent-usage-analyzer time --source claude --last 7d
```

`--source codex|claude|all` (or `AGENT_USAGE_SOURCE`) selects the session logs
used by `report`, `latency`, and `workflow`; the default is `all`. Claude Code
transcripts are read from `$CLAUDE_CONFIG_DIR/projects`, or
`~/.claude/projects` when that variable is unset. Override the location with
`--claude-projects PATH` or `AGENT_USAGE_CLAUDE_PROJECTS`. Subagent transcripts
are included.

Claude Code writes one transcript line per content block, so usage is
deduplicated by API message ID, keeping the final reported counts. Sessions
copied between project directories are counted once. `Input` includes uncached
input, cache reads (`Cached`), and cache writes (`Cache write`); cache writes are
priced at their 5-minute or 1-hour rate, and fast-mode requests at twice the
standard rates. `Reasoning` reports thinking tokens, which are part of `Output`.
The costs are API-rate equivalents and do not reflect subscription plans.

Claude Code does not record turn durations, so a turn is measured from the user
prompt to the last assistant message before the next prompt. `latency` reports
these durations without time-to-first-token values. `status` and `breakdown`
support Codex rollouts only.
