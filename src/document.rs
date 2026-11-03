//! Data blocks, items and loops of a CIF read with the standard lexer.
//!
//! Data names are matched case-insensitively, as CIF requires, and a DDLm
//! name such as `_atom_site.fract_x` equals its CIF 1 spelling
//! `_atom_site_fract_x`: both normalize to the lowercase CIF 1 form. A name
//! given twice keeps its last value and is reported.
//! Save frames, which belong in dictionaries, are skipped and reported.

use crate::lexer::{self, Kind, Value};
use crate::structure::{Diagnostic, DiagnosticKind};
use crate::Error;
use rustc_hash::FxHashMap;

/// A data name in the form lookups use: lowercase, with the DDLm category
/// separator `.` written as `_`.
pub(crate) fn normalize(name: &str) -> String {
    name.to_ascii_lowercase().replace('.', "_")
}

/// The data blocks of a file, in order.
pub(crate) fn parse(text: &str) -> Result<Vec<Block<'_>>, Error> {
    let tokens = lexer::tokens(text)?;
    let mut blocks: Vec<Block> = Vec::new();
    // Tokens are skipped inside save frames, after `global_` and after an
    // `END` line, until the next `data_` heading.
    let mut skipping = false;
    let mut save_depth = 0usize;
    let mut it = tokens.into_iter().peekable();
    while let Some(token) = it.next() {
        let line = token.line;
        match token.kind {
            Kind::Data(name) => {
                blocks.push(Block::new(name));
                skipping = false;
                save_depth = 0;
                continue;
            }
            Kind::Global | Kind::End => {
                skipping = true;
                continue;
            }
            _ if skipping => continue,
            Kind::Save(name) => {
                if name.is_empty() {
                    save_depth = save_depth.saturating_sub(1);
                } else {
                    save_depth += 1;
                    if let Some(block) = blocks.last_mut() {
                        block.diagnostics.push(Diagnostic::new(
                            DiagnosticKind::SkippedSaveFrame,
                            format!("line {line}: save frame save_{name} is skipped"),
                        ));
                    }
                }
                continue;
            }
            _ if save_depth > 0 => continue,
            Kind::Stop => continue,
            _ => {}
        }
        let Some(block) = blocks.last_mut() else {
            return Err(Error::cif(format!(
                "line {line}: data before the first data_ heading"
            )));
        };
        match token.kind {
            Kind::Name(name) => match it.next() {
                Some(lexer::Token {
                    kind: Kind::Value(value),
                    ..
                }) => {
                    let index = block.pairs.len();
                    block.pairs.push(value);
                    block.insert(name, Entry::Pair(index), line);
                }
                _ => {
                    return Err(Error::cif(format!(
                        "line {line}: data name {name} has no value"
                    )))
                }
            },
            Kind::Loop => {
                let mut names = Vec::new();
                while let Some(lexer::Token {
                    kind: Kind::Name(name),
                    ..
                }) = it.peek()
                {
                    names.push(*name);
                    it.next();
                }
                if names.is_empty() {
                    return Err(Error::cif(format!("line {line}: loop_ without data names")));
                }
                let mut values = Vec::new();
                while let Some(lexer::Token {
                    kind: Kind::Value(_),
                    ..
                }) = it.peek()
                {
                    if let Some(lexer::Token {
                        kind: Kind::Value(value),
                        ..
                    }) = it.next()
                    {
                        values.push(value);
                    }
                }
                if values.len() % names.len() != 0 {
                    return Err(Error::cif(format!(
                        "line {line}: loop of {} data names holds {} values, not whole rows",
                        names.len(),
                        values.len()
                    )));
                }
                let index = block.loops.len();
                block.loops.push(Loop {
                    width: names.len(),
                    values,
                });
                for (column, name) in names.into_iter().enumerate() {
                    block.insert(name, Entry::Column { index, column }, line);
                }
            }
            Kind::Value(value) => {
                return Err(Error::cif(format!(
                    "line {line}: value {:?} follows no data name",
                    value.text
                )))
            }
            _ => unreachable!("headings, frames and stops are handled above"),
        }
    }
    Ok(blocks)
}

/// One `data_` block.
pub(crate) struct Block<'a> {
    pub(crate) name: &'a str,
    entries: FxHashMap<String, Entry>,
    pairs: Vec<Value<'a>>,
    loops: Vec<Loop<'a>>,
    /// Findings while reading the block: duplicate names, save frames.
    pub(crate) diagnostics: Vec<Diagnostic>,
}

struct Loop<'a> {
    width: usize,
    /// Row-major values.
    values: Vec<Value<'a>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Pair(usize),
    Column { index: usize, column: usize },
}

