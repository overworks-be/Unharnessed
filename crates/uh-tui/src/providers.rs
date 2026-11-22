//! The providers as the agent tells them over its
//! `_unharnessed/providers/*` methods
//! (`crates/uh-daemon/src/acp/providers.rs`): the rows of its list, the
//! browser sign-in under way with its steps, and what saving a key or
//! removing a credential came to. It follows the extension's
//! `vscode/src/providers.ts`, without the servers of this machine being
//! added from here.

use std::collections::HashMap;

use serde_json::Value;

use crate::text::clean;

pub const LIST: &str = "_unharnessed/providers/list";
pub const SET_API_KEY: &str = "_unharnessed/providers/set_api_key";
pub const LOGOUT: &str = "_unharnessed/providers/logout";
pub const LOGIN: &str = "_unharnessed/providers/login";
pub const LOGIN_ANSWER: &str = "_unharnessed/providers/login_answer";
pub const LOGIN_OPEN: &str = "_unharnessed/providers/login_open";
pub const LOGIN_CANCEL: &str = "_unharnessed/providers/login_cancel";
pub const LOGIN_UPDATE: &str = "_unharnessed/providers/login_update";
pub const LOCAL_REMOVE: &str = "_unharnessed/providers/local_remove";

/// Which row of the list: Anthropic and xAI have one for their
/// subscription and one for their API key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub id: String,
    pub subscription: bool,
}

/// One row of the agent's list.
#[derive(Clone, Debug)]
pub struct Provider {
    pub key: Key,
    pub name: String,
    /// The agent runs the sign-in, in a browser; else the key is asked for here.
    pub browser: bool,
    pub connected: bool,
    /// The credential is in the agent's own file: it can be removed.
    pub removable: bool,
    /// Where an outside key comes from, or why the row is not connected.
    pub note: Option<String>,
    pub warning: Option<String>,
    /// A server on this machine: the models picked from it.
    pub local: Option<Vec<String>>,
}

/// Something a sign-in asks to be written.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub id: u64,
    pub message: String,
    pub secret: bool,
    pub allow_empty: bool,
    /// Another way than the browser sign-in running beside it.
    pub optional: bool,
}

/// The teams a sign-in asks to choose from.
#[derive(Clone, Debug)]
pub struct Teams {
    pub prompt: u64,
    /// Each team's id, its name, and the user's role in it.
    pub list: Vec<(String, String, Option<String>)>,
    pub current: Option<String>,
}

/// What a sign-in waits for the user to answer.
#[derive(Clone, Debug)]
enum Waits {
    Prompt(Prompt),
    Teams(Teams),
}

/// The browser sign-in under way.
#[derive(Clone, Debug)]
pub struct SignIn {
    pub provider: String,
    pub name: String,
    /// The sign-in page, and what the provider says to do there.
    pub url: Option<String>,
    pub instructions: Option<String>,
    /// The latest word on how it goes.
    pub status: Option<String>,
    pub prompt: Option<Prompt>,
    pub teams: Option<Teams>,
    /// It failed: the screen stays until the user leaves it.
    pub error: Option<String>,
    /// Its request.
    request: i64,
}

/// What a request was for, until its answer comes.
enum Asked {
    List,
    /// A key saved, a credential removed.
    Change,
    SignIn,
    /// An answer to what the sign-in asked, kept to show again if it is refused.
    Answer(Waits),
}

#[derive(Default)]
pub struct Providers {
    /// The rows; `None` until the agent listed them.
    pub list: Option<Vec<Provider>>,
    pub sign_in: Option<SignIn>,
    /// A key is being saved, or a credential removed.
    pub busy: bool,
    /// Why the last of those failed.
    pub error: Option<String>,
    /// How many of them went through.
    pub changes: u32,
    /// The provider a sign-in just went through for, until it is read.
    pub signed: Option<String>,
    asked: HashMap<i64, Asked>,
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(clean)
}

fn flag(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool) == Some(true)
}

fn rows(result: &Value) -> Option<Vec<Provider>> {
    let rows = result.get("providers")?.as_array()?;
    Some(
        rows.iter()
            .filter_map(|row| {
                Some(Provider {
                    key: Key {
                        id: row.get("id")?.as_str()?.to_string(),
                        subscription: row.get("method").and_then(Value::as_str)
                            == Some("subscription"),
                    },
                    name: text(row, "name")?,
                    browser: row.get("login").and_then(Value::as_str) == Some("browser"),
                    connected: flag(row, "connected"),
                    removable: flag(row, "removable"),
                    note: text(row, "note"),
                    warning: text(row, "warning"),
                    local: row.get("local").map(|local| {
                        (local.get("models").and_then(Value::as_array))
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str)
                            .map(clean)
                            .collect()
                    }),
                })
            })
            .collect(),
    )
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

impl Providers {
    /// A row of the list.
    #[must_use]
    pub fn row(&self, key: &Key) -> Option<&Provider> {
        self.list.iter().flatten().find(|row| row.key == *key)
    }

    /// The same provider's other credential, when that one is connected.
    #[must_use]
    pub fn other(&self, key: &Key) -> Option<&Provider> {
        (self.list.iter().flatten())
            .find(|row| row.key.id == key.id && row.key != *key && row.connected)
    }

    /// The agent listed its providers, and none is connected.
    #[must_use]
    pub fn none_connected(&self) -> bool {
        (self.list.as_ref()).is_some_and(|list| !list.iter().any(|row| row.connected))
    }

    /// A sign-in is under way: one that failed is over.
    #[must_use]
    pub fn signing_in(&self) -> bool {
        (self.sign_in.as_ref()).is_some_and(|sign_in| sign_in.error.is_none())
    }

