//! A row of a view before it is on the screen: its glyphs, where they
//! start, the surface behind them, and the fold a click on it opens.

use crate::canvas::Canvas;
use crate::fold::Fold;
use crate::text::Glyph;
use crate::theme::Rgb;

/// A surface behind a row, over its cells `from..to`. Surfaces are whole
/// cells: their corners are square in every terminal.
#[derive(Clone, Debug)]
pub struct Fill {
    pub rgb: Rgb,
    pub from: u16,
    pub to: u16,
}

#[derive(Clone, Debug)]
pub struct Line {
    /// Where the glyphs start.
    pub x: u16,
    pub glyphs: Vec<Glyph>,
    pub fill: Option<Fill>,
    /// Scales the row whole: something coming in, or leaving.
    pub alpha: f32,
    /// The fold this row heads.
    pub fold: Option<Fold>,
}

impl Line {
    #[must_use]
    pub fn blank() -> Line {
        Line::at(0, Vec::new())
    }

    #[must_use]
    pub fn of(glyphs: Vec<Glyph>) -> Line {
        Line::at(0, glyphs)
    }

    #[must_use]
    pub fn at(x: usize, glyphs: Vec<Glyph>) -> Line {
        Line {
            x: cells(x),
            glyphs,
            fill: None,
            alpha: 1.0,
            fold: None,
        }
    }

    #[must_use]
    pub fn on(mut self, rgb: Rgb, from: usize, to: usize) -> Line {
        self.fill = Some(Fill {
            rgb,
            from: cells(from),
            to: cells(to),
        });
        self
    }

    /// An empty row of a surface over cells `from..to`.
    #[must_use]
    pub fn surface(rgb: Rgb, from: usize, to: usize) -> Line {
        Line::blank().on(rgb, from, to)
    }

    #[must_use]
    pub fn faded(mut self, alpha: f64) -> Line {
        self.alpha *= alpha as f32;
        self
    }

    /// A click on the row opens or closes `fold`.
    #[must_use]
    pub fn heads(mut self, fold: Fold) -> Line {
        self.fold = Some(fold);
        self
    }

    /// The row moved `by` cells to the right.
    #[must_use]
    pub fn shifted(mut self, by: usize) -> Line {
        let by = cells(by);
        self.x += by;
        if let Some(fill) = &mut self.fill {
            fill.from += by;
            fill.to += by;
        }
        self
    }
}

/// A count of cells as the screen's coordinates take it.
#[must_use]
pub fn cells(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

impl Canvas {
    /// `line` on row `y` of the column that starts at `x` and is `width` cells wide.
    pub fn line(&mut self, x: u16, y: u16, width: u16, line: &Line) {
        let end = x.saturating_add(width);
        if let Some(fill) = &line.fill {
            let from = x.saturating_add(fill.from).min(end);
            let to = x.saturating_add(fill.to).min(end);
            self.fill(y, from, to, fill.rgb, line.alpha);
        }
        self.put(x.saturating_add(line.x), y, end, &line.glyphs, line.alpha);
    }
}
