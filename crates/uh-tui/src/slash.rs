//! The `/` menu: what a `/` at the start of the message opens over the
//! message box (the extension's `slashCommands` and `SlashMenu`,
//! `vscode/webview/Composer.tsx`). It offers the agent's commands, prompt
//! templates and skills, as its `available_commands_update` lists them
//! (`crates/uh-daemon/src/acp/commands.rs`), then the screen's own
//! (`/new`, `/history`, ...). This file holds how they are read, which of
//! them a message opens, what goes to the agent for a skill, and how the
//! menu is drawn.

use serde_json::Value;

use crate::canvas::Canvas;
use crate::connect::Spot;
use crate::pick::{self, Entry};
use crate::text::{clean, fit, glyphs, width, wrap, Glyph, Ink};
use crate::theme::INK;
use crate::view::{Area, Ui};

/// The most commands the menu shows at once.
pub const ROWS: usize = 12;
/// The widest the menu runs, and the narrowest.
const WIDE_MAX: usize = 88;
const WIDE_MIN: usize = 34;
/// The widest the names' column runs, and the cells between it and what
/// is said of each command.
const NAME_MAX: usize = 32;
const GAP: usize = 3;
/// The narrowest what is said of a command runs beside its name: in a
/// narrower menu, the rows keep to the names.
const ABOUT_LEAST: usize = 16;
/// The most rows that say what a command does, while its text is written.
const ABOUT_ROWS: usize = 3;

/// The agent's commands the menu offers: what each takes after its name,
/// what it does in a line of the menu, and said whole (the agent's own
/// descriptions are terse). Its others (`/autonomous`) run typed.
const TOLD: [(&str, &str, &str, &str); 2] = [
    (
        "compact",
        "instructions",
        "Summarizes the conversation to free context.",
        "Summarizes the conversation so far, to free room in the context. \
         Instructions, if you write some, say what the summary should keep.",
    ),
    (
        "goal",
        "objective",
        "Gives the agent a goal it keeps working on.",
        "Gives the agent a goal it keeps working on, turn after turn, until it is reached. \
         Pause, play or end it from the message box.",
    ),
];
/// The screen's own commands: the name of each, the other names it goes
/// by, and what it does.
const OWN: [(&str, &[&str], &str); 7] = [
    ("new", &[], "Starts a new conversation."),
    (
        "history",
        &["resume"],
        "Opens a saved conversation of this folder.",
    ),
    ("model", &[], "Chooses the model."),
    ("effort", &[], "Sets the reasoning effort."),
    (
        "settings",
        &[],
        "Opens the settings, the MCP servers and the skills.",
    ),
    ("login", &[], "Connects a provider, or removes one."),
    ("quit", &["exit"], "Leaves Unharnessed."),
];
/// What a skill takes after its name.
const TAKES: &str = "instructions";
/// What the menu says while no command matches.
const NONE: &str = "No command, prompt or skill by that name.";
/// What the agent puts before a skill's name.
const SKILL: &str = "skill:";
/// The order the menu lists them in: the screen's own have other ways in.
const ORDER: [Kind; 4] = [Kind::Command, Kind::Prompt, Kind::Skill, Kind::Own];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The agent runs it (`/compact`).
    Command,
    /// A prompt template: its text goes as the message.
    Prompt,
    /// A skill: instructions the agent follows.
    Skill,
    /// The screen's own (`/new`).
    Own,
}

/// One command, typed as `/name` at the start of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub name: String,
    pub kind: Kind,
    /// What to write after the name, as the menu says it.
    pub takes: Option<String>,
    /// What it does, and that said whole where `about` is short for it.
    pub about: String,
    pub whole: Option<String>,
}

impl Command {
    /// The menu lists it: of the agent's commands, the ones it has words for.
    fn offered(&self) -> bool {
        self.kind != Kind::Command || TOLD.iter().any(|told| told.0 == self.name)
    }

