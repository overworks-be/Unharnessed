//! The screen as the views paint it: a grid of cells in true colours. It
//! goes to the terminal through ratatui's buffer (in the colours the
//! terminal has), or to a file as text, for the checks.

use std::fmt::Write as _;

use ratatui::buffer::Buffer;
use ratatui::style::{Modifier, Style};

use crate::text::Glyph;
use crate::theme::{Depth, Rgb, BG, INK};

/// How a cell's character is set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Marks {
    bold: bool,
    italic: bool,
    underline: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct Cell {
    ch: char,
    more: Option<Box<str>>,
    fg: Rgb,
    bg: Rgb,
    marks: Marks,
    /// The second cell of the wide glyph before it.
    tail: bool,
}

impl Cell {
    fn blank(bg: Rgb) -> Cell {
        Cell {
            ch: ' ',
            more: None,
            fg: INK,
            bg,
            marks: Marks::default(),
            tail: false,
        }
    }
}

#[derive(PartialEq)]
pub struct Canvas {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

impl Canvas {
    /// A page of `width` by `height` cells.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Canvas {
        Canvas {
            width,
            height,
            cells: vec![Cell::blank(BG); usize::from(width) * usize::from(height)],
        }
    }

    fn at(&self, x: u16, y: u16) -> Option<usize> {
        (x < self.width && y < self.height)
            .then(|| usize::from(y) * usize::from(self.width) + usize::from(x))
    }

    /// Whatever wide glyph lies over the cell goes: both its cells come back blank.
    fn clear(&mut self, x: u16, y: u16) {
        let Some(at) = self.at(x, y) else { return };
        if self.cells[at].tail && x > 0 {
            let bg = self.cells[at - 1].bg;
            self.cells[at - 1] = Cell::blank(bg);
        }
        if self.at(x + 1, y).is_some_and(|next| self.cells[next].tail) {
            let bg = self.cells[at + 1].bg;
            self.cells[at + 1] = Cell::blank(bg);
        }
        let bg = self.cells[at].bg;
        self.cells[at] = Cell::blank(bg);
    }

    /// Cells `from..to` of row `y` come back blank, the page behind them:
    /// what is drawn there next lies over what was.
    pub fn wipe(&mut self, y: u16, from: u16, to: u16) {
        for x in from..to.min(self.width) {
            self.clear(x, y);
            if let Some(at) = self.at(x, y) {
                self.cells[at] = Cell::blank(BG);
            }
        }
    }

    /// Cells `from..to` of row `y` take `rgb` behind them, at `alpha` over what was there.
    pub fn fill(&mut self, y: u16, from: u16, to: u16, rgb: Rgb, alpha: f32) {
        for x in from..to.min(self.width) {
            if let Some(at) = self.at(x, y) {
                self.cells[at].bg = self.cells[at].bg.mix(rgb, alpha);
            }
        }
    }

    /// `glyphs` on row `y` from `x`, as far as `end` (not included); what
    /// does not fit is not drawn. `alpha` scales every glyph's own.
    pub fn put(&mut self, x: u16, y: u16, end: u16, glyphs: &[Glyph], alpha: f32) {
        let end = end.min(self.width);
        let mut x = x;
        for glyph in glyphs {
            let cells = u16::from(glyph.width);
            if cells == 0 {
                continue;
            }
            if x + cells > end {
                break;
            }
            self.clear(x, y);
            if cells == 2 {
                self.clear(x + 1, y);
            }
            let Some(at) = self.at(x, y) else { break };
            if let Some((tint, strength)) = glyph.ink.bg {
                for cell in &mut self.cells[at..at + usize::from(cells)] {
                    cell.bg = cell.bg.mix(tint, strength * alpha);
                }
            }
            let cell = &mut self.cells[at];
            cell.ch = glyph.ch;
            cell.more.clone_from(&glyph.more);
            cell.fg = cell.bg.mix(glyph.ink.rgb, glyph.ink.alpha * alpha);
            cell.marks = Marks {
                bold: glyph.ink.bold,
                italic: glyph.ink.italic,
                underline: glyph.ink.underline,
            };
            if cells == 2 {
                self.cells[at + 1].tail = true;
            }
            x += cells;
        }
    }

    /// The screen into ratatui's buffer, in the colours the terminal has.
    pub fn render(&self, buffer: &mut Buffer, depth: Depth) {
        let area = buffer.area;
        for y in 0..self.height.min(area.height) {
            for x in 0..self.width.min(area.width) {
                let Some(at) = self.at(x, y) else { continue };
                let cell = &self.cells[at];
                let Some(target) = buffer.cell_mut((area.x + x, area.y + y)) else {
                    continue;
                };
                if cell.tail {
                    // Hidden by the wide glyph before it, as ratatui expects.
                    target.reset();
                    continue;
                }
                let mut modifier = Modifier::empty();
                modifier.set(Modifier::BOLD, cell.marks.bold);
                modifier.set(Modifier::ITALIC, cell.marks.italic);
                modifier.set(Modifier::UNDERLINED, cell.marks.underline);
                let mut symbol = String::from(cell.ch);
                if let Some(more) = &cell.more {
                    symbol.push_str(more);
                }
                target.set_symbol(&symbol);
                target.set_style(
                    Style::new()
                        .fg(depth.color(cell.fg))
                        .bg(depth.color(cell.bg))
                        .add_modifier(modifier),
                );
            }
        }
    }

    /// The screen as plain text, a line per row, without the spaces that end a row.
    #[must_use]
    pub fn text(&self) -> String {
        let mut out = String::new();
        for y in 0..self.height {
            let mut row = String::new();
            for x in 0..self.width {
                let Some(at) = self.at(x, y) else { continue };
                let cell = &self.cells[at];
                if cell.tail {
                    continue;
                }
                row.push(cell.ch);
                if let Some(more) = &cell.more {
                    row.push_str(more);
                }
            }
            out.push_str(row.trim_end());
            out.push('\n');
        }
        out
    }

    /// The screen as text with its colours (24-bit SGR), a line per row:
    /// `cat` shows the frame in a terminal.
    #[must_use]
    pub fn ansi(&self) -> String {
        let mut out = String::new();
        for y in 0..self.height {
            let mut style = None;
            for x in 0..self.width {
                let Some(at) = self.at(x, y) else { continue };
                let cell = &self.cells[at];
                if cell.tail {
                    continue;
                }
                let next = (cell.fg, cell.bg, cell.marks);
                if style != Some(next) {
                    style = Some(next);
                    let _ = write!(
                        out,
                        "\x1b[0;38;2;{};{};{};48;2;{};{};{}",
                        cell.fg.0, cell.fg.1, cell.fg.2, cell.bg.0, cell.bg.1, cell.bg.2
                    );
                    for (on, code) in [
                        (cell.marks.bold, ";1"),
                        (cell.marks.italic, ";3"),
                        (cell.marks.underline, ";4"),
                    ] {
                        if on {
                            out.push_str(code);
                        }
                    }
                    out.push('m');
                }
                out.push(cell.ch);
                if let Some(more) = &cell.more {
                    out.push_str(more);
                }
            }
            out.push_str("\x1b[0m\n");
        }
        out
    }
}
