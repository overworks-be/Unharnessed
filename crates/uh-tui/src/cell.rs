//! A Python cell on the page: its code quietly coloured, and what it does
//! in a few words. A part of the extension's reading of a cell
//! (`vscode/src/cell.ts`): a command it runs, else its telling line.

use crate::text::{glyphs, Glyph, Ink};
use crate::theme::{ACCENT, INK};

const KEYWORDS: [&str; 35] = [
    "False", "None", "True", "and", "as", "assert", "async", "await", "break", "class", "continue",
    "def", "del", "elif", "else", "except", "finally", "for", "from", "global", "if", "import",
    "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield",
];

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Where the string literal that starts at `at` ends, if one starts there:
/// its prefix letters, its quotes (one or three), up to the closing ones;
/// one left open ends with its line, or with the code when triple-quoted.
fn string_end(code: &str, at: usize) -> Option<usize> {
    let bytes = code.as_bytes();
    let mut open = at;
    while open < bytes.len() && open - at < 2 && b"rRbBfFuU".contains(&bytes[open]) {
        open += 1;
    }
    let quote = *bytes.get(open)?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    if bytes.get(open + 1) == Some(&quote) && bytes.get(open + 2) == Some(&quote) {
        let body = open + 3;
        let close = [quote; 3];
        let end = bytes[body..].windows(3).position(|window| window == close);
        return Some(end.map_or(bytes.len(), |end| body + end + 3));
    }
    let mut end = open + 1;
    while end < bytes.len() {
        match bytes[end] {
            b'\\' => end += 2,
            b'\n' => return Some(end),
            byte if byte == quote => return Some(end + 1),
            _ => end += 1,
        }
    }
    Some(bytes.len())
}

/// `code`'s glyphs: its keywords in the cyan, its strings a step back, its
/// comments faint. Their `src` counts from `src`.
#[must_use]
pub fn python(code: &str, src: u32) -> Vec<Glyph> {
    let keyword = Ink::of(ACCENT, 0.9);
    let string = Ink::of(INK, 0.6);
    let comment = Ink::faint().italic();
    let bytes = code.as_bytes();
    let mut out = Vec::with_capacity(code.len());
    let mut emit = |from: usize, to: usize, ink: Ink| {
        if from < to {
            let offset = src.saturating_add(u32::try_from(from).unwrap_or(u32::MAX));
            out.extend(glyphs(&code[from..to], ink, offset));
        }
    };
    let mut plain = 0;
    let mut at = 0;
    while at < bytes.len() {
        let (end, ink) = if bytes[at] == b'#' {
            let end = bytes[at..].iter().position(|&byte| byte == b'\n');
            (end.map_or(bytes.len(), |end| at + end), comment)
        } else if let Some(end) = string_end(code, at) {
            (end, string)
        } else if is_word(bytes[at]) {
            let mut end = at;
            while end < bytes.len() && is_word(bytes[end]) {
                end += 1;
            }
            if !KEYWORDS.contains(&&code[at..end]) {
                at = end;
                continue;
            }
            (end, keyword)
        } else {
            at += 1;
            continue;
        };
        emit(plain, at, Ink::text());
        emit(at, end, ink);
        plain = end;
        at = end;
    }
    emit(plain, bytes.len(), Ink::text());
    out
}

/// What a cell does, in a few words.
pub struct Summary {
    /// It runs a command: `target` is that command.
    pub shell: bool,
    pub target: String,
}

/// The command a cell runs (the first line of the first `bash("...")`), or
/// its telling line: its first that is not an import or a comment.
#[must_use]
pub fn summary(code: &str) -> Summary {
    let bytes = code.as_bytes();
    let mut from = 0;
    while let Some(found) = code[from..].find("bash(") {
        let at = from + found;
        from = at + 5;
        if at > 0 && (is_word(bytes[at - 1]) || bytes[at - 1] == b'.') {
            continue;
        }
        let start = from + code[from..].len() - code[from..].trim_start().len();
        let Some(end) = string_end(code, start) else {
            continue;
        };
        let literal = code[start..end].trim_start_matches(|ch: char| "rRbBfFuU".contains(ch));
        let quote = if literal.starts_with('"') { '"' } else { '\'' };
        let command = literal.trim_matches(quote);
        if let Some(line) = command.lines().map(str::trim).find(|line| !line.is_empty()) {
            return Summary {
                shell: true,
                target: line.to_string(),
            };
        }
    }
    let mut lines = code.lines().map(str::trim).filter(|line| !line.is_empty());
    let telling = lines.clone().find(|line| {
        !(line.starts_with('#') || line.starts_with("import ") || is_from_import(line))
    });
    Summary {
        shell: false,
        target: telling
            .or_else(|| lines.next())
            .unwrap_or_default()
            .to_string(),
    }
}

