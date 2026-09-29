use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use chrono_tz::Tz;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::breakdown;
use crate::claude;
use crate::ingest::{ScanResult, scan_rollouts};
use crate::latency;
use crate::pricing::Pricing;
use crate::report::{self, GroupBy, PeriodGroup, ReportFormat};
use crate::workflow;

const DEFAULT_TIMEZONE: &str = "Europe/Stockholm";
type DateRange = (Option<DateTime<Utc>>, Option<DateTime<Utc>>);

#[derive(Debug, Parser)]
#[command(
    name = "agent-usage-analyzer",
    version,
    about = "Analyze Codex and Claude Code token usage without a database",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    report: ReportArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Aggregate token usage and estimated costs")]
    Report(ReportArgs),
    #[command(about = "Show the latest token usage snapshot")]
    Status(StatusArgs),
    #[command(about = "Show turn duration and time-to-first-token statistics")]
    Latency(LatencyArgs),
    #[command(about = "Show daily agent-hours and effective parallelism")]
    Workflow(WorkflowArgs),
    #[command(about = "Estimate which kinds of content make up model input and cached input")]
    Breakdown(BreakdownArgs),
}

#[derive(Clone, Debug, Args)]
struct SourceArgs {
    #[arg(
        long,
        value_enum,
        env = "AGENT_USAGE_SOURCE",
        default_value_t = SourceArg::Codex,
        help = "Session logs to analyze"
    )]
    source: SourceArg,
    #[arg(
        long,
        env = "AGENT_USAGE_ROLLOUTS",
        help = "Directory containing rollout-*.jsonl files"
    )]
    rollouts: Option<PathBuf>,
    #[arg(
        long,
        env = "AGENT_USAGE_CLAUDE_PROJECTS",
        help = "Directory containing Claude Code project session files"
    )]
    claude_projects: Option<PathBuf>,
    #[arg(
        long,
        default_value = DEFAULT_TIMEZONE,
        help = "IANA timezone used for ranges and grouping"
    )]
    timezone: String,
}

#[derive(Clone, Debug, Args)]
struct RangeArgs {
    #[arg(
        long,
        conflicts_with_all = ["today", "from", "to"],
        help = "Relative range such as 7d, 12h, 1m, 30min, or all"
    )]
    last: Option<String>,
    #[arg(
        long,
        conflicts_with_all = ["last", "from", "to"],
        help = "Analyze local midnight through now"
    )]
    today: bool,
    #[arg(long, help = "Range start (YYYY-MM-DD or ISO-8601)")]
    from: Option<String>,
    #[arg(long, help = "Range end (YYYY-MM-DD or ISO-8601)")]
    to: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct ReportArgs {
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    range: RangeArgs,
    #[arg(
        long,
        value_enum,
        default_value_t = PeriodArg::All,
        help = "Period used to aggregate rows"
    )]
    group: PeriodArg,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        help = "Optional secondary grouping dimensions"
    )]
    by: Vec<GroupArg>,
    #[arg(
        long,
        value_enum,
        default_value_t = ReportFormatArg::Table,
        help = "Output format"
    )]
    format: ReportFormatArg,
    #[arg(long, short = 'o', help = "Write output to a file instead of stdout")]
    output: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct StatusArgs {
    #[command(flatten)]
    source: SourceArgs,
}

