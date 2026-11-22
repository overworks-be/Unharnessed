//! What folds on the screen (a turn's work, cells in a row, a cell, the
//! reasoning) and which of them the user opened or closed. A fold moves
//! over the screen's own clock, so it opens the same over a replay that is
//! paused.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::motion::{progress, FOLD};

/// Something that folds, by its place in the conversation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fold {
    /// What a turn did before its answer.
    Work(usize),
    /// Cells in a row: the turn, and the first of them.
    Run(usize, usize),
    /// A cell or the reasoning: the turn, and its place there.
    Item(usize, usize),
    /// The message a turn starts with. It folds nothing: a click on it
    /// brings it back to the message box, to be changed.
    Prompt(usize),
    /// On the kernel's panel, a command's last lines and the start of a
    /// variable's value: the row's own number.
    Command(usize),
    Variable(usize),
}

impl Fold {
    /// The turn it is in, when it is in the conversation.
    #[must_use]
    pub fn turn(self) -> Option<usize> {
        match self {
            Fold::Work(turn) | Fold::Run(turn, _) | Fold::Item(turn, _) | Fold::Prompt(turn) => {
                Some(turn)
            }
            Fold::Command(_) | Fold::Variable(_) => None,
        }
    }
}

#[derive(Clone, Default)]
pub struct Folds {
    /// Everything in the conversation is open, but what the user set since.
    all: bool,
    /// What the user set: open or closed, and when.
    set: HashMap<Fold, (bool, f64)>,
}

impl Folds {
    /// Folds that start all open, or as the conversation leaves them.
    #[must_use]
    pub fn all(open: bool) -> Folds {
        Folds {
            all: open,
            set: HashMap::new(),
        }
    }

    /// Opening everything is for the conversation: the panel's rows open one by one.
    fn all_for(&self, fold: Fold) -> bool {
        self.all && !matches!(fold, Fold::Command(_) | Fold::Variable(_))
    }

    #[must_use]
    pub fn open(&self, fold: Fold) -> bool {
        self.set
            .get(&fold)
            .map_or_else(|| self.all_for(fold), |set| set.0)
    }

    /// The user decided for this fold: it no longer follows the conversation.
    #[must_use]
    pub fn held(&self, fold: Fold) -> bool {
        self.all_for(fold) || self.set.contains_key(&fold)
    }

    /// How open the fold is at `wall`, 0 to 1; `otherwise` where the user
    /// decided nothing.
    #[must_use]
    pub fn amount(&self, fold: Fold, wall: f64, otherwise: f64) -> f64 {
        match self.set.get(&fold) {
            Some(&(true, since)) => progress(wall, since, FOLD),
            Some(&(false, since)) => 1.0 - progress(wall, since, FOLD),
            None if self.all_for(fold) => 1.0,
            None => otherwise,
        }
    }

    pub fn toggle(&mut self, fold: Fold, wall: f64) {
        let open = self.open(fold);
        self.set.insert(fold, (!open, wall));
    }

    /// Everything in the conversation opens, or closes; the panel's rows stay as they are.
    pub fn toggle_all(&mut self) {
        self.all = !self.all;
        self.set
            .retain(|fold, _| matches!(fold, Fold::Command(_) | Fold::Variable(_)));
    }

    /// A fold of turn `turn` is on its way at `wall`.
    #[must_use]
    pub fn moving_in(&self, turn: usize, wall: f64) -> bool {
        self.set
            .iter()
            .any(|(fold, &(_, since))| fold.turn() == Some(turn) && wall - since < FOLD)
    }

    /// What is set for turn `turn`'s folds, as one number: the same while none of them changes.
    #[must_use]
    pub fn print(&self, turn: usize) -> u64 {
        let mut print = u64::from(self.all);
        for (fold, (open, _)) in &self.set {
            if fold.turn() == Some(turn) {
                let mut hasher = DefaultHasher::new();
                (fold, open).hash(&mut hasher);
                // Whatever the order they are met in.
                print = print.wrapping_add(hasher.finish() | 1);
            }
        }
        print
    }

    /// The conversation went back to before turn `turn`: what was set for
    /// that turn and the ones after is forgotten, others take their place.
    pub fn forget_from(&mut self, turn: usize) {
        self.set
            .retain(|fold, _| fold.turn().is_none_or(|of| of < turn));
    }

    /// A fold is on its way at `wall`.
    #[must_use]
    pub fn moving(&self, wall: f64) -> bool {
        self.set.values().any(|&(_, since)| wall - since < FOLD)
    }
}
