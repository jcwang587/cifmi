//! The standard CIF lexer: CIF 1.1, and CIF 2.0 for files that declare it
//! with the `#\#CIF_2.0` magic comment.
//!
//! Comments start at a `#` that begins a token. A CIF 1.1 quoted string
//! closes at its quote followed by whitespace or the end of the line, so
//! `'O'Neil'` reads as `O'Neil`; a CIF 2.0 one closes at its next quote, may
//! be triple-quoted across lines, and lists `[...]` and tables `{...}` are
//! values; data names and `data_` and `save_` headings still run to the next
//! whitespace, brackets and braces included. A text field runs from a line starting with `;` to the next such
//! line and keeps its line breaks. A line holding only `END`, with which
//! AMCSD and RRUFF files close, ends the data block.

use crate::Error;
use std::borrow::Cow;

/// A token and the line (from 1) it starts on.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Token<'a> {
    pub(crate) kind: Kind<'a>,
    pub(crate) line: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind<'a> {
    /// `data_name`, holding the name.
    Data(&'a str),
    /// `save_name` opening a save frame, or `save_` (empty) closing one.
    Save(&'a str),
    Loop,
    Global,
    Stop,
    /// A data name such as `_cell_length_a`, as written.
    Name(&'a str),
    Value(Value<'a>),
    /// A line holding only `END`.
    End,
}

/// A data value as written, without its delimiters.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Value<'a> {
    pub(crate) text: Cow<'a, str>,
    pub(crate) form: Form,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Form {
    /// Unquoted, where `?` (unknown) and `.` (inapplicable) are placeholders.
    Bare,
    /// A quoted string or text field: its text, whatever it reads.
    Quoted,
    /// A CIF 2.0 list or table, kept as written.
    Composite,
}

impl Value<'_> {
    /// The placeholder `?` or `.`, unquoted.
    pub(crate) fn is_placeholder(&self) -> bool {
        self.form == Form::Bare && matches!(self.text.as_ref(), "?" | ".")
    }
}

/// CIF whitespace: space, tab and line breaks.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// Split `text` into tokens.
pub(crate) fn tokens(text: &str) -> Result<Vec<Token<'_>>, Error> {
    // A byte order mark is not data before the first heading.
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let cif2 = text.starts_with("#\\#CIF_2.0");
    let mut lexer = Lexer {
        text,
        lines: lines(text),
        index: 0,
        cif2,
        out: Vec::new(),
    };
    lexer.run()?;
    Ok(lexer.out)
}

/// Byte ranges of the lines of `text`, without their terminators (`\n`,
/// `\r\n` or `\r`).
fn lines(text: &str) -> Vec<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                out.push((start, i));
                start = i + 1;
            }
            b'\r' => {
                out.push((start, i));
                if bytes.get(i + 1) == Some(&b'\n') {
                    i += 1;
                }
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if start < bytes.len() {
        out.push((start, bytes.len()));
    }
    out
}

struct Lexer<'a> {
    text: &'a str,
    lines: Vec<(usize, usize)>,
    /// The line being read.
    index: usize,
    cif2: bool,
    out: Vec<Token<'a>>,
}

