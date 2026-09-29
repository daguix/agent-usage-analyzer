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
- Group results by model, reasoning effort, directory, or session
- Inspect end-to-end latency, time to first token (TTFT), medians, and p95
- Measure agent-hours and effective parallelism from completed turns
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
- `--group all|day|week|month` (default: `all`)
- `--by model|effort|directory|session`, with comma-separated dimensions such as `--by model,effort`
- `--source codex|claude|all`
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

`workflow` accepts the same range, `--group all|day|week|month`,
`--by model|effort|directory|session`, timezone, and `--format table|json|csv`
options as `latency`. The default grouping is `all`. Agent-hours sum the recorded
durations of completed turns within each period and group. Active wall-hours
measure the union of their time intervals, counting overlaps once within each
group. Effective parallelism is agent-hours divided by active wall-hours.
Intervals crossing a day or range boundary are split or clipped at that boundary.
Turns without a recorded duration are excluded. These measures describe recorded
agent activity, not verified human time saved.

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
```

`--source codex|claude|all` (or `AGENT_USAGE_SOURCE`) selects the session logs
used by `report`, `latency`, and `workflow`; the default is `codex`. Claude Code
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
