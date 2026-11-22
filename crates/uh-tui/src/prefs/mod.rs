//! The settings' screen: the agent's global settings, its MCP servers, its
//! skills and its instructions per model, a tab each, in the
//! conversation's place (the extension's `vscode/webview/Settings.tsx`,
//! `SettingsMcp.tsx`, `SettingsSkills.tsx`, `SettingsInstructions.tsx`). A
//! change is written at once; the ones the agent reads when it starts
//! wait for a restart, which the screen offers. This file holds what the
//! user set on the screen and what a key or a click on it asks for;
//! `draw` shows it; what the agent says of its settings is in `settings`.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};

use crate::chat::Chat;
use crate::connect::{Look, Spot};
use crate::desk;
use crate::draft::Draft;
use crate::models::{maker, matching, Model};
use crate::settings::{
    Server, Skill, CREATE, FOLDER, INSTRUCTIONS, MCP_LOGOUT, MCP_SET_TOKEN, SERVERS, SET,
    SKILLS_ADD_FOLDER, SKILLS_CREATE, SKILLS_REMOVE_FOLDER, SKILLS_SET,
};
use crate::text::Ink;

mod draw;
pub mod general;
pub mod groups;
pub mod servers;

pub use draw::draw;
use general::{Control, Unit};
use servers::Written;

/// From this many skills on, a field filters them (the extension's).
const SEARCH_FROM: usize = 9;
/// The most rows a list of models shows in a form.
pub const LIST_ROWS: usize = 8;
/// The rows a text of several lines shows.
const AREA_ROWS: usize = 5;

/// The fields of a server's form, and its choices, by their place.
const NAME: usize = 0;
const URL: usize = 1;
const VARIABLE: usize = 2;
const LINE: usize = 3;
const PASSED: usize = 4;
const KIND: usize = 0;
const AUTH: usize = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    General,
    Mcp,
    Skills,
    Instructions,
}

pub const TABS: [(Tab, &str); 4] = [
    (Tab::General, "General"),
    (Tab::Mcp, "MCP"),
    (Tab::Skills, "Skills"),
    (Tab::Instructions, "Instructions"),
];

/// A row of the screen that the arrows choose and Enter acts on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// What restarts the agent, while a change waits for it.
    Restart,
    /// A setting of the General tab, by its key.
    Setting(&'static str),
    /// One of the user's servers, by its id; what adds one; a service of
    /// the Marketplace.
    Server(String),
    AddServer,
    Market(String),
    /// A skill, by its file; what makes one; a folder skills are read
    /// from; what adds one.
    Skill(String),
    NewSkill,
    Folder(String),
    AddFolder,
    /// A group of instructions, by its place in the file; what makes one.
    Group(usize),
    NewGroup,
}

/// What a button of a form does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Press {
    /// What the form is for: it is saved, added, created.
    Main,
    Cancel,
    /// The question before a removal comes; then the removal.
    Remove,
    Confirm,
    SignIn,
    SignOut,
}

/// What the screen asks of the session.
#[derive(Clone, Debug, PartialEq)]
pub enum Wish {
    /// A request to the agent.
    Ask(&'static str, Value),
    /// The sign-in to an MCP server, by its id: its screen takes over.
    SignIn(String),
    /// The agent restarts on the conversation.
    Restart,
    /// The conversation comes back.
    Leave,
}

/// A list of models in a form: what they are searched by, the one the
/// arrows chose among those shown, and the first one in view.
#[derive(Clone, Default)]
pub struct List {
    pub query: Draft,
    pub at: usize,
    pub top: usize,
}

impl List {
    fn step(&mut self, by: isize, count: usize) -> bool {
        let Some(to) = self.at.checked_add_signed(by).filter(|to| *to < count) else {
            return false;
        };
        self.at = to;
        self.reveal();
        true
    }

    fn reveal(&mut self) {
        self.top = (self.top)
            .min(self.at)
            .max((self.at + 1).saturating_sub(LIST_ROWS));
    }
}

/// A row of a form's list: the model it stands for (`None`: none at
/// all), how it reads, and whether it has the check.
pub struct Entry {
    pub key: Option<String>,
    pub name: String,
    pub maker: String,
    pub marked: bool,
}

/// The form a row is open on.
#[derive(Clone)]
pub struct Sheet {
    pub item: Item,
    /// What is written in its fields, each by its place.
    pub fields: Vec<Draft>,
    /// The option each of its choices is on.
    pub choices: Vec<usize>,
    pub list: List,
    /// The models checked in its list.
    pub checked: Vec<String>,
    /// Where what is typed goes, among its stops.
    pub focus: usize,
    /// It asks once more before it removes.
    pub asking: bool,
    /// The change it asked for, until the agent answers: it closes on a yes.
    pub sent: Option<String>,
    /// Why what is written cannot be sent.
    pub problem: Option<String>,
}

/// A part of a form, top to bottom.
pub enum Piece {
    Say(String, Ink),
    /// A field: its place among the form's, what it is, what it asks for
    /// while empty, whether it hides what is written, and its rows.
    Field {
        at: usize,
        label: &'static str,
        asks: String,
        secret: bool,
        rows: usize,
    },
    /// A choice of a few: its place among the form's, and its options.
    Choice {
        at: usize,
        label: &'static str,
        options: &'static [&'static str],
    },
    /// The form's list of models.
    List {
        label: &'static str,
    },
    /// Buttons side by side: what each does, its word, its look, whether it can be pressed.
    Buttons(Vec<(Press, String, Look, bool)>),
}

/// Where the keys go in a form, in the order Tab goes through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// A field, and its rows.
    Field(usize, usize),
    /// A choice, and how many options it has.
    Choice(usize, usize),
    List,
    Button(Press, bool),
}

/// The settings' screen as the user left it.
#[derive(Clone)]
pub struct Prefs {
    pub tab: Tab,
    /// The row the arrows chose.
    pub chosen: Option<Item>,
    pub sheet: Option<Sheet>,
    /// What the tab's rows are searched by.
    pub query: Draft,
    /// The file of the skill made from here.
    pub made: Option<String>,
    /// How wide a form's text runs, as last drawn: Up and Down move a caret by its rows.
    wide: Cell<usize>,
    /// The first row in view, and whether the chosen row is to be brought into it.
    top: Cell<usize>,
    reveal: Cell<bool>,
}

