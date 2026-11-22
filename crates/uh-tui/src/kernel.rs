//! The kernel's panel: the commands the agent left serving in the
//! background, with their last lines, and the variables of its REPL
//! (`_unharnessed/kernel/*`, crates/uh-daemon/src/acp/kernel.rs). The rows
//! follow the extension's (`vscode/webview/Kernel.tsx`): only a command that
//! listens on a port is listed, while it runs; one that ends well lights up
//! and goes, any other goes at once; a variable lights up when it is made
//! or bound to another object, and opens on the start of its value
//! (`_unharnessed/kernel/preview`). A command stopped from the panel
//! (`_unharnessed/kernel/kill`) shows stopped at once.

use serde_json::Value;

use crate::fold::{Fold, Folds};
use crate::text::clean;

/// A command's lines the panel shows.
pub const TAIL_LINES: usize = 7;
/// An ended command stays this long, then goes; a stopped one, less.
const OK_STAYS: f64 = 2.8;
const KILLED_STAYS: f64 = 0.9;
/// How long a row takes to leave.
pub const LEAVE: f64 = 0.46;
/// How often an open command's last lines are asked for again, and a value
/// the kernel was too busy to give.
const TAIL_EVERY: f64 = 1.0;
const PREVIEW_AGAIN: f64 = 0.6;
/// What a stopped command's lines end on.
const INTERRUPT: &str = "^C";

/// How a command stands on the list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Running,
    Ok,
    Killed,
    Ended,
}

#[derive(Clone, Debug)]
pub struct TailLine {
    pub text: String,
    /// When it came, if the tail had lines before it: it comes in.
    pub fresh: Option<f64>,
}

#[derive(Clone, Debug)]
pub struct Command {
    /// Its own number on the panel, for as long as it is listed.
    pub key: usize,
    pub id: String,
    /// The command as the agent wrote it.
    pub line: String,
    pub phase: Phase,
    pub ports: Vec<u64>,
    /// How long it ran, once ended.
    pub duration: Option<f64>,
    /// When the panel first listed it, and when the agent first told of it.
    pub shown: f64,
    pub started: f64,
    /// When it ended, on the list.
    pub ended: Option<f64>,
    pub tail: Vec<TailLine>,
    /// When its last lines were last asked for.
    asked: Option<f64>,
}

impl Command {
    /// The interrupt closes its lines.
    fn interrupt(&mut self, now: f64) {
        self.tail.push(TailLine {
            text: INTERRUPT.to_string(),
            fresh: Some(now),
        });
        let extra = self.tail.len().saturating_sub(TAIL_LINES);
        self.tail.drain(..extra);
    }

    /// When the row starts to leave, once the command has ended.
    #[must_use]
    pub fn leaves(&self) -> Option<f64> {
        let stays = match self.phase {
            Phase::Running => return None,
            Phase::Ok => OK_STAYS,
            Phase::Killed => KILLED_STAYS,
            Phase::Ended => 0.0,
        };
        self.ended.map(|ended| ended + stays)
    }
}

/// The start of a variable's value, as the kernel formats it.
#[derive(Clone, Debug)]
pub struct Preview {
    pub text: String,
    /// It keeps its columns (a table, a repr over lines); else it wraps.
    pub code: bool,
    /// The value goes on after it.
    pub truncated: bool,
}

#[derive(Clone, Debug)]
pub struct Variable {
    /// Its own number on the panel, for as long as it is listed.
    pub key: usize,
    pub name: String,
    pub kind: String,
    rev: String,
    /// Its value when short, a function's signature, or a size.
    pub meta: String,
    /// The start of its value, once the kernel gave it; else why it could not.
    pub preview: Option<Result<Preview, String>>,
    /// When the panel first listed it.
    pub shown: f64,
    /// When it was made or last changed: a light runs through its name.
    pub lit: Option<f64>,
    /// When the kernel dropped it: the row leaves.
    pub left: Option<f64>,
    /// When its value was last asked for, and whether the answer is still to come.
    asked: Option<f64>,
    waits: bool,
    /// The object the value shown is of: asked again once it is another.
    of: Option<String>,
}

#[derive(Debug, Default)]
pub struct Kernel {
    pub commands: Vec<Command>,
    pub variables: Vec<Variable>,
    /// When the panel first had something to show.
    pub opened: Option<f64>,
    /// When each command was first on the agent's list: its time counts from there.
    seen: std::collections::HashMap<String, f64>,
    /// How many rows the panel has listed: the next one's number.
    listed: usize,
}

