//! The saved conversations of the session's folder, as the agent lists
//! them (`session/list`, `crates/uh-daemon/src/acp/history.rs`), and the
//! screen that shows them, the newest first, in the conversation's place:
//! one opens (its agent starts on its file), the one that is open has the
//! check, and one can be deleted once the row asked again. It follows the
//! extension's history menu (`vscode/src/history.ts`,
//! `vscode/webview/History.tsx`).

use std::cell::Cell;

use serde_json::Value;

use crate::canvas::Canvas;
use crate::chat::Chat;
use crate::connect::{self, field, words, Row, Spot};
use crate::draft::Draft;
use crate::line::Line;
use crate::mention::without_links;
use crate::motion::spinner;
use crate::text::{clean, fit, glyphs, width, Glyph, Ink};
use crate::theme::{DANGER, INK, RAISED};
use crate::view::{Screen, Ui};

/// Deletes a saved conversation.
pub const DELETE: &str = "_unharnessed/sessions/delete";
/// From this many conversations on, a field filters them (the extension's).
const SEARCH_FROM: usize = 8;

/// One saved conversation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub id: String,
    /// The start of its first message.
    pub title: Option<String>,
    /// When its file was last written, in seconds since 1970.
    pub updated: Option<i64>,
    /// Its file, which an agent starts on.
    pub path: String,
    /// It is the one the agent that listed it holds.
    pub current: bool,
}

/// The conversations a `session/list` answered, in its order.
#[must_use]
pub fn listed(result: &Value) -> Vec<Saved> {
    let rows = result.get("sessions").and_then(Value::as_array);
    (rows.into_iter().flatten())
        .filter_map(|row| {
            let said = |key: &str| row.get(key).and_then(Value::as_str);
            // As it was written, on one line: the links of its mentions are the agent's.
            let title = said("title")
                .map(|title| clean(without_links(title)))
                .map(|title| title.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|title| !title.is_empty());
            Some(Saved {
                id: said("sessionId")?.to_string(),
                title,
                updated: said("updatedAt").and_then(seconds),
                path: (row.pointer("/_meta/unharnessed/path"))
                    .and_then(Value::as_str)?
                    .to_string(),
                current: row.pointer("/_meta/unharnessed/current") == Some(&Value::Bool(true)),
            })
        })
        .collect()
}

/// The seconds since 1970 of a moment the agent wrote
/// (`2026-11-20T08:15:30.250Z`); `None` for what is not one.
fn seconds(moment: &str) -> Option<i64> {
    let (date, time) = moment.strip_suffix('Z')?.split_once('T')?;
    let mut date = date.splitn(3, '-').map(str::parse::<i64>);
    let (year, month, day) = (date.next()?.ok()?, date.next()?.ok()?, date.next()?.ok()?);
    let whole = time.split_once('.').map_or(time, |(whole, _)| whole);
    let mut time = whole.splitn(3, ':').map(str::parse::<i64>);
    let (hour, minute, second) = (time.next()?.ok()?, time.next()?.ok()?, time.next()?.ok()?);
    let fine = (1..=12).contains(&month)
        && (1..=31).contains(&day)
        && (0..24).contains(&hour)
        && (0..60).contains(&minute)
        && (0..=60).contains(&second);
    if !fine {
        return None;
    }
    // The days since 1970 of a date of the calendar, its year from March.
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let of_era = year.rem_euclid(400);
    let of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let days = era * 146_097 + of_era * 365 + of_era / 4 - of_era / 100 + of_year - 719_468;
    Some(days * 86_400 + hour * 3_600 + minute * 60 + second)
}

/// How long ago something `seconds` old happened, in words: no clock of
/// the user's is needed to read it.
#[must_use]
pub fn ago(seconds: i64) -> String {
    let (minutes, hours, days) = (seconds / 60, seconds / 3_600, seconds / 86_400);
    match () {
        () if minutes < 1 => "just now".to_string(),
        () if hours < 1 => format!("{minutes} min ago"),
        () if days < 1 => format!("{hours} h ago"),
        () if days < 2 => "yesterday".to_string(),
        () if days < 14 => format!("{days} days ago"),
        () if days < 60 => format!("{} weeks ago", days / 7),
        () if days < 730 => format!("{} months ago", days / 30),
        () => format!("{} years ago", days / 365),
    }
}