impl Prefs {
    #[must_use]
    pub fn on(tab: Tab) -> Prefs {
        Prefs {
            tab,
            chosen: None,
            sheet: None,
            query: Draft::default(),
            made: None,
            wide: Cell::new(40),
            top: Cell::new(0),
            reveal: Cell::new(false),
        }
    }

    /// The screen's first row in view moves to `to`.
    pub fn show(&mut self, to: usize) {
        self.top.set(to);
    }

    /// Another tab: nothing set on the one before stays.
    fn turn(&mut self, tab: Tab) {
        if tab != self.tab {
            *self = Prefs::on(tab);
        }
    }

    /// The agent restarts: a form open on what it said is no longer one.
    pub fn restarted(&mut self) {
        self.sheet = None;
    }
}

/// The `mcpServers` setting as the screen shows it: the file's, with the
/// change being written.
fn config(chat: &Chat) -> &Value {
    chat.settings.value(SERVERS)
}

/// The servers of the MCP tab: the user's, by id (the file's, one being
/// added before the list has it, and the ones a workspace's settings add);
/// the catalog's services signed in to; and the Marketplace, the others,
/// A to Z, those `query` finds.
pub struct Listed<'a> {
    pub yours: Vec<String>,
    pub services: Vec<&'a Server>,
    pub market: Vec<&'a Server>,
    /// How many services the Marketplace has to add, searched or not;
    /// `None` when the catalog could not be read.
    pub offered: Option<usize>,
}

#[must_use]
pub fn listed<'a>(chat: &'a Chat, query: &str) -> Listed<'a> {
    let servers = chat.settings.servers.as_deref().unwrap_or_default();
    let config = config(chat).as_object();
    let saved = (chat.settings.view.as_ref())
        .and_then(|view| view.values.get(SERVERS))
        .and_then(Value::as_object);
    let has = |map: Option<&serde_json::Map<String, Value>>, id: &str| {
        map.is_some_and(|map| map.contains_key(id))
    };
    let mut yours: Vec<String> = config
        .into_iter()
        .flat_map(|map| map.keys().cloned())
        .collect();
    yours.extend(
        (servers.iter())
            .filter(|server| server.user && !has(config, &server.id) && !has(saved, &server.id))
            .map(|server| server.id.clone()),
    );
    let catalog = || servers.iter().filter(|server| !server.user);
    let words = query.trim().to_lowercase();
    let found = |server: &&Server| {
        words.is_empty()
            || (std::iter::once(&server.label)
                .chain([&server.id])
                .chain(&server.found_by))
            .any(|text| text.to_lowercase().contains(&words))
    };
    let mut market: Vec<&Server> = catalog().filter(|server| !server.signed_in).collect();
    let offered = (catalog().next().is_some()).then_some(market.len());
    market.retain(found);
    market.sort_by_key(|server| server.label.to_lowercase());
    Listed {
        yours,
        services: catalog().filter(|server| server.signed_in).collect(),
        market,
        offered,
    }
}

/// The skills the Skills tab shows under `label`: those `query` finds, of
/// the group.
#[must_use]
pub fn skills<'a>(chat: &'a Chat, query: &str, label: &str) -> Vec<&'a Skill> {
    let query = query.to_lowercase();
    let words: Vec<&str> = query.split_whitespace().collect();
    let all = chat.settings.skills.as_ref().map(|skills| &skills.list);
    (all.into_iter().flatten())
        .filter(|skill| match label {
            "Yours" => matches!(skill.source.as_str(), "user" | "folder"),
            "This workspace" => skill.source == "project",
            _ => skill.source == "builtin",
        })
        .filter(|skill| {
            let all = format!("{} {}", skill.name, skill.description).to_lowercase();
            words.iter().all(|word| all.contains(word))
        })
        .collect()
}

pub const SKILL_GROUPS: [&str; 3] = ["Yours", "This workspace", "Built in"];

/// The models a group of instructions can name: those of the connected providers.
#[must_use]
pub fn connected(chat: &Chat) -> Vec<&Model> {
    let providers = chat.providers.list.as_deref().unwrap_or_default();
    groups::connected(&chat.models, providers)
}

/// The groups of instructions the screen shows: each by its place in the
/// file, with those of its models that are connected; one left with none
/// stays out of sight.
#[must_use]
pub fn shown_groups(chat: &Chat) -> Vec<(usize, Vec<&Model>, String)> {
    let connected = connected(chat);
    (groups::saved(chat.settings.value(INSTRUCTIONS))
        .into_iter()
        .enumerate())
    .filter_map(|(at, group)| {
        let models: Vec<&Model> = (group.models.iter())
            .filter_map(|key| connected.iter().find(|model| groups::key(model) == *key))
            .copied()
            .collect();
        (!models.is_empty()).then_some((at, models, group.text))
    })
    .collect()
}

impl Prefs {
    /// The tab has a field that searches its rows: what is typed goes there.
    #[must_use]
    pub fn searches(&self, chat: &Chat) -> bool {
        let typed = !self.query.is_empty();
        match self.tab {
            Tab::Mcp => listed(chat, "").offered.is_some(),
            Tab::Skills => {
                let count = (chat.settings.skills.as_ref()).map_or(0, |skills| skills.list.len());
                count >= SEARCH_FROM || typed
            }
            Tab::General | Tab::Instructions => false,
        }
    }

    /// The tab's rows, in the order the arrows go through them.
    #[must_use]
    pub fn items(&self, chat: &Chat) -> Vec<Item> {
        let settings = &chat.settings;
        let mut items = Vec::new();
        if !settings.pending().is_empty() {
            items.push(Item::Restart);
        }
        let query = self.query.text();
        match self.tab {
            Tab::General if settings.view.is_some() => {
                let fields = general::GROUPS.iter().flat_map(|group| group.fields);
                items.extend(fields.map(|field| Item::Setting(field.key)));
            }
            Tab::Mcp if settings.servers.is_some() => {
                let listed = listed(chat, query);
                items.extend(listed.yours.into_iter().map(Item::Server));
                items
                    .extend((listed.services.iter()).map(|server| Item::Server(server.id.clone())));
                items.push(Item::AddServer);
                items.extend((listed.market.iter()).map(|server| Item::Market(server.id.clone())));
            }
            Tab::Skills if settings.skills.is_some() => {
                items.push(Item::NewSkill);
                for label in SKILL_GROUPS {
                    let skills = skills(chat, query, label);
                    let own = skills.iter().filter(|skill| skill.source != "builtin");
                    items.extend(own.map(|skill| Item::Skill(skill.path.clone())));
                }
                let folders = settings.skills.iter().flat_map(|skills| &skills.folders);
                items.extend(folders.map(|folder| Item::Folder(folder.clone())));
                items.push(Item::AddFolder);
            }
            Tab::Instructions if settings.view.is_some() && !chat.models.is_empty() => {
                items.extend((shown_groups(chat).iter()).map(|(at, ..)| Item::Group(*at)));
                items.push(Item::NewGroup);
            }
            _ => {}
        }
        items
    }

