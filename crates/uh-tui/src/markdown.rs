//! The agent's text: Markdown as rows of the page. Paragraphs, headings,
//! emphasis, inline code, lists, quotes, rules, fenced code and tables, their
//! columns aligned. Every glyph remembers the byte of the
//! text it renders, so the streaming wave can time it.

use pulldown_cmark::{Alignment, CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

use crate::cell::python;
use crate::line::Line;
use crate::text::{glyphs, width, wrap, wrap_hard, Glyph, Ink};
use crate::theme::{code, code_head, INK};

/// A text still streaming, with the emphasis its last line left open closed
/// (`**bold` reads bold at once instead of showing its asterisks), and
/// without the lone asterisk it may end on (half of a `**` to come). Code is
/// left alone: a fenced block, an inline span.
#[must_use]
pub fn healed(text: &str) -> std::borrow::Cow<'_, str> {
    let fences = text
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count();
    if fences % 2 == 1 {
        return text.into();
    }
    let start = text.rfind('\n').map_or(0, |at| at + 1);
    let line = &text[start..];
    let line = match line.strip_suffix('*') {
        Some(rest) if !rest.ends_with('*') => rest,
        _ => line,
    };
    // Where bold opened and did not close, outside inline code.
    let mut open = None;
    let mut code = false;
    let mut chars = line.char_indices().peekable();
    let mut before = ' ';
    while let Some((at, ch)) = chars.next() {
        if ch == '`' {
            code = !code;
        } else if ch == '*' && !code && chars.peek().is_some_and(|next| next.1 == '*') {
            chars.next();
            // Before a word, it opens; at the line's end, its word is still to come.
            let opens = chars.peek().is_none_or(|next| !next.1.is_whitespace());
            match open {
                None if opens => open = Some(at),
                Some(_) if !before.is_whitespace() => open = None,
                _ => {}
            }
        }
        before = ch;
    }
    let kept = start + line.len();
    let Some(open) = open else {
        return if kept == text.len() {
            text.into()
        } else {
            text[..kept].to_string().into()
        };
    };
    let said = line.trim_end();
    let mut out = text[..start].to_string();
    if said.len() <= open + 2 {
        // Nothing after the asterisks yet.
        out.push_str(&line[..open]);
    } else {
        out.push_str(said);
        out.push_str("**");
        out.push_str(&line[said.len()..]);
    }
    out.into()
}

/// What a block sits in.
enum Container {
    Quote,
    /// A list's item: its marker, until the item's first row took it.
    Item {
        marker: Vec<Glyph>,
        used: bool,
    },
}

/// A fenced block being read.
struct Fence {
    language: String,
    text: String,
    src: u32,
}

/// A table being read: its rows of cells, the head's first.
struct Table {
    aligns: Vec<Alignment>,
    rows: Vec<Vec<Vec<Glyph>>>,
    /// The rows that are its head.
    head: usize,
}

/// The cells between two columns of a table, and the narrowest a column gets.
const TABLE_GAP: usize = 3;
const COLUMN_LEAST: usize = 4;

/// A table as rows of at most `room` cells: each column as wide as its
/// widest cell, the widest ones narrowed (their cells wrap) until the table
/// fits; a rule under the head. `None` when `room` cannot hold its columns.
fn table_rows(table: &Table, room: usize) -> Option<Vec<Vec<Glyph>>> {
    let columns = table.rows.iter().map(Vec::len).max().filter(|n| *n > 0)?;
    let budget = room.checked_sub(TABLE_GAP * (columns - 1))?;
    let mut widths = vec![1; columns];
    for row in &table.rows {
        for (at, cell) in row.iter().enumerate() {
            widths[at] = widths[at].max(width(cell));
        }
    }
    while widths.iter().sum::<usize>() > budget {
        let widest = (0..columns).max_by_key(|at| widths[*at])?;
        if widths[widest] <= COLUMN_LEAST {
            return None;
        }
        widths[widest] -= 1;
    }
    let space = |cells: usize| glyphs(&" ".repeat(cells), Ink::faint(), 0);
    let mut out = Vec::new();
    for (at, row) in table.rows.iter().enumerate() {
        let cells: Vec<Vec<Vec<Glyph>>> = (0..columns)
            .map(|column| {
                let cell = row.get(column).cloned().unwrap_or_default();
                wrap(cell, widths[column])
            })
            .collect();
        let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
        for line in 0..height {
            let mut glyphs = Vec::new();
            for (column, cell) in cells.iter().enumerate() {
                if column > 0 {
                    glyphs.extend(space(TABLE_GAP));
                }
                let text = cell.get(line).cloned().unwrap_or_default();
                let pad = widths[column].saturating_sub(width(&text));
                let before = match table.aligns.get(column) {
                    Some(Alignment::Right) => pad,
                    Some(Alignment::Center) => pad / 2,
                    _ => 0,
                };
                glyphs.extend(space(before));
                glyphs.extend(text);
                glyphs.extend(space(pad - before));
            }
            out.push(glyphs);
        }
        if at + 1 == table.head {
            // The rule comes in with the head's last word.
            let src = row.iter().flatten().last().map_or(0, |glyph| glyph.src);
            let wide = widths.iter().sum::<usize>() + TABLE_GAP * (columns - 1);
            out.push(glyphs(&"─".repeat(wide), Ink::of(INK, 0.14), src));
        }
    }
    Some(out)
}

