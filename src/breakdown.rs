use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::ValueEnum;
use rayon::prelude::*;
use serde::Serialize;
use serde_json::Value;
use walkdir::WalkDir;

use crate::sort::{SortOrder, directed};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Instructions,
    Conversation,
    ToolCalls,
    ToolOutputs,
    Compaction,
    Protocol,
}

impl Family {
    pub fn label(self) -> &'static str {
        match self {
            Self::Instructions => "Instructions",
            Self::Conversation => "Conversation",
            Self::ToolCalls => "Tool calls",
            Self::ToolOutputs => "Tool outputs",
            Self::Compaction => "Compaction / summary",
            Self::Protocol => "Protocol / unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    UserPrompts,
    AssistantText,
    AssistantReasoning,
    PatchEditEnvelope,
    PatchEditPayload,
    RepositorySource,
    OtherCode,
    BuildTestLint,
    SearchListings,
    VersionControl,
    PatchEditResult,
    WebExternal,
    UiMedia,
    ProcessControl,
    DataAnalysis,
    SystemEnvironment,
    Diagnostics,
    Shell,
    Other,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Self::UserPrompts => "User prompts",
            Self::AssistantText => "Assistant text",
            Self::AssistantReasoning => "Assistant reasoning",
            Self::Other => "Other",
            Self::PatchEditEnvelope => "Patch/edit envelope",
            Self::PatchEditPayload => "Patch/edit code payload",
            Self::RepositorySource => "Repository source",
            Self::OtherCode => "Other code",
            Self::BuildTestLint => "Build / test / lint",
            Self::SearchListings => "Search / listings",
            Self::VersionControl => "Version control",
            Self::PatchEditResult => "Patch/edit result",
            Self::WebExternal => "Web / external data",
            Self::UiMedia => "UI / media",
            Self::ProcessControl => "Process control",
            Self::DataAnalysis => "Data / analysis",
            Self::SystemEnvironment => "System / environment",
            Self::Diagnostics => "Errors / diagnostics",
            Self::Shell => "Other shell",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Rg,
    Grep,
    Find,
    Ls,
    Pwd,
    ListFiles,
    ReadFile,
    Cat,
    Sed,
    Head,
    Tail,
    McpResources,
    Other,
}

impl Source {
    pub fn label(self) -> &'static str {
        match self {
            Self::Other => "Other",
            Self::Rg => "rg",
            Self::Grep => "grep",
            Self::Find => "find",
            Self::Ls => "ls",
            Self::Pwd => "pwd",
            Self::ListFiles => "list_files",
            Self::ReadFile => "read_file",
            Self::Cat => "cat",
            Self::Sed => "sed",
            Self::Head => "head",
            Self::Tail => "tail",
            Self::McpResources => "MCP resources",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct CategoryPath {
    pub family: Family,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<Source>,
}

impl CategoryPath {
    const fn family(family: Family) -> Self {
        Self {
            family,
            kind: None,
            source: None,
        }
    }

    const fn kind(family: Family, kind: Kind) -> Self {
        Self {
            family,
            kind: Some(kind),
            source: None,
        }
    }

    const fn source(family: Family, kind: Kind, source: Source) -> Self {
        Self {
            family,
            kind: Some(kind),
            source: Some(source),
        }
    }

    pub fn leaf_label(self) -> &'static str {
        self.source
            .map(Source::label)
            .or_else(|| self.kind.map(Kind::label))
            .unwrap_or_else(|| self.family.label())
    }

    #[allow(non_upper_case_globals)]
    pub const Instructions: Self = Self::family(Family::Instructions);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCode: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Other);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCodeRg: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Rg);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCodeSed: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Sed);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCodeHead: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Head);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCodeCat: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Cat);
    #[allow(non_upper_case_globals)]
    pub const RepositoryCodeTail: Self =
        Self::source(Family::ToolOutputs, Kind::RepositorySource, Source::Tail);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputCode: Self = Self::kind(Family::ToolOutputs, Kind::OtherCode);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputBuildTest: Self = Self::kind(Family::ToolOutputs, Kind::BuildTestLint);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputSearchListing: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Other);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputSearchRg: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Rg);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputSearchGrep: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Grep);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputListingFind: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Find);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputListingLs: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Ls);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputListingPwd: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Pwd);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputListingFiles: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::ListFiles);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputReadFile: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::ReadFile);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputCat: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Cat);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputSed: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Sed);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputHead: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Head);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputTail: Self =
        Self::source(Family::ToolOutputs, Kind::SearchListings, Source::Tail);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputListingMcpResources: Self = Self::source(
        Family::ToolOutputs,
        Kind::SearchListings,
        Source::McpResources,
    );
    #[allow(non_upper_case_globals)]
    pub const ToolOutputVersionControl: Self =
        Self::kind(Family::ToolOutputs, Kind::VersionControl);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputPatchEdit: Self = Self::kind(Family::ToolOutputs, Kind::PatchEditResult);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputWeb: Self = Self::kind(Family::ToolOutputs, Kind::WebExternal);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputUiMedia: Self = Self::kind(Family::ToolOutputs, Kind::UiMedia);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputProcessControl: Self =
        Self::kind(Family::ToolOutputs, Kind::ProcessControl);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputDataAnalysis: Self = Self::kind(Family::ToolOutputs, Kind::DataAnalysis);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputSystemInfo: Self = Self::kind(Family::ToolOutputs, Kind::SystemEnvironment);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputDiagnostics: Self = Self::kind(Family::ToolOutputs, Kind::Diagnostics);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputShell: Self = Self::kind(Family::ToolOutputs, Kind::Shell);
    #[allow(non_upper_case_globals)]
    pub const ToolOutputOther: Self = Self::kind(Family::ToolOutputs, Kind::Other);
    #[allow(non_upper_case_globals)]
    pub const UserPrompts: Self = Self::kind(Family::Conversation, Kind::UserPrompts);
    #[allow(non_upper_case_globals)]
    pub const AssistantText: Self = Self::kind(Family::Conversation, Kind::AssistantText);
    #[allow(non_upper_case_globals)]
    pub const AssistantReasoning: Self = Self::kind(Family::Conversation, Kind::AssistantReasoning);
    #[allow(non_upper_case_globals)]
    pub const ToolCalls: Self = Self::kind(Family::ToolCalls, Kind::Other);
    #[allow(non_upper_case_globals)]
    pub const PatchEditEnvelope: Self = Self::kind(Family::ToolCalls, Kind::PatchEditEnvelope);
    #[allow(non_upper_case_globals)]
    pub const PatchEditPayload: Self = Self::kind(Family::ToolCalls, Kind::PatchEditPayload);
    #[allow(non_upper_case_globals)]
    pub const CompactionSummary: Self = Self::family(Family::Compaction);
    #[allow(non_upper_case_globals)]
    pub const Unknown: Self = Self::family(Family::Protocol);
}

