//! The message being written in the message box: its text, the caret, and
//! its rows as the box shows them. The caret always stands between two
//! grapheme clusters; a row keeps every character of the text (a space a row
//! breaks at stays on it), so the caret has a place wherever it stands.

use std::cell::Cell;

use unicode_segmentation::UnicodeSegmentation;

use crate::text::{clean, glyphs, Glyph, Ink};

/// The most a message holds, in bytes: what is written or pasted past it is left out.
const LIMIT: usize = 256 * 1024;

#[derive(Clone, Default)]
pub struct Draft {
    text: String,
    /// The caret's byte in `text`.
    caret: usize,
    /// The cell Up and Down aim for, kept while they follow each other.
    aim: Option<usize>,
    /// The first row the box shows, as the last layout left it.
    first: Cell<usize>,
}

/// The message as the box shows it.
pub struct Laid {
    /// The rows in view.
    pub rows: Vec<Vec<Glyph>>,
    /// The caret's row among them, and its cell there.
    pub caret: (usize, usize),
}

/// Rows of at most `width` cells holding every glyph, each row with the
/// byte it starts at: broken after a line break (which stays on its row),
/// before a word that does not fit, inside a word or a run of spaces longer
/// than a row.
fn rows(glyphs: Vec<Glyph>, width: usize) -> Vec<(u32, Vec<Glyph>)> {
    let mut done: Vec<(u32, Vec<Glyph>)> = Vec::new();
    let mut row: Vec<Glyph> = Vec::new();
    let mut start = 0;
    let mut used = 0;
    // Where the row's last word starts, when something comes before it.
    let mut word: Option<usize> = None;
    for glyph in glyphs {
        let cells = usize::from(glyph.width);
        if cells > width {
            continue;
        }
        if used + cells > width {
            let tail = match word {
                Some(at) if !glyph.is_space() => row.split_off(at),
                _ => Vec::new(),
            };
            let next = tail.first().map_or(glyph.src, |first| first.src);
            done.push((start, std::mem::take(&mut row)));
            start = next;
            used = tail.iter().map(|glyph| usize::from(glyph.width)).sum();
            row = tail;
            word = None;
            // The word brought down fills its row: it breaks where it is.
            if used + cells > width {
                done.push((start, std::mem::take(&mut row)));
                start = glyph.src;
                used = 0;
            }
        }
        if glyph.is_break() {
            let next = glyph.src + 1;
            row.push(glyph);
            done.push((start, std::mem::take(&mut row)));
            start = next;
            used = 0;
            word = None;
            continue;
        }
        if !glyph.is_space() && row.last().is_some_and(Glyph::is_space) {
            word = Some(row.len());
        }
        used += cells;
        row.push(glyph);
    }
    done.push((start, row));
    done
}

/// The byte of the edge nearest before cell `aim` of a row: where the
/// caret stands there.
fn near((start, glyphs): &(u32, Vec<Glyph>), aim: usize) -> usize {
    let mut at = *start;
    let mut used = 0;
    for glyph in glyphs {
        if glyph.is_break() || used + usize::from(glyph.width) > aim {
            break;
        }
        used += usize::from(glyph.width);
        at = glyph.src + u32::try_from(glyph.len()).unwrap_or(1);
    }
    at as usize
}

impl Draft {
    /// What is written.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// The message as written, the box left empty.
    pub fn take(&mut self) -> String {
        std::mem::take(self).text
    }

    /// `text` as if it had just been written: the caret at its end.
    #[must_use]
    pub fn holding(text: &str) -> Draft {
        let mut draft = Draft::default();
        draft.insert(text);
        draft
    }

    /// The caret's byte in what is written.
    #[must_use]
    pub fn caret(&self) -> usize {
        self.caret
    }