impl Kernel {
    /// Nothing to show: the panel is not drawn.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty() && self.variables.is_empty()
    }

    /// `_unharnessed/kernel/changed`: a list, again, each time it changes.
    pub fn changed(&mut self, now: f64, params: &Value) {
        if let Some(commands) = params.get("commands").and_then(Value::as_array) {
            self.set_commands(now, commands);
        }
        if let Some(variables) = params.get("variables").and_then(Value::as_array) {
            self.set_variables(now, variables);
        }
        if self.opened.is_none() && !self.is_empty() {
            self.opened = Some(now);
        }
    }

    fn set_commands(&mut self, now: f64, list: &[Value]) {
        for command in list {
            let text = |key: &str| command.get(key).and_then(Value::as_str).unwrap_or_default();
            let id = text("id");
            let started = *self.seen.entry(id.to_string()).or_insert(now);
            let running = text("status") == "running";
            let killed = command
                .get("killed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let ports: Vec<u64> = command
                .get("ports")
                .and_then(Value::as_array)
                .map(|ports| ports.iter().filter_map(Value::as_u64).collect())
                .unwrap_or_default();
            // One stopped from the panel shows so before the agent says it ended.
            let stopping = (self.commands.iter())
                .any(|row| row.id == id && row.phase == Phase::Killed && row.ended.is_none());
            let phase = if killed || stopping {
                Phase::Killed
            } else if running {
                Phase::Running
            } else if command.get("exitCode").and_then(Value::as_i64) == Some(0) {
                Phase::Ok
            } else {
                Phase::Ended
            };
            let Some(row) = self.commands.iter_mut().find(|row| row.id == id) else {
                // Ended before the panel saw it serve, or not serving (yet): a
                // build or a test run ends on its own.
                if running && !ports.is_empty() {
                    self.listed += 1;
                    self.commands.push(Command {
                        key: self.listed,
                        id: id.to_string(),
                        line: clean(text("command")),
                        phase,
                        ports,
                        duration: None,
                        shown: now,
                        started,
                        ended: None,
                        tail: Vec::new(),
                        asked: None,
                    });
                }
                continue;
            };
            if row.phase == Phase::Running && phase == Phase::Killed {
                row.interrupt(now);
            }
            if row.ended.is_none() && !running {
                row.ended = Some(now);
            }
            row.phase = phase;
            row.ports = ports;
            row.duration = command
                .get("durationMs")
                .and_then(Value::as_f64)
                .map(|ms| ms / 1000.0);
        }
        // The ones the agent dropped leave.
        for row in &mut self.commands {
            let listed = list
                .iter()
                .any(|command| command.get("id").and_then(Value::as_str) == Some(&row.id));
            if !listed && row.ended.is_none() {
                row.phase = Phase::Ended;
                row.ended = Some(now);
            }
        }
    }

    fn set_variables(&mut self, now: f64, list: &[Value]) {
        let mut listed = Vec::with_capacity(list.len());
        for variable in list {
            let text = |key: &str| {
                variable
                    .get(key)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
            };
            let name = clean(text("name"));
            let meta = meta_of(variable);
            if let Some(row) = self
                .variables
                .iter_mut()
                .find(|row| row.name == name && row.left.is_none())
            {
                if row.rev != text("rev") || row.meta != meta {
                    row.lit = Some(now);
                }
                row.kind = clean(text("type"));
                row.rev = text("rev").to_string();
                row.meta = meta;
            } else {
                self.listed += 1;
                self.variables.push(Variable {
                    key: self.listed,
                    name: name.clone(),
                    kind: clean(text("type")),
                    rev: text("rev").to_string(),
                    meta,
                    preview: None,
                    shown: now,
                    lit: Some(now),
                    left: None,
                    asked: None,
                    waits: false,
                    of: None,
                });
            }
            listed.push(name);
        }
        // The ones the kernel dropped leave.
        for row in &mut self.variables {
            if row.left.is_none() && !listed.contains(&row.name) {
                row.left = Some(now);
            }
        }
    }

    /// The panel asked the agent to stop command `id`
    /// (`_unharnessed/kernel/kill`): it shows stopped at once; the agent's
    /// list then says it ended.
    pub fn stop(&mut self, now: f64, id: &str) {
        let running = |row: &&mut Command| row.id == id && row.phase == Phase::Running;
        if let Some(row) = self.commands.iter_mut().find(running) {
            row.phase = Phase::Killed;
            row.interrupt(now);
        }
    }

    /// The agent could not stop command `id`: it runs on, and shows so.
    pub fn unstopped(&mut self, id: &str) {
        let stopping =
            |row: &&mut Command| row.id == id && row.phase == Phase::Killed && row.ended.is_none();
        if let Some(row) = self.commands.iter_mut().find(stopping) {
            row.phase = Phase::Running;
            if row.tail.last().is_some_and(|line| line.text == INTERRUPT) {
                row.tail.pop();
            }
        }
    }

    /// `_unharnessed/kernel/preview` answered for the variable `name`: the
    /// start of its value, or why the kernel could not show it. While a cell
    /// runs the kernel reads no value (`busy`): what was shown stays.
    pub fn previewed(&mut self, name: &str, preview: Result<Preview, String>) {
        let Some(row) = self
            .variables
            .iter_mut()
            .find(|row| row.name == name && row.left.is_none())
        else {
            return;
        };
        row.waits = false;
        let busy = matches!(&preview, Err(why) if why == "busy");
        if !busy {
            row.of = Some(row.rev.clone());
        }
        if busy && row.preview.is_some() {
            return;
        }
        row.preview = Some(preview);
    }

    /// What the panel shows and does not have: the names of the open
    /// variables whose value is missing or of an object since replaced, and
    /// the ids of the open running commands whose last lines are due. They
    /// count as asked from `now`.
    pub fn wanted(&mut self, now: f64, open: &Folds) -> (Vec<String>, Vec<String>) {
        let due = |asked: Option<f64>, every: f64| asked.is_none_or(|asked| now - asked >= every);
        let mut names = Vec::new();
        for row in &mut self.variables {
            let shows = row.left.is_none() && open.open(Fold::Variable(row.key));
            if shows
                && !row.waits
                && row.of.as_ref() != Some(&row.rev)
                && due(row.asked, PREVIEW_AGAIN)
            {
                row.asked = Some(now);
                row.waits = true;
                names.push(row.name.clone());
            }
        }
        let mut ids = Vec::new();
        for row in &mut self.commands {
            let shows = row.phase == Phase::Running && open.open(Fold::Command(row.key));
            if shows && due(row.asked, TAIL_EVERY) {
                row.asked = Some(now);
                ids.push(row.id.clone());
            }
        }
        (names, ids)
    }

    /// `_unharnessed/kernel/tail` answered: command `id`'s last lines. The
    /// new tail is the old one moved up; the lines that came since come in.
    pub fn tail(&mut self, now: f64, id: &str, tail: &str) {
        // A stopped command's lines are closed: what was asked before stays out.
        let listed = |row: &&mut Command| row.id == id && row.phase != Phase::Killed;
        let Some(row) = self.commands.iter_mut().find(listed) else {
            return;
        };
        let text = clean(tail);
        if text.is_empty() {
            row.tail.clear();
            return;
        }
        let lines: Vec<&str> = text.split('\n').collect();
        let lines = &lines[lines.len().saturating_sub(TAIL_LINES)..];
        let before = &row.tail;
        let mut shift = 0;
        while shift < before.len()
            && !before[shift..]
                .iter()
                .enumerate()
                .all(|(at, line)| lines.get(at) == Some(&line.text.as_str()))
        {
            shift += 1;
        }
        let kept = before.len() - shift;
        let had = !before.is_empty();
        row.tail = lines
            .iter()
            .enumerate()
            .map(|(at, line)| TailLine {
                text: (*line).to_string(),
                fresh: if at < kept {
                    before[shift + at].fresh
                } else {
                    had.then_some(now)
                },
            })
            .collect();
    }

    /// The kernel went with its agent: every row leaves.
    pub fn gone(&mut self, now: f64) {
        for row in &mut self.commands {
            if row.ended.is_none() {
                row.phase = Phase::Ended;
                row.ended = Some(now);
            }
        }
        for row in &mut self.variables {
            row.left.get_or_insert(now);
        }
    }

    /// The rows that finished leaving are dropped.
    pub fn tick(&mut self, now: f64) {
        self.commands
            .retain(|row| row.leaves().is_none_or(|leaves| now < leaves + LEAVE));
        self.variables
            .retain(|row| row.left.is_none_or(|left| now < left + LEAVE));
    }
}

/// What a variable holds, in a few characters: its value when short, else
/// its signature or size.
fn meta_of(variable: &Value) -> String {
    if let Some(value) = variable.get("value").and_then(Value::as_str) {
        return clean(value);
    }
    if let Some(signature) = variable.get("signature").and_then(Value::as_str) {
        return clean(signature);
    }
    if let Some(shape) = variable.get("shape").and_then(Value::as_array) {
        let shape: Vec<String> = shape
            .iter()
            .filter_map(Value::as_u64)
            .map(grouped)
            .collect();
        let shape = shape.join(" × ");
        return match variable.get("dtype").and_then(Value::as_str) {
            Some(dtype) => format!("{shape} {}", clean(dtype)),
            None => shape,
        };
    }
    let unit = variable.get("unit").and_then(Value::as_str);
    if let (Some(len), Some(unit)) = (variable.get("len").and_then(Value::as_u64), unit) {
        let unit = if len == 1 {
            unit.trim_end_matches('s')
        } else {
            unit
        };
        return format!("{} {unit}", grouped(len));
    }
    String::new()
}

/// `1,204,000`.
#[must_use]
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
