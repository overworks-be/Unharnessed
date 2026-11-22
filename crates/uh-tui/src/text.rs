//! Text as the screen takes it: one glyph per grapheme cluster with the
//! cells it fills, cleaned of what would drive the terminal, and broken into
//! rows.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::theme::{Rgb, ACCENT, DANGER, FAINT, INK, MUTED};

/// How a glyph is drawn. Its colour lies over what is under it at `alpha`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    pub rgb: Rgb,
    pub alpha: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// A tint behind it, over the surface (inline code, a port's pill).
    pub bg: Option<(Rgb, f32)>,
}

impl Ink {
    #[must_use]
    pub const fn of(rgb: Rgb, alpha: f32) -> Ink {
        Ink {
            rgb,
            alpha,
            bold: false,
            italic: false,
            underline: false,
            bg: None,
        }
    }

    #[must_use]
    pub const fn text() -> Ink {
        Ink::of(INK, 1.0)
    }

    #[must_use]
    pub const fn muted() -> Ink {
        Ink::of(INK, MUTED)
    }

    #[must_use]
    pub const fn faint() -> Ink {
        Ink::of(INK, FAINT)
    }

    #[must_use]
    pub const fn accent() -> Ink {
        Ink::of(ACCENT, 1.0)
    }

    #[must_use]
    pub const fn danger() -> Ink {
        Ink::of(DANGER, 1.0)
    }

    #[must_use]
    pub const fn bold(mut self) -> Ink {
        self.bold = true;
        self
    }

    #[must_use]
    pub const fn italic(mut self) -> Ink {
        self.italic = true;
        self
    }

    #[must_use]
    pub const fn alpha(mut self, alpha: f32) -> Ink {
        self.alpha = alpha;
        self
    }

    #[must_use]
    pub const fn on(mut self, rgb: Rgb, alpha: f32) -> Ink {
        self.bg = Some((rgb, alpha));
        self
    }
}

/// One grapheme cluster on the screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Glyph {
    pub ch: char,
    /// The rest of its cluster: combining marks, joiners.
    pub more: Option<Box<str>>,
    /// The cells it fills, 1 or 2; 0 for a line break.
    pub width: u8,
    pub ink: Ink,
    /// Its byte in the text it renders: the streaming wave times it by that.
    pub src: u32,
}

impl Glyph {
    #[must_use]
    pub fn is_break(&self) -> bool {
        self.ch == '\n'
    }

    #[must_use]
    pub fn is_space(&self) -> bool {
        self.ch == ' '
    }

    /// The bytes of the text it renders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ch.len_utf8() + self.more.as_ref().map_or(0, |more| more.len())
    }
}

/// `text`'s glyphs, their `src` counted from `src`. A cluster that fills no
/// cell is left out; `text` is expected clean ([`clean`]).
#[must_use]
pub fn glyphs(text: &str, ink: Ink, src: u32) -> Vec<Glyph> {
    let mut out = Vec::with_capacity(text.len());
    for (at, cluster) in text.grapheme_indices(true) {
        let at = src.saturating_add(u32::try_from(at).unwrap_or(u32::MAX));
        if cluster == "\n" || cluster == "\r\n" {
            out.push(Glyph {
                ch: '\n',
                more: None,
                width: 0,
                ink,
                src: at,
            });
            continue;
        }
        let width = UnicodeWidthStr::width(cluster).min(2);
        if width == 0 {
            continue;
        }
        let mut chars = cluster.chars();
        let Some(ch) = chars.next() else { continue };
        let rest = chars.as_str();
        out.push(Glyph {
            ch,
            more: (!rest.is_empty()).then(|| rest.into()),
            width: width as u8,
            ink,
            src: at,
        });
    }
    out
}

/// The cells `glyphs` fill.
#[must_use]
pub fn width(glyphs: &[Glyph]) -> usize {
    glyphs.iter().map(|glyph| usize::from(glyph.width)).sum()
}