impl<'a> Lexer<'a> {
    fn line(&self, index: usize) -> &'a str {
        let (start, end) = self.lines[index];
        &self.text[start..end]
    }

    fn push(&mut self, kind: Kind<'a>, line: usize) {
        self.out.push(Token {
            kind,
            line: line + 1,
        });
    }

    fn error(line: usize, message: impl std::fmt::Display) -> Error {
        Error::cif(format!("line {}: {message}", line + 1))
    }

    fn run(&mut self) -> Result<(), Error> {
        while self.index < self.lines.len() {
            let line = self.line(self.index);
            if line.trim().eq_ignore_ascii_case("END") {
                self.push(Kind::End, self.index);
                self.index += 1;
            } else if let Some(first) = line.strip_prefix(';') {
                self.text_field(first)?;
            } else {
                self.tokenize(self.index, 0)?;
                self.index += 1;
            }
        }
        Ok(())
    }

    /// A text field opened on the current line, whose rest is `first`; reads
    /// on to the closing `;` and tokenizes what follows it on that line.
    fn text_field(&mut self, first: &'a str) -> Result<(), Error> {
        let open = self.index;
        let mut content = String::from(first);
        let mut index = open + 1;
        while index < self.lines.len() {
            let line = self.line(index);
            if line.starts_with(';') {
                self.push(
                    Kind::Value(Value {
                        text: Cow::Owned(content),
                        form: Form::Quoted,
                    }),
                    open,
                );
                self.index = index;
                self.tokenize(index, 1)?;
                self.index = index + 1;
                return Ok(());
            }
            content.push('\n');
            content.push_str(line);
            index += 1;
        }
        Err(Self::error(open, "text field opened here is never closed"))
    }

    /// Tokenize line `index` from byte `from` (relative to the line).
    fn tokenize(&mut self, index: usize, from: usize) -> Result<(), Error> {
        let line = self.line(index);
        let bytes = line.as_bytes();
        let mut i = from;
        while i < bytes.len() {
            let c = bytes[i];
            if is_space(c) {
                i += 1;
                continue;
            }
            if c == b'#' {
                break;
            }
            if c == b'\'' || c == b'"' {
                i = self.quoted(index, i)?;
                continue;
            }
            if self.cif2 && (c == b'[' || c == b'{') {
                i = self.composite(index, i)?;
                continue;
            }
            if self.cif2 && (c == b']' || c == b'}') {
                return Err(Self::error(index, format!("unmatched {:?}", c as char)));
            }
            let start = i;
            let rest = &bytes[i..];
            let name_or_heading = c == b'_'
                || [b"data_", b"save_"].iter().any(|p| {
                    rest.get(..p.len())
                        .is_some_and(|head| head.eq_ignore_ascii_case(*p))
                });
            while i < bytes.len()
                && !is_space(bytes[i])
                && (name_or_heading
                    || !(self.cif2 && matches!(bytes[i], b'[' | b']' | b'{' | b'}')))
            {
                i += 1;
            }
            let word = &line[start..i];
            self.push(classify(word), index);
        }
        Ok(())
    }

    /// A quoted string starting at byte `start` of line `index`; returns
    /// where reading resumes on the line it ends on.
    fn quoted(&mut self, index: usize, start: usize) -> Result<usize, Error> {
        let line = self.line(index);
        let bytes = line.as_bytes();
        let quote = bytes[start];
        if self.cif2 && bytes.get(start + 1) == Some(&quote) && bytes.get(start + 2) == Some(&quote)
        {
            return self.triple_quoted(index, start);
        }
        let mut j = start + 1;
        while j < bytes.len() {
            let closes =
                bytes[j] == quote && (self.cif2 || j + 1 == bytes.len() || is_space(bytes[j + 1]));
            if closes {
                self.push(
                    Kind::Value(Value {
                        text: Cow::Borrowed(&line[start + 1..j]),
                        form: Form::Quoted,
                    }),
                    index,
                );
                return Ok(j + 1);
            }
            j += 1;
        }
        Err(Self::error(index, "quoted string is never closed"))
    }

    /// A CIF 2.0 triple-quoted string, which may span lines.
    fn triple_quoted(&mut self, index: usize, start: usize) -> Result<usize, Error> {
        let quote = &self.line(index)[start..start + 3];
        let (line_start, _) = self.lines[index];
        let from = line_start + start + 3;
        let Some(offset) = self.text[from..].find(quote) else {
            return Err(Self::error(index, "triple-quoted string is never closed"));
        };
        let end = from + offset;
        self.push(
            Kind::Value(Value {
                text: Cow::Borrowed(&self.text[from..end]),
                form: Form::Quoted,
            }),
            index,
        );
        self.resume_at(end + 3)
    }

    /// A CIF 2.0 list or table, read to its matching bracket across quotes
    /// and lines, and kept as written. A bracket of the wrong kind (`[1 }`)
    /// is an error.
    fn composite(&mut self, index: usize, start: usize) -> Result<usize, Error> {
        let (line_start, _) = self.lines[index];
        let from = line_start + start;
        let bytes = self.text.as_bytes();
        // The brackets open and not yet closed, innermost last.
        let mut open: Vec<u8> = Vec::new();
        let mut i = from;
        while i < bytes.len() {
            match bytes[i] {
                opener @ (b'[' | b'{') => open.push(opener),
                closer @ (b']' | b'}') => {
                    let opener = open.pop();
                    let wanted = if opener == Some(b'[') { b']' } else { b'}' };
                    if opener.is_none() || closer != wanted {
                        return Err(Self::error(
                            index,
                            format!(
                                "'{}' does not close the {} opened with '{}'",
                                closer as char,
                                if opener == Some(b'[') {
                                    "list"
                                } else {
                                    "table"
                                },
                                opener.unwrap_or(b'{') as char
                            ),
                        ));
                    }
                    if open.is_empty() {
                        self.push(
                            Kind::Value(Value {
                                text: Cow::Borrowed(&self.text[from..=i]),
                                form: Form::Composite,
                            }),
                            index,
                        );
                        return self.resume_at(i + 1);
                    }
                }
                q @ (b'\'' | b'"') => {
                    let triple = bytes.get(i + 1) == Some(&q) && bytes.get(i + 2) == Some(&q);
                    let width = if triple { 3 } else { 1 };
                    let close = &self.text[i..i + width];
                    match self.text[i + width..].find(close) {
                        Some(offset) => i += width + offset + width - 1,
                        None => return Err(Self::error(index, "quoted string is never closed")),
                    }
                }
                b'#' if i == 0 || is_space(bytes[i - 1]) => {
                    // A comment inside a list or table runs to the end of its line.
                    while i < bytes.len() && !matches!(bytes[i], b'\n' | b'\r') {
                        i += 1;
                    }
                    continue;
                }
                _ => {}
            }
            i += 1;
        }
        Err(Self::error(index, "list or table is never closed"))
    }

    /// Continue reading at byte `at` of the text: moves to the line holding it
    /// and returns the offset within that line.
    fn resume_at(&mut self, at: usize) -> Result<usize, Error> {
        let index = self
            .lines
            .partition_point(|&(start, _)| start <= at)
            .saturating_sub(1);
        let (start, end) = self.lines[index];
        if index != self.index {
            // The value ended on a later line: read the rest of it here.
            self.index = index;
            if at < end {
                self.tokenize(index, at - start)?;
            }
            return Ok(usize::MAX);
        }
        Ok(at - start)
    }
}