    /// The names it is found by: its own, a skill's without the agent's
    /// `skill:`, the other names of one of the screen's.
    fn names(&self) -> Vec<&str> {
        let mut names = vec![self.name.as_str()];
        match self.kind {
            Kind::Skill => names.extend(self.name.strip_prefix(SKILL)),
            Kind::Own => {
                let own = OWN.iter().filter(|own| own.0 == self.name);
                names.extend(own.flat_map(|own| own.1.iter().copied()));
            }
            Kind::Command | Kind::Prompt => {}
        }
        names
    }
}

/// The commands an `available_commands_update` lists: the agent's
/// commands, the prompt templates and the skills, in its order. Its
/// built-in skills are its own business.
#[must_use]
pub fn listed(update: &Value) -> Vec<Command> {
    let entries = update.get("availableCommands").and_then(Value::as_array);
    let commands = entries.into_iter().flatten().filter_map(|entry| {
        let meta = entry.pointer("/_meta/unharnessed");
        let said = |from: Option<&Value>, key: &str| {
            let said = clean(from?.get(key)?.as_str()?);
            let said = said.split_whitespace().collect::<Vec<_>>().join(" ");
            (!said.is_empty()).then_some(said)
        };
        if said(meta, "source").as_deref() == Some("builtin") {
            return None;
        }
        // A name is one word: what follows it is written for the command.
        let name = clean(entry.get("name")?.as_str()?);
        if name.is_empty() || name.contains(char::is_whitespace) {
            return None;
        }
        let kind = match said(meta, "kind").as_deref() {
            Some("prompt") => Kind::Prompt,
            Some("skill") => Kind::Skill,
            _ => Kind::Command,
        };
        let told = (TOLD.iter()).find(|told| kind == Kind::Command && told.0 == name);
        let takes = match kind {
            Kind::Command => told.map(|told| told.1.to_string()),
            // A template says what it takes itself.
            Kind::Prompt => said(entry.get("input"), "hint"),
            Kind::Skill | Kind::Own => Some(TAKES.to_string()),
        };
        let about = (told.map(|told| told.2.to_string()))
            .or_else(|| said(Some(entry), "description"))
            .unwrap_or_else(|| {
                match kind {
                    Kind::Prompt => "A prompt template: its text goes as your message.",
                    Kind::Skill => "A skill: instructions the agent follows for this kind of task.",
                    Kind::Command | Kind::Own => "A command of the agent.",
                }
                .to_string()
            });
        Some(Command {
            name,
            kind,
            takes,
            about,
            whole: told.map(|told| told.3.to_string()),
        })
    });
    commands.collect()
}

/// The agent's commands with the screen's own after them. One of the
/// agent's named like one of the screen's is left out: that name runs the
/// screen's.
#[must_use]
pub fn with_own(agent: &[Command]) -> Vec<Command> {
    let own = |name: &str| (OWN.iter()).any(|own| own.0 == name || own.1.contains(&name));
    let agent = agent.iter().filter(|command| !own(&command.name)).cloned();
    let own = OWN.iter().map(|(name, _, about)| Command {
        name: (*name).to_string(),
        kind: Kind::Own,
        takes: None,
        about: (*about).to_string(),
        whole: None,
    });
    agent.chain(own).collect()
}

/// What a message opens of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// The commands that match what is typed after the `/`, the ones
    /// whose name starts with it first; once a space follows a command's
    /// name, that command alone.
    pub found: Vec<Command>,
    /// How many bytes are written for that command, at the message's end.
    pub written: Option<usize>,
}