/// Text that came from outside (a command's output, the model's words) made
/// safe to put on the screen: no escape sequence and no control character
/// reaches the terminal, a carriage return restarts its line as a terminal
/// would, a tab becomes spaces.
#[must_use]
pub fn clean(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    // A carriage return seen, not followed by a line feed so far.
    let mut returned = false;
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            returned = true;
            continue;
        }
        if returned {
            returned = false;
            if ch != '\n' {
                let line = out.rfind('\n').map_or(0, |at| at + 1);
                out.truncate(line);
            }
        }
        match ch {
            '\n' => out.push('\n'),
            '\t' => {
                let line = out.rfind('\n').map_or(0, |at| at + 1);
                let column = out[line..].chars().count();
                for _ in 0..4 - column % 4 {
                    out.push(' ');
                }
            }
            '\u{1b}' => skip_escape(&mut chars),
            // The C0 and C1 controls, and what reorders or hides text.
            '\u{0}'..='\u{1f}'
            | '\u{7f}'..='\u{9f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}' => {}
            _ => out.push(ch),
        }
    }
    out
}

/// What follows an ESC, to its end: a CSI sequence, a string (OSC, DCS and
/// the like, closed by BEL or ESC \), or a two- or three-character escape.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for ch in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&ch) {
                    break;
                }
            }
        }
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(ch) = chars.next() {
                if ch == '\u{7}' {
                    break;
                }
                if ch == '\u{1b}' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        // A character set's designation: one more character.
        Some('(' | ')' | '*' | '+' | '#' | '%' | ' ') => {
            chars.next();
        }
        _ => {}
    }
}

/// Rows of at most `width` cells: broken at spaces, inside a word longer
/// than a row, and at each line break. The spaces a row breaks at are left
/// out; those that start a line stay (an indent).
#[must_use]
pub fn wrap(glyphs: Vec<Glyph>, width: usize) -> Vec<Vec<Glyph>> {
    if width == 0 {
        return Vec::new();
    }
    let mut rows = Rows {
        width,
        done: Vec::new(),
        row: Vec::new(),
        used: 0,
        spaces: Vec::new(),
        word: Vec::new(),
        first: true,
    };
    for glyph in glyphs {
        if glyph.is_break() {
            rows.place();
            rows.end_row();
            rows.spaces.clear();
            rows.first = true;
        } else if glyph.is_space() {
            rows.place();
            rows.spaces.push(glyph);
        } else {
            rows.word.push(glyph);
        }
    }
    rows.place();
    rows.end_row();
    rows.done
}

/// The rows [`wrap`] builds.
struct Rows {
    width: usize,
    done: Vec<Vec<Glyph>>,
    row: Vec<Glyph>,
    used: usize,
    /// The spaces since the last word.
    spaces: Vec<Glyph>,
    /// The word being read.
    word: Vec<Glyph>,
    /// On a line's first row: the spaces that start it are its indent.
    first: bool,
}

impl Rows {
    fn end_row(&mut self) {
        self.done.push(std::mem::take(&mut self.row));
        self.used = 0;
    }

    /// One glyph, on the next row when this one is full; one wider than a row is left out.
    fn push(&mut self, glyph: Glyph) {
        let cells = usize::from(glyph.width);
        if cells > self.width {
            return;
        }
        if self.used + cells > self.width {
            self.end_row();
            self.first = false;
        }
        self.used += cells;
        self.row.push(glyph);
    }

    /// The word read, after the spaces before it, or on the next row without them.
    fn place(&mut self) {
        if self.word.is_empty() {
            return;
        }
        let spaces = std::mem::take(&mut self.spaces);
        let word = std::mem::take(&mut self.word);
        let fits = self.used + width(&spaces) + width(&word) <= self.width;
        if !fits && !self.row.is_empty() {
            self.end_row();
            self.first = false;
        } else if !self.row.is_empty() || self.first {
            for glyph in spaces {
                self.push(glyph);
            }
        }
        for glyph in word {
            self.push(glyph);
        }
    }
}