struct Page {
    width: usize,
    base: Ink,
    out: Vec<Line>,
    /// The block being read.
    inline: Vec<Glyph>,
    bold: u32,
    italic: u32,
    link: u32,
    heading: bool,
    containers: Vec<Container>,
    /// Each open list's next number; `None` for bullets.
    lists: Vec<Option<u64>>,
    fence: Option<Fence>,
    table: Option<Table>,
}

/// `text` as rows of at most `width` cells, in `base` ink.
#[must_use]
pub fn render(text: &str, width: usize, base: Ink) -> Vec<Line> {
    let mut page = Page {
        width,
        base,
        out: Vec::new(),
        inline: Vec::new(),
        bold: 0,
        italic: 0,
        link: 0,
        heading: false,
        containers: Vec::new(),
        lists: Vec::new(),
        fence: None,
        table: None,
    };
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        page.event(event, u32::try_from(range.start).unwrap_or(u32::MAX));
    }
    page.flush();
    page.out
}

impl Page {
    fn ink(&self) -> Ink {
        let mut ink = if self.heading {
            Ink::text().bold()
        } else {
            self.base
        };
        ink.bold |= self.bold > 0;
        ink.italic |= self.italic > 0;
        ink.underline = self.link > 0;
        ink
    }

    /// A blank row of the page before a block that starts at the page's own
    /// level, whatever comes before it: a paragraph, a heading, a list, a
    /// fenced block, a table.
    fn gap(&mut self) {
        let after_block = self
            .out
            .last()
            .is_some_and(|line| !line.glyphs.is_empty() || line.fill.is_some());
        if self.containers.is_empty() && after_block {
            self.out.push(Line::blank());
        }
    }

    /// What starts a row inside the open containers: a quote's bar, an
    /// item's marker on its first row and its width in spaces after.
    fn prefix(&mut self, first: bool) -> Vec<Glyph> {
        let mut out = Vec::new();
        for container in &mut self.containers {
            match container {
                Container::Quote => out.extend(glyphs("▎ ", Ink::faint(), 0)),
                Container::Item { marker, used } => {
                    if first && !*used {
                        *used = true;
                        out.extend(marker.iter().cloned());
                    } else {
                        out.extend(glyphs(&" ".repeat(width(marker)), Ink::faint(), 0));
                    }
                }
            }
        }
        out
    }

    /// The block read so far goes to the page, wrapped.
    fn flush(&mut self) {
        if self.inline.is_empty() {
            return;
        }
        let inline = std::mem::take(&mut self.inline);
        let first = self.prefix(true);
        let rest = self.prefix(false);
        let room = self.width.saturating_sub(width(&first)).max(1);
        for (at, row) in wrap(inline, room).into_iter().enumerate() {
            let mut line = if at == 0 { first.clone() } else { rest.clone() };
            line.extend(row);
            self.out.push(Line::of(line));
        }
    }

