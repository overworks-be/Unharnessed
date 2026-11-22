//! The agent's settings as it tells them over its `_unharnessed/settings/*`,
//! `mcp/*` and `skills/*` methods
//! (`crates/uh-daemon/src/acp/{settings,mcp_servers,skills}.rs`): the
//! global settings file's values and their defaults, the MCP servers, the
//! skills; what is being changed, and why a change was refused. It follows
//! the extension's `vscode/src/settings.ts`. An agent keeps what it
//! started with of some of them: what a restart would apply is told from
//! what its first answer said.

use std::collections::HashMap;

use serde_json::{Map, Value};

use crate::text::clean;

pub const GET: &str = "_unharnessed/settings/get";
pub const SET: &str = "_unharnessed/settings/set";
pub const MCP_LIST: &str = "_unharnessed/mcp/list";
pub const MCP_SET_TOKEN: &str = "_unharnessed/mcp/set_token";
pub const MCP_LOGOUT: &str = "_unharnessed/mcp/logout";
pub const SKILLS_LIST: &str = "_unharnessed/skills/list";
pub const SKILLS_SET: &str = "_unharnessed/skills/set";
pub const SKILLS_CREATE: &str = "_unharnessed/skills/create";
pub const SKILLS_ADD_FOLDER: &str = "_unharnessed/skills/add_folder";
pub const SKILLS_REMOVE_FOLDER: &str = "_unharnessed/skills/remove_folder";

/// The settings whose change is told by these names rather than by a
/// key of the file: a skill made, and a folder of skills added or removed.
pub const CREATE: &str = "skills/create";
pub const FOLDER: &str = "skills/folder";
/// The settings the screen writes whole.
pub const SERVERS: &str = "mcpServers";
pub const INSTRUCTIONS: &str = "modelInstructions";

/// The global settings (`settings/get`): keys are the file's dotted paths.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct View {
    /// Each key's value in the file; `null` when unset.
    pub values: Map<String, Value>,
    /// What the agent uses for an unset key; `null`: nothing.
    pub defaults: Map<String, Value>,
    /// The keys read when the agent starts.
    pub restart: Vec<String>,
}

/// One MCP server (`mcp/list`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Server {
    pub id: String,
    pub label: String,
    /// One of the `mcpServers` setting; else a service of the catalog.
    pub user: bool,
    /// `connected`, `pending`, `not_connected`, `setup_required`, `disabled`, `error`.
    pub status: String,
    /// An OAuth sign-in can run now.
    pub sign_in: bool,
    /// A sign-in or a token is stored.
    pub signed_in: bool,
    pub description: Option<String>,
    /// What it needs, or why it fails.
    pub hint: Option<String>,
    /// What else it is found by: its category, its publisher, its other names.
    pub found_by: Vec<String>,
    /// A catalog service connected by a pasted token: what to paste.
    pub token: Option<String>,
}

/// One skill (`skills/list`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Its file.
    pub path: String,
    /// `builtin`, `user`, `folder`, `project`.
    pub source: String,
    /// The agent loads it when it starts; the running agent has it.
    pub enabled: bool,
    pub active: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Skills {
    pub list: Vec<Skill>,
    /// The folders added to the `skills` setting.
    pub folders: Vec<String>,
    /// What keeps a skill from loading, and its file.
    pub problems: Vec<(String, Option<String>)>,
    /// The next start loads other skills than the running agent has.
    pub restart: bool,
}

/// What a request was for, until its answer comes.
enum Asked {
    /// The settings read, or one of them written under that key.
    View(Option<String>),
    /// The servers listed, or one changed under that key.
    Servers(Option<String>),
    /// The skills listed, or changed under that key.
    Skills(Option<String>),
}

/// What an answer brings, for the change that goes by that key.
type Brings = fn(Option<String>) -> Asked;

