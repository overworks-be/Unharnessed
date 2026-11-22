//! The conversation's goal (`/goal`), as the agent reports it on its
//! updates (`_meta.unharnessed.goal`): what it is after, where it stands,
//! and how long it has run. The message box shows it on top, as the
//! extension's does (`vscode/webview/Goal.tsx`): one reached or failed stays
//! until the next message.

use serde_json::Value;

use crate::text::clean;

/// Where a goal stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The agent goes on by itself until it is reached.
    Active,
    Paused,
    /// The agent cannot go on without the user: the goal says why.
    Blocked,
    /// It used what it was allowed to.
    Budget,
    Complete,
    Failed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Goal {
    pub status: Status,
    /// The status in the agent's word: what the conversation's line says.
    pub named: String,
    /// What it is after, on one line.
    pub objective: String,
    /// Why it stands there.
    pub reason: Option<String>,
    /// How long it had run when the agent last told of it, in seconds, if
    /// the agent dates it; and when that was, on the conversation's clock.
    ran: Option<f64>,
    told: f64,
    /// When the message box first showed it.
    pub shown: f64,
}

impl Goal {
    /// The goal an update tells of at `now`; none once it is cleared.
    /// `before`: the one the box shows, which this one may be still.
    #[must_use]
    pub fn read(now: f64, goal: &Value, before: Option<&Goal>) -> Option<Goal> {
        let text = |key: &str| goal.get(key).and_then(Value::as_str);
        let named = text("status")?;
        let status = match named {
            "active" => Status::Active,
            "paused" => Status::Paused,
            "blocked" => Status::Blocked,
            "budget_limited" => Status::Budget,
            "complete" => Status::Complete,
            "error" => Status::Failed,
            // `idle`, or a status of a later agent: nothing the box can show.
            _ => return None,
        };
        let objective = one_line(text("objective")?);
        if objective.is_empty() {
            return None;
        }
        let ms = |key: &str| goal.get(key).and_then(Value::as_f64);
        let ran = ms("createdAt").map(|created| {
            // Two clocks may disagree: it never ran less than no time.
            (ms("updatedAt").unwrap_or(created) - created).max(0.0) / 1000.0
        });
        // The same goal going on keeps its place in the box; another one comes in.
        let shown = before
            .filter(|before| before.objective == objective)
            .map_or(now, |before| before.shown);
        Some(Goal {
            status,
            named: named.to_string(),
            objective,
            reason: text("lastReason")
                .map(one_line)
                .filter(|why| !why.is_empty()),
            ran,
            told: now,
            shown,
        })
    }

    /// The agent goes on by itself.
    #[must_use]
    pub fn going(&self) -> bool {
        self.status == Status::Active
    }

    /// It waits for the user to let it go on.
    #[must_use]
    pub fn waits(&self) -> bool {
        matches!(
            self.status,
            Status::Paused | Status::Blocked | Status::Budget
        )
    }

    /// Reached or failed: it only stays to be read.
    #[must_use]
    pub fn over(&self) -> bool {
        matches!(self.status, Status::Complete | Status::Failed)
    }

    /// Where it stands, in a word or two.
    #[must_use]
    pub fn state(&self) -> &'static str {
        match self.status {
            Status::Active => "Goal",
            Status::Paused => "Paused",
            Status::Blocked => "Blocked",
            Status::Budget => "Budget reached",
            Status::Complete => "Done",
            Status::Failed => "Failed",
        }
    }

    /// What the box says beside where it stands: what it is after; blocked, why.
    #[must_use]
    pub fn says(&self) -> &str {
        match (&self.reason, self.status) {
            (Some(why), Status::Blocked) => why,
            _ => &self.objective,
        }
    }

    /// How long it has run at `now`, in whole seconds: it counts while the
    /// goal is active, and stands where it last changed otherwise. None
    /// when the agent does not date it.
    #[must_use]
    pub fn seconds(&self, now: f64) -> Option<u64> {
        let since = if self.going() {
            (now - self.told).max(0.0)
        } else {
            0.0
        };
        self.ran.map(|ran| (ran + since).floor() as u64)
    }
}

/// `4:07`, `1:04:07`.
#[must_use]
pub fn clock(seconds: u64) -> String {
    let (hours, minutes, rest) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{rest:02}")
    } else {
        format!("{minutes}:{rest:02}")
    }
}