#[derive(Clone, Debug, Args)]
struct LatencyArgs {
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    range: RangeArgs,
    #[arg(
        long,
        value_enum,
        default_value_t = PeriodArg::All,
        help = "Period used to aggregate rows"
    )]
    group: PeriodArg,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        help = "Optional secondary grouping dimensions"
    )]
    by: Vec<GroupArg>,
    #[arg(
        long,
        value_enum,
        default_value_t = FormatArg::Table,
        help = "Output format"
    )]
    format: FormatArg,
    #[arg(long, short = 'o', help = "Write output to a file instead of stdout")]
    output: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct WorkflowArgs {
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    range: RangeArgs,
    #[arg(
        long,
        value_enum,
        default_value_t = PeriodArg::All,
        help = "Period used to aggregate rows"
    )]
    group: PeriodArg,
    #[arg(
        long,
        value_enum,
        value_delimiter = ',',
        help = "Optional secondary grouping dimensions"
    )]
    by: Vec<GroupArg>,
    #[arg(long, value_enum, default_value_t = FormatArg::Table, help = "Output format")]
    format: FormatArg,
    #[arg(long, short = 'o', help = "Write output to a file instead of stdout")]
    output: Option<PathBuf>,
}

#[derive(Clone, Debug, Args)]
struct BreakdownArgs {
    #[command(flatten)]
    source: SourceArgs,
    #[command(flatten)]
    range: RangeArgs,
    #[arg(
        long,
        value_enum,
        default_value_t = FormatArg::Table,
        help = "Output format"
    )]
    format: FormatArg,
    #[arg(long, short = 'o', help = "Write output to a file instead of stdout")]
    output: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum SourceArg {
    Codex,
    Claude,
    All,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum PeriodArg {
    All,
    Day,
    Week,
    Month,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum GroupArg {
    Model,
    Effort,
    Directory,
    Session,
}

impl PeriodArg {
    fn name(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Day => "day",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
}

impl GroupArg {
    fn name(&self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Effort => "effort",
            Self::Directory => "directory",
            Self::Session => "session",
        }
    }
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum FormatArg {
    Table,
    Json,
    Csv,
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum ReportFormatArg {
    Table,
    Json,
    Csv,
    TelemetryJson,
}

pub fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Some(Command::Report(args)) => run_report(args),
        Some(Command::Status(args)) => run_status(args),
        Some(Command::Latency(args)) => run_latency(args),
        Some(Command::Workflow(args)) => run_workflow(args),
        Some(Command::Breakdown(args)) => run_breakdown(args),
        None => run_report(cli.report),
    }
}

fn run_workflow(args: WorkflowArgs) -> Result<()> {
    let timezone = parse_timezone(&args.source.timezone)?;
    let (start, end) = resolve_range(&args.range, timezone)?;
    let scan = scan(&args.source)?;
    emit_scan_warnings(&scan);
    let rows = workflow::aggregate(
        scan.latencies,
        start,
        end,
        timezone,
        match args.group {
            PeriodArg::All => PeriodGroup::All,
            PeriodArg::Day => PeriodGroup::Day,
            PeriodArg::Week => PeriodGroup::Week,
            PeriodArg::Month => PeriodGroup::Month,
        },
        &args
            .by
            .iter()
            .map(|value| match value {
                GroupArg::Model => GroupBy::Model,
                GroupArg::Effort => GroupBy::Effort,
                GroupArg::Directory => GroupBy::Directory,
                GroupArg::Session => GroupBy::Session,
            })
            .collect::<Vec<_>>(),
    );
    let output = workflow::render(
        &rows,
        match args.format {
            FormatArg::Table => ReportFormat::Table,
            FormatArg::Json => ReportFormat::Json,
            FormatArg::Csv => ReportFormat::Csv,
        },
    )?;
    if let Some(path) = args.output {
        std::fs::write(&path, format!("{output}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

fn run_latency(args: LatencyArgs) -> Result<()> {
    let timezone = parse_timezone(&args.source.timezone)?;
    let (start, end) = resolve_range(&args.range, timezone)?;
    let scan = scan(&args.source)?;
    emit_scan_warnings(&scan);
    let events = scan.latencies.into_iter().filter(|event| {
        start.is_none_or(|start| event.captured_at >= start)
            && end.is_none_or(|end| event.captured_at <= end)
    });
    let rows = latency::aggregate(
        events,
        match args.group {
            PeriodArg::All => PeriodGroup::All,
            PeriodArg::Day => PeriodGroup::Day,
            PeriodArg::Week => PeriodGroup::Week,
            PeriodArg::Month => PeriodGroup::Month,
        },
        &args
            .by
            .iter()
            .map(|value| match value {
                GroupArg::Model => GroupBy::Model,
                GroupArg::Effort => GroupBy::Effort,
                GroupArg::Directory => GroupBy::Directory,
                GroupArg::Session => GroupBy::Session,
            })
            .collect::<Vec<_>>(),
        timezone,
    );
    let output = latency::render(
        &rows,
        !args.by.is_empty(),
        match args.format {
            FormatArg::Table => ReportFormat::Table,
            FormatArg::Json => ReportFormat::Json,
            FormatArg::Csv => ReportFormat::Csv,
        },
    )?;
    if let Some(path) = args.output {
        std::fs::write(&path, format!("{output}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

fn run_breakdown(args: BreakdownArgs) -> Result<()> {
    require_codex_source(&args.source, "breakdown")?;
    let timezone = parse_timezone(&args.source.timezone)?;
    let (start, end) = resolve_range(&args.range, timezone)?;
    let root = args
        .source
        .rollouts
        .clone()
        .unwrap_or_else(default_rollouts_dir);
    let result = breakdown::analyze(&root, start, end)
        .with_context(|| format!("failed to scan {}", root.display()))?;
    if result.invalid_lines > 0 {
        eprintln!(
            "warning: ignored {} malformed JSONL line(s) across {} rollout file(s)",
            result.invalid_lines, result.files
        );
    }
    let output = match args.format {
        FormatArg::Table => render_breakdown_table(&result),
        FormatArg::Json => serde_json::to_string_pretty(&result)?,
        FormatArg::Csv => render_breakdown_csv(&result.rows)?,
    };
    if let Some(path) = args.output {
        std::fs::write(&path, format!("{output}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

fn render_breakdown_table(result: &breakdown::Breakdown) -> String {
    let mut lines = vec![format!(
        "Estimated context composition ({} model calls; input {}, cached {}, output {}, reasoning {})",
        result.calls,
        format_count(result.input_tokens),
        format_count(result.cached_input_tokens),
        format_count(result.output_tokens),
        format_count(result.reasoning_output_tokens),
    )];
    let headers = [
        "Category",
        "Input",
        "Input %",
        "Code input",
        "Cached",
        "Cached code",
        "Output",
        "Code output",
        "Reasoning",
    ];
    let values: Vec<[String; 9]> = breakdown_tree_rows(result)
        .into_iter()
        .map(|row| {
            [
                row.label,
                format_count(row.metrics.input),
                format!("{:.1}%", percent(row.metrics.input, result.input_tokens)),
                format_count(row.metrics.code_input),
                format_count(row.metrics.cached_input),
                format_count(row.metrics.cached_code_input),
                format_count(row.metrics.output),
                format_count(row.metrics.code_output),
                format_count(row.metrics.reasoning_output),
            ]
        })
        .collect();
    let mut widths = headers.map(str::len);
    for row in &values {
        for (index, value) in row.iter().enumerate() {
            widths[index] = widths[index].max(value.len());
        }
    }
    lines.push(format_breakdown_row(&headers, &widths));
    let separators = widths.map(|width| "-".repeat(width));
    lines.push(separators.join("  "));
    lines.extend(values.iter().map(|row| format_breakdown_row(row, &widths)));
    lines.push(format!(
        "\nDetected source code totals: input {}, cached {}, output {}",
        format_count(result.estimated_code_input_tokens),
        format_count(result.estimated_cached_code_input_tokens),
        format_count(result.estimated_code_output_tokens),
    ));
    lines.push("\nEstimate: reported token totals allocated from recorded context order; encrypted summaries and protocol/tool-schema overhead are inferred.".to_owned());
    lines.join("\n")
}

#[derive(Clone, Copy, Debug, Default)]
struct BreakdownMetrics {
    input: u64,
    cached_input: u64,
    output: u64,
    reasoning_output: u64,
    code_input: u64,
    cached_code_input: u64,
    code_output: u64,
}

impl BreakdownMetrics {
    fn add_row(&mut self, row: &breakdown::BreakdownRow) {
        self.input += row.estimated_input_tokens;
        self.cached_input += row.estimated_cached_input_tokens;
        self.output += row.estimated_output_tokens;
        self.reasoning_output += row.reasoning_output_tokens;
        self.code_input += row.estimated_code_input_tokens;
        self.cached_code_input += row.estimated_cached_code_input_tokens;
        self.code_output += row.estimated_code_output_tokens;
    }
}

#[derive(Debug)]
struct BreakdownTreeRow {
    label: String,
    metrics: BreakdownMetrics,
}

fn breakdown_tree_rows(result: &breakdown::Breakdown) -> Vec<BreakdownTreeRow> {
    use std::collections::BTreeMap;

    let mut families = BTreeMap::<breakdown::Family, BreakdownMetrics>::new();
    let mut kinds = BTreeMap::<(breakdown::Family, breakdown::Kind), BreakdownMetrics>::new();
    for row in &result.rows {
        families
            .entry(row.category.family)
            .or_default()
            .add_row(row);
        if let Some(kind) = row.category.kind {
            kinds
                .entry((row.category.family, kind))
                .or_default()
                .add_row(row);
        }
    }

    let mut tree = Vec::new();
    for (family, metrics) in families {
        tree.push(BreakdownTreeRow {
            label: family.label().to_owned(),
            metrics,
        });
        for ((_, kind), metrics) in kinds
            .iter()
            .filter(|((kind_family, _), _)| *kind_family == family)
        {
            tree.push(BreakdownTreeRow {
                label: format!("  {}", kind.label()),
                metrics: *metrics,
            });
            for row in result.rows.iter().filter(|row| {
                row.category.family == family
                    && row.category.kind == Some(*kind)
                    && row.category.source.is_some()
            }) {
                tree.push(BreakdownTreeRow {
                    label: format!("    {}", row.category.leaf_label()),
                    metrics: BreakdownMetrics {
                        input: row.estimated_input_tokens,
                        cached_input: row.estimated_cached_input_tokens,
                        output: row.estimated_output_tokens,
                        reasoning_output: row.reasoning_output_tokens,
                        code_input: row.estimated_code_input_tokens,
                        cached_code_input: row.estimated_cached_code_input_tokens,
                        code_output: row.estimated_code_output_tokens,
                    },
                });
            }
        }
    }
    tree
}

fn format_breakdown_row<S: AsRef<str>>(values: &[S; 9], widths: &[usize; 9]) -> String {
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let value = value.as_ref();
            if index == 0 {
                format!("{value:<width$}", width = widths[index])
            } else {
                format!("{value:>width$}", width = widths[index])
            }
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn render_breakdown_csv(rows: &[breakdown::BreakdownRow]) -> Result<String> {
    let mut writer = csv::WriterBuilder::new()
        .has_headers(false)
        .from_writer(Vec::new());
    writer.write_record([
        "family",
        "kind",
        "source",
        "estimated_input_tokens",
        "estimated_cached_input_tokens",
        "estimated_output_tokens",
        "reasoning_output_tokens",
        "estimated_code_input_tokens",
        "estimated_cached_code_input_tokens",
        "estimated_code_output_tokens",
        "input_percent",
        "cached_percent",
        "output_percent",
        "reasoning_percent",
    ])?;
    for row in rows {
        writer.write_record([
            serialized_enum(row.category.family)?,
            row.category
                .kind
                .map(serialized_enum)
                .transpose()?
                .unwrap_or_default(),
            row.category
                .source
                .map(serialized_enum)
                .transpose()?
                .unwrap_or_default(),
            row.estimated_input_tokens.to_string(),
            row.estimated_cached_input_tokens.to_string(),
            row.estimated_output_tokens.to_string(),
            row.reasoning_output_tokens.to_string(),
            row.estimated_code_input_tokens.to_string(),
            row.estimated_cached_code_input_tokens.to_string(),
            row.estimated_code_output_tokens.to_string(),
            row.input_percent.to_string(),
            row.cached_percent.to_string(),
            row.output_percent.to_string(),
            row.reasoning_percent.to_string(),
        ])?;
    }
    Ok(String::from_utf8(writer.into_inner()?)?
        .trim_end()
        .to_owned())
}

fn serialized_enum<T: serde::Serialize>(value: T) -> Result<String> {
    serde_json::to_value(value)?
        .as_str()
        .map(str::to_owned)
        .context("breakdown enum did not serialize as a string")
}

fn percent(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        value as f64 * 100.0 / total as f64
    }
}

fn format_count(value: u64) -> String {
    let source = value.to_string();
    source
        .chars()
        .enumerate()
        .flat_map(|(index, character)| {
            let comma = (index > 0 && (source.len() - index).is_multiple_of(3)).then_some(',');
            comma.into_iter().chain(std::iter::once(character))
        })
        .collect()
}

fn run_report(args: ReportArgs) -> Result<()> {
    let timezone = parse_timezone(&args.source.timezone)?;
    let (start, end) = resolve_range(&args.range, timezone)?;
    let scan = scan(&args.source)?;
    emit_scan_warnings(&scan);
    let events = scan
        .events
        .into_iter()
        .filter(|event| {
            start.is_none_or(|start| event.captured_at >= start)
                && end.is_none_or(|end| event.captured_at <= end)
        })
        .collect::<Vec<_>>();
    let window_start = start.or_else(|| events.iter().map(|event| event.captured_at).min());
    let window_end = end.or_else(|| events.iter().map(|event| event.captured_at).max());
    let rows = report::aggregate(
        events.into_iter(),
        match args.group {
            PeriodArg::All => PeriodGroup::All,
            PeriodArg::Day => PeriodGroup::Day,
            PeriodArg::Week => PeriodGroup::Week,
            PeriodArg::Month => PeriodGroup::Month,
        },
        &args
            .by
            .iter()
            .map(|value| match value {
                GroupArg::Model => GroupBy::Model,
                GroupArg::Effort => GroupBy::Effort,
                GroupArg::Directory => GroupBy::Directory,
                GroupArg::Session => GroupBy::Session,
            })
            .collect::<Vec<_>>(),
        timezone,
        &Pricing::default(),
    );
    let output = match args.format {
        ReportFormatArg::TelemetryJson => {
            let dimensions = args.by.iter().map(GroupArg::name).collect::<Vec<_>>();
            report::render_telemetry(
                &rows,
                window_start,
                window_end,
                args.group.name(),
                &dimensions,
            )?
        }
        ReportFormatArg::Table | ReportFormatArg::Json | ReportFormatArg::Csv => report::render(
            &rows,
            !args.by.is_empty(),
            match args.format {
                ReportFormatArg::Table => ReportFormat::Table,
                ReportFormatArg::Json => ReportFormat::Json,
                ReportFormatArg::Csv => ReportFormat::Csv,
                ReportFormatArg::TelemetryJson => unreachable!(),
            },
        )?,
    };
    if let Some(path) = args.output {
        std::fs::write(&path, format!("{output}\n"))
            .with_context(|| format!("failed to write {}", path.display()))?;
    } else {
        println!("{output}");
    }
    Ok(())
}

fn run_status(args: StatusArgs) -> Result<()> {
    require_codex_source(&args.source, "status")?;
    let timezone = parse_timezone(&args.source.timezone)?;
    let scan = scan(&args.source)?;
    emit_scan_warnings(&scan);
    let Some(event) = scan.events.iter().max_by_key(|event| event.captured_at) else {
        println!("No usage data captured yet.");
        return Ok(());
    };
    report::render_status(event, timezone);
    Ok(())
}

fn require_codex_source(args: &SourceArgs, command: &str) -> Result<()> {
    if args.source != SourceArg::Codex {
        bail!("{command} is only available for Codex rollouts; use --source codex");
    }
    Ok(())
}

fn scan(args: &SourceArgs) -> Result<ScanResult> {
    let mut result = ScanResult::default();
    if matches!(args.source, SourceArg::Codex | SourceArg::All) {
        let root = args.rollouts.clone().unwrap_or_else(default_rollouts_dir);
        merge_scan(
            &mut result,
            scan_rollouts(&root).with_context(|| format!("failed to scan {}", root.display()))?,
        );
    }
    if matches!(args.source, SourceArg::Claude | SourceArg::All) {
        let root = args
            .claude_projects
            .clone()
            .unwrap_or_else(default_claude_projects_dir);
        merge_scan(
            &mut result,
            claude::scan_projects(&root)
                .with_context(|| format!("failed to scan {}", root.display()))?,
        );
    }
    Ok(result)
}

fn merge_scan(target: &mut ScanResult, source: ScanResult) {
    target.events.extend(source.events);
    target.latencies.extend(source.latencies);
    target.files += source.files;
    target.invalid_lines += source.invalid_lines;
}

fn emit_scan_warnings(scan: &ScanResult) {
    if scan.invalid_lines > 0 {
        eprintln!(
            "warning: ignored {} malformed JSONL line(s) across {} session file(s)",
            scan.invalid_lines, scan.files
        );
    }
}

fn default_claude_projects_dir() -> PathBuf {
    std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"))
        .join("projects")
}

fn default_rollouts_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".codex/sessions")
}

fn parse_timezone(value: &str) -> Result<Tz> {
    value
        .parse::<Tz>()
        .with_context(|| format!("invalid IANA timezone '{value}'"))
}

fn resolve_range(args: &RangeArgs, timezone: Tz) -> Result<DateRange> {
    let now = Utc::now();
    if args.today {
        let local_now = now.with_timezone(&timezone);
        let midnight = timezone
            .with_ymd_and_hms(
                local_now.year(),
                local_now.month(),
                local_now.day(),
                0,
                0,
                0,
            )
            .single()
            .context("local midnight is ambiguous in the selected timezone")?;
        return Ok((Some(midnight.to_utc()), Some(now)));
    }
    if let Some(value) = &args.last {
        return parse_last(value, now, timezone);
    }
    Ok((
        args.from
            .as_deref()
            .map(|value| parse_datetime(value, timezone))
            .transpose()?,
        args.to
            .as_deref()
            .map(|value| parse_datetime(value, timezone))
            .transpose()?,
    ))
}

fn parse_last(value: &str, now: DateTime<Utc>, timezone: Tz) -> Result<DateRange> {
    let normalized = value.trim().to_ascii_lowercase();
    if normalized == "all" {
        return Ok((None, None));
    }
    let digit_count = normalized.bytes().take_while(u8::is_ascii_digit).count();
    if digit_count == 0 || digit_count == normalized.len() {
        bail!("invalid --last value; expected all, 7d, 12h, 1m, or 30min");
    }
    let amount: i64 = normalized[..digit_count].parse()?;
    let start = match &normalized[digit_count..] {
        "min" | "mins" | "minute" | "minutes" => now - chrono::Duration::minutes(amount),
        "h" | "hour" | "hours" => now - chrono::Duration::hours(amount),
        "d" | "day" | "days" => now - chrono::Duration::days(amount),
        "m" | "mo" | "mon" | "month" | "months" => subtract_months(now, timezone, amount)?,
        _ => bail!("invalid --last unit; expected d, h, m (month), or min"),
    };
    Ok((Some(start), Some(now)))
}

fn subtract_months(now: DateTime<Utc>, timezone: Tz, months: i64) -> Result<DateTime<Utc>> {
    let local = now.with_timezone(&timezone);
    let absolute = i64::from(local.year()) * 12 + i64::from(local.month0()) - months;
    let year = i32::try_from(absolute.div_euclid(12))?;
    let month = u32::try_from(absolute.rem_euclid(12) + 1)?;
    let max_day = last_day_of_month(year, month)?;
    let date = NaiveDate::from_ymd_opt(year, month, local.day().min(max_day))
        .context("month range is outside the supported calendar")?;
    local_datetime(timezone, date.and_time(local.time()))
}

fn last_day_of_month(year: i32, month: u32) -> Result<u32> {
    let (next_year, next_month) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    let first_next = NaiveDate::from_ymd_opt(next_year, next_month, 1)
        .context("month range is outside the supported calendar")?;
    Ok((first_next - chrono::Duration::days(1)).day())
}

fn parse_datetime(value: &str, timezone: Tz) -> Result<DateTime<Utc>> {
    let value = value.trim();
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.to_utc());
    }
    if let Ok(parsed) = NaiveDateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%.f") {
        return local_datetime(timezone, parsed);
    }
    if let Ok(date) = NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        return local_datetime(timezone, date.and_hms_opt(0, 0, 0).context("invalid date")?);
    }
    bail!("invalid date '{value}'; expected YYYY-MM-DD or ISO-8601")
}

fn local_datetime(timezone: Tz, value: NaiveDateTime) -> Result<DateTime<Utc>> {
    match timezone.from_local_datetime(&value) {
        LocalResult::Single(value) => Ok(value.to_utc()),
        LocalResult::Ambiguous(first, _) => Ok(first.to_utc()),
        LocalResult::None => bail!("local time {value} does not exist in {timezone}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minute_and_month_ranges_are_distinct() {
        let tz: Tz = DEFAULT_TIMEZONE.parse().unwrap();
        let now = DateTime::parse_from_rfc3339("2026-09-22T12:00:00Z")
            .unwrap()
            .to_utc();
        let minute = parse_last("30min", now, tz).unwrap().0.unwrap();
        let month = parse_last("1m", now, tz).unwrap().0.unwrap();
        assert_eq!((now - minute).num_minutes(), 30);
        assert!(month < now - chrono::Duration::days(28));
    }

    #[test]
    fn all_selects_the_entire_range() {
        let tz: Tz = DEFAULT_TIMEZONE.parse().unwrap();
        let now = Utc::now();
        assert_eq!(parse_last("all", now, tz).unwrap(), (None, None));
        assert!(parse_last("total", now, tz).is_err());
    }

    #[test]
    fn breakdown_table_uses_dynamic_aligned_columns() {
        let result = breakdown::Breakdown {
            rows: vec![breakdown::BreakdownRow {
                category: breakdown::Category::PatchEditEnvelope,
                estimated_input_tokens: 1_234,
                estimated_cached_input_tokens: 123,
                estimated_output_tokens: 12,
                reasoning_output_tokens: 0,
                estimated_code_input_tokens: 0,
                estimated_cached_code_input_tokens: 0,
                estimated_code_output_tokens: 0,
                input_percent: 12.3,
                cached_percent: 4.5,
                output_percent: 6.7,
                reasoning_percent: 0.0,
            }],
            ..breakdown::Breakdown::default()
        };
        let table = render_breakdown_table(&result);
        let lines: Vec<&str> = table.lines().collect();
        let table_width = lines[1].chars().count();
        assert_eq!(lines[2].chars().count(), table_width);
        assert_eq!(lines[3].chars().count(), table_width);
        assert_eq!(lines[4].chars().count(), table_width);
        assert!(lines[3].starts_with("Tool calls"));
        assert!(lines[4].starts_with("  Patch/edit envelope"));
    }
}