/// Rows of at most `width` cells, broken wherever a row is full and at each
/// line break: code keeps its columns.
#[must_use]
pub fn wrap_hard(glyphs: Vec<Glyph>, width: usize) -> Vec<Vec<Glyph>> {
    if width == 0 {
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut used = 0;
    for glyph in glyphs {
        if glyph.is_break() {
            rows.push(std::mem::take(&mut row));
            used = 0;
            continue;
        }
        let cells = usize::from(glyph.width);
        if cells > width {
            continue;
        }
        if used + cells > width {
            rows.push(std::mem::take(&mut row));
            used = 0;
        }
        used += cells;
        row.push(glyph);
    }
    rows.push(row);
    rows
}

/// `glyphs` on one row of at most `width` cells: cut, with an ellipsis, when
/// they do not fit. A line break ends them.
#[must_use]
pub fn fit(mut glyphs: Vec<Glyph>, width: usize) -> Vec<Glyph> {
    if let Some(at) = glyphs.iter().position(Glyph::is_break) {
        glyphs.truncate(at);
    }
    if self::width(&glyphs) <= width {
        return glyphs;
    }
    let Some(room) = width.checked_sub(1) else {
        return Vec::new();
    };
    let mut used = 0;
    let mut keep = 0;
    for glyph in &glyphs {
        if used + usize::from(glyph.width) > room {
            break;
        }
        used += usize::from(glyph.width);
        keep += 1;
    }
    let ink = glyphs.get(keep).map_or_else(Ink::faint, |glyph| glyph.ink);
    let src = glyphs.get(keep).map_or(0, |glyph| glyph.src);
    glyphs.truncate(keep);
    glyphs.push(Glyph {
        ch: '…',
        more: None,
        width: 1,
        ink: Ink { bg: None, ..ink },
        src,
    });
    glyphs
}

#[cfg(test)]
mod tests {
    use super::*;

    // What can go wrong, listed before the code was written: an escape
    // sequence reaching the terminal (colours, a title, a hyperlink, cursor
    // moves, one cut short at the end), a carriage return of a progress
    // line, a tab, a control or a text-reordering character.
    #[test]
    fn clean_keeps_the_terminal_out_of_reach() {
        for (dirty, clean_text) in [
            ("a\u{1b}[31mred\u{1b}[0m", "ared"),
            ("\u{1b}]0;title\u{7}x", "x"),
            ("\u{1b}]8;;http://a\u{1b}\\link\u{1b}]8;;\u{1b}\\", "link"),
            ("\u{1b}[2K\u{1b}[1Gline", "line"),
            ("abc\u{1b}", "abc"),
            ("abc\u{1b}[12", "abc"),
            ("\u{1b}(Bplain", "plain"),
            ("10%\r50%\r100%\n", "100%\n"),
            ("a\r\nb", "a\nb"),
            ("done\r", "done"),
            ("a\tb", "a   b"),
            ("x\n\ty", "x\n    y"),
            ("a\u{7}b\u{0}c\u{9b}d\u{7f}e", "abcde"),
            ("a\u{202e}b\u{feff}c", "abc"),
        ] {
            assert_eq!(clean(dirty), clean_text, "{dirty:?}");
        }
    }

    fn rows(text: &str, width: usize) -> Vec<String> {
        wrap(glyphs(text, Ink::text(), 0), width)
            .iter()
            .map(|row| {
                let mut text = String::new();
                for glyph in row {
                    text.push(glyph.ch);
                    text.push_str(glyph.more.as_deref().unwrap_or_default());
                }
                text
            })
            .collect()
    }

    // Listed before the code: a word longer than a row, a wide character
    // where one cell is left, a combining mark cut from its letter, the
    // spaces a row breaks at, an indent, an indent wider than the row, a
    // row of no cell or of one.
    #[test]
    fn wrap_breaks_where_it_should() {
        assert_eq!(rows("aaa bbb ccc", 7), ["aaa bbb", "ccc"]);
        assert_eq!(rows("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(rows("ab漢", 3), ["ab", "漢"]);
        assert_eq!(rows("aaa   bbb", 3), ["aaa", "bbb"]);
        assert_eq!(rows("  x y", 10), ["  x y"]);
        assert_eq!(rows("a\n\nb", 5), ["a", "", "b"]);
        assert_eq!(rows("e\u{301}x", 1), ["e\u{301}", "x"]);
        assert_eq!(rows("漢a", 1), ["a"]);
        assert!(rows("anything", 0).is_empty());
    }

    #[test]
    fn no_row_is_wider_than_asked() {
        let text = "      indented, a-very-long-word-without-any-space 漢字漢字漢字 e\u{301}\u{301} 👩‍👩‍👧 end\n\n  \n x";
        for cells in 1..=24 {
            for row in wrap(glyphs(text, Ink::text(), 0), cells) {
                assert!(
                    width(&row) <= cells,
                    "{cells} cells: a row of {}",
                    width(&row)
                );
            }
            for row in wrap_hard(glyphs(text, Ink::text(), 0), cells) {
                assert!(
                    width(&row) <= cells,
                    "{cells} cells, hard: a row of {}",
                    width(&row)
                );
            }
            assert!(width(&fit(glyphs(text, Ink::text(), 0), cells)) <= cells);
        }
    }
}
