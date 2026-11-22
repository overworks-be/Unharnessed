//! `@` mentions in a message, as the extension has them
//! (`vscode/src/mentions.ts`, `vscode/webview/mentions.tsx`,
//! `vscode/src/files.ts`): a path of the session's folder after an `@` that
//! starts a word, quoted when it holds a space (`@"docs/read me.md"`), a
//! folder ending with `/`. The message box offers the folder's files for the
//! `@` word being written and marks each mention it knows; a message goes
//! to the agent with a link for each one that exists, and comes back from a
//! saved conversation without them.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf, MAIN_SEPARATOR_STR};

use url::Url;

/// The files the `@` menu offers at most.
pub const SHOWN: usize = 50;

/// A mention in a text: from its `@` to its end, and the path it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mention {
    pub start: usize,
    pub end: usize,
    pub path: String,
}

/// What ends a sentence rather than a path: `see @a.rs.` names `a.rs`.
const TRAILING: [char; 8] = ['.', ',', ';', ':', '!', '?', ')', ']'];

/// The mentions in `text` whose path `known` says names something.
pub fn mentions(text: &str, known: impl Fn(&str) -> bool) -> Vec<Mention> {
    let mut found = Vec::new();
    // Whether an `@` here would start a word.
    let mut opens = true;
    let mut at = 0;
    while let Some(ch) = text[at..].chars().next() {
        if ch != '@' || !opens {
            opens = ch.is_whitespace();
            at += ch.len_utf8();
            continue;
        }
        opens = false;
        let rest = &text[at + 1..];
        if let Some(quoted) = rest.strip_prefix('"') {
            // Up to the quote that closes it, on the same line.
            let closed = quoted.find(['"', '\n']).filter(|end| *end > 0);
            match closed.filter(|end| quoted[*end..].starts_with('"')) {
                Some(end) => {
                    let path = &quoted[..end];
                    let stop = at + path.len() + 3;
                    if known(path) {
                        let path = path.to_string();
                        found.push(Mention {
                            start: at,
                            end: stop,
                            path,
                        });
                    }
                    at = stop;
                }
                None => at += 1,
            }
            continue;
        }
        let raw = rest
            .find(|ch: char| ch.is_whitespace() || ch == '"')
            .map_or(rest, |end| &rest[..end]);
        let path = if known(raw) {
            raw
        } else {
            raw.trim_end_matches(TRAILING)
        };
        if !path.is_empty() && known(path) {
            found.push(Mention {
                start: at,
                end: at + 1 + path.len(),
                path: path.to_string(),
            });
        }
        at += 1 + raw.len();
    }
    found
}

/// How `path` is written after its `@`.
#[must_use]
pub fn written(path: &str) -> String {
    if path.contains(char::is_whitespace) {
        format!("@\"{path}\"")
    } else {
        format!("@{path}")
    }
}

/// The `@` word the caret is in: from its `@` to its end, and what is
/// typed of it up to the caret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Word {
    pub start: usize,
    pub end: usize,
    pub query: String,
}

/// The `@` word of `text` the caret (its byte) is in, if it is in one.
#[must_use]
pub fn word_at(text: &str, caret: usize) -> Option<Word> {
    let head = text.get(..caret)?;
    // The word runs back to a space, which opens it, or to a quote, which does not.
    let edge = |ch: char| ch.is_whitespace() || ch == '"';
    let start = match head.char_indices().rev().find(|(_, ch)| edge(*ch)) {
        Some((_, '"')) => return None,
        Some((at, ch)) => at + ch.len_utf8(),
        None => 0,
    };
    let query = head[start..].strip_prefix('@')?;
    let tail = &text[caret..];
    let rest = tail.find(char::is_whitespace).unwrap_or(tail.len());
    Some(Word {
        start,
        end: caret + rest,
        query: query.to_string(),
    })
}

/// The folder's files as the `@` menu has them, and its subfolders: a
/// mention may also name a folder (ending with `/`).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Paths {
    files: Vec<String>,
    known: HashSet<String>,
}