pub type Category = CategoryPath;

#[derive(Clone, Debug, Serialize)]
pub struct BreakdownRow {
    pub category: CategoryPath,
    pub estimated_input_tokens: u64,
    pub estimated_cached_input_tokens: u64,
    pub estimated_output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub estimated_code_input_tokens: u64,
    pub estimated_cached_code_input_tokens: u64,
    pub estimated_code_output_tokens: u64,
    pub input_percent: f64,
    pub cached_percent: f64,
    pub output_percent: f64,
    pub reasoning_percent: f64,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum BreakdownSortKey {
    Category,
    Input,
    Cached,
    Output,
    Reasoning,
    CodeInput,
    CachedCodeInput,
    CodeOutput,
}

impl BreakdownRow {
    fn sort_value(&self, key: BreakdownSortKey) -> u64 {
        match key {
            BreakdownSortKey::Category => 0,
            BreakdownSortKey::Input => self.estimated_input_tokens,
            BreakdownSortKey::Cached => self.estimated_cached_input_tokens,
            BreakdownSortKey::Output => self.estimated_output_tokens,
            BreakdownSortKey::Reasoning => self.reasoning_output_tokens,
            BreakdownSortKey::CodeInput => self.estimated_code_input_tokens,
            BreakdownSortKey::CachedCodeInput => self.estimated_cached_code_input_tokens,
            BreakdownSortKey::CodeOutput => self.estimated_code_output_tokens,
        }
    }
}

pub fn sort(rows: &mut [BreakdownRow], key: BreakdownSortKey, order: Option<SortOrder>) {
    if let BreakdownSortKey::Category = key {
        let order = order.unwrap_or(SortOrder::Asc);
        rows.sort_by(|left, right| directed(left.category.cmp(&right.category), order));
        return;
    }
    let order = order.unwrap_or(SortOrder::Desc);
    let mut families = BTreeMap::<Family, u64>::new();
    let mut kinds = BTreeMap::<(Family, Option<Kind>), u64>::new();
    for row in rows.iter() {
        let value = row.sort_value(key);
        *families.entry(row.category.family).or_default() += value;
        *kinds
            .entry((row.category.family, row.category.kind))
            .or_default() += value;
    }
    rows.sort_by(|left, right| {
        let (left_path, right_path) = (left.category, right.category);
        directed(
            families[&left_path.family].cmp(&families[&right_path.family]),
            order,
        )
        .then_with(|| left_path.family.cmp(&right_path.family))
        .then_with(|| {
            directed(
                kinds[&(left_path.family, left_path.kind)]
                    .cmp(&kinds[&(right_path.family, right_path.kind)]),
                order,
            )
        })
        .then_with(|| left_path.kind.cmp(&right_path.kind))
        .then_with(|| directed(left.sort_value(key).cmp(&right.sort_value(key)), order))
        .then_with(|| left_path.cmp(&right_path))
    });
}

#[derive(Debug, Default, Serialize)]
pub struct Breakdown {
    pub rows: Vec<BreakdownRow>,
    pub files: usize,
    pub calls: usize,
    pub invalid_lines: usize,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_output_tokens: u64,
    pub estimated_code_input_tokens: u64,
    pub estimated_cached_code_input_tokens: u64,
    pub estimated_code_output_tokens: u64,
}

#[derive(Clone, Copy, Debug)]
struct Item {
    category: CategoryPath,
    tokens: u64,
    code_tokens: u64,
}

#[derive(Debug, Default)]
struct FileBreakdown {
    totals: BTreeMap<CategoryPath, CategoryTotals>,
    calls: usize,
    invalid_lines: usize,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    reasoning_output_tokens: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct CategoryTotals {
    input: u64,
    cached_input: u64,
    output: u64,
    reasoning_output: u64,
    code_input: u64,
    cached_code_input: u64,
    code_output: u64,
}

pub fn analyze(
    root: &Path,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<Breakdown> {
    if !root.exists() {
        return Ok(Breakdown::default());
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
    let parsed: Vec<Result<FileBreakdown>> = files
        .par_iter()
        .map(|path| analyze_file(path, start, end))
        .collect();
    let mut result = Breakdown {
        files: files.len(),
        ..Breakdown::default()
    };
    let mut totals = BTreeMap::<CategoryPath, CategoryTotals>::new();
    for file in parsed {
        let file = file?;
        result.calls += file.calls;
        result.invalid_lines += file.invalid_lines;
        result.input_tokens += file.input_tokens;
        result.cached_input_tokens += file.cached_input_tokens;
        result.output_tokens += file.output_tokens;
        result.reasoning_output_tokens += file.reasoning_output_tokens;
        for (category, value) in file.totals {
            let total = totals.entry(category).or_default();
            total.input += value.input;
            total.cached_input += value.cached_input;
            total.output += value.output;
            total.reasoning_output += value.reasoning_output;
            total.code_input += value.code_input;
            total.cached_code_input += value.cached_code_input;
            total.code_output += value.code_output;
        }
    }
    result.rows = totals
        .into_iter()
        .map(|(category, total)| BreakdownRow {
            category,
            estimated_input_tokens: total.input,
            estimated_cached_input_tokens: total.cached_input,
            estimated_output_tokens: total.output,
            reasoning_output_tokens: total.reasoning_output,
            estimated_code_input_tokens: total.code_input,
            estimated_cached_code_input_tokens: total.cached_code_input,
            estimated_code_output_tokens: total.code_output,
            input_percent: percent(total.input, result.input_tokens),
            cached_percent: percent(total.cached_input, result.cached_input_tokens),
            output_percent: percent(total.output, result.output_tokens),
            reasoning_percent: percent(total.reasoning_output, result.reasoning_output_tokens),
        })
        .filter(|row| {
            row.estimated_input_tokens > 0
                || row.estimated_cached_input_tokens > 0
                || row.estimated_output_tokens > 0
                || row.reasoning_output_tokens > 0
        })
        .collect();
    result.estimated_code_input_tokens = result
        .rows
        .iter()
        .map(|row| row.estimated_code_input_tokens)
        .sum();
    result.estimated_cached_code_input_tokens = result
        .rows
        .iter()
        .map(|row| row.estimated_cached_code_input_tokens)
        .sum();
    result.estimated_code_output_tokens = result
        .rows
        .iter()
        .map(|row| row.estimated_code_output_tokens)
        .sum();
    result.rows.sort_by_key(|row| row.category);
    Ok(result)
}

fn analyze_file(
    path: &Path,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
) -> Result<FileBreakdown> {
    let mut result = FileBreakdown::default();
    let mut context = Vec::<Item>::new();
    let mut pending = Vec::<Item>::new();
    let mut calls = HashMap::<String, (String, String)>::new();
    let mut has_usage_records = false;
    let mut protocol_overhead_hint = None;
    for line in BufReader::new(File::open(path)?).lines() {
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
        let item_type = value.get("type").and_then(Value::as_str).unwrap_or("");
        let payload = value.get("payload").unwrap_or(&Value::Null);
        match item_type {
            "response_item" => {
                ingest_response_item(payload, &mut context, &mut pending, &mut calls)
            }
            "compacted" => {
                context.clear();
                pending.clear();
                if let Some(history) = payload.get("replacement_history").and_then(Value::as_array)
                {
                    for item in history {
                        ingest_history_item(item, &mut context, &calls);
                    }
                }
            }
            "token_usage_record" => {
                has_usage_records = true;
                let timestamp = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp);
                let usage = payload.get("usage").unwrap_or(&Value::Null);
                if timestamp.is_some_and(|timestamp| {
                    start.is_none_or(|start| timestamp >= start)
                        && end.is_none_or(|end| timestamp <= end)
                }) {
                    let input = number(usage, "input_tokens");
                    let cached = number(usage, "cached_input_tokens").min(input);
                    let output = number(usage, "output_tokens");
                    let reasoning = number(usage, "reasoning_output_tokens");
                    update_overhead_hint(&context, input, &mut protocol_overhead_hint);
                    attribute(
                        &context,
                        input,
                        cached,
                        protocol_overhead_hint,
                        &mut result.totals,
                    );
                    attribute_output(
                        &pending,
                        output,
                        reasoning,
                        reasoning_is_included(usage),
                        &mut result.totals,
                    );
                    result.calls += 1;
                    result.input_tokens += input;
                    result.cached_input_tokens += cached;
                    result.output_tokens += output;
                    result.reasoning_output_tokens += reasoning;
                }
                materialize_reasoning(&mut pending, number(usage, "reasoning_output_tokens"));
                append_compact(&mut context, pending.drain(..));
            }
            "event_msg"
                if !has_usage_records
                    && payload.get("type").and_then(Value::as_str) == Some("token_count") =>
            {
                let timestamp = value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .and_then(parse_timestamp);
                let event = payload
                    .get("payload")
                    .filter(|value| value.is_object())
                    .unwrap_or(payload);
                let usage = event
                    .get("info")
                    .and_then(|info| info.get("last_token_usage"))
                    .unwrap_or(&Value::Null);
                if timestamp.is_some_and(|timestamp| {
                    start.is_none_or(|start| timestamp >= start)
                        && end.is_none_or(|end| timestamp <= end)
                }) {
                    let input = number(usage, "input_tokens");
                    let cached = number(usage, "cached_input_tokens").min(input);
                    let output = number(usage, "output_tokens");
                    let reasoning = number(usage, "reasoning_output_tokens");
                    update_overhead_hint(&context, input, &mut protocol_overhead_hint);
                    attribute(
                        &context,
                        input,
                        cached,
                        protocol_overhead_hint,
                        &mut result.totals,
                    );
                    attribute_output(
                        &pending,
                        output,
                        reasoning,
                        reasoning_is_included(usage),
                        &mut result.totals,
                    );
                    result.calls += 1;
                    result.input_tokens += input;
                    result.cached_input_tokens += cached;
                    result.output_tokens += output;
                    result.reasoning_output_tokens += reasoning;
                }
                materialize_reasoning(&mut pending, number(usage, "reasoning_output_tokens"));
                append_compact(&mut context, pending.drain(..));
            }
            _ => {}
        }
    }
    Ok(result)
}

fn ingest_response_item(
    payload: &Value,
    context: &mut Vec<Item>,
    pending: &mut Vec<Item>,
    calls: &mut HashMap<String, (String, String)>,
) {
    match payload.get("type").and_then(Value::as_str).unwrap_or("") {
        "message" => {
            let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
            let target = if role == "assistant" {
                pending
            } else {
                context
            };
            for text in content_texts(payload.get("content")) {
                let category = match role {
                    "developer" | "system" => Category::Instructions,
                    "user" if looks_like_instructions(text) => Category::Instructions,
                    "user" => Category::UserPrompts,
                    "assistant" => Category::AssistantText,
                    _ => Category::Unknown,
                };
                let tokens = estimate_tokens(text);
                push_item_with_code(
                    target,
                    category,
                    tokens,
                    estimate_embedded_code_tokens(text).min(tokens),
                );
            }
        }
        "reasoning" => {
            for text in output_texts(payload.get("summary"))
                .into_iter()
                .chain(output_texts(payload.get("content")))
            {
                let tokens = estimate_tokens(text);
                push_item_with_code(
                    pending,
                    Category::AssistantReasoning,
                    tokens,
                    estimate_embedded_code_tokens(text).min(tokens),
                );
            }
        }
        "function_call" | "custom_tool_call" => {
            let name = payload.get("name").and_then(Value::as_str).unwrap_or("");
            let input = payload
                .get("arguments")
                .or_else(|| payload.get("input"))
                .and_then(Value::as_str)
                .unwrap_or("");
            if let Some(id) = payload.get("call_id").and_then(Value::as_str) {
                calls.insert(id.to_owned(), (name.to_owned(), input.to_owned()));
            }
            ingest_tool_call(pending, name, input);
        }
        "function_call_output" | "custom_tool_call_output" => {
            let call = payload
                .get("call_id")
                .and_then(Value::as_str)
                .and_then(|id| calls.get(id));
            for text in output_texts(payload.get("output")) {
                let category = classify_tool_output(text, call);
                let tokens = estimate_tokens(text);
                let code_tokens = if is_repository_code_category(category) {
                    tokens
                } else {
                    estimate_embedded_code_tokens(text).min(tokens)
                };
                push_item_with_code(context, category, tokens, code_tokens);
            }
        }
        "compaction" => push_item(context, Category::CompactionSummary, 0),
        _ => push_item(
            context,
            Category::Unknown,
            estimate_tokens(&payload.to_string()),
        ),
    }
}

fn ingest_history_item(
    item: &Value,
    context: &mut Vec<Item>,
    calls: &HashMap<String, (String, String)>,
) {
    let mut ignored_pending = Vec::new();
    let mut cloned_calls = calls.clone();
    ingest_response_item(item, context, &mut ignored_pending, &mut cloned_calls);
    append_compact(context, ignored_pending);
}

fn ingest_tool_call(pending: &mut Vec<Item>, name: &str, input: &str) {
    let total = estimate_tokens(name) + estimate_tokens(input);
    if !is_patch_edit_call(name, input) {
        push_item(pending, Category::ToolCalls, total);
        return;
    }
    let payload = patch_payload_tokens(input).min(total);
    push_item(
        pending,
        Category::PatchEditEnvelope,
        total.saturating_sub(payload),
    );
    push_item_with_code(pending, Category::PatchEditPayload, payload, payload);
}

fn is_patch_edit_call(name: &str, input: &str) -> bool {
    let normalized_name = name.to_ascii_lowercase();
    let namespaced_leaf = normalized_name
        .rsplit_once("__")
        .map(|(_, leaf)| leaf)
        .unwrap_or(&normalized_name);
    let leaf_name = namespaced_leaf
        .rsplit(['.', '/'])
        .next()
        .unwrap_or(namespaced_leaf);
    matches!(
        leaf_name,
        "apply_patch" | "write_file" | "edit_file" | "replace_in_file"
    ) || [
        "tools.apply_patch(",
        "tools.write_file(",
        "tools.edit_file(",
        "tools.replace_in_file(",
    ]
    .iter()
    .any(|marker| input.contains(marker))
}

fn patch_payload_tokens(input: &str) -> u64 {
    const START: &str = "*** Begin Patch";
    const END: &str = "*** End Patch";
    if let Some(start) = input.find(START)
        && let Some(relative_end) = input[start..].find(END)
    {
        let end = start + relative_end + END.len();
        return estimate_tokens(&input[start..end]);
    }
    serde_json::from_str::<Value>(input)
        .ok()
        .map(|value| editable_json_tokens(&value))
        .unwrap_or(0)
}

fn editable_json_tokens(value: &Value) -> u64 {
    let Some(object) = value.as_object() else {
        return 0;
    };
    ["patch", "content", "new_string", "replacement", "new_text"]
        .iter()
        .filter_map(|key| object.get(*key).and_then(Value::as_str))
        .map(estimate_tokens)
        .sum()
}

fn attribute(
    context: &[Item],
    input: u64,
    cached: u64,
    protocol_overhead_hint: Option<u64>,
    totals: &mut BTreeMap<Category, CategoryTotals>,
) {
    if input == 0 {
        return;
    }
    let raw: u64 = context.iter().map(|item| item.tokens).sum();
    let mut sized = if raw > input {
        proportional_sizes(context, input, raw)
    } else {
        context.to_vec()
    };
    if raw < input {
        let mut extra = input - raw;
        if let Some(index) = sized
            .iter()
            .rposition(|item| item.category == Category::CompactionSummary)
        {
            let overhead = protocol_overhead_hint.unwrap_or(0).min(extra);
            extra -= overhead;
            sized[index].tokens += extra;
            if overhead > 0 {
                sized.insert(
                    0,
                    Item {
                        category: Category::Unknown,
                        tokens: overhead,
                        code_tokens: 0,
                    },
                );
            }
        } else {
            sized.insert(
                0,
                Item {
                    category: Category::Unknown,
                    tokens: extra,
                    code_tokens: 0,
                },
            );
        }
    }
    let mut cached_left = cached;
    for item in sized {
        let item_cached = item.tokens.min(cached_left);
        cached_left -= item_cached;
        let total = totals.entry(item.category).or_default();
        total.input += item.tokens;
        total.cached_input += item_cached;
        total.code_input += item.code_tokens;
        total.cached_code_input += proportional_part(item.code_tokens, item_cached, item.tokens);
    }
}

fn attribute_output(
    pending: &[Item],
    output: u64,
    reasoning: u64,
    reasoning_included: bool,
    totals: &mut BTreeMap<Category, CategoryTotals>,
) {
    totals
        .entry(Category::AssistantReasoning)
        .or_default()
        .reasoning_output += reasoning;
    let reasoning_in_output = if reasoning_included {
        reasoning.min(output)
    } else {
        0
    };
    totals
        .entry(Category::AssistantReasoning)
        .or_default()
        .output += reasoning_in_output;
    let reasoning_raw: u64 = pending
        .iter()
        .filter(|item| item.category == Category::AssistantReasoning)
        .map(|item| item.tokens)
        .sum();
    let reasoning_code: u64 = pending
        .iter()
        .filter(|item| item.category == Category::AssistantReasoning)
        .map(|item| item.code_tokens)
        .sum();
    totals
        .entry(Category::AssistantReasoning)
        .or_default()
        .code_output += proportional_part(reasoning_code, reasoning_in_output, reasoning_raw);
    let remaining = output - reasoning_in_output;
    if remaining == 0 {
        return;
    }
    let visible: Vec<Item> = pending
        .iter()
        .copied()
        .filter(|item| item.category != Category::AssistantReasoning && item.tokens > 0)
        .collect();
    let raw: u64 = visible.iter().map(|item| item.tokens).sum();
    if raw == 0 {
        totals.entry(Category::Unknown).or_default().output += remaining;
        return;
    }
    for item in proportional_sizes(&visible, remaining, raw) {
        let total = totals.entry(item.category).or_default();
        total.output += item.tokens;
        total.code_output += item.code_tokens;
    }
}

fn materialize_reasoning(pending: &mut Vec<Item>, reasoning: u64) {
    if reasoning == 0 {
        return;
    }
    if let Some(item) = pending
        .iter_mut()
        .rfind(|item| item.category == Category::AssistantReasoning)
    {
        item.tokens = item.tokens.max(reasoning);
    } else {
        push_item(pending, Category::AssistantReasoning, reasoning);
    }
}

fn reasoning_is_included(usage: &Value) -> bool {
    let input = number(usage, "input_tokens");
    let output = number(usage, "output_tokens");
    let total = number(usage, "total_tokens");
    total == 0 || total == input.saturating_add(output)
}

fn update_overhead_hint(context: &[Item], input: u64, hint: &mut Option<u64>) {
    if hint.is_some()
        || context
            .iter()
            .any(|item| item.category == Category::CompactionSummary)
    {
        return;
    }
    let raw: u64 = context.iter().map(|item| item.tokens).sum();
    *hint = Some(input.saturating_sub(raw));
}

fn proportional_sizes(context: &[Item], input: u64, raw: u64) -> Vec<Item> {
    let mut result: Vec<Item> = context
        .iter()
        .map(|item| Item {
            category: item.category,
            tokens: item.tokens.saturating_mul(input) / raw,
            code_tokens: 0,
        })
        .collect();
    for (scaled, original) in result.iter_mut().zip(context) {
        scaled.code_tokens =
            proportional_part(original.code_tokens, scaled.tokens, original.tokens);
    }
    let assigned: u64 = result.iter().map(|item| item.tokens).sum();
    for item in result.iter_mut().take((input - assigned) as usize) {
        item.tokens += 1;
    }
    result
}

fn append_compact(target: &mut Vec<Item>, items: impl IntoIterator<Item = Item>) {
    for item in items {
        push_item_with_code(target, item.category, item.tokens, item.code_tokens);
    }
}

fn push_item(items: &mut Vec<Item>, category: Category, tokens: u64) {
    push_item_with_code(items, category, tokens, 0);
}

fn push_item_with_code(items: &mut Vec<Item>, category: Category, tokens: u64, code_tokens: u64) {
    if tokens == 0
        && category != Category::CompactionSummary
        && category != Category::AssistantReasoning
    {
        return;
    }
    if let Some(last) = items.last_mut().filter(|last| last.category == category) {
        last.tokens += tokens;
        last.code_tokens += code_tokens.min(tokens);
    } else {
        items.push(Item {
            category,
            tokens,
            code_tokens: code_tokens.min(tokens),
        });
    }
}

fn content_texts(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .collect()
}

fn output_texts(value: Option<&Value>) -> Vec<&str> {
    match value {
        Some(Value::String(text)) => vec![text],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.get("text").and_then(Value::as_str))
            .collect(),
        _ => Vec::new(),
    }
}

fn looks_like_instructions(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("agents.md")
        || lower.contains("<environment_context>")
        || lower.contains("<permissions instructions>")
        || lower.contains("<skills_instructions>")
}

fn classify_tool_output(text: &str, call: Option<&(String, String)>) -> Category {
    let invocation = call
        .map(|(name, input)| format!("{name} {input}").to_ascii_lowercase())
        .unwrap_or_default();
    let search_listing = search_listing_category(&invocation);
    let is_reader = search_listing.is_some_and(is_file_reader_category);
    let reads_source_file = search_listing.is_some_and(is_direct_file_reader_category)
        && looks_like_source_path(&invocation);
    if (is_reader && looks_like_code(text)) || reads_source_file {
        repository_code_category(search_listing)
    } else if call.is_some_and(|(name, input)| is_patch_edit_call(name, input))
        || text.contains("diff --git ")
    {
        Category::ToolOutputPatchEdit
    } else if is_build_or_test(&invocation) {
        Category::ToolOutputBuildTest
    } else if is_version_control(&invocation) {
        Category::ToolOutputVersionControl
    } else if contains_any(
        &invocation,
        &[
            "web__run",
            "search_query",
            "image_query",
            "cmd:curl ",
            "cmd:\"curl ",
            " curl ",
            "cmd:wget ",
            "cmd:\"wget ",
            " wget ",
            "createbrowsertab",
            "gettab(",
            "cua.",
        ],
    ) {
        Category::ToolOutputWeb
    } else if contains_any(
        &invocation,
        &["view_image", "imagegen", "emitimage", "generatedimage"],
    ) {
        Category::ToolOutputUiMedia
    } else if let Some(category) = search_listing {
        category
    } else if contains_any(
        &invocation,
        &["write_stdin", "tools.wait", "wait_agent", "yield_control"],
    ) {
        Category::ToolOutputProcessControl
    } else if contains_any(
        &invocation,
        &[
            "cmd:jq ",
            "cmd:\"jq ",
            " jq ",
            "cmd:python ",
            "cmd:\"python ",
            " python -c ",
            "cmd:perl ",
            "cmd:\"perl ",
            " perl ",
            "dataframe",
            "query_database",
        ],
    ) {
        Category::ToolOutputDataAnalysis
    } else if contains_any(
        &invocation,
        &[
            "cmd:env",
            "cmd:\"env",
            "cmd:which ",
            "cmd:\"which ",
            "cmd:du ",
            "cmd:\"du ",
            "cmd:df ",
            "cmd:\"df ",
            "cmd:ps ",
            "cmd:\"ps ",
            "cmd:stat ",
            "cmd:\"stat ",
            "nvidia-smi",
            "rustc --version",
            "node --version",
        ],
    ) {
        Category::ToolOutputSystemInfo
    } else if looks_like_diagnostic(text) {
        Category::ToolOutputDiagnostics
    } else if invocation.contains("exec_command") {
        Category::ToolOutputShell
    } else if looks_like_code(text) {
        Category::ToolOutputCode
    } else {
        Category::ToolOutputOther
    }
}

fn repository_code_category(source: Option<Category>) -> Category {
    match source {
        Some(Category::ToolOutputSearchRg) => Category::RepositoryCodeRg,
        Some(Category::ToolOutputSed) => Category::RepositoryCodeSed,
        Some(Category::ToolOutputHead) => Category::RepositoryCodeHead,
        Some(Category::ToolOutputCat) => Category::RepositoryCodeCat,
        Some(Category::ToolOutputTail) => Category::RepositoryCodeTail,
        _ => Category::RepositoryCode,
    }
}

fn is_repository_code_category(category: Category) -> bool {
    matches!(
        category,
        Category::RepositoryCode
            | Category::RepositoryCodeRg
            | Category::RepositoryCodeSed
            | Category::RepositoryCodeHead
            | Category::RepositoryCodeCat
            | Category::RepositoryCodeTail
    )
}

fn looks_like_source_path(invocation: &str) -> bool {
    contains_any(
        invocation,
        &[
            ".rs", ".py", ".js", ".jsx", ".ts", ".tsx", ".go", ".java", ".c ", ".cpp", ".cc",
            ".h ", ".hpp", ".cs", ".swift", ".kt", ".kts", ".rb", ".php", ".sh", ".bash", ".zsh",
            ".fish", ".sql", ".html", ".css", ".scss", ".sass", ".vue", ".svelte", ".json",
            ".toml", ".yaml", ".yml", ".xml",
        ],
    )
}

fn search_listing_category(invocation: &str) -> Option<Category> {
    if is_shell_command(invocation, "rg") {
        Some(Category::ToolOutputSearchRg)
    } else if is_shell_command(invocation, "grep") {
        Some(Category::ToolOutputSearchGrep)
    } else if is_shell_command(invocation, "find") {
        Some(Category::ToolOutputListingFind)
    } else if is_shell_command(invocation, "ls") {
        Some(Category::ToolOutputListingLs)
    } else if contains_any(invocation, &["cmd:pwd", "cmd:\"pwd"]) {
        Some(Category::ToolOutputListingPwd)
    } else if invocation.contains("list_mcp_resources") {
        Some(Category::ToolOutputListingMcpResources)
    } else if invocation.contains("list_files") {
        Some(Category::ToolOutputListingFiles)
    } else if contains_any(invocation, &["read_file", "get_file", "open_file"]) {
        Some(Category::ToolOutputReadFile)
    } else if is_shell_command(invocation, "cat") {
        Some(Category::ToolOutputCat)
    } else if is_shell_command(invocation, "sed") {
        Some(Category::ToolOutputSed)
    } else if is_shell_command(invocation, "head") {
        Some(Category::ToolOutputHead)
    } else if is_shell_command(invocation, "tail") {
        Some(Category::ToolOutputTail)
    } else if contains_any(
        invocation,
        &[
            "search_files",
            "file_search",
            "find_files",
            "list_directory",
            "glob",
        ],
    ) {
        Some(Category::ToolOutputSearchListing)
    } else {
        None
    }
}

fn is_shell_command(invocation: &str, name: &str) -> bool {
    [
        format!("cmd:{name} "),
        format!("cmd:\"{name} "),
        format!("cmd: \"{name} "),
        format!("\"cmd\":\"{name} "),
        format!("\"cmd\": \"{name} "),
        format!("&& {name} "),
        format!("; {name} "),
        format!("| {name} "),
    ]
    .iter()
    .any(|pattern| invocation.contains(pattern))
}

fn is_file_reader_category(category: Category) -> bool {
    matches!(
        category,
        Category::ToolOutputSearchRg
            | Category::ToolOutputSearchGrep
            | Category::ToolOutputListingFiles
            | Category::ToolOutputReadFile
            | Category::ToolOutputCat
            | Category::ToolOutputSed
            | Category::ToolOutputHead
            | Category::ToolOutputTail
    )
}

fn is_direct_file_reader_category(category: Category) -> bool {
    matches!(
        category,
        Category::ToolOutputReadFile
            | Category::ToolOutputCat
            | Category::ToolOutputSed
            | Category::ToolOutputHead
            | Category::ToolOutputTail
    )
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| haystack.contains(needle))
}

fn is_build_or_test(invocation: &str) -> bool {
    contains_any(
        invocation,
        &[
            "cargo test",
            "cargo build",
            "cargo check",
            "cargo clippy",
            "cargo fmt",
            "npm test",
            "npm run",
            "pnpm test",
            "pnpm run",
            "yarn test",
            "yarn run",
            "pytest",
            "python -m pytest",
            "go test",
            "go build",
            "dotnet test",
            "dotnet build",
            "mvn test",
            "gradle test",
            "./gradlew",
            "make test",
            "cmake --build",
            "swift test",
            "rustc ",
        ],
    )
}

fn is_version_control(invocation: &str) -> bool {
    contains_any(
        invocation,
        &[
            "cmd:git ",
            "cmd:\"git ",
            " git status",
            " git diff",
            " git show",
            " git log",
            " git add",
            " git commit",
        ],
    )
}

fn looks_like_diagnostic(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    contains_any(
        &lower,
        &[
            "\"iserror\":true",
            "fatal:",
            "error:",
            "traceback (most recent call last)",
            "permission denied",
            "command not found",
            "no such file or directory",
        ],
    ) || nonzero_exit_code(&lower)
}

fn nonzero_exit_code(text: &str) -> bool {
    let Some((_, suffix)) = text.split_once("\"exit_code\":") else {
        return false;
    };
    let value = suffix
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    !value.is_empty() && value != "0"
}

fn looks_like_code(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().take(400).collect();
    if lines.len() < 2 {
        return false;
    }
    let code_lines = lines
        .iter()
        .filter(|line| looks_like_code_line(line))
        .count();
    code_lines >= 2 && code_lines * 5 >= lines.len()
}

fn estimate_embedded_code_tokens(text: &str) -> u64 {
    let total = estimate_tokens(text);
    embedded_code_regions_tokens(text).min(total)
}

fn embedded_code_regions_tokens(text: &str) -> u64 {
    let mut in_fence = false;
    let mut in_hunk = false;
    let mut total: u64 = 0;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            total = total.saturating_add(estimate_tokens(line));
            continue;
        }
        if line.starts_with("diff --git ") {
            in_hunk = false;
        } else if line.starts_with("@@ ") {
            in_hunk = true;
        } else if in_hunk && !line.starts_with("\\ No newline at end of file") {
            let code = line
                .strip_prefix('+')
                .or_else(|| line.strip_prefix('-'))
                .or_else(|| line.strip_prefix(' '))
                .unwrap_or(line);
            total = total.saturating_add(estimate_tokens(code));
        } else if let Some(code) = diagnostic_code(line) {
            total = total.saturating_add(estimate_tokens(code));
        } else if looks_like_code_line(line) {
            total = total.saturating_add(estimate_tokens(line.trim()));
        }
    }
    total
}