    /// The first row the search found, while something is searched: a
    /// service of the Marketplace, a skill.
    fn found(&self, chat: &Chat) -> Option<Item> {
        if self.query.text().trim().is_empty() {
            return None;
        }
        let searched = |item: &Item| matches!(item, Item::Market(_) | Item::Skill(_));
        self.items(chat).into_iter().find(searched)
    }

    /// The arrows: the chosen row moves `by` rows through the tab's.
    fn step(&mut self, by: isize, chat: &Chat) {
        let items = self.items(chat);
        let at =
            (self.chosen.as_ref()).and_then(|chosen| items.iter().position(|item| item == chosen));
        let next = match at {
            Some(at) => at
                .saturating_add_signed(by)
                .min(items.len().saturating_sub(1)),
            None if by > 0 => 0,
            None => items.len().saturating_sub(1),
        };
        self.chosen = items.get(next).cloned();
        self.reveal.set(true);
    }

    /// The screen follows what the agent said: a form closes once what it
    /// asked for went through, and stays open on why it did not.
    pub fn follow(&mut self, chat: &Chat) {
        let settings = &chat.settings;
        // A form open under a row that left the list (a service signed out
        // of, a server removed elsewhere) has no row to be under.
        let open = self.sheet.as_ref().map(|sheet| &sheet.item);
        if open.is_some_and(|item| !self.items(chat).contains(item)) {
            self.sheet = None;
        }
        let Some(key) = self.sheet.as_ref().and_then(|sheet| sheet.sent.clone()) else {
            return;
        };
        if settings.busy(&key) {
            return;
        }
        if settings.errors.contains_key(&key) {
            if let Some(sheet) = &mut self.sheet {
                sheet.sent = None;
            }
            return;
        }
        self.sheet = None;
        if key == CREATE {
            // The new skill is the chosen row, and the screen says where its file is.
            self.made.clone_from(&settings.created);
            self.chosen = settings.created.clone().map(Item::Skill);
            self.reveal.set(true);
        }
    }
}

impl Sheet {
    fn new(item: Item) -> Sheet {
        Sheet {
            item,
            fields: Vec::new(),
            choices: Vec::new(),
            list: List::default(),
            checked: Vec::new(),
            focus: 0,
            asking: false,
            sent: None,
            problem: None,
        }
    }

    fn text(&self, at: usize) -> &str {
        self.fields.get(at).map_or("", Draft::text)
    }

    fn choice(&self, at: usize) -> usize {
        self.choices.get(at).copied().unwrap_or(0)
    }

    /// What a server's form holds.
    fn written(&self) -> Written {
        Written {
            name: self.text(NAME).trim().to_string(),
            command: self.choice(KIND) == 1,
            url: self.text(URL).to_string(),
            auth: self.choice(AUTH),
            variable: self.text(VARIABLE).to_string(),
            line: self.text(LINE).to_string(),
            passed: self.text(PASSED).to_string(),
        }
    }

    /// A server's form, on what the file holds of it.
    fn server(item: Item, written: &Written) -> Sheet {
        let fields = [
            &written.name,
            &written.url,
            &written.variable,
            &written.line,
            &written.passed,
        ];
        Sheet {
            fields: fields
                .into_iter()
                .map(|text| Draft::holding(text))
                .collect(),
            choices: vec![usize::from(written.command), written.auth],
            ..Sheet::new(item)
        }
    }

    /// The form's stops, as its pieces give them.
    #[must_use]
    pub fn stops(pieces: &[Piece]) -> Vec<Stop> {
        let mut stops = Vec::new();
        for piece in pieces {
            match piece {
                Piece::Say(..) => {}
                Piece::Field { at, rows, .. } => stops.push(Stop::Field(*at, *rows)),
                Piece::Choice { at, options, .. } => stops.push(Stop::Choice(*at, options.len())),
                Piece::List { .. } => stops.push(Stop::List),
                Piece::Buttons(buttons) => {
                    stops.extend(
                        buttons
                            .iter()
                            .map(|(press, _, _, ready)| Stop::Button(*press, *ready)),
                    );
                }
            }
        }
        stops
    }

    /// Why the name written for a server cannot be its name.
    fn name_problem(&self, chat: &Chat) -> Option<String> {
        let id = match &self.item {
            Item::Server(id) => Some(id.as_str()),
            _ => None,
        };
        let taken: Vec<&str> = (config(chat).as_object().into_iter())
            .flat_map(|map| map.keys().map(String::as_str))
            .collect();
        servers::name_problem(self.text(NAME), id, &taken)
    }