impl Paths {
    /// `files`, relative to the folder and `/`-separated.
    #[must_use]
    pub fn of(files: Vec<String>) -> Paths {
        let mut known: HashSet<String> = HashSet::new();
        for file in &files {
            for (at, _) in file.match_indices('/') {
                if !known.contains(&file[..=at]) {
                    known.insert(file[..=at].to_string());
                }
            }
        }
        known.extend(files.iter().cloned());
        Paths { files, known }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether `path` names a file or a folder; a folder without its `/` too.
    #[must_use]
    pub fn knows(&self, path: &str) -> bool {
        self.known.contains(path) || self.known.contains(&format!("{path}/"))
    }

    /// The files matching what is typed after the `@`, best first: by
    /// name, then anywhere in the path, then by its letters in order.
    /// Nothing typed: the least deep first.
    #[must_use]
    pub fn find(&self, query: &str) -> Vec<String> {
        let query = query.to_lowercase();
        if query.is_empty() {
            let mut all: Vec<&String> = self.files.iter().collect();
            all.sort_by_key(|path| path.split('/').count());
            return all.into_iter().take(SHOWN).cloned().collect();
        }
        let mut ranked: Vec<(u8, &String)> = (self.files.iter())
            .filter_map(|path| Some((rank(&path.to_lowercase(), &query)?, path)))
            .collect();
        ranked.sort_by(|(a, one), (b, other)| {
            (a.cmp(b))
                .then(one.len().cmp(&other.len()))
                .then(one.cmp(other))
        });
        (ranked.into_iter().take(SHOWN))
            .map(|(_, path)| path.clone())
            .collect()
    }
}

fn rank(path: &str, query: &str) -> Option<u8> {
    let name = name_of(path);
    if name.starts_with(query) {
        return Some(0);
    }
    if name.contains(query) {
        return Some(1);
    }
    if path.contains(query) {
        return Some(2);
    }
    let mut wanted = query.chars().peekable();
    for ch in path.chars() {
        if wanted.peek() == Some(&ch) {
            wanted.next();
        }
    }
    wanted.peek().is_none().then_some(3)
}

/// A path's last part: a folder keeps its `/`.
#[must_use]
pub fn name_of(path: &str) -> &str {
    let inner = path.strip_suffix('/').unwrap_or(path);
    inner.rfind('/').map_or(path, |at| &path[at + 1..])
}

/// `path` under `folder`, its `.` and `..` resolved as they are written.
fn resolved(folder: &Path, path: &str) -> PathBuf {
    let mut out = PathBuf::new();
    for part in folder.join(path).components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            part => out.push(part),
        }
    }
    out
}

/// The mentions of `text` that name something in `folder`, as the links a
/// prompt carries: each one's address and its path as written, each path
/// once. The agent gets the path, not the content.
#[must_use]
pub fn links(folder: &Path, text: &str) -> Vec<(String, String)> {
    let here = resolved(folder, "");
    let exists = |path: &str| {
        let full = resolved(folder, path);
        full != here && full.exists()
    };
    let mut out: Vec<(String, String)> = Vec::new();
    for mention in mentions(text, exists) {
        let Ok(address) = Url::from_file_path(resolved(folder, &mention.path)) else {
            continue;
        };
        let address = String::from(address);
        if out.iter().all(|(known, _)| *known != address) {
            out.push((address, mention.path));
        }
    }
    out
}