    /// A request the client sent.
    pub fn sent(&mut self, id: i64, method: &str, params: &Value) {
        match method {
            LIST => {
                self.asked.insert(id, Asked::List);
            }
            SET_API_KEY | LOGOUT | LOCAL_REMOVE => {
                self.busy = true;
                self.error = None;
                self.asked.insert(id, Asked::Change);
            }
            LOGIN => {
                let provider = text(params, "providerId").unwrap_or_default();
                let name = (self.list.iter().flatten())
                    .find(|row| row.key.id == provider && row.browser)
                    .map_or_else(|| provider.clone(), |row| row.name.clone());
                self.sign_in = Some(SignIn {
                    provider,
                    name,
                    url: None,
                    instructions: None,
                    status: None,
                    prompt: None,
                    teams: None,
                    error: None,
                    request: id,
                });
                self.asked.insert(id, Asked::SignIn);
            }
            LOGIN_ANSWER => {
                // What was asked leaves at once: the sign-in's next step takes its place.
                let Some(sign_in) = &mut self.sign_in else {
                    return;
                };
                let prompt = params.get("promptId").and_then(Value::as_u64);
                let waits = match (sign_in.prompt.take(), sign_in.teams.take()) {
                    (Some(asked), _) if Some(asked.id) == prompt => Waits::Prompt(asked),
                    (_, Some(asked)) if Some(asked.prompt) == prompt => Waits::Teams(asked),
                    (asked, teams) => {
                        sign_in.prompt = asked;
                        sign_in.teams = teams;
                        return;
                    }
                };
                self.asked.insert(id, Asked::Answer(waits));
            }
            _ => {}
        }
    }

    /// A step of the sign-in under way (`login_update`).
    pub fn update(&mut self, params: &Value) {
        let provider = params.get("providerId").and_then(Value::as_str);
        let Some(sign_in) = (self.sign_in.as_mut()).filter(|sign_in| {
            sign_in.error.is_none() && Some(sign_in.provider.as_str()) == provider
        }) else {
            return;
        };
        let update = params.get("update").unwrap_or(&Value::Null);
        let prompt = update.get("promptId").and_then(Value::as_u64);
        match update.get("kind").and_then(Value::as_str) {
            Some("url") => {
                sign_in.url = text(update, "url");
                sign_in.instructions = text(update, "instructions");
            }
            Some("progress" | "waiting") => sign_in.status = text(update, "message"),
            Some("prompt") => {
                sign_in.teams = None;
                sign_in.prompt = prompt.map(|id| Prompt {
                    id,
                    message: text(update, "message").unwrap_or_default(),
                    secret: flag(update, "secret"),
                    allow_empty: flag(update, "allowEmpty"),
                    optional: flag(update, "optional"),
                });
            }
            Some("team") => {
                sign_in.prompt = None;
                sign_in.teams = prompt.map(|prompt| Teams {
                    prompt,
                    list: (update.get("teams").and_then(Value::as_array))
                        .into_iter()
                        .flatten()
                        .filter_map(|team| {
                            Some((
                                team.get("id")?.as_str()?.to_string(),
                                text(team, "name")?,
                                text(team, "role"),
                            ))
                        })
                        .collect(),
                    current: update
                        .get("current")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                });
            }
            _ => {}
        }
    }

    /// The answer to request `id`, if it was one of these.
    pub fn answered(&mut self, id: i64, frame: &Value) {
        let Some(asked) = self.asked.remove(&id) else {
            return;
        };
        let result = frame.get("result").unwrap_or(&Value::Null);
        if let Some(list) = rows(result) {
            self.list = Some(list);
        }
        // The agent's words when it could not: the status line of what went through stays unsaid.
        let failed = refused(frame).or_else(|| {
            (result.get("outcome").and_then(Value::as_str) == Some("failed"))
                .then(|| text(result, "message").unwrap_or_else(|| "It failed.".to_string()))
        });
        match asked {
            Asked::List => {}
            Asked::Change => {
                self.busy = false;
                if failed.is_none() {
                    self.changes += 1;
                }
                self.error = failed;
            }
            Asked::SignIn => {
                // A sign-in started since: this answer is the one before's.
                let Some(sign_in) = (self.sign_in.as_mut()).filter(|sign_in| sign_in.request == id)
                else {
                    return;
                };
                if let Some(why) = failed {
                    sign_in.error = Some(why);
                    sign_in.prompt = None;
                    sign_in.teams = None;
                } else {
                    let done = result.get("outcome").and_then(Value::as_str) == Some("done");
                    self.signed = (self.sign_in.take())
                        .filter(|_| done)
                        .map(|sign_in| sign_in.provider);
                }
            }
            Asked::Answer(waits) => {
                // Refused, what was asked shows again, unless the sign-in moved on.
                let Some(sign_in) = (self.sign_in.as_mut()).filter(|sign_in| {
                    refused(frame).is_some()
                        && sign_in.error.is_none()
                        && sign_in.prompt.is_none()
                        && sign_in.teams.is_none()
                }) else {
                    return;
                };
                match waits {
                    Waits::Prompt(prompt) => sign_in.prompt = Some(prompt),
                    Waits::Teams(teams) => sign_in.teams = Some(teams),
                }
            }
        }
    }

    /// The user leaves the screen of a sign-in that failed.
    pub fn dismiss(&mut self) {
        if (self.sign_in.as_ref()).is_some_and(|sign_in| sign_in.error.is_some()) {
            self.sign_in = None;
        }
    }

    /// The agent's process ended: a sign-in runs in it, and ends with it.
    pub fn stopped(&mut self) {
        self.sign_in = None;
        self.busy = false;
        self.asked.clear();
    }
}