/// The conversations' screen as the user left it.
#[derive(Clone, Default)]
pub struct History {
    /// What the rows are filtered by.
    pub query: Draft,
    /// The row the arrows chose, by its id.
    pub chosen: Option<String>,
    /// The row that asks once more before it is deleted.
    pub asking: Option<String>,
    /// The first row in view, and whether the chosen row is to be brought into it.
    top: Cell<usize>,
    reveal: Cell<bool>,
}

impl History {
    /// The conversations whose title holds what is typed.
    #[must_use]
    pub fn shown<'a>(&self, saved: &'a [Saved]) -> Vec<&'a Saved> {
        let words = self.query.text().trim().to_lowercase();
        (saved.iter())
            .filter(|chat| words.is_empty() || title(chat).to_lowercase().contains(&words))
            .collect()
    }

    /// The row Enter opens: the one the arrows chose, while it shows; else
    /// the first that is not open already.
    #[must_use]
    pub fn on<'a>(&self, saved: &'a [Saved]) -> Option<&'a Saved> {
        let shown = self.shown(saved);
        let chosen = |chat: &&&Saved| Some(&chat.id) == self.chosen.as_ref();
        (shown.iter().find(chosen))
            .or_else(|| shown.iter().find(|chat| !chat.current))
            .or(shown.first())
            .copied()
    }

    /// The arrows: the chosen row moves `by` rows through those shown.
    pub fn step(&mut self, by: isize, saved: &[Saved]) {
        let shown = self.shown(saved);
        let on = self.on(saved).map(|chat| &chat.id);
        let at = shown.iter().position(|chat| Some(&chat.id) == on);
        let next = at.map_or(0, |at| {
            (at.saturating_add_signed(by)).min(shown.len().saturating_sub(1))
        });
        self.chosen = shown.get(next).map(|chat| chat.id.clone());
        self.asking = None;
        self.reveal.set(true);
    }

    /// The filter changed: the rows did, none of them is chosen, and they
    /// show from the first.
    pub fn searched(&mut self) {
        self.top.set(0);
        self.chosen = None;
        self.asking = None;
    }

    /// The row `id` asks once more before it is deleted.
    pub fn ask(&mut self, id: &str) {
        self.chosen = Some(id.to_string());
        self.asking = Some(id.to_string());
        self.reveal.set(true);
    }

    /// The screen's first row in view moves to `to`.
    pub fn show(&mut self, to: usize) {
        self.top.set(to);
    }
}

/// What a conversation is called: the start of its first message.
fn title(chat: &Saved) -> &str {
    chat.title.as_deref().unwrap_or("Untitled")
}

/// A conversation's row: its title, and on the right when it was last
/// written to, or the check of the one that is open. Lit (it is chosen, or
/// under the pointer), what deletes it stands before that; asked once
/// more, the button that does.
fn row(chat: &Saved, history: &History, ui: &Ui, lit: bool, inner: usize) -> Row {
    let over = |spot: &Spot| ui.over.as_ref() == Some(spot);
    let forget = Spot::Forget(chat.id.clone());
    let asking = history.asking.as_ref() == Some(&chat.id);
    let when = match chat.updated {
        _ if chat.current => glyphs("✓", Ink::accent(), 0),
        Some(updated) => glyphs(&ago(ui.clock - updated), Ink::faint(), 0),
        None => Vec::new(),
    };
    // What a click deletes with, and the cells it takes with the page's around it.
    let delete: Vec<Glyph> = if chat.current {
        Vec::new()
    } else if asking {
        let ink = Ink::danger().on(DANGER, if over(&forget) { 0.3 } else { 0.2 });
        glyphs(" Delete ", ink, 0)
    } else if lit {
        let ink = if over(&forget) {
            Ink::text()
        } else {
            Ink::faint()
        };
        glyphs("Delete", ink, 0)
    } else {
        Vec::new()
    };
    let right = match (delete.is_empty(), when.is_empty() || asking) {
        (true, _) => when,
        (false, true) => delete.clone(),
        (false, false) => {
            let mut right = delete.clone();
            right.extend(glyphs("   ", Ink::faint(), 0));
            right.extend(when);
            right
        }
    };
    let room = inner.saturating_sub(4 + width(&right) + if right.is_empty() { 0 } else { 2 });
    let ink = match (&chat.title, lit) {
        (None, _) => Ink::faint(),
        (Some(_), true) => Ink::text(),
        (Some(_), false) => Ink::of(INK, 0.78),
    };
    let mut text = fit(glyphs(title(chat), ink, 0), room);
    let at = inner.saturating_sub(2 + width(&right));
    text.extend(glyphs(
        &" ".repeat(at.saturating_sub(2 + width(&text))),
        Ink::faint(),
        0,
    ));
    text.extend(right);
    let mut line = Line::at(2, text);
    if lit {
        line = line.on(RAISED, 0, inner);
    }
    let open = Spot::Chat(chat.id.clone());
    if delete.is_empty() {
        return Row::of(line).on(0, inner, open);
    }
    let to = at + width(&delete) + 1;
    Row::of(line)
        .on(0, at.saturating_sub(1), open.clone())
        .on(at.saturating_sub(1), to, forget)
        .on(to, inner, open)
}

