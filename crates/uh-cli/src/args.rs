//! CLI argument parsing, a faithful port of `cli/args.ts` from the TypeScript
//! product. The parser is intentionally hand-written: `--resume`/`--print`
//! optionally consume the next token and diagnostics carry their own
//! formatting, none of which a stock derive-based parser can express.

use uh_types::ai::ModelThinkingLevel;

/// Supported reasoning-effort levels (`THINKING_LEVELS` in the TS product).
pub const THINKING_LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// Parse a thinking level value into the shared uh-types enum.
pub fn parse_thinking_level(value: &str) -> Option<ModelThinkingLevel> {
    match value {
        "off" => Some(ModelThinkingLevel::Off),
        "minimal" => Some(ModelThinkingLevel::Minimal),
        "low" => Some(ModelThinkingLevel::Low),
        "medium" => Some(ModelThinkingLevel::Medium),
        "high" => Some(ModelThinkingLevel::High),
        "xhigh" => Some(ModelThinkingLevel::Xhigh),
        "max" => Some(ModelThinkingLevel::Max),
        _ => None,
    }
}

/// Removed built-in tool names that used to exist and now produce an error.
const REMOVED_BUILTIN_TOOL_NAMES: [&str; 5] = ["read", "write", "grep", "find", "ls"];
/// Current built-in tool names reported in tool validation errors.
const BUILTIN_TOOL_NAMES: [&str; 1] = ["ipython"];
/// Value flags whose free-form text may legitimately start with a dash.
const FREEFORM_VALUE_FLAGS: [&str; 2] = ["--goal", "--autonomous-gate"];
/// Prompt value flags whose text may look like a long option (YAML frontmatter).
const PROMPT_VALUE_FLAGS: [&str; 2] = ["--system-prompt", "--append-system-prompt"];

/// Marker argv token that re-enables internal runtime-only flags
/// (`--export`, `--list-models`) after the public command rewrite.
pub const INTERNAL_RUNTIME_COMMAND_MARKER: &str = "\0unharnessed-runtime-command";

/// Output mode selected by `--mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Text,
    Json,
    Acp,
}

impl Mode {
    pub fn parse(value: &str) -> Option<Mode> {
        match value {
            "text" => Some(Mode::Text),
            "json" => Some(Mode::Json),
            "acp" => Some(Mode::Acp),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Text => "text",
            Mode::Json => "json",
            Mode::Acp => "acp",
        }
    }
}

/// A parse-time diagnostic: warnings are printed, errors also exit 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub is_error: bool,
    pub message: String,
}

impl Diagnostic {
    fn error(message: impl Into<String>) -> Self {
        Diagnostic {
            is_error: true,
            message: message.into(),
        }
    }
}

/// Autonomous gate options, mirroring `runtimeAutonomousConfigFromArgs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutonomousGates {
    pub commands: Vec<String>,
    pub max_retries: Option<u32>,
    pub timeout_ms: Option<u64>,
}

/// The autonomous configuration inferred from the CLI flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutonomousConfig {
    pub max_continuations: Option<u32>,
    pub max_turns: Option<u32>,
    pub max_tokens: Option<u64>,
    pub timeout_ms: Option<u64>,
    pub gates: Option<AutonomousGates>,
}

impl AutonomousConfig {
    /// Build the autonomous runtime config from parsed args; `None` when no
    /// autonomous flag was given at all.
    pub fn from_args(args: &Args) -> Option<AutonomousConfig> {
        let has_autonomous_options = args.autonomous
            || args.autonomous_gates.is_some()
            || args.autonomous_gate_retries.is_some()
            || args.autonomous_gate_timeout_ms.is_some()
            || args.autonomous_max_continuations.is_some()
            || args.autonomous_max_turns.is_some()
            || args.autonomous_max_tokens.is_some()
            || args.autonomous_timeout_ms.is_some();
        if !has_autonomous_options {
            return None;
        }
        let has_gate_options = args.autonomous_gates.is_some()
            || args.autonomous_gate_retries.is_some()
            || args.autonomous_gate_timeout_ms.is_some();
        Some(AutonomousConfig {
            max_continuations: args.autonomous_max_continuations,
            max_turns: args.autonomous_max_turns,
            max_tokens: args.autonomous_max_tokens,
            timeout_ms: args.autonomous_timeout_ms,
            gates: has_gate_options.then(|| AutonomousGates {
                commands: args.autonomous_gates.clone().unwrap_or_default(),
                max_retries: args.autonomous_gate_retries,
                timeout_ms: args.autonomous_gate_timeout_ms,
            }),
        })
    }
}