fn is_from_import(line: &str) -> bool {
    line.strip_prefix("from ")
        .is_some_and(|rest| rest.contains(" import "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The code's runs of one colour, named: `k`eyword, `s`tring, `c`omment.
    fn runs(code: &str) -> Vec<(char, String)> {
        let mut out: Vec<(char, String)> = Vec::new();
        for glyph in python(code, 0) {
            let kind = match glyph.ink {
                ink if ink.italic => 'c',
                ink if ink.rgb == ACCENT => 'k',
                ink if ink.alpha < 1.0 => 's',
                _ => ' ',
            };
            match out.last_mut() {
                Some((last, text)) if *last == kind => text.push(glyph.ch),
                _ => out.push((kind, glyph.ch.to_string())),
            }
        }
        out.retain(|(kind, _)| *kind != ' ');
        out
    }

    fn run(kind: char, text: &str) -> (char, String) {
        (kind, text.to_string())
    }

    // Listed before the code: a `#` inside a string, a quote inside a
    // comment, an escaped quote, a keyword inside a name, a string left
    // open (on one line, triple-quoted), a prefixed string, a name that is
    // not ASCII.
    #[test]
    fn python_tells_strings_comments_and_keywords_apart() {
        assert_eq!(
            runs("x = 'a # b'  # c"),
            [run('s', "'a # b'"), run('c', "# c")]
        );
        assert_eq!(runs("# don't"), [run('c', "# don't")]);
        assert_eq!(runs(r"s = 'it\'s'"), [run('s', r"'it\'s'")]);
        assert_eq!(runs("format = important"), []);
        assert_eq!(runs("s = 'abc\nnext = 1"), [run('s', "'abc")]);
        assert_eq!(
            runs("'''a\nb''' if x"),
            [run('s', "'''a\nb'''"), run('k', "if")]
        );
        assert_eq!(runs("d = \"\"\"open"), [run('s', "\"\"\"open")]);
        assert_eq!(
            runs("f\"{x}\" or rb'y'"),
            [run('s', "f\"{x}\""), run('k', "or"), run('s', "rb'y'")]
        );
        assert_eq!(runs("données = None"), [run('k', "None")]);
    }

    // Listed before the code: `bash` as part of another name or as a
    // method, a command given as a variable, a command over several lines,
    // a cell of imports only.
    #[test]
    fn summary_finds_the_command_or_the_telling_line() {
        let of = |code: &str| {
            let summary = summary(code);
            (summary.shell, summary.target)
        };
        let shell = |command: &str| (true, command.to_string());
        let line = |line: &str| (false, line.to_string());
        assert_eq!(
            of("server = bash(\"python3 -m http.server\", timeout=None)"),
            shell("python3 -m http.server")
        );
        assert_eq!(
            of("r = await bash('''\n  npm test\n  npm run lint\n''')"),
            shell("npm test")
        );
        assert_eq!(of("h = bash(f'ls {path}')"), shell("ls {path}"));
        assert_eq!(of("rebash(\"x\")"), line("rebash(\"x\")"));
        assert_eq!(of("shell.bash(\"x\")"), line("shell.bash(\"x\")"));
        assert_eq!(of("bash(command)"), line("bash(command)"));
        assert_eq!(
            of("import os\n# setup\nfrom a import b\nx = 1"),
            line("x = 1")
        );
        assert_eq!(of("import os"), line("import os"));
        assert_eq!(of(""), line(""));
    }
}