/// Draws the conversations' screen under the wordmark: its title, a click
/// on which leaves it, the filter once they are many, then a row each.
pub fn draw(canvas: &mut Canvas, chat: &Chat, ui: &Ui, history: &History, now: f64) -> Screen {
    let inner = usize::from(connect::column(canvas));
    let mut rows = Vec::new();
    let mut seen = None;
    let query = history.query.text();
    let saved = chat.history.as_deref();
    if saved.is_some_and(|saved| saved.len() >= SEARCH_FROM) || !query.is_empty() {
        rows.extend(field(
            &history.query,
            "Search conversations",
            false,
            true,
            0,
            inner,
        ));
        rows.push(Row::blank());
    }
    match saved {
        None if chat.history_error.is_none() => {
            let mut wait = glyphs(&spinner(now).to_string(), Ink::accent(), 0);
            wait.extend(glyphs(" Reading the conversations", Ink::muted(), 0));
            rows.push(Row::of(Line::of(wait)));
        }
        None => {}
        Some([]) => {
            let none = "No conversation yet in this folder.";
            rows.extend(words(none, Ink::muted(), 0, inner));
        }
        Some(saved) => {
            let shown = history.shown(saved);
            if shown.is_empty() {
                let none = format!("No conversation matches “{}”.", query.trim());
                rows.extend(words(&none, Ink::muted(), 0, inner));
            }
            let on = history.on(saved).map(|chat| &chat.id);
            for chat in shown {
                let chosen = Some(&chat.id) == on;
                let over =
                    matches!(&ui.over, Some(Spot::Chat(id) | Spot::Forget(id)) if *id == chat.id);
                if chosen {
                    seen = Some((rows.len(), rows.len() + 1));
                }
                rows.push(row(chat, history, ui, chosen || over, inner));
            }
        }
    }
    if let Some(error) = &chat.history_error {
        if !rows.is_empty() {
            rows.push(Row::blank());
        }
        rows.extend(words(error, Ink::danger(), 0, inner));
    }
    let place = (&history.top, &history.reveal);
    connect::page(canvas, ui, "History", rows, seen, place)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // How a moment could be read wrong: its day off by one around the end
    // of February, in a leap year or in a century that is none (2100); the
    // hours taken for the user's, not the agent's; no fraction of a second,
    // or one, breaking it; a moment before 1970 going wrong way; what is no
    // moment (no `Z`, a 13th month, words) read as one.
    #[test]
    fn a_moment_is_its_seconds_since_1970() {
        for (moment, at) in [
            ("1970-01-01T00:00:00.000Z", Some(0)),
            ("2000-03-01T00:00:00Z", Some(951_868_800)),
            ("2024-02-29T12:00:00.000Z", Some(1_709_208_000)),
            ("2026-11-20T08:15:30.250Z", Some(1_795_162_530)),
            ("1969-12-31T23:59:59.000Z", Some(-1)),
            ("2100-03-01T00:00:00.000Z", Some(4_107_542_400)),
            ("2026-11-20T08:15:30+02:00", None),
            ("2026-13-20T08:15:30Z", None),
            ("2026-11-20", None),
            ("yesterday", None),
            ("", None),
        ] {
            assert_eq!(seconds(moment), at, "{moment}");
        }
    }

    // How an age could read wrong: a file dated after now (two clocks) as
    // a negative count; 59 seconds as `0 min ago`; an hour as `60 min ago`;
    // a day as `24 h ago` or `1 days ago`; 13 days as `1 weeks ago`; two
    // months as `8 weeks ago` for ever; two years as `24 months ago`.
    #[test]
    fn an_age_is_said_in_its_largest_unit() {
        let (minute, hour, day) = (60, 3_600, 86_400);
        for (age, said) in [
            (-120, "just now"),
            (0, "just now"),
            (59, "just now"),
            (minute, "1 min ago"),
            (hour - 1, "59 min ago"),
            (hour, "1 h ago"),
            (day - 1, "23 h ago"),
            (day, "yesterday"),
            (2 * day - 1, "yesterday"),
            (2 * day, "2 days ago"),
            (13 * day, "13 days ago"),
            (14 * day, "2 weeks ago"),
            (59 * day, "8 weeks ago"),
            (60 * day, "2 months ago"),
            (729 * day, "24 months ago"),
            (730 * day, "2 years ago"),
            (4_000 * day, "10 years ago"),
        ] {
            assert_eq!(ago(age), said, "{age}");
        }
    }

    // How the list could be read wrong: a row without its file, which no
    // agent could start on, kept; a title on several lines, or with the
    // links of its mentions, shown as it is stored; an empty title shown
    // empty; `current` read from the wrong place; the order changed.
    #[test]
    fn the_conversations_are_read_as_listed() {
        let result = json!({ "sessions": [
            { "sessionId": "b", "title": "Fix the\n  tests", "updatedAt": "1970-01-01T00:01:00.000Z",
              "_meta": { "unharnessed": { "path": "/s/b.jsonl", "current": true } } },
            { "sessionId": "a", "title": "  ", "_meta": { "unharnessed": { "path": "/s/a.jsonl" } } },
            { "sessionId": "nowhere", "title": "No file" },
        ] });
        let saved = listed(&result);
        assert_eq!(saved.len(), 2);
        assert_eq!(saved[0].title.as_deref(), Some("Fix the tests"));
        assert_eq!((saved[0].updated, saved[0].current), (Some(60), true));
        assert_eq!((saved[1].id.as_str(), &saved[1].title), ("a", &None));
        assert_eq!((saved[1].updated, saved[1].current), (None, false));
    }

    fn saved(id: &str, title: &str, current: bool) -> Saved {
        Saved {
            id: id.to_string(),
            title: Some(title.to_string()),
            updated: None,
            path: format!("/s/{id}.jsonl"),
            current,
        }
    }

    // How Enter could open the wrong row: the one that is open already,
    // first in the list; one the filter hides, chosen before it was typed;
    // none, at the list's ends, where the arrows went past them.
    #[test]
    fn enter_is_on_the_chosen_row_or_the_first_that_is_not_open() {
        let list = [
            saved("now", "The open one", true),
            saved("x", "Fix the tests", false),
            saved("y", "Write the docs", false),
        ];
        let mut history = History::default();
        assert_eq!(history.on(&list).map(|chat| chat.id.as_str()), Some("x"));
        history.step(1, &list);
        assert_eq!(history.chosen.as_deref(), Some("y"));
        history.step(5, &list);
        assert_eq!(history.chosen.as_deref(), Some("y"));
        history.step(-5, &list);
        assert_eq!(history.chosen.as_deref(), Some("now"));
        history.chosen = Some("y".to_string());
        history.query.insert("FIX");
        assert_eq!(history.on(&list).map(|chat| chat.id.as_str()), Some("x"));
        history.query.insert(" nothing");
        assert!(history.on(&list).is_none());
        // Alone, the open one is all Enter can be on: it leaves the screen.
        let alone = History::default();
        assert_eq!(
            alone.on(&list[..1]).map(|chat| chat.id.as_str()),
            Some("now")
        );
    }
}