    fn event(&mut self, event: Event<'_>, src: u32) {
        if let Some(fence) = &mut self.fence {
            match event {
                Event::Text(text) => {
                    if fence.text.is_empty() {
                        fence.src = src;
                    }
                    fence.text.push_str(&text);
                }
                Event::End(TagEnd::CodeBlock) => self.fenced(),
                _ => {}
            }
            return;
        }
        match event {
            Event::Start(tag) => self.start(tag, src),
            Event::End(tag) => self.end(tag),
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                let ink = self.ink();
                self.inline.extend(glyphs(&text, ink, src));
            }
            Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                let ink = Ink::text().on(INK, 0.1);
                self.inline
                    .extend(glyphs(&text, ink, src.saturating_add(1)));
            }
            Event::SoftBreak => self.inline.extend(glyphs(" ", self.base, src)),
            Event::HardBreak => self.inline.extend(glyphs("\n", self.base, src)),
            Event::Rule => {
                self.flush();
                self.gap();
                self.out.push(Line::of(glyphs(
                    &"─".repeat(self.width),
                    Ink::of(INK, 0.12),
                    src,
                )));
            }
            Event::TaskListMarker(done) => {
                let mark = if done { "☑ " } else { "☐ " };
                self.inline.extend(glyphs(mark, Ink::faint(), src));
            }
            Event::FootnoteReference(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>, src: u32) {
        match tag {
            Tag::Paragraph => {
                self.flush();
                self.gap();
            }
            Tag::Table(aligns) => {
                self.flush();
                self.gap();
                self.table = Some(Table {
                    aligns,
                    rows: Vec::new(),
                    head: 0,
                });
            }
            Tag::Heading { .. } => {
                self.flush();
                self.gap();
                self.heading = true;
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.gap();
                self.containers.push(Container::Quote);
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.gap();
                self.fence = Some(Fence {
                    language,
                    text: String::new(),
                    src,
                });
            }
            Tag::List(first) => {
                self.flush();
                self.gap();
                self.lists.push(first);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        *number += 1;
                        format!("{}. ", *number - 1)
                    }
                    _ => "• ".to_string(),
                };
                self.containers.push(Container::Item {
                    marker: glyphs(&marker, Ink::faint(), src),
                    used: false,
                });
            }
            Tag::Emphasis => self.italic += 1,
            Tag::Strong => self.bold += 1,
            Tag::TableHead => {
                self.bold += 1;
                self.row();
            }
            Tag::Link { .. } => self.link += 1,
            Tag::TableRow => self.row(),
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::TableCell => {
                let cell = std::mem::take(&mut self.inline);
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(cell);
                }
            }
            TagEnd::Table => self.tabled(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = false;
            }
            TagEnd::BlockQuote(_) | TagEnd::Item => {
                self.flush();
                self.containers.pop();
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
            }
            TagEnd::Emphasis => self.italic = self.italic.saturating_sub(1),
            TagEnd::Strong => self.bold = self.bold.saturating_sub(1),
            TagEnd::TableHead => {
                self.bold = self.bold.saturating_sub(1);
                if let Some(table) = &mut self.table {
                    table.head = table.rows.len();
                }
            }
            TagEnd::Link => self.link = self.link.saturating_sub(1),
            _ => {}
        }
    }

    /// A row of the table being read starts.
    fn row(&mut self) {
        if let Some(table) = &mut self.table {
            table.rows.push(Vec::new());
        }
    }

    /// The table read: its rows, their columns aligned. Where the page is
    /// too narrow for its columns, each row is a line of its cells.
    fn tabled(&mut self) {
        let Some(table) = self.table.take() else {
            return;
        };
        let first = self.prefix(true);
        let rest = self.prefix(false);
        let room = self.width.saturating_sub(width(&first)).max(1);
        let rows = table_rows(&table, room).unwrap_or_else(|| {
            let between = || glyphs(" │ ", Ink::faint(), 0);
            table
                .rows
                .iter()
                .flat_map(|row| {
                    let mut line = Vec::new();
                    for (at, cell) in row.iter().enumerate() {
                        if at > 0 {
                            line.extend(between());
                        }
                        line.extend(cell.iter().cloned());
                    }
                    wrap(line, room)
                })
                .collect()
        });
        for (at, row) in rows.into_iter().enumerate() {
            let mut line = if at == 0 { first.clone() } else { rest.clone() };
            line.extend(row);
            self.out.push(Line::of(line));
        }
    }

    /// The fenced block read: its language on a band, when the model gave
    /// one, then its code on a face of its own, the page's width.
    fn fenced(&mut self) {
        let Some(fence) = self.fence.take() else {
            return;
        };
        let from = width(&self.prefix(false));
        let to = self.width;
        let room = to.saturating_sub(from + 4);
        if room == 0 {
            return;
        }
        // Its language on a band; then, as under every title, a row before the code.
        if !fence.language.is_empty() {
            let label = glyphs(&fence.language, Ink::faint(), fence.src);
            self.out
                .push(Line::at(from + 2, label).on(code_head(), from, to));
        }
        self.out.push(Line::surface(code(), from, to));
        let text = fence.text.strip_suffix('\n').unwrap_or(&fence.text);
        let coloured = if matches!(fence.language.as_str(), "python" | "py") {
            python(text, fence.src)
        } else {
            glyphs(text, Ink::text(), fence.src)
        };
        for row in wrap_hard(coloured, room) {
            self.out.push(Line::at(from + 2, row).on(code(), from, to));
        }
        self.out.push(Line::surface(code(), from, to));
    }
}