/// A replayed prompt as it was written: the agent keeps its mentions'
/// links as its last lines (`file:///…`); the ones its mentions account for
/// go, or none of them does.
#[must_use]
pub fn without_links(text: &str) -> &str {
    let mut body = text;
    let mut linked: Vec<Option<String>> = Vec::new();
    while let Some((before, line)) = body.rsplit_once('\n') {
        let link = line.starts_with("file://") && !line.contains(char::is_whitespace);
        if !link {
            break;
        }
        let file = Url::parse(line)
            .ok()
            .and_then(|url| url.to_file_path().ok());
        linked.push(file.map(|file| file.to_string_lossy().into_owned()));
        body = before;
    }
    if linked.is_empty() {
        return text;
    }
    let names = |path: &str, file: &Option<String>| {
        let inner = path.trim_end_matches('/').replace('/', MAIN_SEPARATOR_STR);
        (file.as_deref()).is_some_and(|file| {
            file == path || file.ends_with(&format!("{MAIN_SEPARATOR_STR}{inner}"))
        })
    };
    let found = mentions(body, |path| linked.iter().any(|file| names(path, file)));
    let accounted =
        (linked.iter()).all(|file| found.iter().any(|mention| names(&mention.path, file)));
    if accounted {
        body
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::{links, mentions, name_of, without_links, word_at, written, Paths, SHOWN};

    // Listed before the code, how the mentions can fail:
    // - an `@` inside a word (an address, `a@b.c`) is taken for a mention;
    // - a path the folder does not have is marked;
    // - the dot that ends a sentence is taken into the path, or a path that
    //   really ends with one loses it;
    // - a quoted path's end is off by its quotes, or a quote never closed
    //   (or closed on another line) makes a mention;
    // - a folder written without its `/` is not known;
    // - a mention at the text's start, after a line break, or right after
    //   another is missed;
    // - a path with letters of several bytes puts its end inside a letter.
    fn found(text: &str, known: &[&str]) -> Vec<(String, String)> {
        mentions(text, |path| known.contains(&path))
            .into_iter()
            .map(|mention| (text[mention.start..mention.end].to_string(), mention.path))
            .collect()
    }

    fn pair(written: &str, path: &str) -> (String, String) {
        (written.to_string(), path.to_string())
    }

    #[test]
    fn mentions_are_the_known_paths_after_an_at_that_starts_a_word() {
        let known = ["a.rs", "src/", "docs/read me.md", "notes.", "é/ü.md"];
        assert_eq!(found("@a.rs", &known), [pair("@a.rs", "a.rs")]);
        assert_eq!(found("mail me@a.rs now", &known), []);
        assert_eq!(found("see @b.rs", &known), []);
        assert_eq!(found("see @a.rs.", &known), [pair("@a.rs", "a.rs")]);
        assert_eq!(found("(@a.rs)!", &known), []);
        assert_eq!(found("in @a.rs)!?", &known), [pair("@a.rs", "a.rs")]);
        assert_eq!(found("@notes. done", &known), [pair("@notes.", "notes.")]);
        assert_eq!(
            found("x\n@src/ @a.rs", &known),
            [pair("@src/", "src/"), pair("@a.rs", "a.rs")]
        );
        assert_eq!(
            found("read @\"docs/read me.md\", then", &known),
            [pair("@\"docs/read me.md\"", "docs/read me.md")]
        );
        assert_eq!(found("@\"docs/read me.md", &known), []);
        assert_eq!(found("@\"docs/read\nme.md\"", &known), []);
        assert_eq!(found("@\"\" @ @@a.rs", &known), []);
        assert_eq!(found("ça @é/ü.md…", &known), []);
        assert_eq!(found("ça @é/ü.md, là", &known), [pair("@é/ü.md", "é/ü.md")]);
    }

    #[test]
    fn a_folder_is_known_with_its_slash_or_without() {
        let paths = Paths::of(vec!["src/view/a.rs".to_string(), "b.rs".to_string()]);
        for known in [
            "src/",
            "src",
            "src/view/",
            "src/view",
            "src/view/a.rs",
            "b.rs",
        ] {
            assert!(paths.knows(known), "{known}");
        }
        for unknown in ["", "sr", "src/view/a", "view/", "b.rs/", "/"] {
            assert!(!paths.knows(unknown), "{unknown}");
        }
        assert_eq!(written("src/a b.rs"), "@\"src/a b.rs\"");
        assert_eq!(written("src/a.rs"), "@src/a.rs");
    }

    // How the word under the caret can fail:
    // - the caret in the middle of the word: what is typed stops at the
    //   caret, the word goes on to its end;
    // - an `@` that does not start a word, or a word a quote is in, opens the menu;
    // - the caret before the `@`, or after the space that ends the word, still opens it;
    // - a second `@` in the word starts another word;
    // - letters of several bytes.
    #[test]
    fn the_word_under_the_caret_is_the_at_word_it_is_in() {
        let word = |text: &str, caret: usize| {
            word_at(text, caret).map(|word| (word.start, word.end, word.query))
        };
        assert_eq!(word("@", 1), Some((0, 1, String::new())));
        assert_eq!(word("see @src/vi", 11), Some((4, 11, "src/vi".to_string())));
        assert_eq!(
            word("see @src/view.rs now", 8),
            Some((4, 16, "src".to_string()))
        );
        assert_eq!(word("see @a", 4), None);
        assert_eq!(word("see @a ", 7), None);
        assert_eq!(word("me@a", 4), None);
        assert_eq!(word("@\"a b", 3), None);
        assert_eq!(word("\"@a", 3), None);
        assert_eq!(word("@a@b", 4), Some((0, 4, "a@b".to_string())));
        assert_eq!(word("é\n@ü", 6), Some((3, 6, "ü".to_string())));
        assert_eq!(word("é @ü", 9), None);
    }

    // How the files offered can fail:
    // - a file whose name starts with what is typed comes after one that
    //   only has it in its folders;
    // - capitals in the query or in the path hide a file;
    // - a file with the letters out of order is offered;
    // - nothing typed offers the deepest files first, or more than the menu takes;
    // - among equals the order changes from one time to the next.
    #[test]
    fn the_files_offered_are_the_nearest_to_what_is_typed() {
        let files = [
            "src/view/main.rs",
            "README.md",
            "src/main.rs",
            "docs/remain.md",
            "main/x.rs",
            "src/mod.rs",
        ];
        let paths = Paths::of(files.iter().map(ToString::to_string).collect());
        assert_eq!(
            paths.find("Main"),
            [
                "src/main.rs",
                "src/view/main.rs",
                "docs/remain.md",
                "main/x.rs"
            ]
        );
        assert_eq!(
            paths.find("smr"),
            ["src/mod.rs", "src/main.rs", "src/view/main.rs"]
        );
        assert_eq!(paths.find("readme"), ["README.md"]);
        assert_eq!(paths.find("zz"), Vec::<String>::new());
        assert_eq!(paths.find("")[..2], ["README.md", "src/main.rs"]);
        assert_eq!(
            paths.find("").last().map(String::as_str),
            Some("src/view/main.rs")
        );
        let many = Paths::of((0..SHOWN + 20).map(|at| format!("f{at:03}.rs")).collect());
        assert_eq!(many.find("").len(), SHOWN);
        assert_eq!(many.find("f0").len(), SHOWN);
        assert_eq!(name_of("src/view/"), "view/");
        assert_eq!(name_of("src/a.rs"), "a.rs");
        assert_eq!(name_of("a.rs"), "a.rs");
    }

    // How a prompt that comes back can fail:
    // - a prompt without links loses its last line;
    // - a link the user wrote themselves, which no mention accounts for, goes;
    // - a prompt that is only links comes back empty;
    // - a path with a space, linked with `%20`, is not matched to its mention;
    // - a mention matches the link of another file whose path ends the same.
    #[test]
    fn a_prompt_comes_back_without_the_links_of_its_mentions() {
        let both = "see @src/a.rs and @\"read me.md\"\nfile:///w/src/a.rs\nfile:///w/read%20me.md";
        assert_eq!(without_links(both), "see @src/a.rs and @\"read me.md\"");
        assert_eq!(without_links("in @src/\nfile:///w/src"), "in @src/");
        assert_eq!(without_links("no link\nat all"), "no link\nat all");
        let own = "look at\nfile:///w/src/a.rs";
        assert_eq!(without_links(own), own);
        let other = "see @a.rs\nfile:///w/src/xa.rs";
        assert_eq!(without_links(other), other);
        let one = "see @a.rs\nfile:///w/a.rs\nfile:///w/b.rs";
        assert_eq!(without_links(one), one);
        assert_eq!(without_links("file:///w/a.rs"), "file:///w/a.rs");
        assert_eq!(without_links("see @a.rs\n\nfile:///w/a.rs"), "see @a.rs\n");
    }

    // How a message's links can fail:
    // - a mention of nothing that exists is linked, or the folder itself (`@.`);
    // - a path named twice is linked twice;
    // - a space in a path makes an address the agent cannot read back.
    #[test]
    fn a_message_links_what_its_mentions_name_once_each() {
        let folder = std::env::temp_dir().join(format!("uh-tui-links-{}", std::process::id()));
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/a b.rs"), "").unwrap();
        std::fs::write(folder.join("c.rs"), "").unwrap();
        let text = "@c.rs @./c.rs @. @src @missing.rs @\"src/a b.rs\" again @c.rs.";
        let got = links(&folder, text);
        std::fs::remove_dir_all(&folder).unwrap();
        let names: Vec<&str> = got.iter().map(|(_, name)| name.as_str()).collect();
        assert_eq!(names, ["c.rs", "src", "src/a b.rs"]);
        assert!(
            got[0].0.starts_with("file:///") && got[0].0.ends_with("/c.rs"),
            "{}",
            got[0].0
        );
        assert!(got[2].0.ends_with("/src/a%20b.rs"), "{}", got[2].0);
        let back = format!("@\"src/a b.rs\"\n{}", got[2].0);
        assert_eq!(without_links(&back), "@\"src/a b.rs\"");
    }
}