/// The menu for the message `text`, among `commands`; `None` when the
/// message opens none: it does not start with a `/`, starts with a path,
/// or names no command before its first space.
#[must_use]
pub fn offer(text: &str, commands: &[Command]) -> Option<Offer> {
    let rest = text.strip_prefix('/')?;
    let Some(end) = rest.find(char::is_whitespace) else {
        // A path (`/tmp/notes.md`), not a command.
        if rest.contains('/') {
            return None;
        }
        let query = rest.to_lowercase();
        // 0 when a name starts with what is typed, 1 when one only has it further in.
        let rank = |command: &Command| {
            let places =
                (command.names().into_iter()).filter_map(|name| name.to_lowercase().find(&query));
            places.map(|at| usize::from(at > 0)).min()
        };
        let mut found = Vec::new();
        for kind in ORDER {
            let mut group: Vec<(usize, &Command)> = (commands.iter())
                .filter(|command| command.kind == kind && command.offered())
                .filter_map(|command| Some((rank(command)?, command)))
                .collect();
            group.sort_by_key(|(rank, _)| *rank);
            found.extend(group.into_iter().map(|(_, command)| command.clone()));
        }
        return Some(Offer {
            found,
            written: None,
        });
    };
    let space = rest[end..].chars().next().map_or(0, char::len_utf8);
    let written = &rest[end + space..];
    let offered: Vec<&Command> = (commands.iter())
        .filter(|command| command.offered())
        .collect();
    let command = named(&rest[..end], &offered)?;
    // The screen's own take nothing: what follows makes it a message.
    if command.kind == Kind::Own && !written.trim().is_empty() {
        return None;
    }
    Some(Offer {
        found: vec![command.clone()],
        written: Some(written.len()),
    })
}

/// The command `name` names among `commands`, whatever its case: by its
/// own name first, a skill's short name may be another's.
fn named<'a>(name: &str, commands: &[&'a Command]) -> Option<&'a Command> {
    let name = name.to_lowercase();
    let whole = (commands.iter()).find(|command| command.name.to_lowercase() == name);
    let other = || {
        commands.iter().find(|command| {
            let mut names = command.names().into_iter();
            names.any(|known| known.to_lowercase() == name)
        })
    };
    whole.or_else(other).copied()
}

/// The message `text` as the agent takes it. The agent knows a command
/// by its exact name: `/name` for the skill `name` goes as `/skill:name`
/// (a command or a prompt of that name keeps it), and a name typed in
/// another case goes as the agent has it.
#[must_use]
pub fn sent(text: &str, commands: &[Command]) -> String {
    let all: Vec<&Command> = commands.iter().collect();
    let known = text.strip_prefix('/').and_then(|rest| {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        let command = named(&rest[..end], &all)?;
        // One of the screen's own with more after it is a message.
        (command.kind != Kind::Own).then(|| format!("/{}{}", command.name, &rest[end..]))
    });
    known.unwrap_or_else(|| text.to_string())
}

/// The menu as it is open over the message box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slash {
    pub found: Vec<Command>,
    /// A command's text is being written (how many bytes of it): its row
    /// stays alone, and Enter sends.
    pub written: Option<usize>,
    /// The one Enter picks, and the first one in view.
    pub at: usize,
    pub top: usize,
}

impl Slash {
    /// How many commands the arrows go through.
    #[must_use]
    pub fn count(&self) -> usize {
        match self.written {
            Some(_) => 0,
            None => self.found.len(),
        }
    }

    /// The arrows: the command Enter picks moves a row up or down, round
    /// the ones found.
    pub fn step(&mut self, down: bool) {
        let count = self.count();
        if count > 0 {
            let by = if down { 1 } else { count - 1 };
            self.at = (self.at.min(count - 1) + by) % count;
            self.top = (self.top)
                .min(self.at)
                .max((self.at + 1).saturating_sub(ROWS));
        }
    }

    /// Whether picking `command` sends it at once, the message being
    /// `text`: one of the screen's own takes nothing more, and a name
    /// typed whole was not waiting for the menu.
    #[must_use]
    pub fn goes(command: &Command, text: &str) -> bool {
        let typed = text.strip_prefix('/').unwrap_or(text).to_lowercase();
        let mut names = command.names().into_iter();
        command.kind == Kind::Own || names.any(|name| name.to_lowercase() == typed)
    }
}