/// Parsed CLI arguments, mirroring `Args` in `cli/args.ts`.
#[derive(Debug, Clone, Default)]
#[allow(clippy::struct_excessive_bools)] // the TS `Args` surface mirrored verbatim
pub struct Args {
    pub provider: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub cwd: Option<String>,
    pub system_prompt: Option<String>,
    pub append_system_prompt: Vec<String>,
    pub thinking: Option<ModelThinkingLevel>,
    pub continue_: bool,
    /// `true` when `--resume` was given without a selector.
    pub resume_bare: bool,
    /// The `--resume <selector>` value.
    pub resume: Option<String>,
    pub help: bool,
    pub version: bool,
    pub mode: Option<Mode>,
    pub no_session: bool,
    pub fork: Option<String>,
    pub session_dir: Option<String>,
    pub models: Option<Vec<String>>,
    pub tools: Option<Vec<String>>,
    pub no_tools: bool,
    pub no_builtin_tools: bool,
    pub print: bool,
    pub export: Option<String>,
    pub no_skills: bool,
    pub skills: Vec<String>,
    pub prompt_templates: Vec<String>,
    pub no_prompt_templates: bool,
    pub no_context_files: bool,
    pub autonomous: bool,
    pub autonomous_gates: Option<Vec<String>>,
    pub autonomous_gate_retries: Option<u32>,
    pub autonomous_gate_timeout_ms: Option<u64>,
    pub autonomous_max_continuations: Option<u32>,
    pub autonomous_max_turns: Option<u32>,
    pub autonomous_max_tokens: Option<u64>,
    pub autonomous_timeout_ms: Option<u64>,
    /// `--background-wait-ms`: how long `-p` waits for pending wakes
    /// (background `bash()`, goal backoff, quota park) once idle.
    pub background_wait_ms: Option<u64>,
    pub goal: Option<String>,
    pub goal_token_budget: Option<u32>,
    /// The `--list-models` flag's tri-state (None = absent,
    /// Some(None) = the bare flag, Some(Some(x)) = with a value).
    #[allow(clippy::option_option)] // the CLI flag's tri-state is genuinely a double Option
    pub list_models: Option<Option<String>>,
    pub offline: bool,
    pub verbose: bool,
    pub messages: Vec<String>,
    #[allow(clippy::struct_field_names)]
    // the trailing _args matches the TS `fileArgs` wire surface
    pub file_args: Vec<String>,
    /// Where the messages and the `@file` arguments stand among the
    /// arguments parsed, and the `--` before them.
    pub positional: Vec<usize>,
    /// Where the arguments that choose the conversation stand: `--continue`,
    /// `--resume` and `--fork`, each with its value; and where
    /// `--no-session` does.
    pub selecting: Vec<usize>,
    pub unsaved: Vec<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Args {
    /// True when `--resume` appeared (with or without a selector).
    pub fn has_resume(&self) -> bool {
        self.resume_bare || self.resume.is_some()
    }
}

fn parse_positive_u32(value: &str, flag: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<u32> {
    // Number() accepts arbitrary precision, so parse as i128 to cover the
    // full accepted range before the integer/positivity/range checks. The
    // u32::MAX bound is the point of the function: a value past it is a
    // user error, never a silent truncation into the flag's u32 field.
    match value.trim().parse::<i128>() {
        Ok(parsed) if parsed > 0 && parsed <= i128::from(u32::MAX) => u32::try_from(parsed).ok(),
        _ => {
            diagnostics.push(Diagnostic::error(format!(
                "{flag} must be a positive integer between 1 and u32::MAX (4294967295)"
            )));
            None
        }
    }
}

fn parse_positive_u64(value: &str, flag: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<u64> {
    // Number() accepts arbitrary precision, so parse as i128 to cover the
    // full accepted range before the integer/positivity check.
    match value.trim().parse::<i128>() {
        Ok(parsed) if parsed > 0 && parsed <= i128::from(u64::MAX) => u64::try_from(parsed).ok(),
        _ => {
            diagnostics.push(Diagnostic::error(format!(
                "{flag} must be a positive integer between 1 and u64::MAX (18446744073709551615)"
            )));
            None
        }
    }
}

fn parse_u64(value: &str, flag: &str, diagnostics: &mut Vec<Diagnostic>) -> Option<u64> {
    let parsed = value.trim().parse::<u64>().ok();
    if parsed.is_none() {
        diagnostics.push(Diagnostic::error(format!(
            "{flag} must be an integer between 0 and u64::MAX (18446744073709551615)"
        )));
    }
    parsed
}

/// Parse the CLI arguments the same way `parseArgs` in `cli/args.ts` does.
pub fn parse_args(args: &[String]) -> Args {
    let mut result = Args::default();
    let internal_runtime_command =
        args.first().map(String::as_str) == Some(INTERNAL_RUNTIME_COMMAND_MARKER);
    let first_arg_index = usize::from(internal_runtime_command);

    let mut end_of_options = false;
    let mut i = first_arg_index;
    while i < args.len() {
        let arg = args[i].as_str();

        if end_of_options {
            result.messages.push(arg.to_string());
            result.positional.push(i);
            i += 1;
            continue;
        }
        if arg == "--" {
            end_of_options = true;
            result.positional.push(i);
            i += 1;
            continue;
        }

        macro_rules! require_value {
            ($flag:expr) => {
                match take_required_value(args, i, $flag, &mut result.diagnostics) {
                    Some(value) => {
                        i += 1;
                        value
                    }
                    None => {
                        i += 1;
                        continue;
                    }
                }
            };
        }

        match arg {
            "--help" | "-h" => result.help = true,
            "--version" | "-v" => result.version = true,
            "--mode" => {
                let mode = require_value!(arg);
                if let Some(mode) = Mode::parse(&mode) {
                    result.mode = Some(mode);
                } else {
                    result.diagnostics.push(Diagnostic::error(format!(
                        "Invalid --mode \"{mode}\". Valid values: text, json, acp"
                    )));
                }
            }
            "--continue" | "-c" => {
                result.continue_ = true;
                result.selecting.push(i);
            }
            "--resume" | "-r" => {
                result.selecting.push(i);
                match args.get(i + 1) {
                    Some(next)
                        if !next.starts_with('-') && !next.starts_with('@') && !next.is_empty() =>
                    {
                        result.resume = Some(next.clone());
                        result.selecting.push(i + 1);
                        i += 1;
                    }
                    Some(next) if next.is_empty() => {
                        result.resume_bare = true;
                        result.selecting.push(i + 1);
                        i += 1;
                    }
                    _ => result.resume_bare = true,
                }
            }
            "--provider" => result.provider = Some(require_value!(arg)),
            "--model" => result.model = Some(require_value!(arg)),
            "--api-key" => result.api_key = Some(require_value!(arg)),
            "--cwd" => result.cwd = Some(require_value!(arg)),
            "--system-prompt" => result.system_prompt = Some(require_value!(arg)),
            "--append-system-prompt" => {
                let value = require_value!(arg);
                result.append_system_prompt.push(value);
            }
            "--no-session" => {
                result.no_session = true;
                result.unsaved.push(i);
            }
            "--fork" => {
                // With its value, or alone where it has none.
                result.selecting.push(i);
                result.fork = Some(require_value!(arg));
                result.selecting.push(i);
            }
            "--session-dir" => result.session_dir = Some(require_value!(arg)),
            "--models" => {
                let value = require_value!(arg);
                result.models = Some(
                    value
                        .split(',')
                        .map(str::trim)
                        .map(str::to_string)
                        .collect(),
                );
            }
            "--no-tools" | "-nt" => result.no_tools = true,
            "--no-builtin-tools" | "-nbt" => result.no_builtin_tools = true,
            "--tools" | "-t" => {
                let value = require_value!(arg);
                let tools: Vec<String> = value
                    .split(',')
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(str::to_string)
                    .collect();
                let removed: Vec<&str> = tools
                    .iter()
                    .filter(|name| REMOVED_BUILTIN_TOOL_NAMES.contains(&name.as_str()))
                    .map(String::as_str)
                    .collect();
                if !removed.is_empty() {
                    result.diagnostics.push(Diagnostic::error(format!(
                        "Unknown built-in tool(s): {}. Available built-in tools: {}",
                        removed.join(", "),
                        BUILTIN_TOOL_NAMES.join(", ")
                    )));
                }
                result.tools = Some(tools);
            }
            "--thinking" => {
                let level = require_value!(arg);
                if let Some(level) = parse_thinking_level(&level) {
                    result.thinking = Some(level);
                } else {
                    result.diagnostics.push(Diagnostic::error(format!(
                        "Invalid thinking level \"{level}\". Valid values: {}",
                        THINKING_LEVELS.join(", ")
                    )));
                }
            }
            "--print" | "-p" => {
                result.print = true;
                if let Some(next) = args.get(i + 1) {
                    if !next.starts_with('@') && (!next.starts_with('-') || next.starts_with("---"))
                    {
                        result.messages.push(next.clone());
                        result.positional.push(i + 1);
                        i += 1;
                    }
                }
            }
            "--export" => {
                if !internal_runtime_command {
                    result.diagnostics.push(Diagnostic::error(
                        "--export was removed. Use \"unharnessed session export <file> [output]\".",
                    ));
                    if let Some(next) = args.get(i + 1) {
                        if !next.starts_with('-') {
                            i += 1;
                        }
                    }
                } else if let Some(next) = args.get(i + 1) {
                    result.export = Some(next.clone());
                    i += 1;
                } else {
                    result
                        .diagnostics
                        .push(Diagnostic::error("--export requires a value"));
                }
            }
            "--skill" => {
                let value = require_value!(arg);
                result.skills.push(value);
            }
            "--prompt-template" => {
                let value = require_value!(arg);
                result.prompt_templates.push(value);
            }
            "--no-skills" | "-ns" => result.no_skills = true,
            "--no-prompt-templates" | "-np" => result.no_prompt_templates = true,
            "--no-context-files" | "-nc" => result.no_context_files = true,
            "--autonomous" => result.autonomous = true,
            "--autonomous-gate" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result
                    .autonomous_gates
                    .get_or_insert_with(Vec::new)
                    .push(value);
            }
            "--autonomous-gate-retries" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_gate_retries =
                    parse_positive_u32(&value, arg, &mut result.diagnostics);
            }
            "--autonomous-gate-timeout-ms" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_gate_timeout_ms =
                    parse_positive_u64(&value, arg, &mut result.diagnostics);
            }
            "--autonomous-max-continuations" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_max_continuations =
                    parse_positive_u32(&value, arg, &mut result.diagnostics);
            }
            "--autonomous-max-turns" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_max_turns =
                    parse_positive_u32(&value, arg, &mut result.diagnostics);
            }
            "--autonomous-max-tokens" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_max_tokens =
                    parse_positive_u64(&value, arg, &mut result.diagnostics);
            }
            "--autonomous-timeout-ms" => {
                result.autonomous = true;
                let value = require_value!(arg);
                result.autonomous_timeout_ms =
                    parse_positive_u64(&value, arg, &mut result.diagnostics);
            }
            "--background-wait-ms" => {
                let value = require_value!(arg);
                result.background_wait_ms = parse_u64(&value, arg, &mut result.diagnostics);
            }
            "--goal" => {
                let value = require_value!(arg);
                if value.trim().is_empty() {
                    result
                        .diagnostics
                        .push(Diagnostic::error("--goal requires a non-empty objective"));
                } else {
                    result.goal = Some(value);
                }
            }
            "--goal-token-budget" => {
                let value = require_value!(arg);
                result.goal_token_budget = parse_positive_u32(&value, arg, &mut result.diagnostics);
            }
            "--list-models" => {
                let has_search = args
                    .get(i + 1)
                    .is_some_and(|next| !next.starts_with('-') && !next.starts_with('@'));
                if !internal_runtime_command {
                    result.diagnostics.push(Diagnostic::error(
                        "--list-models was removed. Use \"unharnessed model list [search]\".",
                    ));
                    if has_search {
                        i += 1;
                    }
                } else if has_search {
                    result.list_models = Some(Some(args[i + 1].clone()));
                    i += 1;
                } else {
                    result.list_models = Some(None);
                }
            }
            "--verbose" => result.verbose = true,
            "--offline" => result.offline = true,
            _ if arg.starts_with("--resume=") => {
                result.selecting.push(i);
                let value = &arg["--resume=".len()..];
                if value.is_empty() {
                    result.resume_bare = true;
                } else {
                    result.resume = Some(value.to_string());
                }
            }
            _ if arg.starts_with("--export=") => {
                result.diagnostics.push(Diagnostic::error(
                    "--export was removed. Use \"unharnessed session export <file> [output]\".",
                ));
            }
            _ if arg.starts_with("--list-models=") => {
                result.diagnostics.push(Diagnostic::error(
                    "--list-models was removed. Use \"unharnessed model list [search]\".",
                ));
            }
            _ if arg.starts_with('@') => {
                result.file_args.push(arg[1..].to_string());
                result.positional.push(i);
            }
            _ if arg.starts_with('-') => {
                result
                    .diagnostics
                    .push(Diagnostic::error(format!("Unknown option: {arg}")));
            }
            _ => {
                result.messages.push(arg.to_string());
                result.positional.push(i);
            }
        }
        i += 1;
    }

    if result.goal_token_budget.is_some() && result.goal.is_none() {
        result
            .diagnostics
            .push(Diagnostic::error("--goal-token-budget requires --goal"));
    }

    result
}