    /// Bytes `from..to` go, the caret where they were; nothing goes if
    /// they are not a stretch of the text.
    pub fn cut(&mut self, from: usize, to: usize) {
        if from <= to && self.text.get(from..to).is_some() {
            self.remove(from, to);
        }
    }

    /// What was typed or pasted goes in at the caret, made safe to show: a
    /// paste's line ends (CR, CR LF) are line breaks.
    pub fn insert(&mut self, text: &str) {
        let mut text = clean(&text.replace("\r\n", "\n").replace('\r', "\n"));
        let mut room = LIMIT.saturating_sub(self.text.len()).min(text.len());
        while !text.is_char_boundary(room) {
            room -= 1;
        }
        text.truncate(room);
        self.text.insert_str(self.caret, &text);
        self.caret += text.len();
        self.settle();
    }

    /// The caret back on a cluster's edge: what is typed may join the
    /// cluster before it (a combining mark, a joiner).
    fn settle(&mut self) {
        self.aim = None;
        while self.caret < self.text.len()
            && !self
                .text
                .grapheme_indices(true)
                .any(|(at, _)| at == self.caret)
        {
            self.caret += 1;
        }
    }

    /// The edge before the caret, and the one after it.
    fn before(&self) -> usize {
        self.text[..self.caret]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(at, _)| at)
    }

    fn after(&self) -> usize {
        self.text[self.caret..]
            .graphemes(true)
            .next()
            .map_or(self.caret, |cluster| self.caret + cluster.len())
    }

    /// Where the word before the caret starts: past the spaces, then the word.
    fn word_before(&self) -> usize {
        let head = &self.text[..self.caret];
        let word = head.trim_end_matches(char::is_whitespace);
        word.rfind(char::is_whitespace).map_or(0, |at| {
            at + word[at..].chars().next().map_or(1, char::len_utf8)
        })
    }

    /// Where the word after the caret ends: past the spaces, then the word.
    fn word_after(&self) -> usize {
        let tail = &self.text[self.caret..];
        let word = tail.trim_start_matches(char::is_whitespace);
        let skipped = tail.len() - word.len();
        self.caret + skipped + word.find(char::is_whitespace).unwrap_or(word.len())
    }

    fn remove(&mut self, from: usize, to: usize) {
        self.text.replace_range(from..to, "");
        self.caret = from;
        self.settle();
    }

    fn go(&mut self, to: usize) {
        self.caret = to;
        self.aim = None;
    }

    pub fn backspace(&mut self) {
        self.remove(self.before(), self.caret);
    }

    pub fn delete(&mut self) {
        self.remove(self.caret, self.after());
    }

    pub fn delete_word(&mut self) {
        self.remove(self.word_before(), self.caret);
    }

    /// Its line goes, from its start to the caret.
    pub fn delete_to_start(&mut self) {
        self.remove(self.line_start(), self.caret);
    }

    pub fn left(&mut self) {
        self.go(self.before());
    }

    pub fn right(&mut self) {
        self.go(self.after());
    }

    pub fn word_left(&mut self) {
        self.go(self.word_before());
    }

    pub fn word_right(&mut self) {
        self.go(self.word_after());
    }

    fn line_start(&self) -> usize {
        self.text[..self.caret].rfind('\n').map_or(0, |at| at + 1)
    }

    /// To the start of the caret's line, and to its end.
    pub fn home(&mut self) {
        self.go(self.line_start());
    }

    pub fn end(&mut self) {
        let end = self.text[self.caret..]
            .find('\n')
            .map_or(self.text.len(), |at| self.caret + at);
        self.go(end);
    }

    /// The caret's row among `rows`, and its cell there.
    fn place(&self, rows: &[(u32, Vec<Glyph>)]) -> (usize, usize) {
        let caret = u32::try_from(self.caret).unwrap_or(u32::MAX);
        let row = rows
            .iter()
            .rposition(|(start, _)| *start <= caret)
            .unwrap_or(0);
        let cell = rows[row]
            .1
            .iter()
            .take_while(|glyph| glyph.src < caret)
            .map(|glyph| usize::from(glyph.width))
            .sum();
        (row, cell)
    }

    /// The caret a row up (`by` -1) or down (1) in a box `width` cells
    /// wide, near the cell it started from; answers whether there was a row
    /// to go to.
    pub fn vertical(&mut self, by: isize, width: usize) -> bool {
        let rows = rows(glyphs(&self.text, Ink::text(), 0), width.max(2) - 1);
        let (row, cell) = self.place(&rows);
        let Some(to) = row.checked_add_signed(by).filter(|to| *to < rows.len()) else {
            return false;
        };
        let aim = self.aim.unwrap_or(cell);
        self.caret = near(&rows[to], aim);
        self.aim = Some(aim);
        true
    }

    /// The caret where a click landed: cell `cell` of the `row`th row the
    /// box shows, `width` cells wide; past the last row, on the last.
    pub fn point(&mut self, row: usize, cell: usize, width: usize) {
        let rows = rows(glyphs(&self.text, Ink::text(), 0), width.max(2) - 1);
        let row = (self.first.get() + row).min(rows.len() - 1);
        self.go(near(&rows[row], cell));
    }

    /// The message in a box `width` cells wide that shows `most` rows: the
    /// rows in view (the caret's among them), written in `ink`. A cell is
    /// kept at each row's end for the caret.
    #[must_use]
    pub fn lay(&self, width: usize, most: usize, ink: Ink) -> Laid {
        let rows = rows(glyphs(&self.text, ink, 0), width.max(2) - 1);
        let (row, cell) = self.place(&rows);
        let most = most.max(1);
        let last = rows.len().saturating_sub(most);
        let first = self
            .first
            .get()
            .min(last)
            .min(row)
            .max((row + 1).saturating_sub(most));
        self.first.set(first);
        Laid {
            rows: rows
                .into_iter()
                .skip(first)
                .take(most)
                .map(|(_, mut glyphs)| {
                    glyphs.retain(|glyph| !glyph.is_break());
                    glyphs
                })
                .collect(),
            caret: (row - first, cell),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{rows, Draft};
    use crate::text::{glyphs, width, Ink};

    // Listed before the code, how the message box's text can fail:
    // - the caret ends inside a cluster (a letter and its accent, a family
    //   emoji, a flag) after a move or a removal, and the next edit panics
    //   or leaves half of it;
    // - a move or a removal at the text's ends panics;
    // - what is typed in the middle lands elsewhere, or the caret does not
    //   follow it; a mark typed after a letter leaves the caret inside their cluster;
    // - a paste loses text at its line ends (CR, CR LF), or lets an escape
    //   sequence or a control character in;
    // - a word move stops on the spaces, or passes a line's end wrongly;
    // - home and end leave the caret's line;
    // - a row loses a character (the space it breaks at), is wider than the
    //   box, or the caret has no cell (after a full row, after a last line
    //   break, in a box one or two cells wide, before a wide character);
    // - Up and Down forget their cell through a shorter row, or say they
    //   moved on the first and last rows;
    // - the box does not follow the caret out of its rows, either way;
    // - a paste past the limit cuts a character in two.

    fn draft(text: &str) -> Draft {
        let mut draft = Draft::default();
        draft.insert(text);
        draft
    }

    #[test]
    fn the_caret_keeps_to_the_clusters_edges() {
        let family = "👨\u{200d}👩\u{200d}👧";
        let mut d = draft(&format!("e\u{301}{family}🇫🇷x"));
        d.left();
        d.left();
        assert_eq!(&d.text()[d.caret..], "🇫🇷x");
        d.backspace();
        assert_eq!(d.text(), "e\u{301}🇫🇷x");
        d.backspace();
        assert_eq!(d.text(), "🇫🇷x");
        d.backspace();
        d.left();
        d.delete();
        assert_eq!(d.text(), "x");
        d.delete();
        d.delete();
        d.right();
        d.backspace();
        d.backspace();
        d.word_left();
        d.word_right();
        d.delete_word();
        d.delete_to_start();
        assert_eq!((d.text(), d.caret), ("", 0));
        // A mark typed after its letter joins it: the caret is after both.
        let mut d = draft("ab");
        d.left();
        d.insert("\u{301}");
        assert_eq!((d.text(), d.caret), ("a\u{301}b", 3));
        d.insert("X");
        assert_eq!(d.text(), "a\u{301}Xb");
    }

    #[test]
    fn a_paste_keeps_its_lines_and_nothing_else() {
        let d = draft("one\r\ntwo\rthree\x1b[31m red\x1b]0;title\x07\x00\ttab");
        assert_eq!(d.text(), "one\ntwo\nthree red   tab");
        let mut d = draft("ad");
        d.left();
        d.insert("b\nc");
        assert_eq!((d.text(), &d.text()[d.caret..]), ("ab\ncd", "d"));
    }

    #[test]
    fn words_and_lines() {
        let mut d = draft("alpha  beta\n  gamma");
        d.word_left();
        assert_eq!(&d.text()[d.caret..], "gamma");
        d.word_left();
        assert_eq!(&d.text()[d.caret..], "beta\n  gamma");
        d.word_left();
        d.word_left();
        assert_eq!(d.caret, 0);
        d.word_right();
        assert_eq!(&d.text()[d.caret..], "  beta\n  gamma");
        d.word_right();
        d.word_right();
        d.word_right();
        assert_eq!(d.caret, d.text().len());
        d.home();
        assert_eq!(&d.text()[d.caret..], "  gamma");
        d.left();
        d.home();
        assert_eq!(d.caret, 0);
        d.end();
        assert_eq!(&d.text()[d.caret..], "\n  gamma");
        d.delete_word();
        assert_eq!(d.text(), "alpha  \n  gamma");
        d.delete_to_start();
        assert_eq!(d.text(), "\n  gamma");
    }

    #[test]
    fn rows_keep_every_character_within_the_box() {
        let text = "one two   three\n\nsupercalifragilistic 漢字かな end \n";
        for wide in 1..24 {
            let laid = rows(glyphs(text, Ink::text(), 0), wide);
            let kept: usize = laid.iter().map(|(_, row)| row.len()).sum();
            let fit = glyphs(text, Ink::text(), 0)
                .iter()
                .filter(|glyph| usize::from(glyph.width) <= wide)
                .count();
            assert_eq!(kept, fit, "a box {wide} wide lost a character");
            assert!(
                laid.iter().all(|(_, row)| width(row) <= wide),
                "a row is wider than {wide}"
            );
            let starts: Vec<u32> = laid.iter().map(|(start, _)| *start).collect();
            assert!(
                starts.is_sorted(),
                "the rows of a box {wide} wide are out of order"
            );
        }
        // A word that does not fit goes down whole; the space before it stays up.
        let laid = rows(glyphs("ab cd", Ink::text(), 0), 4);
        let shown: Vec<String> = laid
            .iter()
            .map(|(_, row)| row.iter().map(|glyph| glyph.ch).collect())
            .collect();
        assert_eq!(shown, ["ab ", "cd"]);
    }

    #[test]
    fn the_caret_always_has_a_cell() {
        for (text, wide) in [
            ("abcd", 5),
            ("abcd\n", 5),
            ("", 1),
            ("a", 2),
            ("漢字", 3),
            ("ab cd ef", 4),
        ] {
            let mut d = draft(text);
            loop {
                let laid = d.lay(wide, 2, Ink::text());
                let (row, cell) = laid.caret;
                assert!(
                    row < laid.rows.len().max(1) && row < 2,
                    "{text:?}: the caret's row is out of view"
                );
                assert!(
                    cell < wide.max(2),
                    "{text:?} in {wide}: the caret is past the box"
                );
                if d.caret == 0 {
                    break;
                }
                d.left();
            }
        }
        // After a last line break, the caret is on a row of its own.
        let d = draft("ab\n");
        let laid = d.lay(10, 5, Ink::text());
        assert_eq!((laid.rows.len(), laid.caret), (2, (1, 0)));
    }

    #[test]
    fn up_and_down_keep_their_cell() {
        let mut d = draft("0123456789\nab\n0123456789");
        d.left();
        d.left();
        assert!(d.vertical(-1, 40));
        assert_eq!(&d.text()[d.caret..], "\n0123456789");
        assert!(d.vertical(-1, 40));
        assert_eq!(&d.text()[d.caret..], "89\nab\n0123456789");
        assert!(!d.vertical(-1, 40));
        assert!(d.vertical(1, 40));
        assert!(d.vertical(1, 40));
        assert_eq!(&d.text()[d.caret..], "89");
        assert!(!d.vertical(1, 40));
        // Through the rows of one long line too.
        let mut d = draft("aaaa bbbb cccc");
        assert!(d.vertical(-1, 6));
        assert_eq!(&d.text()[d.caret..], " cccc");
    }

    #[test]
    fn the_box_follows_the_caret() {
        let mut d = draft("1\n2\n3\n4\n5");
        let shown = |d: &Draft| -> String {
            let laid = d.lay(10, 2, Ink::text());
            laid.rows.iter().flatten().map(|glyph| glyph.ch).collect()
        };
        assert_eq!(shown(&d), "45");
        d.vertical(-1, 10);
        assert_eq!(shown(&d), "45");
        d.vertical(-1, 10);
        assert_eq!(shown(&d), "34");
        d.vertical(1, 10);
        assert_eq!(shown(&d), "34");
        d.vertical(1, 10);
        assert_eq!(shown(&d), "45");
    }

    #[test]
    fn a_paste_past_the_limit_is_cut_between_characters() {
        let mut d = draft(&"é".repeat(super::LIMIT / 2 - 1));
        d.insert("aéé");
        assert_eq!(d.text().len(), super::LIMIT - 1);
        assert!(d.text().ends_with("éa"));
    }

    // How a click in the message box, and a stretch taken out, can fail:
    // - a click past a row's end, or under the last row, leaves the caret
    //   where it was or panics;
    // - a click on the second cell of a wide character puts the caret inside it;
    // - in a box that scrolled, the click counts its rows from the text's first;
    // - a stretch that ends inside a character is cut, and the text is no longer one.
    #[test]
    fn a_click_puts_the_caret_where_it_lands() {
        let mut d = draft("one two\n漢字 end\nlast");
        let rest = |d: &Draft| d.text()[d.caret..].to_string();
        d.point(0, 2, 20);
        assert_eq!(rest(&d), "e two\n漢字 end\nlast");
        d.point(0, 90, 20);
        assert_eq!(rest(&d), "\n漢字 end\nlast");
        d.point(1, 1, 20);
        assert_eq!(rest(&d), "漢字 end\nlast");
        d.point(1, 2, 20);
        assert_eq!(rest(&d), "字 end\nlast");
        d.point(9, 2, 20);
        assert_eq!(rest(&d), "st");
        // Two rows in view, the caret on the last: the box shows the last two.
        d.end();
        let _ = d.lay(20, 2, Ink::text());
        d.point(0, 0, 20);
        assert_eq!(rest(&d), "漢字 end\nlast");
        let mut d = draft("aé");
        d.cut(1, 2);
        d.cut(2, 1);
        d.cut(0, 9);
        assert_eq!(d.text(), "aé");
        d.cut(1, 3);
        assert_eq!((d.text(), d.caret), ("a", 1));
        assert_eq!(Draft::holding("x\ny").caret, 3);
    }
}