fn looks_like_code_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("diff --git")
        || line.starts_with("@@ ")
        || line.starts_with("fn ")
        || line.starts_with("pub ")
        || line.starts_with("def ")
        || line.starts_with("class ")
        || line.starts_with("import ")
        || line.starts_with("use ")
        || line.starts_with("const ")
        || line.starts_with("let ")
        || line.starts_with("function ")
        || (line.contains('=') && (line.ends_with(';') || line.contains("=>")))
}

fn diagnostic_code(line: &str) -> Option<&str> {
    let (prefix, code) = line.split_once('|')?;
    let prefix = prefix.trim();
    (!prefix.is_empty() && prefix.chars().all(|character| character.is_ascii_digit()))
        .then(|| code.trim())
}

fn proportional_part(value: u64, part: u64, whole: u64) -> u64 {
    value.saturating_mul(part).checked_div(whole).unwrap_or(0)
}

fn estimate_tokens(text: &str) -> u64 {
    let chars = text.chars().count() as u64;
    if chars == 0 { 0 } else { chars.div_ceil(4) }
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn parse_timestamp(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.to_utc())
}

fn percent(value: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        value as f64 * 100.0 / total as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_tokens_follow_the_context_prefix() {
        let context = vec![
            Item {
                category: Category::Instructions,
                tokens: 10,
                code_tokens: 0,
            },
            Item {
                category: Category::UserPrompts,
                tokens: 10,
                code_tokens: 0,
            },
            Item {
                category: Category::ToolCalls,
                tokens: 10,
                code_tokens: 0,
            },
        ];
        let mut totals = BTreeMap::new();
        attribute(&context, 30, 15, None, &mut totals);
        assert_eq!(totals[&Category::Instructions].input, 10);
        assert_eq!(totals[&Category::Instructions].cached_input, 10);
        assert_eq!(totals[&Category::UserPrompts].input, 10);
        assert_eq!(totals[&Category::UserPrompts].cached_input, 5);
        assert_eq!(totals[&Category::ToolCalls].input, 10);
        assert_eq!(totals[&Category::ToolCalls].cached_input, 0);
    }

    #[test]
    fn code_tokens_follow_input_and_cache_attribution() {
        let context = vec![
            Item {
                category: Category::UserPrompts,
                tokens: 20,
                code_tokens: 10,
            },
            Item {
                category: Category::AssistantText,
                tokens: 20,
                code_tokens: 20,
            },
        ];
        let mut totals = BTreeMap::new();
        attribute(&context, 40, 30, None, &mut totals);
        assert_eq!(totals[&Category::UserPrompts].code_input, 10);
        assert_eq!(totals[&Category::UserPrompts].cached_code_input, 10);
        assert_eq!(totals[&Category::AssistantText].code_input, 20);
        assert_eq!(totals[&Category::AssistantText].cached_code_input, 10);
    }

    #[test]
    fn embedded_code_detects_fences_diffs_and_diagnostics() {
        assert!(estimate_embedded_code_tokens("text\n```rust\nfn main() {}\n```") > 0);
        assert!(estimate_embedded_code_tokens("diff --git a/a b/a\n@@ -1 +1 @@\n-old\n+new") > 0);
        assert!(estimate_embedded_code_tokens("error\n12 | let value = broken();") > 0);
    }

    #[test]
    fn embedded_code_combines_non_overlapping_regions_and_keeps_untyped_first_line() {
        let text = "before\n```\nfn only_line() {}\n```\nafter\n12 | let value = broken();";
        assert_eq!(
            estimate_embedded_code_tokens(text),
            estimate_tokens("fn only_line() {}") + estimate_tokens("let value = broken();")
        );
    }

    #[test]
    fn opaque_compaction_absorbs_unobserved_context() {
        let context = vec![
            Item {
                category: Category::Instructions,
                tokens: 10,
                code_tokens: 0,
            },
            Item {
                category: Category::CompactionSummary,
                tokens: 0,
                code_tokens: 0,
            },
        ];
        let mut totals = BTreeMap::new();
        attribute(&context, 40, 35, Some(5), &mut totals);
        assert_eq!(totals[&Category::CompactionSummary].input, 25);
        assert_eq!(totals[&Category::CompactionSummary].cached_input, 20);
        assert_eq!(totals[&Category::Unknown].input, 5);
        assert_eq!(totals[&Category::Unknown].cached_input, 5);
    }

    #[test]
    fn tool_outputs_are_split_by_invocation() {
        let build = (
            "exec".to_owned(),
            "tools.exec_command({cmd:\"cargo test\"})".to_owned(),
        );
        let search = (
            "exec".to_owned(),
            "tools.exec_command({cmd:\"rg -n foo src\"})".to_owned(),
        );
        let git = (
            "exec".to_owned(),
            "tools.exec_command({cmd:\"git status\"})".to_owned(),
        );
        assert_eq!(
            classify_tool_output("test result: ok", Some(&build)),
            Category::ToolOutputBuildTest
        );
        assert_eq!(
            classify_tool_output("src/main.rs:12:foo", Some(&search)),
            Category::ToolOutputSearchRg
        );
        assert_eq!(
            classify_tool_output("fn first() {}\nfn second() {}", Some(&search)),
            Category::RepositoryCodeRg
        );
        assert_eq!(
            classify_tool_output("On branch main", Some(&git)),
            Category::ToolOutputVersionControl
        );
    }

    #[test]
    fn search_and_listing_outputs_are_split_by_command() {
        let cases = [
            (
                "tools.exec_command({\"cmd\":\"grep foo src\"})",
                Category::ToolOutputSearchGrep,
            ),
            (
                "tools.exec_command({\"cmd\":\"find src -type f\"})",
                Category::ToolOutputListingFind,
            ),
            (
                "tools.exec_command({\"cmd\":\"ls -la\"})",
                Category::ToolOutputListingLs,
            ),
            (
                "tools.exec_command({\"cmd\":\"sed -n 1,20p README.md\"})",
                Category::ToolOutputSed,
            ),
        ];
        for (input, expected) in cases {
            let call = ("exec".to_owned(), input.to_owned());
            assert_eq!(classify_tool_output("one result", Some(&call)), expected);
        }
    }

    #[test]
    fn patch_tool_call_separates_envelope_from_payload() {
        let input = r#"const patch = "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** End Patch"; text(await tools.apply_patch(patch));"#;
        let mut pending = Vec::new();
        ingest_tool_call(&mut pending, "exec", input);
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].category, Category::PatchEditEnvelope);
        assert_eq!(pending[1].category, Category::PatchEditPayload);
        assert!(pending[0].tokens > 0);
        assert!(pending[1].tokens > 0);
        assert_eq!(
            pending.iter().map(|item| item.tokens).sum::<u64>(),
            estimate_tokens("exec") + estimate_tokens(input)
        );
    }

    #[test]
    fn mentioning_patch_tools_does_not_turn_a_search_into_an_edit() {
        let search = r#"tools.exec_command({"cmd":"rg apply_patch src"})"#;
        assert!(!is_patch_edit_call("exec", search));
        assert!(is_patch_edit_call(
            "exec",
            r#"text(await tools.apply_patch(patch))"#
        ));
    }

    #[test]
    fn source_extension_does_not_turn_plain_search_results_into_code() {
        let search = (
            "exec".to_owned(),
            r#"tools.exec_command({"cmd":"rg TODO src/main.rs"})"#.to_owned(),
        );
        assert_eq!(
            classify_tool_output("src/main.rs:12:TODO document this", Some(&search)),
            Category::ToolOutputSearchRg
        );
    }

    #[test]
    fn output_and_reasoning_are_attributed_separately() {
        let pending = vec![
            Item {
                category: Category::AssistantText,
                tokens: 10,
                code_tokens: 0,
            },
            Item {
                category: Category::ToolCalls,
                tokens: 30,
                code_tokens: 0,
            },
            Item {
                category: Category::AssistantReasoning,
                tokens: 0,
                code_tokens: 0,
            },
        ];
        let mut totals = BTreeMap::new();
        attribute_output(&pending, 100, 20, true, &mut totals);
        assert_eq!(totals[&Category::AssistantReasoning].output, 20);
        assert_eq!(totals[&Category::AssistantReasoning].reasoning_output, 20);
        assert_eq!(totals[&Category::AssistantText].output, 20);
        assert_eq!(totals[&Category::ToolCalls].output, 60);
    }
}