/// The conversation an agent process is started on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOn {
    /// The one the launch chose; a new one where it chose none, or left
    /// the choice to the user (`--resume` alone).
    Launch,
    /// A new one, whatever the launch chose.
    New,
    /// A saved one, which the client names itself (`--resume=<file>`).
    Saved,
}

/// The arguments of the agent process a client starts for this launch (the
/// terminal UI): the launch's own options, without the messages and the
/// `@file` arguments, which the client sends itself, then `--mode acp`.
/// For another conversation than the launch's, they come without what
/// chose that one; for a saved one, without `--no-session` either.
/// `parsed` is what [`parse_args`] made of `args`.
#[must_use]
pub fn agent_args(args: &[String], parsed: &Args, on: AgentOn) -> Vec<String> {
    let internal = args.first().map(String::as_str) == Some(INTERNAL_RUNTIME_COMMAND_MARKER);
    let on = match on {
        AgentOn::Launch if parsed.resume_bare => AgentOn::New,
        on => on,
    };
    let dropped = |index: &usize| {
        parsed.positional.contains(index)
            || (on != AgentOn::Launch && parsed.selecting.contains(index))
            || (on == AgentOn::Saved && parsed.unsaved.contains(index))
    };
    let mut out: Vec<String> = args
        .iter()
        .enumerate()
        .skip(usize::from(internal))
        .filter(|(index, _)| !dropped(index))
        .map(|(_, arg)| arg.clone())
        .collect();
    out.extend(["--mode".to_string(), "acp".to_string()]);
    out
}

