//! An MCP server of the `mcpServers` setting as its form writes it (the
//! extension's `vscode/webview/SettingsMcp.tsx`): reached by a URL, with
//! its sign-in, or by a command, with the variables it gets. What the form
//! does not show of an entry (headers, tool filters, timeouts) stays as
//! the file has it.

use serde_json::{json, Map, Value};

use crate::settings::Server;

/// How a server is signed in to, by its place in the form's choice.
pub const NONE: usize = 0;
pub const OAUTH: usize = 1;
pub const VARIABLE: usize = 2;

/// What the form holds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub name: String,
    /// Reached by a command; else by a URL.
    pub command: bool,
    pub url: String,
    pub auth: usize,
    pub variable: String,
    pub line: String,
    pub passed: String,
}

impl Written {
    /// What the form shows for the entry `config` of the file, named `name`.
    #[must_use]
    pub fn of(name: &str, config: &Value) -> Written {
        let said = |key: &str| config.get(key).and_then(Value::as_str);
        let variable = said("bearerTokenEnvVar");
        Written {
            name: name.to_string(),
            command: said("type") == Some("stdio"),
            url: said("url").unwrap_or_default().to_string(),
            auth: match (variable, config.get("oauth")) {
                (Some(_), _) => VARIABLE,
                (None, Some(Value::Bool(true))) => OAUTH,
                (None, _) => NONE,
            },
            variable: variable.unwrap_or_default().to_string(),
            line: command_line(config),
            passed: env_line(config),
        }
    }

    /// Everything the server needs is written.
    #[must_use]
    pub fn complete(&self) -> bool {
        if self.name.trim().is_empty() {
            return false;
        }
        if self.command {
            !split_command(&self.line).is_empty()
        } else {
            !self.url.trim().is_empty()
                && (self.auth != VARIABLE || !self.variable.trim().is_empty())
        }
    }

    /// The entry to write: `before`, the file's, with what the form shows
    /// replaced.
    #[must_use]
    pub fn config(&self, before: Option<&Value>) -> Value {
        let before = before.and_then(Value::as_object);
        let was_command =
            (before.and_then(|before| before.get("type"))).and_then(Value::as_str) == Some("stdio");
        let kept = |key: &str| before.and_then(|before| before.get(key)).cloned();
        let mut next = before.cloned().unwrap_or_default();
        for shown in [
            "url",
            "oauth",
            "bearerTokenEnvVar",
            "headers",
            "command",
            "args",
            "cwd",
            "env",
        ] {
            next.remove(shown);
        }
        if self.command {
            next.insert("type".to_string(), json!("stdio"));
            let words = split_command(&self.line);
            next.insert("command".to_string(), json!(words.first()));
            if words.len() > 1 {
                next.insert("args".to_string(), json!(words[1..]));
            }
            if let Some(cwd) = kept("cwd").filter(|_| was_command) {
                next.insert("cwd".to_string(), cwd);
            }
            let env = parse_env(&self.passed);
            if !env.is_empty() {
                next.insert("env".to_string(), Value::Object(env));
            }
        } else {
            next.insert("type".to_string(), json!("http"));
            next.insert("url".to_string(), json!(self.url.trim()));
            if let Some(headers) = kept("headers").filter(|_| !was_command) {
                next.insert("headers".to_string(), headers);
            }
            match self.auth {
                OAUTH => {
                    next.insert("oauth".to_string(), json!(true));
                }
                VARIABLE => {
                    next.insert("bearerTokenEnvVar".to_string(), json!(self.variable.trim()));
                }
                _ => {}
            }
        }
        Value::Object(next)
    }
}

/// Why `name` cannot name a server: the agent addresses it by letters,
/// digits, `-` and `_`, 64 at the most, the first a letter or a digit; and
/// no other server (`taken`) has it. `id`: the name it has now.
#[must_use]
pub fn name_problem(name: &str, id: Option<&str>, taken: &[&str]) -> Option<String> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    let mut chars = name.chars();
    let fine = chars
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
        && name.len() <= 64;
    if !fine {
        return Some(
            "A name is letters, digits, - and _, starting with a letter or digit.".to_string(),
        );
    }
    (Some(name) != id && taken.contains(&name))
        .then(|| format!("There is a server named {name} already."))
}