impl<'a> Block<'a> {
    fn new(name: &'a str) -> Self {
        Block {
            name,
            entries: FxHashMap::default(),
            pairs: Vec::new(),
            loops: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn insert(&mut self, name: &str, entry: Entry, line: usize) {
        if self.entries.insert(normalize(name), entry).is_some() {
            self.diagnostics.push(Diagnostic::new(
                DiagnosticKind::DuplicateDataName,
                format!("line {line}: {name} is given again; the last value is read"),
            ));
        }
    }

    /// Whether the block gives `name` (normalized).
    pub(crate) fn has(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// The normalized data names the block gives.
    pub(crate) fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    /// The value of `name` (normalized); for a looped name, its first row.
    pub(crate) fn value(&self, name: &str) -> Option<&Value<'a>> {
        self.column(name)?.get(0)
    }

    /// The values of `name` (normalized): a loop's column, or a single value
    /// as a column of one.
    pub(crate) fn column(&self, name: &str) -> Option<Column<'_, 'a>> {
        Some(match *self.entries.get(name)? {
            Entry::Pair(index) => Column {
                values: std::slice::from_ref(&self.pairs[index]),
                width: 1,
                column: 0,
                source: Entry::Pair(index),
            },
            Entry::Column { index, column } => Column {
                values: &self.loops[index].values,
                width: self.loops[index].width,
                column,
                source: Entry::Column { index, column: 0 },
            },
        })
    }
}

/// The values of one data name.
#[derive(Clone, Copy)]
pub(crate) struct Column<'b, 'a> {
    values: &'b [Value<'a>],
    width: usize,
    column: usize,
    /// The pair or loop the values come from (column zeroed), so columns can
    /// be compared for belonging to one loop.
    source: Entry,
}

impl<'b, 'a> Column<'b, 'a> {
    pub(crate) fn len(&self) -> usize {
        self.values.len() / self.width
    }

    pub(crate) fn get(&self, row: usize) -> Option<&'b Value<'a>> {
        self.values.get(row * self.width + self.column)
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &'b Value<'a>> + '_ {
        (0..self.len()).filter_map(|row| self.get(row))
    }

    /// Whether `other` is a column of the same loop.
    pub(crate) fn same_loop(&self, other: &Column<'_, '_>) -> bool {
        self.is_loop() && self.source == other.source
    }

    /// Whether the values come from a loop rather than a single item.
    pub(crate) fn is_loop(&self) -> bool {
        matches!(self.source, Entry::Column { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_in_any_case_and_ddlm_spelling() {
        let blocks =
            parse("data_x\n_Cell_Length_A 5.1\nloop_\n_atom_site.label\n_ATOM_SITE_FRACT_X\nA 0.1\nB 0.2\n")
                .unwrap();
        let block = &blocks[0];
        assert_eq!(block.value("_cell_length_a").unwrap().text, "5.1");
        let labels = block.column("_atom_site_label").unwrap();
        let xs = block.column("_atom_site_fract_x").unwrap();
        assert_eq!(labels.len(), 2);
        assert_eq!(xs.get(1).unwrap().text, "0.2");
        assert!(labels.same_loop(&xs));
        assert!(!labels.same_loop(&block.column("_cell_length_a").unwrap()));
        assert_eq!(block.value("_atom_site_label").unwrap().text, "A");
    }

    #[test]
    fn duplicates_keep_the_last_value_and_are_reported() {
        let blocks = parse("data_x\n_a 1\n_A 2\n").unwrap();
        assert_eq!(blocks[0].value("_a").unwrap().text, "2");
        assert_eq!(
            blocks[0].diagnostics[0].kind,
            DiagnosticKind::DuplicateDataName
        );
    }

    #[test]
    fn save_frames_global_and_end_are_skipped() {
        let text =
            "global_\n_g 1\ndata_x\n_a 1\nsave_frame\n_b 2\nsave_\n_c 3\nEND\n_d 4\ndata_y\n_e 5\n";
        let blocks = parse(text).unwrap();
        assert_eq!(blocks.len(), 2);
        assert!(blocks[0].has("_a") && blocks[0].has("_c"));
        assert!(!blocks[0].has("_b") && !blocks[0].has("_d"));
        assert_eq!(
            blocks[0].diagnostics[0].kind,
            DiagnosticKind::SkippedSaveFrame
        );
        assert!(blocks[1].has("_e"));
    }

    #[test]
    fn malformed_blocks_are_errors_with_their_line() {
        let cases = [
            ("_a 1\n", "line 1: data before"),
            ("data_x\n_a\n_b 1\n", "line 2: data name _a has no value"),
            (
                "data_x\nloop_\n_a\n_b\n1 2 3\n",
                "line 2: loop of 2 data names holds 3 values",
            ),
            (
                "data_x\n_a 1 2\n",
                "line 2: value \"2\" follows no data name",
            ),
        ];
        for (text, expected) in cases {
            let err = parse(text).err().unwrap().to_string();
            assert!(err.contains(expected), "{text:?}: {err}");
        }
    }
}