/// A command's name as it is typed, then what it takes.
fn head(command: &Command, lit: bool) -> Vec<Glyph> {
    let ink = if lit { Ink::text() } else { Ink::of(INK, 0.78) };
    let mut head = glyphs(&format!("/{}", command.name), ink, 0);
    if let Some(takes) = &command.takes {
        head.extend(glyphs(&format!(" {takes}"), Ink::faint(), 0));
    }
    head
}

/// The menu, over the last rows of the column `area` gives (its left, its
/// first row, the row it ends before, its width): a command per row, its
/// name, what it takes, and what it does, the one Enter picks on a
/// lighter surface. While a command's text is written, its row alone, and
/// under it what it does, whole. Answers the first row it covers, and
/// where a click picks each command; `None` when the screen has no room
/// for it.
pub fn draw(
    canvas: &mut Canvas,
    slash: &Slash,
    ui: &Ui,
    area: (u16, u16, u16, u16),
) -> Option<(u16, Vec<Area>)> {
    let room = pick::room(area);
    if room == 0 {
        return None;
    }
    // As wide as the widest of all it offers: it keeps its width, and
    // its names their column, while they are searched.
    let all = ui.commands.as_deref().map_or(&[][..], Vec::as_slice);
    let (mut names, mut abouts) = (0, 0);
    for command in all.iter().filter(|command| command.offered()) {
        names = names.max(width(&head(command, false)));
        abouts = abouts.max(width(&glyphs(&command.about, Ink::faint(), 0)));
    }
    let names = names.min(NAME_MAX);
    let most = usize::from(area.3).min(WIDE_MAX);
    let wide = (names + GAP + abouts + 4).clamp(WIDE_MIN.min(most), most);
    let inner = wide.saturating_sub(4);
    if slash.found.is_empty() {
        let none = Entry::of(fit(glyphs(NONE, Ink::muted(), 0), inner));
        return pick::frame(canvas, area, wide, vec![none]);
    }
    if slash.written.is_some() {
        let command = &slash.found[0];
        let mut entries = vec![Entry::of(fit(head(command, true), inner))];
        let whole = command.whole.as_ref().unwrap_or(&command.about);
        let mut about = wrap(glyphs(whole, Ink::muted(), 0), inner);
        // What does not fit ends on an ellipsis.
        if about.len() > ABOUT_ROWS {
            let rest = about.split_off(ABOUT_ROWS - 1);
            let space = glyphs(" ", Ink::muted(), 0);
            about.push(fit(rest.join(&space[..]), inner));
        }
        if room >= 2 + about.len() {
            entries.push(Entry::blank());
            entries.extend(about.into_iter().map(Entry::of));
        }
        return pick::frame(canvas, area, wide, entries);
    }
    let shown = slash.found.len().min(ROWS).min(room);
    let at = slash.at.min(slash.found.len() - 1);
    let from = (slash.top.min(at))
        .max((at + 1).saturating_sub(shown))
        .min(slash.found.len() - shown);
    let beside = inner.saturating_sub(names + GAP);
    let entries = (slash.found.iter().enumerate().skip(from).take(shown))
        .map(|(place, command)| {
            let chosen = place == at;
            let lit = chosen || ui.over == Some(Spot::Pick(place));
            let mut row = fit(head(command, lit), names);
            if beside >= ABOUT_LEAST {
                let pad = (names + GAP).saturating_sub(width(&row));
                let ink = if lit { Ink::muted() } else { Ink::faint() };
                row.extend(glyphs(&" ".repeat(pad), ink, 0));
                row.extend(fit(glyphs(&command.about, ink, 0), beside));
            }
            Entry {
                glyphs: fit(row, inner),
                lit: chosen,
                spots: vec![(0, wide, Spot::Pick(place))],
            }
        })
        .collect();
    pick::frame(canvas, area, wide, entries)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{listed, offer, sent, with_own, Command, Kind, Slash};

    // Listed before the code, how the menu can fail:
    // - a built-in skill is offered, or `/autonomous`; `/compact` or `/goal` is not;
    // - a name with an escape sequence or a line break reaches the screen,
    //   or one with a space in it, which could never be typed;
    // - a `/` further in the message, or after a space, opens the menu;
    // - a path written first (`/tmp/a.md`) opens it on "no command";
    // - what is typed is matched by its case;
    // - a name that only has what is typed further in comes before one
    //   that starts with it, or the kinds get mixed;
    // - a skill is not found by its short name, or one of the screen's by
    //   its other name (`/resume`, `/exit`);
    // - once a space follows the name, a command whose name merely has it
    //   stays, or the whole list;
    // - a line break after the name is not taken for that space, or what
    //   is written after it is counted short when it has line breaks or
    //   wide characters;
    // - `/new and more` keeps the row of `/new`, which takes nothing;
    // - a skill typed by its short name goes to the agent as typed, and
    //   is not expanded; or it goes as the skill although a command or a
    //   prompt has that name; `/skill:tidy` gets the prefix twice; a
    //   message that is no command is changed;
    // - a name typed in another case is shown its command by the menu,
    //   then goes as typed to the agent, which does not know it;
    // - a prompt of the agent's named like one of the screen's is listed
    //   beside it;
    // - Enter on a row sends a command half typed, or does not send one
    //   typed whole.
    fn update() -> serde_json::Value {
        let entry = |name: &str, kind: &str, source: Option<&str>, hint: Option<&str>| {
            let mut entry = json!({
                "name": name,
                "description": format!("About {name}."),
                "_meta": { "unharnessed": { "kind": kind } },
            });
            if let Some(source) = source {
                entry["_meta"]["unharnessed"]["source"] = json!(source);
            }
            if let Some(hint) = hint {
                entry["input"] = json!({ "hint": hint });
            }
            entry
        };
        json!({ "availableCommands": [
            entry("compact", "command", None, Some("[instructions]")),
            entry("goal", "command", None, Some("[objective]")),
            entry("autonomous", "command", None, Some("[status|off|on]")),
            entry("review", "prompt", None, Some("[what to review]")),
            entry("regoal", "prompt", None, None),
            entry("new", "prompt", None, None),
            entry("two words", "prompt", None, None),
            entry("bad\u{1b}[31mname\nhere", "prompt", None, None),
            entry("skill:goal", "skill", Some("builtin"), None),
            entry("skill:tidy", "skill", Some("user"), None),
            entry("skill:review", "skill", Some("project"), None),
            entry("skill:Loud", "skill", Some("user"), None),
        ]})
    }

    fn commands() -> Vec<Command> {
        with_own(&listed(&update()))
    }

    fn names(text: &str) -> Option<Vec<String>> {
        let offer = offer(text, &commands())?;
        Some(
            offer
                .found
                .into_iter()
                .map(|command| command.name)
                .collect(),
        )
    }

    #[test]
    fn the_menu_lists_what_the_agent_offers_then_the_screens_own() {
        let all = names("/").unwrap();
        assert_eq!(
            all,
            [
                "compact",
                "goal",
                "review",
                "regoal",
                "skill:tidy",
                "skill:review",
                "skill:Loud",
                "new",
                "history",
                "model",
                "effort",
                "settings",
                "login",
                "quit"
            ]
        );
        let commands = commands();
        let prompts = (commands.iter()).filter(|command| command.kind == Kind::Prompt);
        assert_eq!(prompts.count(), 2, "{commands:?}");
        let compact = commands.iter().find(|command| command.name == "compact");
        assert_eq!(compact.unwrap().takes.as_deref(), Some("instructions"));
        let review = commands.iter().find(|command| command.name == "review");
        assert_eq!(review.unwrap().takes.as_deref(), Some("[what to review]"));
        assert_eq!(review.unwrap().about, "About review.");
    }

    #[test]
    fn only_a_slash_that_starts_the_message_opens_it() {
        for text in [
            "",
            "go",
            " /go",
            "a /goal",
            "see\n/goal",
            "/tmp/a.md",
            "/ goal",
        ] {
            assert_eq!(names(text), None, "{text:?}");
        }
        assert_eq!(names("/zzz"), Some(Vec::new()));
    }

    #[test]
    fn what_is_typed_finds_the_names_that_start_with_it_first() {
        assert_eq!(names("/GO").unwrap(), ["goal", "regoal"]);
        assert_eq!(
            names("/re").unwrap(),
            ["review", "regoal", "skill:review", "history"]
        );
        assert_eq!(names("/ti").unwrap(), ["skill:tidy", "settings"]);
        assert_eq!(names("/lou").unwrap(), ["skill:Loud"]);
        assert_eq!(names("/ex").unwrap(), ["quit"]);
        assert_eq!(names("/auto").unwrap(), Vec::<String>::new());
    }

    #[test]
    fn a_space_after_a_name_keeps_that_command_alone() {
        let commands = commands();
        let alone = |text: &str| {
            let offer = offer(text, &commands)?;
            assert_eq!(offer.found.len(), 1, "{text:?}");
            Some((offer.found[0].name.clone(), offer.written.unwrap()))
        };
        assert_eq!(alone("/goal "), Some(("goal".to_string(), 0)));
        assert_eq!(alone("/Goal ship it"), Some(("goal".to_string(), 7)));
        assert_eq!(alone("/goal\nline\n日本"), Some(("goal".to_string(), 11)));
        assert_eq!(alone("/tidy the docs"), Some(("skill:tidy".to_string(), 8)));
        assert_eq!(alone("/skill:tidy x"), Some(("skill:tidy".to_string(), 1)));
        // The prompt of that name, not the skill.
        assert_eq!(alone("/review x"), Some(("review".to_string(), 1)));
        assert_eq!(alone("/resume "), Some(("history".to_string(), 0)));
        assert_eq!(alone("/new "), Some(("new".to_string(), 0)));
        for text in [
            "/new and more",
            "/go ",
            "/autonomous on",
            "/zzz x",
            "/goa l",
        ] {
            assert_eq!(alone(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_command_goes_to_the_agent_under_the_name_it_knows() {
        let commands = commands();
        let goes = |text: &str| sent(text, &commands);
        assert_eq!(goes("/tidy the docs"), "/skill:tidy the docs");
        assert_eq!(goes("/tidy"), "/skill:tidy");
        assert_eq!(goes("/tidy\nthe docs"), "/skill:tidy\nthe docs");
        assert_eq!(goes("/Tidy up"), "/skill:tidy up");
        assert_eq!(goes("/loud"), "/skill:Loud");
        assert_eq!(goes("/GOAL  ship it "), "/goal  ship it ");
        assert_eq!(goes("/Autonomous on"), "/autonomous on");
        for text in [
            "/skill:tidy the docs",
            "/review the diff",
            "/goal tidy",
            "/new tidy",
            "/NEW tidy",
            "/missing x",
            "tidy /tidy",
            "/ tidy",
            "/tmp/tidy",
            "/",
            "",
        ] {
            assert_eq!(goes(text), text);
        }
    }

    #[test]
    fn enter_sends_one_of_the_screens_own_and_a_name_typed_whole() {
        let commands = commands();
        let goes = |name: &str, text: &str| {
            let command = commands.iter().find(|command| command.name == name);
            Slash::goes(command.unwrap(), text)
        };
        assert!(goes("quit", "/q"));
        assert!(goes("history", "/resume"));
        assert!(goes("compact", "/compact"));
        assert!(goes("compact", "/COMPACT"));
        assert!(goes("skill:tidy", "/tidy"));
        assert!(goes("skill:tidy", "/skill:tidy"));
        assert!(!goes("compact", "/comp"));
        assert!(!goes("compact", "/"));
        assert!(!goes("skill:tidy", "/skill:"));
    }
}