/// A text of the agent's on one line of the box: its line ends are spaces.
fn one_line(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    clean(&words.join(" "))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    // How the box could show a goal wrong:
    // - a cleared goal (`idle`), one without an objective, or a status this
    //   screen does not know, stays in the box or shows as something else;
    // - a goal the agent set on several lines breaks the box's row;
    // - a paused goal's time goes on counting, or an active one's stands still;
    // - two clocks that disagree (changed before it was set) give a time
    //   below zero, which wraps to centuries;
    // - a goal the agent does not date shows a time made up;
    // - the same goal told again comes in again (its row flickers), or
    //   another goal takes the first one's place without coming in;
    // - a time reads wrong at the minute and at the hour (`0:60`, `1:0:07`).

    fn told(now: f64, goal: &Value, before: Option<&Goal>) -> Option<Goal> {
        Goal::read(now, goal, before)
    }

    #[test]
    fn a_goal_the_box_cannot_show_is_no_goal() {
        assert_eq!(
            told(0.0, &json!({"status": "idle", "tokensUsed": 0}), None),
            None
        );
        assert_eq!(
            told(0.0, &json!({"status": "idle", "objective": "ship"}), None),
            None
        );
        assert_eq!(told(0.0, &json!({"status": "active"}), None), None);
        assert_eq!(
            told(0.0, &json!({"status": "active", "objective": " \n "}), None),
            None
        );
        assert_eq!(
            told(
                0.0,
                &json!({"status": "dreaming", "objective": "ship"}),
                None
            ),
            None
        );
        assert_eq!(told(0.0, &json!({"objective": "ship"}), None), None);
    }

    #[test]
    fn what_it_is_after_stays_on_one_line() {
        let goal = told(
            0.0,
            &json!({"status": "blocked", "objective": "ship\n  the\tdocs", "lastReason": "no\nkey"}),
            None,
        )
        .unwrap();
        assert_eq!(goal.objective, "ship the docs");
        assert_eq!(goal.says(), "no key");
        assert_eq!(goal.state(), "Blocked");
    }

    #[test]
    fn only_a_blocked_goal_says_why_in_place_of_what_it_is_after() {
        for (status, says) in [
            ("active", "ship"),
            ("paused", "ship"),
            ("complete", "ship"),
            ("error", "ship"),
            ("blocked", "Paused by user"),
        ] {
            let goal =
                json!({"status": status, "objective": "ship", "lastReason": "Paused by user"});
            assert_eq!(told(0.0, &goal, None).unwrap().says(), says, "{status}");
        }
        let goal = json!({"status": "blocked", "objective": "ship"});
        assert_eq!(told(0.0, &goal, None).unwrap().says(), "ship");
    }

    #[test]
    fn its_time_counts_only_while_it_is_active() {
        let dated = |status: &str| json!({"status": status, "objective": "ship", "createdAt": 1_000_000, "updatedAt": 1_065_400});
        let active = told(10.0, &dated("active"), None).unwrap();
        assert_eq!(active.seconds(10.0), Some(65));
        assert_eq!(active.seconds(12.7), Some(68));
        // A frame dated after the screen's clock (a replay stepping back) takes nothing off.
        assert_eq!(active.seconds(3.0), Some(65));
        for status in ["paused", "blocked", "budget_limited", "complete", "error"] {
            let goal = told(10.0, &dated(status), None).unwrap();
            assert_eq!(goal.seconds(500.0), Some(65), "{status}");
        }
    }

    #[test]
    fn a_time_is_never_made_up_nor_below_zero() {
        let undated = json!({"status": "active", "objective": "ship", "updatedAt": 5});
        assert_eq!(told(0.0, &undated, None).unwrap().seconds(90.0), None);
        let backwards = json!({"status": "paused", "objective": "ship", "createdAt": 9_000, "updatedAt": 2_000});
        assert_eq!(told(0.0, &backwards, None).unwrap().seconds(90.0), Some(0));
        let just_set = json!({"status": "active", "objective": "ship", "createdAt": 9_000});
        assert_eq!(told(4.0, &just_set, None).unwrap().seconds(6.5), Some(2));
    }

    #[test]
    fn the_same_goal_keeps_its_place_and_another_comes_in() {
        let first = told(3.0, &json!({"status": "active", "objective": "ship"}), None).unwrap();
        let paused = json!({"status": "paused", "objective": "ship"});
        let same = told(8.0, &paused, Some(&first)).unwrap();
        assert!((same.shown - 3.0).abs() < f64::EPSILON, "{}", same.shown);
        let other = json!({"status": "active", "objective": "test"});
        let other = told(8.0, &other, Some(&first)).unwrap();
        assert!((other.shown - 8.0).abs() < f64::EPSILON, "{}", other.shown);
    }

    #[test]
    fn a_time_reads_as_a_clock() {
        for (seconds, text) in [
            (0, "0:00"),
            (7, "0:07"),
            (59, "0:59"),
            (60, "1:00"),
            (247, "4:07"),
            (3599, "59:59"),
            (3600, "1:00:00"),
            (3847, "1:04:07"),
            (360_000, "100:00:00"),
        ] {
            assert_eq!(clock(seconds), text);
        }
    }
}