fn classify(word: &str) -> Kind<'_> {
    if word.starts_with('_') {
        return Kind::Name(word);
    }
    // `get`, not slicing: a word whose fifth byte falls inside a character
    // is no heading, and slicing it there would panic.
    let prefix = |p: &str| {
        word.get(..p.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(p))
    };
    if prefix("data_") {
        Kind::Data(&word[5..])
    } else if prefix("save_") {
        Kind::Save(&word[5..])
    } else if word.eq_ignore_ascii_case("loop_") {
        Kind::Loop
    } else if word.eq_ignore_ascii_case("global_") {
        Kind::Global
    } else if word.eq_ignore_ascii_case("stop_") {
        Kind::Stop
    } else {
        Kind::Value(Value {
            text: Cow::Borrowed(word),
            form: Form::Bare,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(text: &str) -> Vec<String> {
        tokens(text)
            .unwrap()
            .into_iter()
            .map(|t| match t.kind {
                Kind::Value(v) => format!("{:?}:{}", v.form, v.text),
                other => format!("{other:?}"),
            })
            .collect()
    }

    #[test]
    fn cif1_quotes_comments_and_text_fields() {
        let text = "data_x # comment\n_a 'O'Neil' \"q r\" bare#notcomment\n;\nline one\n\nline two # kept\n; _b 'c'\n";
        assert_eq!(
            values(text),
            [
                "Data(\"x\")",
                "Name(\"_a\")",
                "Quoted:O'Neil",
                "Quoted:q r",
                "Bare:bare#notcomment",
                "Quoted:\nline one\n\nline two # kept",
                "Name(\"_b\")",
                "Quoted:c",
            ]
        );
    }

    #[test]
    fn reserved_words_ignore_case() {
        let kinds: Vec<Kind> = tokens("DATA_A\nLoop_ _x 1 SAVE_f save_ global_ stop_\n")
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect();
        assert_eq!(kinds[0], Kind::Data("A"));
        assert_eq!(kinds[1], Kind::Loop);
        assert_eq!(kinds[4], Kind::Save("f"));
        assert_eq!(kinds[5], Kind::Save(""));
        assert_eq!(kinds[6], Kind::Global);
        assert_eq!(kinds[7], Kind::Stop);
    }

    #[test]
    fn line_numbers_and_line_endings() {
        let toks = tokens("data_x\r\n_a 1\r_b\n;\r\ntext\r\n;\n_c 3").unwrap();
        let lines: Vec<usize> = toks.iter().map(|t| t.line).collect();
        assert_eq!(lines, [1, 2, 2, 3, 4, 7, 7]);
        assert_eq!(
            toks[4].kind,
            Kind::Value(Value {
                text: Cow::Owned("\ntext".into()),
                form: Form::Quoted
            })
        );
    }

    #[test]
    fn end_line_and_data_inside_text_fields() {
        let text = "data_a\n_x\n;\ndata_b\nEND\n;\n_y 2\nEND\n";
        let kinds: Vec<Kind> = tokens(text).unwrap().into_iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds.iter().filter(|k| matches!(k, Kind::Data(_))).count(),
            1
        );
        assert_eq!(kinds.last(), Some(&Kind::End));
        assert_eq!(kinds.iter().filter(|k| **k == Kind::End).count(), 1);
    }

    #[test]
    fn a_byte_order_mark_precedes_the_first_heading() {
        assert_eq!(
            values("\u{feff}data_x\n_a 1\n"),
            ["Data(\"x\")", "Name(\"_a\")", "Bare:1"]
        );
        // The CIF 2.0 magic is still recognized behind it.
        assert_eq!(
            values("\u{feff}#\\#CIF_2.0\ndata_x\n_a [1 2]\n"),
            ["Data(\"x\")", "Name(\"_a\")", "Composite:[1 2]"]
        );
    }

    #[test]
    fn cif2_strings_lists_and_tables() {
        let text = "#\\#CIF_2.0\ndata_x\n_a 'it's'\n_b '''one\ntwo''' _c [1 [2 '3]'] {'k':v}]\n_d {\"a\":1 # note\n}\n";
        assert_eq!(
            values(text),
            [
                "Data(\"x\")",
                "Name(\"_a\")",
                "Quoted:it",
                "Bare:s'",
                "Name(\"_b\")",
                "Quoted:one\ntwo",
                "Name(\"_c\")",
                "Composite:[1 [2 '3]'] {'k':v}]",
                "Name(\"_d\")",
                "Composite:{\"a\":1 # note\n}",
            ]
        );
    }

    #[test]
    fn cif2_brackets_close_their_own_kind() {
        let err = tokens("#\\#CIF_2.0\ndata_x\n_a [1 {2 ] }\n")
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("line 3") && err.contains("']' does not close the table opened with '{'"),
            "{err}"
        );
    }

    #[test]
    fn unclosed_values_are_errors_with_their_line() {
        let err = tokens("data_x\n_a 'open\n").unwrap_err().to_string();
        assert!(
            err.contains("line 2") && err.contains("never closed"),
            "{err}"
        );
        let err = tokens("data_x\n;\ntext\n").unwrap_err().to_string();
        assert!(
            err.contains("line 2") && err.contains("text field"),
            "{err}"
        );
    }

    #[test]
    fn a_word_beyond_ascii_is_a_value_not_a_heading() {
        // The fifth byte of each value falls inside a character, where the
        // check for a data_ or save_ heading must not slice.
        assert_eq!(
            values("data_x\n_a 𐘾ᚠ⠠ 12.3Å αβγδε\n"),
            [
                "Data(\"x\")",
                "Name(\"_a\")",
                "Bare:𐘾ᚠ⠠",
                "Bare:12.3Å",
                "Bare:αβγδε",
            ]
        );
        assert_eq!(values("data_Größe\n"), ["Data(\"Größe\")"]);
    }

    #[test]
    fn cif2_names_and_headings_run_to_whitespace() {
        let text = "#\\#CIF_2.0\ndata_with[1]\n_item[0] hello\nsave_with{2}\n_b [1 2]\nsave_\n";
        assert_eq!(
            values(text),
            [
                "Data(\"with[1]\")",
                "Name(\"_item[0]\")",
                "Bare:hello",
                "Save(\"with{2}\")",
                "Name(\"_b\")",
                "Composite:[1 2]",
                "Save(\"\")",
            ]
        );
    }
}