    /// The rows of the form's list, those its search leaves.
    #[must_use]
    pub fn entries(&self, chat: &Chat) -> Vec<Entry> {
        let providers = chat.providers.list.as_deref().unwrap_or_default();
        let query = self.list.query.text();
        let entry = |model: &Model, marked: bool| Entry {
            key: Some(groups::key(model)),
            name: model.name.clone(),
            maker: maker(providers, &model.provider),
            marked,
        };
        match &self.item {
            Item::Setting(key) => {
                let Some(Control::Model { none, images }) =
                    general::field(key).map(|field| field.control)
                else {
                    return Vec::new();
                };
                let current = chat.settings.value(key).as_str();
                let mut entries = Vec::new();
                if query.trim().is_empty() {
                    entries.push(Entry {
                        key: None,
                        name: none.to_string(),
                        maker: String::new(),
                        marked: current.is_none(),
                    });
                }
                let reads =
                    |model: &&Model| !images || model.input.iter().any(|input| input == "image");
                let found = matching(&chat.models, query, providers, None);
                entries.extend(
                    (found.into_iter().filter(reads))
                        .map(|model| entry(model, current == Some(groups::key(model).as_str()))),
                );
                entries
            }
            Item::Group(_) | Item::NewGroup => {
                let connected = connected(chat);
                (matching(&chat.models, query, providers, None).into_iter())
                    .filter(|model| connected.contains(model))
                    .map(|model| entry(model, self.checked.contains(&groups::key(model))))
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The form's parts, top to bottom, as what is set on it leaves them.
    #[must_use]
    pub fn pieces(&self, chat: &Chat) -> Vec<Piece> {
        let settings = &chat.settings;
        let danger = |why: &String| Piece::Say(why.clone(), Ink::danger());
        let quiet = |press, word: &str| (press, word.to_string(), Look::Quiet, true);
        let field = |at: usize, label: &'static str, asks: &str| Piece::Field {
            at,
            label,
            asks: asks.to_string(),
            secret: false,
            rows: 1,
        };
        let mut pieces = Vec::new();
        match &self.item {
            Item::Setting(key) => match general::field(key).map(|field| field.control) {
                Some(Control::Number(..)) => {
                    pieces.push(field(0, "", ""));
                    pieces.extend(self.problem.as_ref().map(danger));
                }
                Some(Control::Model { .. }) => pieces.push(Piece::List { label: "" }),
                _ => {}
            },
            Item::Server(id) if self.asking => {
                let signed = settings.server(id).is_some_and(|server| server.signed_in);
                let also = if signed { " Its sign-in goes too." } else { "" };
                pieces.push(Piece::Say(format!("Remove {id}?{also}"), Ink::muted()));
                let remove = (Press::Confirm, "Remove".to_string(), Look::Danger, true);
                pieces.push(Piece::Buttons(vec![remove, quiet(Press::Cancel, "Cancel")]));
            }
            Item::Server(id) => self.service_pieces(chat, id, &mut pieces),
            Item::AddServer => self.server_pieces(chat, "Add", false, &mut pieces),
            Item::Market(id) => {
                let key = format!("mcp:{id}");
                let asks = (settings.server(id).and_then(|server| server.token.clone()))
                    .unwrap_or_else(|| "Token".to_string());
                pieces.push(Piece::Field {
                    at: 0,
                    label: "",
                    asks,
                    secret: true,
                    rows: 1,
                });
                pieces.extend(settings.errors.get(&key).map(danger));
                let busy = settings.busy(&key);
                let ready = !busy && !self.text(0).trim().is_empty();
                let word = if busy { "Adding" } else { "Add" };
                let add = (Press::Main, word.to_string(), Look::Main, ready);
                pieces.push(Piece::Buttons(vec![add, quiet(Press::Cancel, "Cancel")]));
            }
            Item::NewSkill => self.skill_pieces(chat, &mut pieces),
            Item::Folder(path) => {
                let asks = format!("Stop reading skills from {path}?");
                pieces.push(Piece::Say(asks, Ink::muted()));
                let remove = (
                    Press::Confirm,
                    "Remove".to_string(),
                    Look::Danger,
                    !settings.busy(FOLDER),
                );
                pieces.push(Piece::Buttons(vec![remove, quiet(Press::Cancel, "Cancel")]));
            }
            Item::AddFolder => {
                pieces.push(field(0, "", "e.g. ~/.claude/skills"));
                pieces.extend(settings.errors.get(FOLDER).map(danger));
                let ready = !settings.busy(FOLDER) && !self.text(0).trim().is_empty();
                let add = (Press::Main, "Add".to_string(), Look::Main, ready);
                pieces.push(Piece::Buttons(vec![add, quiet(Press::Cancel, "Cancel")]));
            }
            Item::Group(_) | Item::NewGroup => self.group_pieces(chat, &mut pieces),
            Item::Restart | Item::Skill(_) => {}
        }
        pieces
    }

    /// What a row of one of the user's servers opens on: what is said of
    /// it, its sign-in, and the form of one the file holds.
    fn service_pieces(&self, chat: &Chat, id: &str, pieces: &mut Vec<Piece>) {
        let settings = &chat.settings;
        let server = settings.server(id);
        let entry = config(chat).get(id);
        if server.is_some_and(|server| !server.user) {
            let said = server.and_then(|server| server.description.clone());
            pieces.extend(said.map(|said| Piece::Say(said, Ink::muted())));
        } else if entry.is_none() {
            let said = "Set in this workspace's settings, not here.";
            pieces.push(Piece::Say(said.to_string(), Ink::muted()));
        }
        let hint = (server.filter(|server| server.status != "not_connected"))
            .and_then(|server| server.hint.clone());
        pieces.extend(hint.map(|hint| Piece::Say(hint, Ink::muted())));
        let key = format!("mcp:{id}");
        let refused = settings.errors.get(&key);
        pieces.extend(refused.map(|why| Piece::Say(why.clone(), Ink::danger())));
        if let Some(server) = server.filter(|server| server.sign_in || server.signed_in) {
            let mut buttons = Vec::new();
            if server.sign_in {
                let word = if server.signed_in {
                    "Sign in again"
                } else {
                    "Sign in"
                };
                buttons.push((Press::SignIn, word.to_string(), Look::Main, true));
            }
            if server.signed_in {
                let busy = settings.busy(&key);
                let word = if busy { "Signing out" } else { "Sign out" };
                buttons.push((Press::SignOut, word.to_string(), Look::Danger, !busy));
            }
            pieces.push(Piece::Buttons(buttons));
        }
        if entry.is_some() {
            self.server_pieces(chat, "Save", true, pieces);
        }
    }

    /// The form of a new skill: its name, what it is for, what makes it.
    fn skill_pieces(&self, chat: &Chat, pieces: &mut Vec<Piece>) {
        let settings = &chat.settings;
        let field = |at: usize, label: &'static str, asks: &str| Piece::Field {
            at,
            label,
            asks: asks.to_string(),
            secret: false,
            rows: 1,
        };
        pieces.push(field(0, "Name", "e.g. pdf-reports"));
        let asks = "e.g. Write PDF reports from data. Use when asked for a report.";
        pieces.push(field(1, "Description", asks));
        let help = "What it does and when to use it: the agent picks skills by it.";
        pieces.push(Piece::Say(help.to_string(), Ink::faint()));
        let problem = servers::skill_problem(self.text(0));
        let refused = settings.errors.get(CREATE).cloned();
        pieces.extend(
            (problem.clone().into_iter().chain(refused)).map(|why| Piece::Say(why, Ink::danger())),
        );
        let busy = settings.busy(CREATE);
        let complete = !self.text(0).trim().is_empty() && !self.text(1).trim().is_empty();
        let word = if busy { "Creating" } else { "Create" };
        let ready = complete && problem.is_none() && !busy;
        pieces.push(Piece::Buttons(vec![
            (Press::Main, word.to_string(), Look::Main, ready),
            (Press::Cancel, "Cancel".to_string(), Look::Quiet, true),
        ]));
    }

    /// The form of a group of instructions: its models, its text, what
    /// saves it; or the question before it is deleted.
    fn group_pieces(&self, chat: &Chat, pieces: &mut Vec<Piece>) {
        let settings = &chat.settings;
        let busy = settings.busy(INSTRUCTIONS);
        let refused = settings.errors.get(INSTRUCTIONS);
        let refused = refused.map(|why| Piece::Say(why.clone(), Ink::danger()));
        let cancel = (Press::Cancel, "Cancel".to_string(), Look::Quiet, true);
        if self.asking {
            pieces.push(Piece::Say("Delete this group?".to_string(), Ink::muted()));
            pieces.extend(refused);
            let word = if busy { "Deleting" } else { "Delete" };
            let delete = (Press::Confirm, word.to_string(), Look::Danger, !busy);
            pieces.push(Piece::Buttons(vec![delete, cancel]));
            return;
        }
        pieces.push(Piece::List { label: "Models" });
        pieces.push(Piece::Field {
            at: 0,
            label: "Instructions",
            asks: "e.g. Be concise. No summary at the end.".to_string(),
            secret: false,
            rows: AREA_ROWS,
        });
        pieces.extend(refused);
        let complete = !self.checked.is_empty() && !self.text(0).trim().is_empty();
        let word = if busy { "Saving" } else { "Save" };
        let mut buttons = vec![
            (Press::Main, word.to_string(), Look::Main, complete && !busy),
            cancel,
        ];
        if matches!(self.item, Item::Group(_)) {
            buttons.push((Press::Remove, "Delete".to_string(), Look::Quiet, true));
        }
        pieces.push(Piece::Buttons(buttons));
    }

    /// The parts of a server's form: its name, how it is reached and what
    /// goes with that, why it cannot be saved, its buttons.
    fn server_pieces(&self, chat: &Chat, main: &str, removable: bool, pieces: &mut Vec<Piece>) {
        let field = |at: usize, label: &'static str, asks: &str| Piece::Field {
            at,
            label,
            asks: asks.to_string(),
            secret: false,
            rows: 1,
        };
        let written = self.written();
        pieces.push(field(NAME, "Name", "e.g. docs"));
        pieces.push(Piece::Choice {
            at: KIND,
            label: "Reached by",
            options: &["URL", "Command"],
        });
        if written.command {
            let asks = "e.g. npx -y @modelcontextprotocol/server-github";
            pieces.push(field(LINE, "Command", asks));
            pieces.push(field(PASSED, "Variables", "e.g. GITHUB_TOKEN"));
            let help = "From your environment, separated by spaces; it gets PATH and HOME anyway. NAME=OTHER passes OTHER as NAME.";
            pieces.push(Piece::Say(help.to_string(), Ink::faint()));
        } else {
            pieces.push(field(URL, "URL", "e.g. https://mcp.example.com/mcp"));
            pieces.push(Piece::Choice {
                at: AUTH,
                label: "Sign-in",
                options: &["None", "OAuth", "Token variable"],
            });
            match written.auth {
                servers::OAUTH => {
                    let said = "Once saved, sign in from this row.";
                    pieces.push(Piece::Say(said.to_string(), Ink::faint()));
                }
                servers::VARIABLE => {
                    pieces.push(field(VARIABLE, "Token variable", "e.g. DOCS_TOKEN"));
                }
                _ => {}
            }
        }
        let problem = self.name_problem(chat);
        pieces.extend(problem.clone().map(|why| Piece::Say(why, Ink::danger())));
        let refused = chat.settings.errors.get(SERVERS);
        pieces.extend(refused.map(|why| Piece::Say(why.clone(), Ink::danger())));
        let busy = chat.settings.busy(SERVERS);
        let ready = written.complete() && problem.is_none() && !busy;
        let word = if busy { "Saving" } else { main };
        let mut buttons = vec![
            (Press::Main, word.to_string(), Look::Main, ready),
            (Press::Cancel, "Cancel".to_string(), Look::Quiet, true),
        ];
        if removable {
            buttons.push((Press::Remove, "Remove".to_string(), Look::Quiet, true));
        }
        pieces.push(Piece::Buttons(buttons));
    }
}