/// `hasRequiredOptionValue` from args.ts: the value flag either consumes the
/// next token or records a "requires a value" error.
fn take_required_value(
    args: &[String],
    index: usize,
    flag: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let Some(next) = args.get(index + 1) else {
        diagnostics.push(Diagnostic::error(format!("{flag} requires a value")));
        return None;
    };
    let value_may_start_with_dash = FREEFORM_VALUE_FLAGS.contains(&flag);
    let value_is_arbitrary_prompt_text = PROMPT_VALUE_FLAGS.contains(&flag);
    if next == "--"
        || (!value_is_arbitrary_prompt_text
            && next.starts_with(if value_may_start_with_dash { "--" } else { "-" }))
    {
        diagnostics.push(Diagnostic::error(format!("{flag} requires a value")));
        return None;
    }
    Some(next.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        let args: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
        parse_args(&args)
    }

    fn last_error(args: &Args) -> &str {
        args.diagnostics
            .iter()
            .rev()
            .find(|d| d.is_error)
            .map(|d| d.message.as_str())
            .unwrap_or_default()
    }

    /// The agent's arguments, and what the agent then reads in them.
    fn for_agent(args: &[&str]) -> (Vec<String>, Args) {
        let args: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
        let out = agent_args(&args, &parse_args(&args), AgentOn::Launch);
        let read = parse_args(&out);
        (out, read)
    }

    /// The agent's arguments for another conversation than the launch's.
    fn for_agent_on(args: &[&str], on: AgentOn) -> Vec<String> {
        let args: Vec<String> = args.iter().map(std::string::ToString::to_string).collect();
        agent_args(&args, &parse_args(&args), on)
    }

    // How an agent started on another conversation could open the wrong
    // one: what chose the launch's stays, and wins over the one named; its
    // value stays alone and becomes a message (refused in ACP mode);
    // `--resume` alone stays and the agent, which cannot ask, refuses to
    // start; `--resume=<id>` is not seen as a choice; an option that
    // follows `--resume` alone goes with it; `--no-session` stays on a
    // saved conversation, which then does not open, or goes from a new
    // one, which is then saved against the launch's word.
    #[test]
    fn agent_args_for_another_conversation_drop_what_chose_the_first() {
        let launch = ["--model", "gpt-5", "--resume", "abc", "--no-session", "hi"];
        assert_eq!(
            for_agent_on(&launch, AgentOn::New),
            ["--model", "gpt-5", "--no-session", "--mode", "acp"]
        );
        assert_eq!(
            for_agent_on(&launch, AgentOn::Saved),
            ["--model", "gpt-5", "--mode", "acp"]
        );
        assert_eq!(
            for_agent_on(&launch, AgentOn::Launch),
            [
                "--model",
                "gpt-5",
                "--resume",
                "abc",
                "--no-session",
                "--mode",
                "acp"
            ]
        );

        for chose in [
            &["-c"][..],
            &["--continue"],
            &["--resume=abc"],
            &["-r", "abc"],
            &["--fork", "abc"],
            &["--resume", ""],
        ] {
            let launch = [chose, &["--verbose"]].concat();
            for on in [AgentOn::New, AgentOn::Saved] {
                assert_eq!(
                    for_agent_on(&launch, on),
                    ["--verbose", "--mode", "acp"],
                    "{chose:?}"
                );
            }
        }

        // `--resume` alone leaves the choice to the user: the first agent
        // starts on a new conversation, and the option after it stays.
        for on in [AgentOn::Launch, AgentOn::New, AgentOn::Saved] {
            assert_eq!(
                for_agent_on(&["--resume", "--verbose"], on),
                ["--verbose", "--mode", "acp"]
            );
        }
        assert_eq!(for_agent_on(&["-r"], AgentOn::Launch), ["--mode", "acp"]);
    }

    // How the agent the terminal UI starts could get the wrong launch: a
    // message or an `@file` reaches it (it refuses `@file` in ACP mode); a
    // message after `--` that looks like an option becomes that option; an
    // option's value that looks like a message goes with the messages, and
    // the option takes the next argument; the launch's own `--mode` wins
    // over `acp`; the `--` stays and `--mode acp` becomes a message; the
    // internal marker, which no process can be started with, stays first.
    #[test]
    fn agent_args_keep_the_options_and_drop_what_the_client_sends() {
        let (out, read) = for_agent(&["--model", "gpt-5", "hello", "@notes.md", "--no-session"]);
        assert_eq!(out, ["--model", "gpt-5", "--no-session", "--mode", "acp"]);
        assert_eq!(read.model.as_deref(), Some("gpt-5"));
        assert!(read.messages.is_empty() && read.file_args.is_empty() && read.no_session);
        assert_eq!(read.mode, Some(Mode::Acp));

        // A message after `--` that looks like an option stays a message.
        let (out, read) = for_agent(&["-c", "--", "--no-session", "@not-a-file"]);
        assert_eq!(out, ["-c", "--mode", "acp"]);
        assert!(read.continue_ && !read.no_session && read.file_args.is_empty());
        assert_eq!(read.mode, Some(Mode::Acp));

        // A value stays with its option, whatever it looks like.
        let (out, read) = for_agent(&[
            "--system-prompt",
            "be brief",
            "--resume",
            "abc",
            "fix the tests",
            "--append-system-prompt",
            "--not-an-option",
        ]);
        assert_eq!(read.system_prompt.as_deref(), Some("be brief"));
        assert_eq!(read.resume.as_deref(), Some("abc"));
        assert_eq!(read.append_system_prompt, ["--not-an-option"]);
        assert!(read.messages.is_empty() && !out.contains(&"fix the tests".to_string()));

        // The launch's own mode does not win over the agent's.
        let (_, read) = for_agent(&["--mode", "text", "hello"]);
        assert_eq!(read.mode, Some(Mode::Acp));

        // The internal marker is not an argument a process can be started with.
        let (out, _) = for_agent(&[INTERNAL_RUNTIME_COMMAND_MARKER, "--verbose", "hello"]);
        assert_eq!(out, ["--verbose", "--mode", "acp"]);
    }

    #[test]
    fn unknown_short_option_is_an_error() {
        let parsed = parse(&["-x"]);
        assert_eq!(last_error(&parsed), "Unknown option: -x");
    }

    #[test]
    fn unknown_long_flag_is_an_error() {
        let parsed = parse(&["--bogus-flag"]);
        assert_eq!(last_error(&parsed), "Unknown option: --bogus-flag");
        let parsed = parse(&["--bogus-flag", "value"]);
        assert_eq!(last_error(&parsed), "Unknown option: --bogus-flag");
    }

    #[test]
    fn print_consumes_next_message() {
        let parsed = parse(&["-p", "hello", "world"]);
        assert!(parsed.print);
        assert_eq!(parsed.messages, vec!["hello", "world"]);
    }

    #[test]
    fn resume_accepts_selector_bare_and_equals() {
        assert_eq!(parse(&["-r", "abc"]).resume.as_deref(), Some("abc"));
        assert!(parse(&["--resume", "--verbose"]).resume_bare);
        assert_eq!(parse(&["--resume=abc"]).resume.as_deref(), Some("abc"));
        assert!(parse(&["--resume="]).resume_bare);
        // A selector starting with - or @ stays bare.
        assert!(parse(&["-r", "-x"]).resume_bare);
    }

    #[test]
    fn end_of_options_marker() {
        let parsed = parse(&["--", "-x", "--mode"]);
        assert_eq!(parsed.messages, vec!["-x", "--mode"]);
    }

    #[test]
    fn export_removed_without_internal_marker() {
        let parsed = parse(&["--export", "foo"]);
        assert_eq!(
            last_error(&parsed),
            "--export was removed. Use \"unharnessed session export <file> [output]\"."
        );
        let marker: Vec<String> = vec![
            INTERNAL_RUNTIME_COMMAND_MARKER.into(),
            "--export".into(),
            "foo".into(),
        ];
        let parsed = parse_args(&marker);
        assert_eq!(parsed.export.as_deref(), Some("foo"));
    }

    #[test]
    fn goal_budget_requires_goal() {
        let parsed = parse(&["--goal-token-budget", "5"]);
        assert_eq!(last_error(&parsed), "--goal-token-budget requires --goal");
    }

    #[test]
    fn positive_int_rejects_zero_and_text() {
        let parsed = parse(&["--autonomous-max-turns", "0"]);
        assert_eq!(
            last_error(&parsed),
            "--autonomous-max-turns must be a positive integer between 1 and u32::MAX (4294967295)"
        );
        let parsed = parse(&["--autonomous-max-turns", "abc"]);
        assert_eq!(
            last_error(&parsed),
            "--autonomous-max-turns must be a positive integer between 1 and u32::MAX (4294967295)"
        );
    }

    #[test]
    fn positive_int_rejects_past_the_field_range() {
        // 2**32: the u32 flags reject it out loud instead of silently
        // truncating into the flag's u32 field.
        let parsed = parse(&["--autonomous-max-turns", "4294967296"]);
        assert_eq!(
            last_error(&parsed),
            "--autonomous-max-turns must be a positive integer between 1 and u32::MAX (4294967295)"
        );
        // The u64 flags accept the same value.
        let parsed = parse(&["--autonomous-timeout-ms", "4294967296"]);
        assert_eq!(parsed.autonomous_timeout_ms, Some(4_294_967_296));
    }
}