#[derive(Default)]
pub struct Settings {
    /// `None` until the agent has answered.
    pub view: Option<View>,
    pub servers: Option<Vec<Server>>,
    pub skills: Option<Skills>,
    /// What is being changed, by its key (a setting's, `mcp:<id>`,
    /// `skill:<path>`, [`CREATE`], [`FOLDER`]): the value shown until the
    /// agent answers.
    pub drafts: HashMap<String, Value>,
    /// Why the last change under a key was refused.
    pub errors: HashMap<String, String>,
    /// The file of the skill just made.
    pub created: Option<String>,
    /// The restart keys' values when this agent first told them.
    started: Option<Map<String, Value>>,
    /// A sign-in to a server, a sign-out or a token since: the agent keeps
    /// the servers it started with.
    signed: bool,
    /// The servers are to be listed again: the setting that holds them changed.
    stale: bool,
    asked: HashMap<i64, Asked>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(clean)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn view(result: &Value) -> Option<View> {
    let map = |key: &str| result.get(key).and_then(Value::as_object).cloned();
    Some(View {
        values: map("values")?,
        defaults: map("defaults")?,
        restart: (result.get("restart").and_then(Value::as_array))
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    })
}

fn servers(result: &Value) -> Option<Vec<Server>> {
    let rows = result.get("servers")?.as_array()?;
    Some(
        rows.iter()
            .filter_map(|row| {
                let id = row.get("id")?.as_str()?.to_string();
                let mut found_by: Vec<String> = ["category", "publisher"]
                    .into_iter()
                    .filter_map(|key| text(row, key))
                    .collect();
                found_by.extend(
                    (row.get("aliases").and_then(Value::as_array))
                        .into_iter()
                        .flatten()
                        .filter_map(Value::as_str)
                        .map(clean),
                );
                Some(Server {
                    label: text(row, "label").unwrap_or_else(|| clean(&id)),
                    id,
                    user: row.get("source").and_then(Value::as_str) == Some("user"),
                    status: text(row, "status").unwrap_or_default(),
                    sign_in: flag(row, "signIn"),
                    signed_in: flag(row, "signedIn"),
                    description: text(row, "description"),
                    hint: text(row, "hint"),
                    found_by,
                    token: text(row, "token"),
                })
            })
            .collect(),
    )
}

fn skills(result: &Value) -> Option<Skills> {
    let rows = result.get("skills")?.as_array()?;
    let list = |key: &str| (result.get(key).and_then(Value::as_array).into_iter()).flatten();
    Some(Skills {
        list: rows
            .iter()
            .filter_map(|row| {
                Some(Skill {
                    name: text(row, "name")?,
                    description: text(row, "description").unwrap_or_default(),
                    path: row.get("path")?.as_str()?.to_string(),
                    source: text(row, "source").unwrap_or_default(),
                    enabled: flag(row, "enabled"),
                    active: flag(row, "active"),
                })
            })
            .collect(),
        folders: list("folders")
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        problems: list("problems")
            .filter_map(|problem| Some((text(problem, "message")?, text(problem, "path"))))
            .collect(),
        restart: flag(result, "restart"),
    })
}

/// Why the agent refused: its own words, else the error's message.
fn refused(frame: &Value) -> Option<String> {
    let error = frame.get("error")?;
    let why = error
        .pointer("/data/details")
        .or_else(|| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("The agent refused, and did not say why.");
    Some(clean(why))
}

impl Settings {
    /// What the screen shows for `key`: what is being written, else the
    /// file's value, else what the agent uses without one.
    #[must_use]
    pub fn value(&self, key: &str) -> &Value {
        if let Some(draft) = self.drafts.get(key) {
            return draft;
        }
        let Some(view) = &self.view else {
            return &Value::Null;
        };
        (view.values.get(key).filter(|value| !value.is_null()))
            .or_else(|| view.defaults.get(key))
            .unwrap_or(&Value::Null)
    }

    /// What the agent uses for `key` while the file sets nothing.
    #[must_use]
    pub fn fallback(&self, key: &str) -> &Value {
        (self.view.as_ref())
            .and_then(|view| view.defaults.get(key))
            .unwrap_or(&Value::Null)
    }

    /// `key` is `true`, in the file or by default.
    #[must_use]
    pub fn on(&self, key: &str) -> bool {
        self.value(key) == &Value::Bool(true)
    }

    /// A change under `key` waits for the agent's answer.
    #[must_use]
    pub fn busy(&self, key: &str) -> bool {
        self.drafts.contains_key(key)
    }

    /// What `key` is written as for it to hold `value`: a value that is
    /// the default leaves the file, and the default applies.
    #[must_use]
    pub fn stored(&self, key: &str, value: Value) -> Value {
        if &value == self.fallback(key) {
            Value::Null
        } else {
            value
        }
    }

    /// What a restart of the agent would apply: the restart keys changed
    /// since it started, `skills`, `mcp`.
    #[must_use]
    pub fn pending(&self) -> Vec<String> {
        let mut keys = Vec::new();
        if let (Some(view), Some(started)) = (&self.view, &self.started) {
            keys.extend(
                (view.restart.iter())
                    .filter(|key| view.values.get(*key) != started.get(*key))
                    .cloned(),
            );
        }
        if self.skills.as_ref().is_some_and(|skills| skills.restart) {
            keys.push("skills".to_string());
        }
        if self.signed {
            keys.push("mcp".to_string());
        }
        keys
    }

    /// The server of that id.
    #[must_use]
    pub fn server(&self, id: &str) -> Option<&Server> {
        self.servers.iter().flatten().find(|server| server.id == id)
    }

    /// A sign-in to an MCP server went through: its list shows it, a restart applies it.
    pub fn signed_in(&mut self) {
        self.signed = true;
        self.stale = true;
    }

    /// The servers are to be listed again: asked once.
    pub fn stale(&mut self) -> bool {
        std::mem::take(&mut self.stale)
    }

    /// A request the client sent; answers whether it was one of these.
    pub fn sent(&mut self, id: i64, method: &str, params: &Value) -> bool {
        let said = |key: &str| params.get(key).and_then(Value::as_str);
        // What the answer brings, the key the change goes by, and what shows meanwhile.
        let (brings, key, draft) = match method {
            GET => (Asked::View as Brings, None, Value::Null),
            MCP_LIST => (Asked::Servers as Brings, None, Value::Null),
            SKILLS_LIST => (Asked::Skills as Brings, None, Value::Null),
            SET => {
                let Some(key) = said("key") else {
                    return true;
                };
                let value = params.get("value").filter(|value| !value.is_null());
                let shown = value.unwrap_or_else(|| self.fallback(key)).clone();
                (Asked::View as Brings, Some(key.to_string()), shown)
            }
            MCP_SET_TOKEN | MCP_LOGOUT => {
                let key = format!("mcp:{}", said("id").unwrap_or_default());
                (Asked::Servers as Brings, Some(key), Value::Bool(true))
            }
            SKILLS_SET => {
                let key = format!("skill:{}", said("path").unwrap_or_default());
                let enabled = params.get("enabled").cloned().unwrap_or(Value::Null);
                (Asked::Skills as Brings, Some(key), enabled)
            }
            SKILLS_CREATE => (
                Asked::Skills as Brings,
                Some(CREATE.to_string()),
                Value::Bool(true),
            ),
            SKILLS_ADD_FOLDER | SKILLS_REMOVE_FOLDER => (
                Asked::Skills as Brings,
                Some(FOLDER.to_string()),
                Value::Bool(true),
            ),
            _ => return false,
        };
        if let Some(key) = &key {
            self.drafts.insert(key.clone(), draft);
            self.errors.remove(key);
            if key == CREATE {
                self.created = None;
            }
        }
        self.asked.insert(id, brings(key));
        true
    }

    /// The answer to request `id`; answers whether it was one of these.
    pub fn answered(&mut self, id: i64, frame: &Value) -> bool {
        let Some(asked) = self.asked.remove(&id) else {
            return false;
        };
        let result = frame.get("result").unwrap_or(&Value::Null);
        let key = match asked {
            Asked::View(key) => {
                if let Some(view) = view(result) {
                    // What this agent started with: its first answer, before any change.
                    if self.started.is_none() {
                        let kept = |key: &String| {
                            let value = view.values.get(key).cloned().unwrap_or(Value::Null);
                            (key.clone(), value)
                        };
                        self.started = Some(view.restart.iter().map(kept).collect());
                    }
                    self.view = Some(view);
                    // A server added, changed or removed changes their list.
                    self.stale |= key.as_deref() == Some(SERVERS);
                }
                key
            }
            Asked::Servers(key) => {
                if let Some(servers) = servers(result) {
                    self.servers = Some(servers);
                    self.signed |= key.is_some();
                }
                key
            }
            Asked::Skills(key) => {
                if let Some(skills) = skills(result) {
                    self.skills = Some(skills);
                    if key.as_deref() == Some(CREATE) {
                        self.created = text(result, "created");
                    }
                }
                key
            }
        };
        if let Some(key) = key {
            self.drafts.remove(&key);
            if let Some(why) = refused(frame) {
                self.errors.insert(key, why);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn told(values: &Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "result": {
            "values": values,
            "defaults": { "compaction.enabled": true, "retry.maxRetries": 3, "mcpServers": {} },
            "restart": ["compaction.enabled", "mcpServers"],
        } })
    }

    // How what a restart would apply could be told wrong: a key that
    // applies at once (retries) counted; a restart key put back to what
    // the agent started with still counted; what the agent started with
    // taken from an answer after a change, so nothing ever counts; a
    // refused change counted, or its draft left showing; the servers not
    // listed again once their setting changed.
    #[test]
    fn a_restart_applies_what_changed_since_the_agent_started() {
        let mut settings = Settings::default();
        let unset =
            json!({ "compaction.enabled": null, "retry.maxRetries": null, "mcpServers": null });
        assert!(settings.sent(1, GET, &json!({})));
        assert!(settings.answered(1, &told(&unset)));
        assert!(settings.on("compaction.enabled"));
        assert!(settings.pending().is_empty());

        settings.sent(2, SET, &json!({ "key": "retry.maxRetries", "value": 5 }));
        assert_eq!(settings.value("retry.maxRetries"), &json!(5));
        let mut set = unset;
        set["retry.maxRetries"] = json!(5);
        settings.answered(2, &told(&set));
        assert!(settings.pending().is_empty() && !settings.busy("retry.maxRetries"));

        settings.sent(
            3,
            SET,
            &json!({ "key": "compaction.enabled", "value": false }),
        );
        assert!(!settings.on("compaction.enabled"));
        set["compaction.enabled"] = json!(false);
        settings.answered(3, &told(&set));
        assert_eq!(settings.pending(), ["compaction.enabled"]);

        settings.sent(
            4,
            SET,
            &json!({ "key": "compaction.enabled", "value": null }),
        );
        assert!(settings.on("compaction.enabled"));
        set["compaction.enabled"] = json!(null);
        settings.answered(4, &told(&set));
        assert!(settings.pending().is_empty());

        settings.sent(
            5,
            SET,
            &json!({ "key": "mcpServers", "value": { "docs": {} } }),
        );
        let refusal = json!({ "id": 5, "error": { "message": "Invalid params", "data": { "details": "docs needs a url" } } });
        settings.answered(5, &refusal);
        assert_eq!(
            settings.errors.get(SERVERS).map(String::as_str),
            Some("docs needs a url")
        );
        assert!(settings.pending().is_empty() && !settings.stale());
        assert_eq!(settings.value(SERVERS), &json!({}));

        settings.sent(
            6,
            SET,
            &json!({ "key": "mcpServers", "value": { "docs": { "url": "x" } } }),
        );
        assert!(settings.errors.is_empty());
        set["mcpServers"] = json!({ "docs": { "url": "x" } });
        settings.answered(6, &told(&set));
        assert_eq!(settings.pending(), ["mcpServers"]);
        assert!(settings.stale() && !settings.stale());
        // What is the default is written as nothing: the file keeps no copy of it.
        assert_eq!(settings.stored("retry.maxRetries", json!(3)), Value::Null);
        assert_eq!(settings.stored("retry.maxRetries", json!(4)), json!(4));
    }

    // How the servers and the skills could be read wrong: a row without
    // its id or its file kept; a service's other names lost to the search;
    // a sign-out not counted for the restart, or a mere listing counted.
    #[test]
    fn the_servers_and_the_skills_are_read_as_listed() {
        let mut settings = Settings::default();
        settings.sent(1, MCP_LIST, &json!({}));
        let listed = json!({ "id": 1, "result": { "servers": [
            { "id": "docs", "label": "docs", "source": "user", "status": "not_connected", "signIn": true, "signedIn": false },
            { "id": "linear", "label": "Linear", "source": "catalog", "status": "connected", "signIn": true, "signedIn": true,
              "category": "Project management", "aliases": ["issues"], "token": "Linear API key" },
            { "label": "No id" },
        ] } });
        settings.answered(1, &listed);
        let servers = settings.servers.clone().unwrap_or_default();
        assert_eq!(servers.len(), 2);
        assert!(servers[0].user && servers[0].sign_in && !servers[0].signed_in);
        assert!(!servers[1].user && servers[1].signed_in);
        assert_eq!(servers[1].found_by, ["Project management", "issues"]);
        assert!(settings.pending().is_empty());
        settings.sent(2, MCP_LOGOUT, &json!({ "id": "linear" }));
        assert!(settings.busy("mcp:linear"));
        settings.answered(2, &listed);
        assert_eq!(settings.pending(), ["mcp"]);

        settings.sent(
            3,
            SKILLS_CREATE,
            &json!({ "name": "tidy", "description": "Tidy." }),
        );
        let made = json!({ "id": 3, "result": { "created": "/a/skills/tidy/SKILL.md", "restart": true,
            "folders": ["/more"], "problems": [{ "message": "no name", "path": "/x/SKILL.md" }, { "message": "odd" }],
            "skills": [
                { "name": "tidy", "description": "Tidy.", "path": "/a/skills/tidy/SKILL.md", "source": "user", "enabled": true, "active": false },
                { "name": "nowhere" },
            ] } });
        settings.answered(3, &made);
        let skills = settings.skills.clone().unwrap_or_default();
        assert_eq!(
            (
                skills.list.len(),
                skills.folders.len(),
                skills.problems.len()
            ),
            (1, 1, 2)
        );
        assert_eq!(settings.created.as_deref(), Some("/a/skills/tidy/SKILL.md"));
        assert_eq!(settings.pending(), ["skills", "mcp"]);
        assert!(!settings.sent(4, "session/prompt", &json!({})) && !settings.answered(9, &made));
    }
}