/// Writes a setting whole.
fn set(key: &str, value: &Value) -> Wish {
    Wish::Ask(SET, json!({ "key": key, "value": value }))
}

impl Prefs {
    /// A row is acted on: a switch flips, a form opens under the row (or
    /// closes, open already), the agent restarts.
    pub fn activate(&mut self, item: &Item, chat: &Chat) -> Vec<Wish> {
        self.chosen = Some(item.clone());
        self.reveal.set(true);
        if self.sheet.as_ref().is_some_and(|sheet| sheet.item == *item) {
            self.sheet = None;
            return Vec::new();
        }
        self.sheet = None;
        let settings = &chat.settings;
        let mut sheet = Sheet::new(item.clone());
        match item {
            Item::Restart => return vec![Wish::Restart],
            Item::Setting(key) => {
                let Some(field) = general::field(key) else {
                    return Vec::new();
                };
                match field.control {
                    Control::Switch => {
                        let next = json!(!settings.on(key));
                        return vec![set(key, &settings.stored(key, next))];
                    }
                    Control::Number(unit, _) => {
                        // Only a value set in the file fills the field: the default shows without it.
                        let value = settings.value(key).as_u64();
                        let written =
                            value.filter(|value| Some(*value) != settings.fallback(key).as_u64());
                        let text =
                            written.map_or_else(String::new, |value| general::shown(value, unit));
                        sheet.fields.push(Draft::holding(&text));
                    }
                    Control::Model { .. } => {
                        if chat.models.is_empty() {
                            return Vec::new();
                        }
                        let entries = sheet.entries(chat);
                        sheet.list.at = entries.iter().position(|entry| entry.marked).unwrap_or(0);
                        sheet.list.reveal();
                    }
                }
            }
            Item::Server(id) => {
                if let Some(entry) = config(chat).get(id) {
                    sheet = Sheet::server(item.clone(), &Written::of(id, entry));
                }
            }
            Item::AddServer => sheet = Sheet::server(item.clone(), &Written::default()),
            Item::Market(id) => {
                let Some(server) = settings.server(id) else {
                    return Vec::new();
                };
                if server.token.is_none() {
                    // The sign-in screen takes over until it is done.
                    return if server.sign_in {
                        vec![Wish::SignIn(id.clone())]
                    } else {
                        Vec::new()
                    };
                }
                sheet.fields.push(Draft::default());
            }
            Item::Skill(path) => {
                let all = settings.skills.iter().flat_map(|skills| &skills.list);
                let Some(skill) = all.into_iter().find(|skill| skill.path == *path) else {
                    return Vec::new();
                };
                if skill.source == "builtin" {
                    return Vec::new();
                }
                let key = format!("skill:{path}");
                let on = (settings.drafts.get(&key))
                    .map_or(skill.enabled, |draft| draft == &json!(true));
                return vec![Wish::Ask(
                    SKILLS_SET,
                    json!({ "path": path, "enabled": !on }),
                )];
            }
            Item::NewSkill => sheet.fields = vec![Draft::default(), Draft::default()],
            Item::Folder(_) => sheet.asking = true,
            Item::AddFolder | Item::NewGroup => sheet.fields.push(Draft::default()),
            Item::Group(at) => {
                let Some((_, models, text)) = shown_groups(chat)
                    .into_iter()
                    .find(|(place, ..)| place == at)
                else {
                    return Vec::new();
                };
                sheet.checked = models.into_iter().map(groups::key).collect();
                sheet.fields.push(Draft::holding(&text));
            }
        }
        // Enter signs in where that is still to do; else what is typed
        // goes to the form's first field, if it has one.
        let stops = Sheet::stops(&sheet.pieces(chat));
        let unsigned = matches!(item, Item::Server(id) if settings.server(id).is_some_and(|server| !server.signed_in));
        let sign_in = (stops.iter())
            .position(|stop| matches!(stop, Stop::Button(Press::SignIn, _)))
            .filter(|_| unsigned);
        let field = (stops.iter()).position(|stop| matches!(stop, Stop::Field(..) | Stop::List));
        sheet.focus = sign_in.or(field).unwrap_or(0);
        self.sheet = Some(sheet);
        Vec::new()
    }