#[cfg(test)]
mod tests {
    use super::{healed, render};
    use crate::text::Ink;

    // Listed before the code: Python's power operator in a fenced block, in
    // an inline span and in plain prose; bold left open with words after it,
    // with a space after them, with nothing yet; half of a closing `**`; an
    // open inline span; a line above the last one.
    #[test]
    fn a_streaming_line_closes_its_bold_and_nothing_else() {
        for (streamed, shown) in [
            ("**bold", "**bold**"),
            ("**bold text ", "**bold text** "),
            (
                "- **docs.html** is the **heavy",
                "- **docs.html** is the **heavy**",
            ),
            ("a **b** c", "a **b** c"),
            ("text **", "text "),
            ("ends with *", "ends with "),
            ("**bold*", "**bold**"),
            ("2 ** 3", "2 ** 3"),
            ("```python\nx = 2 **", "```python\nx = 2 **"),
            ("`a ** b` and **c", "`a ** b` and **c**"),
            ("`2 **", "`2 **"),
            ("**a\nb", "**a\nb"),
            ("", ""),
        ] {
            assert_eq!(healed(streamed), shown, "{streamed:?}");
        }
    }

    // Listed before the code, how a table can fail: a row runs past the
    // page (long cells, many columns, wide characters); its columns do not
    // line up from row to row (an empty cell, a row with fewer cells than
    // the head or more, a cell on several rows, a wide character in it);
    // a narrowed column loses words; a column set to the right or the
    // centre is padded by characters instead of cells; a page too narrow
    // for the columns panics or loses the table; the head has no rule, or
    // one wider than the table.
    fn shown(text: &str, wide: usize) -> Vec<String> {
        render(text, wide, Ink::text())
            .iter()
            .map(|line| {
                let mut row = String::new();
                for glyph in &line.glyphs {
                    row.push(glyph.ch);
                    row.push_str(glyph.more.as_deref().unwrap_or(""));
                }
                format!("{}{row}", " ".repeat(usize::from(line.x)))
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    const TABLE: &str = "| Page | On disk | Note |\n|:--|--:|:-:|\n| docs.html | 13,794 B | heavy |\n| 漢字.html | 30 B | |\n| a | 1 B | ok | extra |\n| lone |\n";

    #[test]
    fn a_table_lines_its_columns_up() {
        assert_eq!(
            shown(TABLE, 60),
            [
                "Page         On disk   Note",
                "────────────────────────────",
                "docs.html   13,794 B   heavy",
                "漢字.html       30 B",
                "a                1 B    ok",
                "lone",
            ]
        );
    }

    #[test]
    fn a_table_fits_the_page_and_keeps_its_words() {
        let long = "| Name | What it does |\n|---|---|\n| fetch | A page's bytes over HTTP, and the milliseconds it took to come back |\n| 漢字かな | wide ones |\n";
        for text in [TABLE, long] {
            // Its characters, whatever their order: a cell on several rows is read across the others.
            let words = |rows: &[String]| -> Vec<char> {
                let mut all: Vec<char> = rows
                    .concat()
                    .chars()
                    .filter(|ch| !ch.is_whitespace() && !"─│|:-".contains(*ch))
                    .collect();
                all.sort_unstable();
                all
            };
            let whole = words(&shown(text, 200));
            for wide in 1..70 {
                let rows = shown(text, wide);
                for row in &rows {
                    let cells = unicode_width::UnicodeWidthStr::width(row.as_str());
                    assert!(cells <= wide, "{wide} cells: {row:?} takes {cells}");
                }
                if wide >= 8 {
                    assert_eq!(words(&rows), whole, "{wide} cells: words were lost");
                }
            }
        }
        // Narrowed, a cell goes on under itself, in its column.
        assert_eq!(
            shown(long, 30),
            [
                "Name       What it does",
                "──────────────────────────────",
                "fetch      A page's bytes over",
                "           HTTP, and the",
                "           milliseconds it",
                "           took to come back",
                "漢字かな   wide ones",
            ]
        );
    }
}