/// Why `name` cannot name a skill: lowercase letters, digits and single hyphens.
#[must_use]
pub fn skill_problem(name: &str) -> Option<String> {
    let name = name.trim();
    let fine = name.split('-').all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    });
    (!name.is_empty() && !fine)
        .then(|| "Lowercase letters, digits and single hyphens, like pdf-reports.".to_string())
}

/// The command and its arguments as one line, quoted where needed.
#[must_use]
pub fn command_line(config: &Value) -> String {
    let said = |key: &str| config.get(key).and_then(Value::as_str);
    let (Some("stdio"), Some(command)) = (said("type"), said("command")) else {
        return String::new();
    };
    let args = (config.get("args").and_then(Value::as_array).into_iter())
        .flatten()
        .map(|arg| arg.as_str().map_or_else(|| arg.to_string(), str::to_string));
    let plain = |word: &str| {
        !word.is_empty() && !word.contains(|ch: char| ch.is_whitespace() || "\"'\\".contains(ch))
    };
    (std::iter::once(command.to_string()).chain(args))
        .map(|word| {
            if plain(&word) {
                word
            } else {
                json!(word).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Words split on spaces; quotes keep spaces in, a backslash keeps the
/// next character (not inside single quotes).
#[must_use]
pub fn split_command(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut started = false;
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && quote != Some('\'') && chars.peek().is_some() {
            word.extend(chars.next());
            started = true;
        } else if let Some(open) = quote {
            if ch == open {
                quote = None;
            } else {
                word.push(ch);
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
            started = true;
        } else if ch.is_whitespace() {
            if std::mem::take(&mut started) {
                words.push(std::mem::take(&mut word));
            }
        } else {
            word.push(ch);
            started = true;
        }
    }
    if started {
        words.push(word);
    }
    words
}

/// The `env` entries as `NAME`, or `NAME=OTHER` when another variable feeds it.
#[must_use]
pub fn env_line(config: &Value) -> String {
    let stdio = config.get("type").and_then(Value::as_str) == Some("stdio");
    let Some(env) = config
        .get("env")
        .and_then(Value::as_object)
        .filter(|_| stdio)
    else {
        return String::new();
    };
    env.iter()
        .map(
            |(name, from)| match from.get("env").and_then(Value::as_str) {
                Some(other) if other != name && !other.is_empty() => format!("{name}={other}"),
                _ => name.clone(),
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// The variables a line names, each fed by the one of the same name, or
/// by the one after its `=`.
#[must_use]
pub fn parse_env(line: &str) -> Map<String, Value> {
    let mut env = Map::new();
    for word in line.split(|ch: char| ch.is_whitespace() || ch == ',') {
        let (name, from) = word.split_once('=').unwrap_or((word, ""));
        if !name.is_empty() {
            let from = if from.is_empty() { name } else { from };
            env.insert(name.to_string(), json!({ "env": from }));
        }
    }
    env
}

/// Where a server is: its URL's host, or its command.
#[must_use]
pub fn target(config: &Value) -> Option<String> {
    let said = |key: &str| config.get(key).and_then(Value::as_str);
    if said("type") == Some("stdio") {
        return said("command").map(crate::text::clean);
    }
    let url = url::Url::parse(said("url")?).ok()?;
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    })
}

/// What a server's state comes to, for its row, and whether it is said in
/// the accent. `None`: it is being saved, the list does not have it yet.
#[must_use]
pub fn status(server: Option<&Server>) -> (String, bool) {
    let Some(server) = server else {
        return ("Saving".to_string(), false);
    };
    let hint = |otherwise: &str| server.hint.clone().unwrap_or_else(|| otherwise.to_string());
    match server.status.as_str() {
        "disabled" => ("Off".to_string(), false),
        // Nothing to sign in to: the agent reaches it when it needs it.
        "connected" if server.user && !server.signed_in => ("Ready".to_string(), false),
        "connected" => ("Connected".to_string(), true),
        "pending" => ("Signed in".to_string(), true),
        "not_connected" if server.sign_in => ("Not signed in".to_string(), false),
        "not_connected" => (hint("Not connected"), false),
        _ => (hint("Needs setup"), false),
    }
}

/// The `mcpServers` map with the entry `id` written as `config` under
/// `name` (`id` `None`: a new one), or removed (`config` `None`).
#[must_use]
pub fn with(map: &Value, id: Option<&str>, name: &str, config: Option<Value>) -> Value {
    let mut next = Map::new();
    let mut placed = false;
    for (key, entry) in map.as_object().into_iter().flatten() {
        if Some(key.as_str()) == id {
            // A server renamed keeps its place.
            if let Some(config) = config.clone() {
                next.insert(name.to_string(), config);
                placed = true;
            }
        } else if key != name || config.is_none() {
            next.insert(key.clone(), entry.clone());
        }
    }
    if let (false, Some(config)) = (placed, config) {
        next.insert(name.to_string(), config);
    }
    Value::Object(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    // How a command line could be read or written wrong:
    // - an argument with a space (`--root "/my files"`) split in two, or
    //   written back without its quotes, so the next save splits it;
    // - an empty argument (`""`) lost; a quote inside a word (`a"b c"d`)
    //   breaking the word; a backslash taken literally inside single quotes
    //   only; a lone backslash at the end dropped or panicking;
    // - runs of spaces and tabs making empty words;
    // - an argument that is a number in the file written as nothing.
    #[test]
    fn a_command_line_splits_and_joins_like_a_shell() {
        for (line, words) in [
            ("npx -y @scope/server", vec!["npx", "-y", "@scope/server"]),
            ("  uv   run\tserver.py ", vec!["uv", "run", "server.py"]),
            (
                "serve --root \"/my files\"",
                vec!["serve", "--root", "/my files"],
            ),
            ("serve '' x", vec!["serve", "", "x"]),
            ("a\"b c\"d", vec!["ab cd"]),
            ("say it\\'s", vec!["say", "it's"]),
            ("say 'a\\b'", vec!["say", "a\\b"]),
            ("back\\", vec!["back\\"]),
            ("", vec![]),
            ("   ", vec![]),
        ] {
            assert_eq!(split_command(line), words, "{line:?}");
        }
        let config = json!({ "type": "stdio", "command": "serve", "args": ["--root", "/my files", "", 8080, "it's"] });
        let line = command_line(&config);
        assert_eq!(line, r#"serve --root "/my files" "" 8080 "it's""#);
        assert_eq!(
            split_command(&line),
            ["serve", "--root", "/my files", "", "8080", "it's"]
        );
        assert_eq!(command_line(&json!({ "type": "http", "command": "x" })), "");
    }

    // How the variables could be read or written wrong: `NAME=OTHER` kept
    // as a name with an `=` in it; commas not taken for separators; a
    // variable fed by itself written as `NAME=NAME`; an entry of the file
    // that is no `{ env }` breaking the line.
    #[test]
    fn the_variables_a_server_gets_are_names_or_name_from_other() {
        let env = parse_env("GITHUB_TOKEN, API=MY_KEY  PLAIN= =x");
        assert_eq!(
            Value::Object(env),
            json!({ "GITHUB_TOKEN": { "env": "GITHUB_TOKEN" }, "API": { "env": "MY_KEY" }, "PLAIN": { "env": "PLAIN" } })
        );
        let config = json!({ "type": "stdio", "command": "x", "env": { "A": { "env": "A" }, "B": { "env": "C" }, "D": "odd" } });
        assert_eq!(env_line(&config), "A B=C D");
        assert_eq!(
            env_line(&json!({ "type": "http", "env": { "A": { "env": "A" } } })),
            ""
        );
    }

    // How saving could lose or invent something: the headers, the tool
    // filters and the timeouts of an entry dropped because the form does
    // not show them; the URL's fields left on a server now reached by a
    // command (and its headers, another server's secrets, kept with it);
    // `oauth` and the token variable both written; `enabled: false` lost;
    // a renamed server left under both names, or moved to the end; a new
    // one replacing its namesake silently.
    #[test]
    fn saving_a_server_keeps_what_the_form_does_not_show() {
        let before = json!({ "type": "http", "url": "https://old.example/mcp", "oauth": true,
            "headers": { "X-Team": "a" }, "enabled": false, "toolTimeoutMs": 5000 });
        let mut written = Written::of("docs", &before);
        assert_eq!(
            (written.auth, written.url.as_str(), written.command),
            (OAUTH, "https://old.example/mcp", false)
        );
        written.url = " https://new.example/mcp ".to_string();
        written.auth = VARIABLE;
        written.variable = "DOCS_TOKEN".to_string();
        assert!(written.complete());
        assert_eq!(
            written.config(Some(&before)),
            json!({ "type": "http", "url": "https://new.example/mcp", "bearerTokenEnvVar": "DOCS_TOKEN",
                "headers": { "X-Team": "a" }, "enabled": false, "toolTimeoutMs": 5000 })
        );
        written.command = true;
        assert!(!written.complete());
        written.line = "npx -y docs".to_string();
        written.passed = "DOCS_TOKEN".to_string();
        assert_eq!(
            written.config(Some(&before)),
            json!({ "type": "stdio", "command": "npx", "args": ["-y", "docs"],
                "env": { "DOCS_TOKEN": { "env": "DOCS_TOKEN" } }, "enabled": false, "toolTimeoutMs": 5000 })
        );
        let blank = Written {
            name: "x".to_string(),
            url: "http://h/mcp".to_string(),
            ..Written::default()
        };
        assert_eq!(
            blank.config(None),
            json!({ "type": "http", "url": "http://h/mcp" })
        );
        let variable = Written {
            auth: VARIABLE,
            ..blank.clone()
        };
        assert!(blank.complete() && !variable.complete());

        let map = json!({ "a": { "url": "1" }, "b": { "url": "2" }, "c": { "url": "3" } });
        let keys = |map: &Value| {
            map.as_object()
                .map(|map| map.keys().cloned().collect::<Vec<_>>().join(","))
        };
        let renamed = with(&map, Some("b"), "bee", Some(json!({ "url": "2" })));
        assert_eq!(keys(&renamed).as_deref(), Some("a,bee,c"));
        assert_eq!(
            keys(&with(&map, None, "d", Some(json!({})))).as_deref(),
            Some("a,b,c,d")
        );
        assert_eq!(
            keys(&with(&map, Some("b"), "b", None)).as_deref(),
            Some("a,c")
        );
        assert_eq!(
            keys(&with(&Value::Null, None, "d", Some(json!({})))).as_deref(),
            Some("d")
        );
    }

    // How a name could be let through wrong: one the agent cannot address
    // (a space, a dot, a leading hyphen, 65 letters); another server's; its
    // own name refused as taken when the server is only edited.
    #[test]
    fn a_name_is_one_the_agent_can_address_and_no_other_server_has() {
        let taken = ["docs", "gh"];
        assert_eq!(name_problem("", None, &taken), None);
        assert_eq!(name_problem("new_one-2", None, &taken), None);
        assert_eq!(name_problem(" docs ", Some("docs"), &taken), None);
        assert_eq!(
            name_problem("docs", None, &taken),
            Some("There is a server named docs already.".to_string())
        );
        assert!(name_problem("gh", Some("docs"), &taken).is_some());
        for name in ["my docs", "docs.v2", "-docs", "_docs", "é", &"x".repeat(65)] {
            assert!(
                name_problem(name, None, &taken).is_some_and(|why| why.starts_with("A name is")),
                "{name}"
            );
        }
        assert_eq!(skill_problem("pdf-reports"), None);
        assert_eq!(skill_problem(""), None);
        for name in [
            "PDF",
            "pdf--reports",
            "-pdf",
            "pdf-",
            "pdf reports",
            "pdf_reports",
        ] {
            assert!(skill_problem(name).is_some(), "{name}");
        }
    }

    // Where a server is, for its row: the host of its URL with a port
    // that is not the usual one, or its command; nothing for a URL that is none.
    #[test]
    fn a_server_is_at_its_host_or_its_command() {
        assert_eq!(
            target(&json!({ "type": "http", "url": "https://mcp.example.com/mcp" })).as_deref(),
            Some("mcp.example.com")
        );
        assert_eq!(
            target(&json!({ "url": "http://127.0.0.1:9/mcp" })).as_deref(),
            Some("127.0.0.1:9")
        );
        assert_eq!(
            target(&json!({ "type": "stdio", "command": "npx", "args": ["x"] })).as_deref(),
            Some("npx")
        );
        assert_eq!(target(&json!({ "url": "not a url" })), None);
        assert_eq!(target(&json!({})), None);
    }
}