    /// A server of the file is turned on or off.
    fn flip(&mut self, item: &Item, chat: &Chat) -> Vec<Wish> {
        let Item::Server(id) = item else {
            return self.activate(item, chat);
        };
        let map = config(chat);
        let Some(Value::Object(entry)) = map.get(id) else {
            return Vec::new();
        };
        let mut entry = entry.clone();
        // On is said by no word at all.
        if entry.remove("enabled") != Some(Value::Bool(false)) {
            entry.insert("enabled".to_string(), json!(false));
        }
        let next = servers::with(map, Some(id), id, Some(Value::Object(entry)));
        vec![set(SERVERS, &chat.settings.stored(SERVERS, next))]
    }

    /// A button of the open form.
    fn press(&mut self, press: Press, chat: &Chat) -> Vec<Wish> {
        let settings = &chat.settings;
        let Some(sheet) = &mut self.sheet else {
            return Vec::new();
        };
        let item = sheet.item.clone();
        match (press, &item) {
            (Press::Cancel, Item::Folder(_)) => self.sheet = None,
            (Press::Cancel, _) if sheet.asking => sheet.asking = false,
            (Press::Cancel, _) => self.sheet = None,
            (Press::Remove, _) => {
                sheet.asking = true;
                sheet.focus = 0;
            }
            (Press::SignIn, Item::Server(id)) => return vec![Wish::SignIn(id.clone())],
            (Press::SignOut, Item::Server(id)) => {
                return vec![Wish::Ask(MCP_LOGOUT, json!({ "id": id }))];
            }
            (Press::Confirm, Item::Server(id)) => {
                let next = servers::with(config(chat), Some(id), id, None);
                let mut wishes = vec![set(SERVERS, &settings.stored(SERVERS, next))];
                if settings.server(id).is_some_and(|server| server.signed_in) {
                    wishes.push(Wish::Ask(MCP_LOGOUT, json!({ "id": id })));
                }
                self.sheet = None;
                return wishes;
            }
            (Press::Confirm, Item::Folder(path)) => {
                self.sheet = None;
                return vec![Wish::Ask(SKILLS_REMOVE_FOLDER, json!({ "path": path }))];
            }
            (Press::Confirm, Item::Group(at)) => {
                let groups = groups::saved(settings.value(INSTRUCTIONS));
                sheet.sent = Some(INSTRUCTIONS.to_string());
                let next = groups::with(&groups, Some(*at), None, &[]);
                return vec![set(INSTRUCTIONS, &next)];
            }
            (Press::Main, _) => return self.submit(chat),
            _ => {}
        }
        Vec::new()
    }

