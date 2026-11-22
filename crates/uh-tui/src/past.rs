//! The messages sent before, for the message box to bring back: Up goes
//! to the one before, Down to the one after, and past the last what was
//! being written comes back.

#[derive(Clone, Debug, Default)]
pub struct Past {
    sent: Vec<String>,
    /// The one the message box shows, while it goes through them.
    at: Option<usize>,
    /// What was written when it started going through them.
    written: String,
}

impl Past {
    /// `message` was sent: it is the newest, unless the newest is the same.
    pub fn keep(&mut self, message: &str) {
        self.leave();
        if !message.is_empty() && self.sent.last().map(String::as_str) != Some(message) {
            self.sent.push(message.to_string());
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sent.is_empty()
    }

    /// The message box shows one of them, as it was sent.
    #[must_use]
    pub fn browsing(&self) -> bool {
        self.at.is_some()
    }

    /// The message before the one shown, `written` kept for the way back;
    /// `None` at the oldest.
    pub fn older(&mut self, written: &str) -> Option<&str> {
        let to = if let Some(at) = self.at {
            at.checked_sub(1)?
        } else {
            let newest = self.sent.len().checked_sub(1)?;
            self.written = written.to_string();
            newest
        };
        self.at = Some(to);
        Some(&self.sent[to])
    }

    /// The message after the one shown; past the newest, what was being
    /// written. `None` when none is shown.
    pub fn newer(&mut self) -> Option<String> {
        let to = self.at? + 1;
        if to < self.sent.len() {
            self.at = Some(to);
            return Some(self.sent[to].clone());
        }
        self.at = None;
        Some(std::mem::take(&mut self.written))
    }

    /// The message shown was changed: it is what is being written now.
    pub fn leave(&mut self) {
        self.at = None;
        self.written.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::Past;

    // Listed before the code, how the way back through the messages can fail:
    // - Up with nothing sent yet changes what is written;
    // - what was being written is lost once Up is pressed, or comes back
    //   changed after a round trip;
    // - Up at the oldest wraps around, or forgets where it was;
    // - Down while nothing is shown empties the message box;
    // - the same message sent twice in a row takes two steps;
    // - after a recalled message is changed, Up goes on from where it was
    //   and the change is lost without a way back.
    #[test]
    fn up_goes_back_through_the_messages_and_down_returns_to_what_was_written() {
        let mut past = Past::default();
        assert_eq!(past.older("draft"), None);
        assert_eq!(past.newer(), None);
        for message in ["one", "two", "two", "", "three"] {
            past.keep(message);
        }
        assert_eq!(past.newer(), None);
        assert_eq!(past.older("draft"), Some("three"));
        assert_eq!(past.older("three"), Some("two"));
        assert_eq!(past.older("two"), Some("one"));
        assert_eq!(past.older("one"), None);
        assert_eq!(past.newer().as_deref(), Some("two"));
        assert_eq!(past.newer().as_deref(), Some("three"));
        assert_eq!(past.newer().as_deref(), Some("draft"));
        assert_eq!(past.newer(), None);
    }

    #[test]
    fn a_recalled_message_that_is_changed_is_what_is_written() {
        let mut past = Past::default();
        past.keep("one");
        past.keep("two");
        assert_eq!(past.older(""), Some("two"));
        past.leave();
        assert_eq!(past.older("two, changed"), Some("two"));
        assert_eq!(past.older("two"), Some("one"));
        assert_eq!(past.newer().as_deref(), Some("two"));
        assert_eq!(past.newer().as_deref(), Some("two, changed"));
    }
}