    /// What the open form is for: it is sent, when all it needs is written.
    fn submit(&mut self, chat: &Chat) -> Vec<Wish> {
        let settings = &chat.settings;
        let Some(sheet) = &mut self.sheet else {
            return Vec::new();
        };
        let item = sheet.item.clone();
        match &item {
            Item::Setting(key) => {
                let Some(Control::Number(unit, max)) =
                    general::field(key).map(|field| field.control)
                else {
                    return Vec::new();
                };
                return self.number(key, unit, max, chat);
            }
            Item::Server(_) | Item::AddServer => {
                let written = sheet.written();
                if !written.complete()
                    || sheet.name_problem(chat).is_some()
                    || settings.busy(SERVERS)
                {
                    return Vec::new();
                }
                let id = match &item {
                    Item::Server(id) => Some(id.as_str()),
                    _ => None,
                };
                let map = config(chat);
                let entry = written.config(id.and_then(|id| map.get(id)));
                let next = servers::with(map, id, &written.name, Some(entry));
                sheet.sent = Some(SERVERS.to_string());
                return vec![set(SERVERS, &settings.stored(SERVERS, next))];
            }
            Item::Market(id) => {
                let token = sheet.text(0).trim().to_string();
                if token.is_empty() || settings.busy(&format!("mcp:{id}")) {
                    return Vec::new();
                }
                sheet.sent = Some(format!("mcp:{id}"));
                return vec![Wish::Ask(
                    MCP_SET_TOKEN,
                    json!({ "id": id, "token": token }),
                )];
            }
            Item::NewSkill => {
                let (name, description) = (sheet.text(0).trim(), sheet.text(1).trim());
                let refused = name.is_empty()
                    || description.is_empty()
                    || servers::skill_problem(name).is_some()
                    || settings.busy(CREATE);
                if refused {
                    return Vec::new();
                }
                let params = json!({ "name": name, "description": description });
                sheet.sent = Some(CREATE.to_string());
                return vec![Wish::Ask(SKILLS_CREATE, params)];
            }
            Item::AddFolder => {
                let path = sheet.text(0).trim().to_string();
                if path.is_empty() || settings.busy(FOLDER) {
                    return Vec::new();
                }
                sheet.sent = Some(FOLDER.to_string());
                return vec![Wish::Ask(SKILLS_ADD_FOLDER, json!({ "path": path }))];
            }
            Item::Group(_) | Item::NewGroup => {
                let text = sheet.text(0).trim().to_string();
                if sheet.checked.is_empty() || text.is_empty() || settings.busy(INSTRUCTIONS) {
                    return Vec::new();
                }
                let index = match &item {
                    Item::Group(at) => Some(*at),
                    _ => None,
                };
                let shown: Vec<String> = connected(chat).into_iter().map(groups::key).collect();
                let groups = groups::saved(settings.value(INSTRUCTIONS));
                let next = groups::with(&groups, index, Some((&sheet.checked, &text)), &shown);
                sheet.sent = Some(INSTRUCTIONS.to_string());
                return vec![set(INSTRUCTIONS, &next)];
            }
            _ => {}
        }
        Vec::new()
    }

    /// The number written for `key` is stored: nothing written, or the
    /// default, puts the default back. What is no number stays, with why.
    fn number(&mut self, key: &'static str, unit: Unit, max: u64, chat: &Chat) -> Vec<Wish> {
        let settings = &chat.settings;
        let Some(sheet) = &mut self.sheet else {
            return Vec::new();
        };
        let stored = match general::stored(sheet.text(0), unit, max) {
            Ok(stored) => stored,
            Err(why) => {
                sheet.problem = Some(why);
                return Vec::new();
            }
        };
        self.sheet = None;
        let default = settings.fallback(key).as_u64();
        let now = settings.value(key).as_u64();
        let next = stored.filter(|stored| Some(*stored) != default);
        match next {
            Some(next) if Some(next) != now => vec![set(key, &json!(next))],
            None if now != default => vec![set(key, &Value::Null)],
            _ => Vec::new(),
        }
    }

    /// The row at place `at` of the open form's list: the model is picked
    /// for the setting, or its check flips in the group.
    fn pick(&mut self, at: usize, chat: &Chat) -> Vec<Wish> {
        let Some(sheet) = &mut self.sheet else {
            return Vec::new();
        };
        let entries = sheet.entries(chat);
        let Some(entry) = entries.get(at) else {
            return Vec::new();
        };
        sheet.list.at = at;
        if let Item::Setting(key) = &sheet.item {
            let key = *key;
            let next = entry.key.clone().map_or(Value::Null, Value::String);
            let same = chat.settings.value(key) == &next;
            self.sheet = None;
            return if same {
                Vec::new()
            } else {
                vec![set(key, &next)]
            };
        }
        if let Some(key) = &entry.key {
            match sheet.checked.iter().position(|checked| checked == key) {
                Some(at) => {
                    sheet.checked.remove(at);
                }
                None => sheet.checked.push(key.clone()),
            }
        }
        Vec::new()
    }

    /// A key on the screen. Without a form: the arrows choose a row, Enter
    /// acts on it, Tab goes to the next tab, what is typed searches the
    /// rows where they are searched, Esc empties the search, then leaves.
    /// In a form: Tab and the arrows go through its fields and buttons,
    /// the arrows also through its list and a choice's options, Enter
    /// presses the button it is on (elsewhere, the form's own; in a list,
    /// the row), Esc steps back.
    pub fn key(&mut self, key: KeyEvent, chat: &Chat) -> Vec<Wish> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.sheet.is_some() {
            return self.sheet_key(key, chat);
        }
        let searches = self.searches(chat);
        match key.code {
            KeyCode::Esc | KeyCode::Char('c') if key.code == KeyCode::Esc || ctrl => {
                if self.query.is_empty() {
                    return vec![Wish::Leave];
                }
                self.query.take();
                self.top.set(0);
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let at = TABS
                    .iter()
                    .position(|(tab, _)| *tab == self.tab)
                    .unwrap_or(0);
                let by = if key.code == KeyCode::Tab {
                    1
                } else {
                    TABS.len() - 1
                };
                self.turn(TABS[(at + by) % TABS.len()].0);
            }
            KeyCode::Up => self.step(-1, chat),
            // From the search, the arrows start at the first row it found.
            KeyCode::Down if self.chosen.is_none() && self.found(chat).is_some() => {
                self.chosen = self.found(chat);
                self.reveal.set(true);
            }
            KeyCode::Down => self.step(1, chat),
            KeyCode::Enter => {
                if let Some(item) = self.chosen.clone().or_else(|| self.found(chat)) {
                    return self.activate(&item, chat);
                }
            }
            KeyCode::Char(' ') if !searches => {
                if let Some(item) = self.chosen.clone() {
                    return self.activate(&item, chat);
                }
            }
            _ if searches => {
                let was = self.query.text().to_string();
                desk::edit(&mut self.query, key, false);
                if self.query.text() != was {
                    self.chosen = None;
                    self.top.set(0);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// A key while a form is open.
    fn sheet_key(&mut self, key: KeyEvent, chat: &Chat) -> Vec<Wish> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let wide = self.wide.get();
        let Some(sheet) = &mut self.sheet else {
            return Vec::new();
        };
        let stops = Sheet::stops(&sheet.pieces(chat));
        let last = stops.len().saturating_sub(1);
        sheet.focus = sheet.focus.min(last);
        let stop = stops.get(sheet.focus).copied();
        let count = sheet.entries(chat).len();
        let go = |sheet: &mut Sheet, by: isize| {
            sheet.focus = sheet.focus.saturating_add_signed(by).min(last);
        };
        match (key.code, stop) {
            (KeyCode::Esc | KeyCode::Char('c'), _) if key.code == KeyCode::Esc || ctrl => {
                if !sheet.list.query.is_empty() && stop == Some(Stop::List) {
                    sheet.list = List::default();
                } else {
                    return self.press(Press::Cancel, chat);
                }
            }
            (KeyCode::Tab, _) if !stops.is_empty() => sheet.focus = (sheet.focus + 1) % stops.len(),
            (KeyCode::BackTab, _) if !stops.is_empty() => {
                sheet.focus = (sheet.focus + last) % stops.len();
            }
            (KeyCode::Enter, Some(Stop::Button(press, ready))) => {
                if ready {
                    return self.press(press, chat);
                }
            }
            (KeyCode::Enter, Some(Stop::List)) => {
                let at = sheet.list.at;
                return self.pick(at, chat);
            }
            // A line break in a text of several rows.
            (KeyCode::Enter, Some(Stop::Field(at, rows)))
                if rows > 1
                    && key
                        .modifiers
                        .intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) =>
            {
                if let Some(field) = sheet.fields.get_mut(at) {
                    field.insert("\n");
                }
            }
            (KeyCode::Enter, _) => return self.press(Press::Main, chat),
            (KeyCode::Up | KeyCode::Down, Some(Stop::List)) => {
                let by = if key.code == KeyCode::Up { -1 } else { 1 };
                if !sheet.list.step(by, count) {
                    go(sheet, by);
                }
            }
            (KeyCode::Up | KeyCode::Down, Some(Stop::Field(at, rows))) if rows > 1 => {
                let by = if key.code == KeyCode::Up { -1 } else { 1 };
                let moved =
                    (sheet.fields.get_mut(at)).is_some_and(|field| field.vertical(by, wide));
                if !moved {
                    go(sheet, by);
                }
            }
            (KeyCode::Up, _) | (KeyCode::Left, Some(Stop::Button(..))) => go(sheet, -1),
            (KeyCode::Down, _) | (KeyCode::Right, Some(Stop::Button(..))) => go(sheet, 1),
            (KeyCode::Left | KeyCode::Right, Some(Stop::Choice(at, options))) => {
                let by = if key.code == KeyCode::Left { -1 } else { 1 };
                if let Some(choice) = sheet.choices.get_mut(at) {
                    *choice = choice
                        .saturating_add_signed(by)
                        .min(options.saturating_sub(1));
                }
            }
            (_, Some(Stop::Field(at, rows))) => {
                if let Some(field) = sheet.fields.get_mut(at) {
                    desk::edit(field, key, rows > 1);
                    sheet.problem = None;
                }
            }
            (_, Some(Stop::List)) => {
                let was = sheet.list.query.text().to_string();
                desk::edit(&mut sheet.list.query, key, false);
                if sheet.list.query.text() != was {
                    (sheet.list.at, sheet.list.top) = (0, 0);
                }
            }
            _ => {}
        }
        Vec::new()
    }

    /// What is pasted goes where what is typed does: a field takes one
    /// line, a text of several rows takes them all.
    pub fn paste(&mut self, text: &str, chat: &Chat) {
        let line = text.replace(['\r', '\n'], "");
        let searches = self.searches(chat);
        let Some(sheet) = &mut self.sheet else {
            if searches {
                self.query.insert(&line);
                self.chosen = None;
                self.top.set(0);
            }
            return;
        };
        let stops = Sheet::stops(&sheet.pieces(chat));
        match stops.get(sheet.focus) {
            Some(Stop::Field(at, rows)) => {
                if let Some(field) = sheet.fields.get_mut(*at) {
                    field.insert(if *rows > 1 { text } else { &line });
                    sheet.problem = None;
                }
            }
            Some(Stop::List) => {
                sheet.list.query.insert(&line);
                (sheet.list.at, sheet.list.top) = (0, 0);
            }
            _ => {}
        }
    }

    /// A click on the screen.
    pub fn click(&mut self, spot: &Spot, chat: &Chat) -> Vec<Wish> {
        match spot {
            Spot::Tab(tab) => self.turn(*tab),
            Spot::Pref(item) => return self.activate(item, chat),
            Spot::Flip(item) => {
                self.chosen = Some(item.clone());
                return self.flip(item, chat);
            }
            Spot::Drop(item) => {
                // What removes a row opens on the question, whatever was open.
                self.sheet = None;
                return self.activate(item, chat);
            }
            Spot::Stop(at) => {
                let Some(sheet) = &mut self.sheet else {
                    return Vec::new();
                };
                let stops = Sheet::stops(&sheet.pieces(chat));
                sheet.focus = (*at).min(stops.len().saturating_sub(1));
                if let Some(Stop::Button(press, true)) = stops.get(*at) {
                    return self.press(*press, chat);
                }
            }
            Spot::Opt(at, option) => {
                let Some(sheet) = &mut self.sheet else {
                    return Vec::new();
                };
                let stops = Sheet::stops(&sheet.pieces(chat));
                if let Some(Stop::Choice(choice, _)) = stops.get(*at) {
                    sheet.focus = *at;
                    if let Some(choice) = sheet.choices.get_mut(*choice) {
                        *choice = *option;
                    }
                }
            }
            Spot::Entry(at) => {
                if let Some(sheet) = &mut self.sheet {
                    let stops = Sheet::stops(&sheet.pieces(chat));
                    sheet.focus = (stops.iter().position(|stop| *stop == Stop::List)).unwrap_or(0);
                }
                return self.pick(*at, chat);
            }
            _ => {}
        }
        Vec::new()
    }

    /// The wheel over the open form's list: its rows in view move `by`.
    pub fn roll(&mut self, by: isize, chat: &Chat) -> bool {
        let Some(sheet) = &mut self.sheet else {
            return false;
        };
        let count = sheet.entries(chat).len();
        if count <= LIST_ROWS {
            return false;
        }
        sheet.list.top = (sheet.list.top.saturating_add_signed(by)).min(count - LIST_ROWS);
        true
    }
}
